use std::collections::HashSet;

use hypr_fs_format::TranscriptWithData;

use super::{SessionStore, StoreError, paths};

/// Summary of a `rebuild_index`/`refresh_session` pass. Counts reflect rows *upserted* this
/// pass, not the resulting table size. `errors` never aborts the scan -- an unparseable file
/// is logged here and its existing index row is left untouched (see the hard rule in each
/// match arm below: corruption must never look like deletion).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RebuildReport {
    pub sessions: usize,
    pub notes: usize,
    pub transcripts: usize,
    pub errors: Vec<String>,
}

impl SessionStore {
    /// One-way: scan sessions/*/ -> upsert index rows; delete index rows whose folder is gone.
    /// Never writes to the vault -- read-only on the filesystem, write-only on the index.
    pub async fn rebuild_index(&self) -> Result<RebuildReport, StoreError> {
        let mut report = RebuildReport::default();

        let folder_ids = self.scan_session_ids().await?;
        for id in &folder_ids {
            self.refresh_one(id, &mut report).await?;
        }

        let present: HashSet<&str> = folder_ids.iter().map(String::as_str).collect();
        for indexed_id in self.all_indexed_session_ids().await? {
            if !present.contains(indexed_id.as_str()) {
                self.delete_session_index_tx(&indexed_id).await?;
            }
        }

        Ok(report)
    }

    /// Watcher + focus entry point: re-read one session's files, refresh its index rows.
    /// Missing `_meta.json` -> delete the session's index rows. Never touches files.
    pub async fn refresh_session(&self, session_id: &str) -> Result<(), StoreError> {
        let mut report = RebuildReport::default();
        self.refresh_one(session_id, &mut report).await?;
        if let Some(first) = report.errors.into_iter().next() {
            return Err(StoreError::Serialize(first));
        }
        Ok(())
    }

    /// Shared by `rebuild_index` (looped over every folder) and `refresh_session` (one id).
    /// A missing `_meta.json` means this id has no session identity in the index -- every
    /// row for it is wiped and we return early without inspecting the other files. Anything
    /// else that fails to parse is logged and its existing row is left exactly as it was.
    async fn refresh_one(&self, id: &str, report: &mut RebuildReport) -> Result<(), StoreError> {
        match self.read_meta(id).await {
            Ok(None) => {
                self.delete_session_index_tx(id).await?;
                return Ok(());
            }
            Ok(Some(meta)) => {
                self.upsert_session_row(&meta).await?;
                report.sessions += 1;
            }
            Err(e) => report.errors.push(format!("{id}: _meta.json: {e}")),
        }

        match self.read_note(id).await {
            Ok(None) => self.delete_document_row(id, "note").await?,
            Ok(Some(body)) => {
                self.upsert_document_row(id, "note", &body).await?;
                report.notes += 1;
            }
            Err(e) => report.errors.push(format!("{id}: _memo.md: {e}")),
        }

        // "note" is always protected here: either just upserted above, deliberately left
        // alone after a parse error, or already deleted -- in the last case there's no row
        // left to prune anyway, so including it unconditionally is harmless.
        let mut keep_kinds = vec!["note".to_string()];
        match self.scan_document_files(id).await {
            Ok(doc_files) => {
                for (kind, content) in &doc_files {
                    keep_kinds.push(kind.clone());
                    match content {
                        Ok(body) => {
                            self.upsert_document_row(id, kind, body).await?;
                            report.notes += 1;
                        }
                        Err(e) => report.errors.push(format!("{id}: {kind}.md: {e}")),
                    }
                }
                self.prune_document_rows(id, &keep_kinds).await?;
            }
            Err(e) => {
                // Couldn't even list the directory -- treat like any other unparseable file:
                // log it, touch nothing. Pruning here would risk mistaking "can't tell" for
                // "definitely gone".
                report.errors.push(format!("{id}: {e}"));
            }
        }

        match self.read_transcript_json(id).await {
            Ok(file) => {
                let mut keep_ids = Vec::with_capacity(file.transcripts.len());
                for t in &file.transcripts {
                    self.upsert_transcript_row(id, t).await?;
                    keep_ids.push(t.id.clone());
                    report.transcripts += 1;
                }
                // A missing transcript.json parses to an empty list via read_transcript_json,
                // so this also correctly prunes every row when the file itself is gone.
                self.prune_transcript_rows(id, &keep_ids).await?;
            }
            Err(e) => report.errors.push(format!("{id}: transcript.json: {e}")),
        }

        Ok(())
    }

    // -- filesystem reads (read-only; never writes to the vault) --

    async fn scan_session_ids(&self) -> Result<Vec<String>, StoreError> {
        let dir = self.vault_base.join(paths::sessions_root());
        tokio::task::spawn_blocking(move || -> Result<Vec<String>, StoreError> {
            let mut ids = Vec::new();
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
                Err(e) => return Err(StoreError::Io(format!("failed to read sessions dir: {e}"))),
            };
            for entry in entries {
                let entry =
                    entry.map_err(|e| StoreError::Io(format!("failed to read dir entry: {e}")))?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                ids.push(name.to_string());
            }
            Ok(ids)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// Lists every `<kind>.md` file in the session directory except `_memo.md`. Per-file read
    /// failures are carried in the inner `Result` (unparseable -> caller logs, leaves the row
    /// alone); an `Err` here means the directory itself couldn't be listed, which the caller
    /// must not treat as "zero files" (that would look like every document vanished).
    async fn scan_document_files(
        &self,
        id: &str,
    ) -> Result<Vec<(String, Result<String, String>)>, String> {
        let dir = self.vault_base.join(paths::session_dir(id));
        tokio::task::spawn_blocking(
            move || -> Result<Vec<(String, Result<String, String>)>, String> {
                let mut out = Vec::new();
                let entries = match std::fs::read_dir(&dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
                    Err(e) => return Err(format!("failed to read session directory: {e}")),
                };
                for entry in entries {
                    let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if stem == "_memo" {
                        continue;
                    }
                    out.push((
                        stem.to_string(),
                        std::fs::read_to_string(&path).map_err(|e| e.to_string()),
                    ));
                }
                Ok(out)
            },
        )
        .await
        .unwrap_or_else(|e| Err(format!("task join error: {e}")))
    }

    // -- index writes (never touch the filesystem) --

    async fn upsert_session_row(
        &self,
        meta: &super::content::SessionMeta,
    ) -> Result<(), StoreError> {
        // Deliberately does not touch `updated_at` on conflict -- rebuild is a read-side
        // reconciliation, not a write, so replaying it against unchanged files must not
        // manufacture a new "last modified" time (rebuild_is_idempotent depends on this).
        sqlx::query(
            "INSERT INTO sessions (id, title, started_at, ended_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               started_at = excluded.started_at,
               ended_at = excluded.ended_at",
        )
        .bind(&meta.id)
        .bind(&meta.title)
        .bind(meta.started_at.as_deref().unwrap_or(""))
        .bind(meta.ended_at.as_deref().unwrap_or(""))
        .bind(&meta.created_at)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn upsert_document_row(
        &self,
        id: &str,
        kind: &str,
        body: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body, updated_at)
             VALUES (?, ?, ?, 'md', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               body = excluded.body",
        )
        .bind(format!("{id}:{kind}"))
        .bind(id)
        .bind(kind)
        .bind(body)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn upsert_transcript_row(
        &self,
        id: &str,
        t: &TranscriptWithData,
    ) -> Result<(), StoreError> {
        // Bind the scanned folder id, not `t.session_id` from the file -- the folder is the
        // source of truth for which session this transcript belongs to.
        let words_json =
            serde_json::to_string(&t.words).map_err(|e| StoreError::Serialize(e.to_string()))?;
        let hints_json = serde_json::to_string(&t.speaker_hints)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;

        sqlx::query(
            "INSERT INTO transcripts (id, session_id, started_at_ms, memo, words_json, speaker_hints_json, updated_at)
             VALUES (?, ?, ?, '', ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               session_id = excluded.session_id,
               started_at_ms = excluded.started_at_ms,
               words_json = excluded.words_json,
               speaker_hints_json = excluded.speaker_hints_json",
        )
        .bind(&t.id)
        .bind(id)
        .bind(t.started_at.round() as i64)
        .bind(&words_json)
        .bind(&hints_json)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn delete_document_row(&self, id: &str, kind: &str) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM session_documents WHERE id = ?")
            .bind(format!("{id}:{kind}"))
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Deletes `session_documents` rows for `id` whose kind isn't in `keep_kinds` -- the files
    /// backing them are confirmed gone (the directory scan that produced `keep_kinds` did
    /// succeed), so per mirror honesty their index rows go too.
    async fn prune_document_rows(&self, id: &str, keep_kinds: &[String]) -> Result<(), StoreError> {
        let existing: Vec<String> =
            sqlx::query_scalar("SELECT id FROM session_documents WHERE session_id = ?")
                .bind(id)
                .fetch_all(self.pool())
                .await?;
        let keep_ids: Vec<String> = keep_kinds.iter().map(|k| format!("{id}:{k}")).collect();
        for existing_id in existing {
            if !keep_ids.contains(&existing_id) {
                sqlx::query("DELETE FROM session_documents WHERE id = ?")
                    .bind(&existing_id)
                    .execute(self.pool())
                    .await?;
            }
        }
        Ok(())
    }

    async fn prune_transcript_rows(&self, id: &str, keep_ids: &[String]) -> Result<(), StoreError> {
        let existing: Vec<String> =
            sqlx::query_scalar("SELECT id FROM transcripts WHERE session_id = ?")
                .bind(id)
                .fetch_all(self.pool())
                .await?;
        for existing_id in existing {
            if !keep_ids.contains(&existing_id) {
                sqlx::query("DELETE FROM transcripts WHERE id = ?")
                    .bind(&existing_id)
                    .execute(self.pool())
                    .await?;
            }
        }
        Ok(())
    }

    async fn all_indexed_session_ids(&self) -> Result<Vec<String>, StoreError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM sessions
             UNION SELECT DISTINCT session_id FROM session_documents WHERE session_id <> ''
             UNION SELECT DISTINCT session_id FROM transcripts WHERE session_id <> ''",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(ids)
    }

    /// Same rationale as Task 6's `delete_session` fix: a partial delete across the three
    /// index tables must never be observable, so it's one transaction. File-system-free by
    /// design -- rebuild/refresh never touch the vault.
    async fn delete_session_index_tx(&self, id: &str) -> Result<(), StoreError> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| StoreError::Db(format!("failed to start transaction: {e}")))?;

        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;
        sqlx::query("DELETE FROM session_documents WHERE session_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;
        sqlx::query("DELETE FROM transcripts WHERE session_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Db(format!("failed to commit transaction: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::session_store::content::SessionMeta;

    fn meta(id: &str, title: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            title: title.to_string(),
            started_at: None,
            ended_at: None,
            created_at: "2026-07-24T00:00:00Z".to_string(),
            tags: vec![],
        }
    }

    fn transcript(id: &str, word_text: &str) -> TranscriptWithData {
        TranscriptWithData {
            id: id.to_string(),
            user_id: String::new(),
            created_at: "2026-07-24T00:00:00Z".to_string(),
            session_id: "ignored-by-rebuild".to_string(),
            started_at: 0.0,
            ended_at: None,
            memo_md: String::new(),
            words: vec![hypr_fs_format::TranscriptWord {
                id: Some("w0".to_string()),
                text: word_text.to_string(),
                start_ms: 0.0,
                end_ms: 0.0,
                channel: 0.0,
                speaker: None,
                metadata: None,
            }],
            speaker_hints: vec![],
        }
    }

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
        (store, temp)
    }

    async fn index_dump(pool: &SqlitePool) -> Vec<String> {
        let mut dump = Vec::new();

        let sessions: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, title, started_at, ended_at, created_at, updated_at FROM sessions ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        dump.extend(sessions.into_iter().map(|row| format!("{row:?}")));

        let documents: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, session_id, kind, body_format, body, updated_at FROM session_documents ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        dump.extend(documents.into_iter().map(|row| format!("{row:?}")));

        let transcripts: Vec<(String, String, i64, String, String, String, String)> = sqlx::query_as(
            "SELECT id, session_id, started_at_ms, memo, words_json, speaker_hints_json, updated_at FROM transcripts ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        dump.extend(transcripts.into_iter().map(|row| format!("{row:?}")));

        dump
    }

    #[tokio::test]
    async fn rebuild_is_idempotent() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "# hi").await.unwrap();
        store.rebuild_index().await.unwrap();
        let first = index_dump(store.pool()).await;
        store.rebuild_index().await.unwrap();
        assert_eq!(first, index_dump(store.pool()).await);
    }

    #[tokio::test]
    async fn rebuild_from_empty_db_restores_index_from_files() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        sqlx::query("DELETE FROM sessions")
            .execute(store.pool())
            .await
            .unwrap();
        store.rebuild_index().await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn refresh_missing_meta_removes_index_row_but_no_files() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "keep me").await.unwrap();
        std::fs::remove_file(vault.path().join("sessions/s1/_meta.json")).unwrap();
        store.refresh_session("s1").await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0);
        assert!(vault.path().join("sessions/s1/_memo.md").is_file()); // vault untouched
    }

    #[tokio::test]
    async fn rebuild_unparseable_meta_leaves_existing_row_and_logs_error() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "Original")).await.unwrap();
        std::fs::write(vault.path().join("sessions/s1/_meta.json"), b"{ not json").unwrap();

        let report = store.rebuild_index().await.unwrap();

        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(
            title, "Original",
            "existing row must survive a corrupt file"
        );
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("s1"));
    }

    #[tokio::test]
    async fn rebuild_deletes_rows_for_vanished_folder_across_all_tables() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "notes").await.unwrap();
        store
            .write_transcript("s1", transcript("t1", "hi"))
            .await
            .unwrap();

        std::fs::remove_dir_all(vault.path().join("sessions/s1")).unwrap();

        store.rebuild_index().await.unwrap();

        let sessions_n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(sessions_n, 0);
        let docs_n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_documents WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(docs_n, 0);
        let transcripts_n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(transcripts_n, 0);
    }

    #[tokio::test]
    async fn rebuild_restores_transcript_words_after_table_wipe() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_transcript("s1", transcript("t1", "restored-word"))
            .await
            .unwrap();

        sqlx::query("DELETE FROM transcripts")
            .execute(store.pool())
            .await
            .unwrap();

        store.rebuild_index().await.unwrap();

        let words_json: String =
            sqlx::query_scalar("SELECT words_json FROM transcripts WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(words_json.contains("restored-word"));
    }
}
