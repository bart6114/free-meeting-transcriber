use sqlx::SqlitePool;
use std::path::PathBuf;

pub mod content;
pub mod journal;
pub mod paths;

pub use content::SessionMeta;

#[derive(Debug)]
pub struct SessionStore {
    vault_base: PathBuf,
    pool: SqlitePool,
    journal: journal::WriteJournal,
    write_lock: tokio::sync::Mutex<()>, // single store-wide lock; can become per-path if contention matters
}

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    Db(String),
    Serialize(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(msg) => write!(f, "I/O error: {}", msg),
            StoreError::Db(msg) => write!(f, "Database error: {}", msg),
            StoreError::Serialize(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<sqlx::Error> for StoreError {
    fn from(err: sqlx::Error) -> Self {
        StoreError::Db(err.to_string())
    }
}

impl SessionStore {
    pub fn new(vault_base: PathBuf, pool: SqlitePool) -> Self {
        Self {
            vault_base,
            pool,
            journal: journal::WriteJournal::new(),
            write_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn write_file(&self, relative: PathBuf, bytes: Vec<u8>) -> Result<(), StoreError> {
        let _lock = self.write_lock.lock().await;

        let abs = self.vault_base.join(&relative);
        let parent = abs
            .parent()
            .ok_or_else(|| StoreError::Io("failed to get parent directory".to_string()))?;

        let parent_path = parent.to_path_buf();
        let abs_path = abs.clone();

        let hash = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&parent_path)
                .map_err(|e| StoreError::Io(format!("failed to create parent directory: {}", e)))?;

            let tmp_path = hypr_fs_sync_core::export::tmp_sibling_path(&abs_path);
            {
                use std::io::Write;
                let mut file = std::fs::File::create(&tmp_path)
                    .map_err(|e| StoreError::Io(format!("failed to create temp file: {}", e)))?;
                file.write_all(&bytes)
                    .map_err(|e| StoreError::Io(format!("failed to write temp file: {}", e)))?;
                file.sync_all()
                    .map_err(|e| StoreError::Io(format!("failed to sync temp file: {}", e)))?;
            }

            std::fs::rename(&tmp_path, &abs_path)
                .map_err(|e| StoreError::Io(format!("failed to rename temp file: {}", e)))?;

            Ok::<String, StoreError>(sha256(&bytes))
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))??;

        let relative_str = relative
            .to_str()
            .ok_or_else(|| StoreError::Io("invalid relative path".to_string()))?;
        self.journal.record(relative_str, &hash);

        Ok(())
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output: String, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
        (store, temp)
    }

    #[tokio::test]
    async fn write_file_creates_parents_and_is_atomic() {
        let (store, temp) = test_store().await;
        let vault = temp.path();
        store
            .write_file(paths::note_path("s1"), b"hello".to_vec())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(vault.join("sessions/s1/_memo.md")).unwrap(),
            b"hello"
        );
        // no tmp leftovers
        assert_eq!(
            std::fs::read_dir(vault.join("sessions/s1"))
                .unwrap()
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn journal_recognizes_own_write_and_external_change() {
        let (store, temp) = test_store().await;
        let vault = temp.path();
        store
            .write_file(paths::note_path("s1"), b"hello".to_vec())
            .await
            .unwrap();
        assert!(
            store
                .journal
                .matches_current_file(vault, "sessions/s1/_memo.md")
        );
        std::fs::write(vault.join("sessions/s1/_memo.md"), b"edited outside").unwrap();
        assert!(
            !store
                .journal
                .matches_current_file(vault, "sessions/s1/_memo.md")
        );
    }

    #[tokio::test]
    async fn concurrent_writes_to_same_path_maintain_journal_consistency() {
        let (store, temp) = test_store().await;
        let vault = temp.path();
        let store = std::sync::Arc::new(store);

        let store1 = store.clone();
        let store2 = store.clone();
        let task1 = async {
            store1
                .write_file(paths::note_path("s1"), b"content1".to_vec())
                .await
        };
        let task2 = async {
            store2
                .write_file(paths::note_path("s1"), b"content2".to_vec())
                .await
        };

        let (r1, r2) = tokio::join!(task1, task2);
        r1.unwrap();
        r2.unwrap();

        assert!(
            store
                .journal
                .matches_current_file(vault, "sessions/s1/_memo.md"),
            "journal hash must match whichever content won the race"
        );
    }
}
