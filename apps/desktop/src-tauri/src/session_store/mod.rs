use sqlx::SqlitePool;
use std::path::PathBuf;

pub mod journal;
pub mod paths;

#[derive(Debug)]
pub struct SessionStore {
    vault_base: PathBuf,
    pool: SqlitePool,
    journal: journal::WriteJournal,
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
        }
    }

    pub async fn write_file(&self, relative: PathBuf, bytes: Vec<u8>) -> Result<(), StoreError> {
        let abs = self.vault_base.join(&relative);
        let parent = abs
            .parent()
            .ok_or_else(|| StoreError::Io("failed to get parent directory".to_string()))?;

        let parent_path = parent.to_path_buf();
        let abs_path = abs.clone();
        let bytes_clone = bytes.clone();

        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&parent_path)
                .map_err(|e| StoreError::Io(format!("failed to create parent directory: {}", e)))?;

            let tmp_path = hypr_fs_sync_core::export::tmp_sibling_path(&abs_path);
            std::fs::write(&tmp_path, &bytes_clone)
                .map_err(|e| StoreError::Io(format!("failed to write temp file: {}", e)))?;

            std::fs::rename(&tmp_path, &abs_path)
                .map_err(|e| StoreError::Io(format!("failed to rename temp file: {}", e)))?;

            Ok::<(), StoreError>(())
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))?
        .map_err(|e| e)?;

        let relative_str = relative
            .to_str()
            .ok_or_else(|| StoreError::Io("invalid relative path".to_string()))?;
        self.journal.record(relative_str, &bytes);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    async fn test_store() -> (SessionStore, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault.clone(), db.pool().clone());
        // Keep temp alive for the duration of the test
        std::mem::forget(temp);
        (store, vault)
    }

    #[tokio::test]
    async fn write_file_creates_parents_and_is_atomic() {
        let (store, vault) = test_store().await;
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
        let (store, vault) = test_store().await;
        store
            .write_file(paths::note_path("s1"), b"hello".to_vec())
            .await
            .unwrap();
        assert!(
            store
                .journal
                .matches_current_file(&vault, "sessions/s1/_memo.md")
        );
        std::fs::write(vault.join("sessions/s1/_memo.md"), b"edited outside").unwrap();
        assert!(
            !store
                .journal
                .matches_current_file(&vault, "sessions/s1/_memo.md")
        );
    }
}
