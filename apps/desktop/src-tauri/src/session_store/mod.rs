use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub mod audio;
pub mod commands;
pub mod content;
pub mod journal;
pub mod migrate;
pub mod paths;
pub mod rebuild;
pub mod transcript;

pub use content::SessionMeta;
pub use rebuild::RebuildReport;
pub use transcript::TranscriptDelta;

#[derive(Debug, Clone)]
pub struct SessionStore {
    vault_base: PathBuf,
    pool: SqlitePool,
    journal: Arc<journal::WriteJournal>,
    write_lock: Arc<tokio::sync::Mutex<()>>, // single store-wide lock; can become per-path if contention matters
    // one live buffer per actively-recording session; guards the debounced-flush lifecycle
    live: Arc<tokio::sync::Mutex<HashMap<String, transcript::LiveTranscriptBuffer>>>,
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
            journal: Arc::new(journal::WriteJournal::new()),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
            live: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Whether the current on-disk bytes at `relative` match the hash this store itself last
    /// wrote there via `write_file`. `false` for anything this store has never written, or
    /// whose bytes have since changed. This is `vault_watch.rs`'s authoritative own-write
    /// filter -- no TTL, unlike `plugins/notify`'s upstream `mark_own_writes` mechanism (see
    /// that module's doc for why a TTL caused a real data-loss incident in the previous
    /// watcher).
    pub fn journal_matches_current_file(&self, relative: &str) -> bool {
        self.journal
            .matches_current_file(&self.vault_base, relative)
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

/// `write_note`/`write_document` never write a frontmatter block -- `_memo.md` and every
/// other `sessions/<id>/<kind>.md` file are meant to hold raw markdown only. A file can still
/// gain a leading frontmatter block from outside those writers: an external edit, or -- until
/// Task 13 removed it -- the legacy `vault_export` DB-to-vault mirror, which always wrapped a
/// `session_documents` row's body in one on export, and which (before this function existed)
/// could nest a wrapper on top of an already-wrapped file, boot/focus after boot/focus. Those
/// wrapped files still exist in real vaults, so the strip stays load-bearing.
///
/// Strips repeatedly, one layer per loop iteration, so a file carrying two or more nested
/// exporter wrappers (the shape that specific bug left behind) converges to the true inner
/// content in a single call rather than only losing its outermost layer and leaving stale
/// wrapper content indexed forever.
///
/// Each layer is only stripped if it's *recognizable as the exporter's own wrapping* -- its
/// frontmatter has an `id` and/or `position` key, the keys the legacy exporter's
/// `render_session_document` always wrote (see `crates/fs-sync-core/src/export.rs`). A block
/// that parses as well-formed frontmatter but has neither key is treated as genuine user
/// content (some other note/document convention, not this app's own wrapper) and the function
/// stops and returns everything from that point on, untouched. This is what makes it safe
/// against eating real user text that happens to open with a valid-looking `---` block: only
/// an unambiguous, exporter-shaped wrapper is ever removed, never guessed at by shape alone.
///
/// A file with no frontmatter at all (the overwhelmingly common case) round-trips through this
/// unchanged -- `ParsedDocument::from_str` returns the original string verbatim when it
/// doesn't start with a `---` delimiter. A file that starts with `---` but doesn't parse as a
/// well-formed frontmatter block (no closing delimiter, or invalid YAML -- which includes a
/// legitimate note that just happens to open with a horizontal rule) is likewise left
/// completely untouched.
fn strip_leading_frontmatter(content: String) -> String {
    use std::str::FromStr;

    let mut current = content;
    loop {
        let parsed = match hypr_fs_sync_core::frontmatter::ParsedDocument::from_str(&current) {
            Ok(parsed) => parsed,
            Err(_) => return current,
        };
        if !is_exporter_wrapper(&parsed.frontmatter) {
            return current;
        }
        current = parsed.content;
    }
}

/// The specific, narrow signal that a parsed leading frontmatter block is the legacy
/// exporter's own wrapping rather than arbitrary user/third-party frontmatter:
/// `render_session_document` always wrote an `id` key, and always wrote a `position` key (see
/// `crates/fs-sync-core/src/export.rs`'s `render_session_document`). Either one present is
/// enough to treat the block as this app's own wrapper.
fn is_exporter_wrapper(frontmatter: &HashMap<String, serde_json::Value>) -> bool {
    frontmatter.contains_key("id") || frontmatter.contains_key("position")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces the boot-1 corruption artifact exactly: two nested `vault_export` wrappers
    /// (each with `id`/`position` keys) around real content. A single call must unwrap both
    /// layers, not just the outer one, converging to the true inner content.
    #[test]
    fn strip_leading_frontmatter_unwraps_nested_exporter_layers() {
        let input = "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\n\
                     ---\nid: s1\nposition: 0\nsession_id: s1\n---\n\nreal content";
        assert_eq!(strip_leading_frontmatter(input.to_string()), "real content");
    }

    /// A block that parses as well-formed frontmatter but carries neither `id` nor `position`
    /// is not the exporter's wrapper -- it's genuine user/third-party content that merely
    /// opens with a valid-looking `---` block, and must be left completely untouched.
    #[test]
    fn strip_leading_frontmatter_leaves_non_exporter_frontmatter_untouched() {
        let input = "---\ntitle: My Doc\nauthor: me\n---\n\nActual user content.";
        assert_eq!(strip_leading_frontmatter(input.to_string()), input);
    }

    /// A file that is *only* an exporter wrapper (empty body) strips to the empty string, not
    /// to some leftover fragment of the wrapper.
    #[test]
    fn strip_leading_frontmatter_of_an_empty_exporter_wrapper_returns_empty_string() {
        let input = "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\n";
        assert_eq!(strip_leading_frontmatter(input.to_string()), "");
    }

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
