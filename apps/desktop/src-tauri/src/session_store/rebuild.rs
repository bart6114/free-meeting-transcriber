use std::collections::HashSet;

use hypr_fs_format::TranscriptWithData;

use super::{SessionStore, StoreError, paths};

/// Summary of a `rebuild_index`/`refresh_session` pass. Counts reflect rows *upserted* this
/// pass, not the resulting table size. `errors` never aborts the scan -- an unparseable file
/// is logged here and its existing index row is left untouched (see the hard rule in each
/// match arm below: corruption must never look like deletion).
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct RebuildReport {
    pub sessions: usize,
    /// Upserted `session_documents` rows this pass -- every `<kind>.md` file including the
    /// note (`_memo.md`) and every `enhanced/<doc_id>.md` doc, not just the note.
    pub notes: usize,
    pub transcripts: usize,
    /// Folder ids that have at least one recognized content file (a `<kind>.md` document or
    /// `transcript.json`) but no `_meta.json` -- left deliberately unindexed; files untouched.
    pub ghost_sessions: Vec<String>,
    pub errors: Vec<String>,
}

impl SessionStore {
    /// One-way: scan sessions/*/ -> upsert index rows; delete index rows whose folder is gone.
    /// Never writes to the vault -- read-only on the filesystem, write-only on the index.
    pub async fn rebuild_index(&self) -> Result<RebuildReport, StoreError> {
        let mut report = RebuildReport::default();

        let folder_ids = self.scan_session_ids().await?;
        for id in &folder_ids {
            // The per-session raw error (if any) is only useful to refresh_session's single-id
            // caller; rebuild_index already has the full picture in report.errors.
            let _ = self.refresh_one(id, &mut report).await?;
        }

        let present: HashSet<&str> = folder_ids.iter().map(String::as_str).collect();
        let stale: Vec<String> = self
            .all_indexed_session_ids()
            .await?
            .into_iter()
            .filter(|indexed_id| !present.contains(indexed_id.as_str()))
            .collect();
        self.delete_session_index_tx(&stale).await?;

        Ok(report)
    }

    /// Watcher + focus entry point: re-read one session's files, refresh its index rows.
    /// Missing `_meta.json` -> delete the session's index rows. Never touches files.
    ///
    /// `Err` does not mean nothing happened: any upserts that succeeded before the failing
    /// artifact are already committed to the index. rebuild/refresh are idempotent, so a
    /// caller can simply retry -- the next pass converges on the same result rather than
    /// double-applying anything.
    pub async fn refresh_session(&self, session_id: &str) -> Result<(), StoreError> {
        let mut report = RebuildReport::default();
        if let Some(first_error) = self.refresh_one(session_id, &mut report).await? {
            // Propagate the original variant (Io/Db/Serialize) rather than relabeling every
            // per-artifact failure as Serialize -- callers may want to distinguish, e.g., a
            // transient permission error (Io, worth retrying) from real corruption.
            return Err(first_error);
        }
        Ok(())
    }

    /// Shared by `rebuild_index` (looped over every folder) and `refresh_session` (one id).
    /// A missing `_meta.json` means this id has no session identity in the index -- every
    /// row for it is wiped and we return early without inspecting the other files. Anything
    /// else that fails to parse is logged and its existing row is left exactly as it was.
    ///
    /// Returns the first raw `StoreError` encountered among the per-artifact reads (already
    /// also logged into `report.errors` as a formatted string) so `refresh_session` can hand
    /// its caller the real error variant. The outer `Result` is reserved for failures that
    /// must abort this session's refresh entirely (index writes, task-join failures).
    async fn refresh_one(
        &self,
        id: &str,
        report: &mut RebuildReport,
    ) -> Result<Option<StoreError>, StoreError> {
        let mut first_error: Option<StoreError> = None;

        match self.read_meta(id).await {
            Ok(None) => {
                self.delete_session_index_tx(&[id.to_string()]).await?;
                match self.session_has_content(id).await {
                    Ok(true) => report.ghost_sessions.push(id.to_string()),
                    Ok(false) => {}
                    Err(e) => record_error(
                        &mut report.errors,
                        &mut first_error,
                        &format!("{id}: ghost-content scan"),
                        e,
                    ),
                }
                return Ok(first_error);
            }
            Ok(Some(meta)) => {
                self.upsert_session_row(&meta).await?;
                report.sessions += 1;
            }
            Err(e) => record_error(
                &mut report.errors,
                &mut first_error,
                &format!("{id}: _meta.json"),
                e,
            ),
        }

        match self.read_note(id).await {
            Ok(None) => self.delete_document_row(id, "note").await?,
            Ok(Some(body)) => {
                self.upsert_document_row(id, "note", &body).await?;
                report.notes += 1;
            }
            Err(e) => record_error(
                &mut report.errors,
                &mut first_error,
                &format!("{id}: _memo.md"),
                e,
            ),
        }

        // "note" is always protected here: either just upserted above, deliberately left
        // alone after a parse error, or already deleted -- in the last case there's no row
        // left to prune anyway, so including it unconditionally is harmless. Rows whose
        // *file* fails to parse are also kept (their id lands in `keep_doc_ids` before the
        // content match): corruption must never look like deletion.
        let mut keep_doc_ids = vec![format!("{id}:note")];
        let mut document_scans_succeeded = true;
        match self.scan_document_files(id).await {
            Ok(doc_files) => {
                for (kind, content) in doc_files {
                    keep_doc_ids.push(format!("{id}:{kind}"));
                    match content {
                        Ok(body) => {
                            self.upsert_document_row(id, &kind, &body).await?;
                            report.notes += 1;
                        }
                        Err(e) => record_error(
                            &mut report.errors,
                            &mut first_error,
                            &format!("{id}: {kind}.md"),
                            e,
                        ),
                    }
                }
            }
            Err(e) => {
                // Couldn't even list the directory -- treat like any other unparseable file:
                // log it, touch nothing. Pruning here would risk mistaking "can't tell" for
                // "definitely gone".
                document_scans_succeeded = false;
                record_error(&mut report.errors, &mut first_error, id, e);
            }
        }

        match self.scan_enhanced_doc_files(id).await {
            Ok(enhanced_files) => {
                for (doc_id, parsed) in enhanced_files {
                    keep_doc_ids.push(doc_id.clone());
                    match parsed {
                        Ok(doc) => {
                            self.upsert_enhanced_doc_row(&doc).await?;
                            report.notes += 1;
                        }
                        Err(e) => record_error(
                            &mut report.errors,
                            &mut first_error,
                            &format!("{id}: enhanced/{doc_id}.md"),
                            e,
                        ),
                    }
                }
            }
            Err(e) => {
                document_scans_succeeded = false;
                record_error(&mut report.errors, &mut first_error, id, e);
            }
        }

        // Only prune when *both* directory listings succeeded: a stale row is only
        // provably stale once every place its file could live has been enumerated.
        // Index-only rows with no file home (pre-cutover UUID summaries -- the owner's
        // no-migration directive leaves them behind deliberately) are pruned here, same
        // as before this task.
        if document_scans_succeeded {
            self.prune_document_rows(id, &keep_doc_ids).await?;
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
            Err(e) => record_error(
                &mut report.errors,
                &mut first_error,
                &format!("{id}: transcript.json"),
                e,
            ),
        }

        Ok(first_error)
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
    ) -> Result<Vec<(String, Result<String, StoreError>)>, StoreError> {
        let dir = self.vault_base.join(paths::session_dir(id));
        tokio::task::spawn_blocking(
            move || -> Result<Vec<(String, Result<String, StoreError>)>, StoreError> {
                let mut out = Vec::new();
                let entries = match std::fs::read_dir(&dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
                    Err(e) => {
                        return Err(StoreError::Io(format!(
                            "failed to read session directory: {e}"
                        )));
                    }
                };
                for entry in entries {
                    let entry = entry
                        .map_err(|e| StoreError::Io(format!("failed to read dir entry: {e}")))?;
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
                    // Hidden files are never session documents: this covers stale
                    // `.tmp-<pid>-<nonce>-<name>` leftovers from a crashed atomic write
                    // (`hypr_fs_sync_core::export::tmp_sibling_path`), which would
                    // otherwise be indexed under a garbage kind.
                    if stem.starts_with('.') {
                        continue;
                    }
                    // The retired sync machinery's files-win reconcile wrote conflict
                    // backups as `<stem>.conflict-<timestamp>.md` siblings
                    // (`unique_conflict_backup_path` in the deleted
                    // `plugins/db/src/import/legacy_vault.rs`). The producer is gone,
                    // but pre-existing vaults can still contain them -- they are frozen
                    // evidence, not live documents, so never index them.
                    if stem.contains(".conflict-") {
                        continue;
                    }
                    let content = std::fs::read_to_string(&path)
                        .map(super::strip_leading_frontmatter)
                        .map_err(|e| StoreError::Io(format!("failed to read {stem}.md: {e}")));
                    out.push((stem.to_string(), content));
                }
                Ok(out)
            },
        )
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// Lists every `<doc_id>.md` under `sessions/<id>/enhanced/` and parses each file's
    /// frontmatter+body into an `EnhancedDoc`. Same error contract as
    /// `scan_document_files`: per-file failures ride the inner `Result` (caller logs and
    /// keeps the row -- the doc id is still reported so pruning never mistakes
    /// "unparseable" for "gone"); an outer `Err` means the directory listing itself
    /// failed and the caller must not prune. A missing `enhanced/` dir is simply "no
    /// docs" -- most sessions never get one.
    async fn scan_enhanced_doc_files(
        &self,
        id: &str,
    ) -> Result<Vec<(String, Result<super::EnhancedDoc, StoreError>)>, StoreError> {
        let dir = self.vault_base.join(paths::enhanced_dir(id));
        let session_id = id.to_string();
        tokio::task::spawn_blocking(
            move || -> Result<Vec<(String, Result<super::EnhancedDoc, StoreError>)>, StoreError> {
                let mut out = Vec::new();
                let entries = match std::fs::read_dir(&dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
                    Err(e) => {
                        return Err(StoreError::Io(format!(
                            "failed to read enhanced docs directory: {e}"
                        )));
                    }
                };
                for entry in entries {
                    let entry = entry
                        .map_err(|e| StoreError::Io(format!("failed to read dir entry: {e}")))?;
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
                    // Same hygiene rules as scan_document_files: hidden files (crashed
                    // atomic-write `.tmp-*` leftovers) and retired conflict backups are
                    // never live documents.
                    if stem.starts_with('.') {
                        continue;
                    }
                    if stem.contains(".conflict-") {
                        continue;
                    }
                    let parsed = std::fs::read_to_string(&path)
                        .map_err(|e| {
                            StoreError::Io(format!("failed to read enhanced/{stem}.md: {e}"))
                        })
                        .and_then(|raw| {
                            super::enhanced::parse_enhanced_file(stem, &session_id, &raw)
                        });
                    out.push((stem.to_string(), parsed));
                }
                Ok(out)
            },
        )
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// Existence-only scan used to populate `RebuildReport.ghost_sessions`: true if the
    /// session directory has at least one recognized content file (a `<kind>.md` document or
    /// `transcript.json`) despite having no `_meta.json`. Never reads file contents.
    async fn session_has_content(&self, id: &str) -> Result<bool, StoreError> {
        let dir = self.vault_base.join(paths::session_dir(id));
        tokio::task::spawn_blocking(move || -> Result<bool, StoreError> {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(e) => {
                    return Err(StoreError::Io(format!(
                        "failed to read session directory: {e}"
                    )));
                }
            };
            for entry in entries {
                let entry =
                    entry.map_err(|e| StoreError::Io(format!("failed to read dir entry: {e}")))?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
                let is_transcript =
                    path.file_name().and_then(|n| n.to_str()) == Some("transcript.json");
                if is_md || is_transcript {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    // -- index writes (never touch the filesystem) --

    async fn upsert_session_row(
        &self,
        meta: &super::content::SessionMeta,
    ) -> Result<(), StoreError> {
        // Deliberately does not touch `updated_at` on conflict -- rebuild is a read-side
        // reconciliation, not a write, so replaying it against unchanged files must not
        // manufacture a new "last modified" time (rebuild_is_idempotent depends on this).
        //
        // The `WHERE` guard on the `DO UPDATE` makes this a true no-op (no row touched, no
        // `AFTER UPDATE` trigger fired) when nothing actually changed -- load-bearing now that
        // Task 10 calls `rebuild_index` automatically on every startup and window focus:
        // without it, every one of those passes fires `sessions`' `search_index_dirty` trigger
        // for every session unconditionally, re-queueing a full search re-projection even
        // when the file on disk hasn't changed since the index already reflects it.
        sqlx::query(
            "INSERT INTO sessions (id, title, started_at, ended_at, created_at, event_json, folder_path, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               started_at = excluded.started_at,
               ended_at = excluded.ended_at,
               event_json = excluded.event_json,
               folder_path = excluded.folder_path
             WHERE title IS NOT excluded.title
                OR started_at IS NOT excluded.started_at
                OR ended_at IS NOT excluded.ended_at
                OR event_json IS NOT excluded.event_json
                OR folder_path IS NOT excluded.folder_path",
        )
        .bind(&meta.id)
        .bind(&meta.title)
        .bind(meta.started_at.as_deref().unwrap_or(""))
        .bind(meta.ended_at.as_deref().unwrap_or(""))
        .bind(&meta.created_at)
        .bind(super::content::event_json_column(meta))
        .bind(meta.folder.as_deref().unwrap_or(""))
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
        // See `upsert_session_row`'s comment: the `WHERE` guard keeps an unchanged file from
        // re-firing `session_documents`' `search_index_dirty` trigger on every automatic
        // rebuild/refresh pass (Task 10).
        sqlx::query(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body, updated_at)
             VALUES (?, ?, ?, 'md', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               body = excluded.body
             WHERE body IS NOT excluded.body",
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

        // See `upsert_session_row`'s comment: the `WHERE` guard keeps an unchanged file from
        // re-firing `transcripts`' `search_index_dirty` trigger on every automatic
        // rebuild/refresh pass (Task 10).
        sqlx::query(
            "INSERT INTO transcripts (id, session_id, started_at_ms, memo, words_json, speaker_hints_json, updated_at)
             VALUES (?, ?, ?, '', ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               session_id = excluded.session_id,
               started_at_ms = excluded.started_at_ms,
               words_json = excluded.words_json,
               speaker_hints_json = excluded.speaker_hints_json
             WHERE session_id IS NOT excluded.session_id
                OR started_at_ms IS NOT excluded.started_at_ms
                OR words_json IS NOT excluded.words_json
                OR speaker_hints_json IS NOT excluded.speaker_hints_json",
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

    /// Deletes `session_documents` rows for `id` whose row id isn't in `keep_ids`
    /// (`<id>:<kind>` for single-slot documents, the bare doc UUID for `enhanced/` docs) --
    /// the files backing them are confirmed gone (every directory scan that fed `keep_ids`
    /// did succeed), so per mirror honesty their index rows go too.
    async fn prune_document_rows(&self, id: &str, keep_ids: &[String]) -> Result<(), StoreError> {
        prune_rows_not_in(self.pool(), "session_documents", id, keep_ids).await
    }

    async fn prune_transcript_rows(&self, id: &str, keep_ids: &[String]) -> Result<(), StoreError> {
        prune_rows_not_in(self.pool(), "transcripts", id, keep_ids).await
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
    /// design -- rebuild/refresh never touch the vault. Takes a batch so `rebuild_index`'s
    /// vanished-session prune is three `IN (...)` deletes per chunk instead of three per id.
    async fn delete_session_index_tx(&self, ids: &[String]) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| StoreError::Db(format!("failed to start transaction: {e}")))?;

        // Chunked to stay far below SQLite's bound-parameter limit (999 on older builds).
        for chunk in ids.chunks(500) {
            let placeholders = placeholder_list(chunk.len());
            for (table, column) in [
                ("sessions", "id"),
                ("session_documents", "session_id"),
                ("transcripts", "session_id"),
            ] {
                let sql = format!("DELETE FROM {table} WHERE {column} IN ({placeholders})");
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                for id in chunk {
                    query = query.bind(id);
                }
                query
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| StoreError::Db(e.to_string()))?;
            }
        }

        tx.commit()
            .await
            .map_err(|e| StoreError::Db(format!("failed to commit transaction: {e}")))?;
        Ok(())
    }
}

fn placeholder_list(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// One `DELETE ... WHERE session_id = ? AND id NOT IN (...)` instead of a select-then-loop
/// issuing one DELETE per stale row. `keep_ids` is bounded by the number of files in a single
/// session directory, so it always fits one statement's bound-parameter budget.
async fn prune_rows_not_in(
    pool: &sqlx::SqlitePool,
    table: &str,
    session_id: &str,
    keep_ids: &[String],
) -> Result<(), StoreError> {
    let sql = if keep_ids.is_empty() {
        format!("DELETE FROM {table} WHERE session_id = ?")
    } else {
        format!(
            "DELETE FROM {table} WHERE session_id = ? AND id NOT IN ({})",
            placeholder_list(keep_ids.len())
        )
    };
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(session_id);
    for keep_id in keep_ids {
        query = query.bind(keep_id);
    }
    query.execute(pool).await?;
    Ok(())
}

/// Pushes a human-readable line to `errors` and remembers the first raw `StoreError`
/// encountered this pass, so `refresh_session` can propagate the real variant instead of
/// relabeling every per-artifact failure as `StoreError::Serialize`.
fn record_error(
    errors: &mut Vec<String>,
    first: &mut Option<StoreError>,
    context: &str,
    err: StoreError,
) {
    errors.push(format!("{context}: {err}"));
    if first.is_none() {
        *first = Some(err);
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
            event: None,
            folder: None,
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

    /// Task 10 calls `rebuild_index` automatically on every startup and window focus, so a
    /// no-op rebuild must be a true DB no-op: the upserts' `WHERE` guards must keep unchanged
    /// rows from firing their `AFTER UPDATE` triggers (observable via the `search_index_dirty`
    /// queue) -- otherwise every boot/focus re-queues every session for a needless search
    /// re-projection, forever, even when nothing on disk changed.
    #[tokio::test]
    async fn rebuild_of_unchanged_files_does_not_requeue_search_reindexing() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "# hi").await.unwrap();
        store.rebuild_index().await.unwrap();

        sqlx::query("DELETE FROM search_index_dirty")
            .execute(store.pool())
            .await
            .unwrap();

        store.rebuild_index().await.unwrap();

        let dirty: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM search_index_dirty")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(dirty, 0);
    }

    /// The mirror image of the no-op test above: the `WHERE` guard must never suppress a
    /// *genuine* change, only a spurious re-fire. Simulates an external edit with a raw
    /// `std::fs::write` (bypassing `write_meta` entirely, the way another device or a
    /// hand-edit would) so the guard's `DO UPDATE` branch -- not just its no-op branch -- gets
    /// exercised, and asserts both halves of "the reconcile actually reconciled": the index
    /// value changed, and the session got queued for search reindexing.
    #[tokio::test]
    async fn rebuild_of_a_genuinely_changed_file_updates_the_index_and_requeues_search_reindexing()
    {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.rebuild_index().await.unwrap();

        sqlx::query("DELETE FROM search_index_dirty")
            .execute(store.pool())
            .await
            .unwrap();

        let meta_path = vault.path().join("sessions/s1/_meta.json");
        let edited = serde_json::to_vec_pretty(&meta("s1", "Two")).unwrap();
        std::fs::write(&meta_path, edited).unwrap();

        store.rebuild_index().await.unwrap();

        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id = 's1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(title, "Two");

        let dirty: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM search_index_dirty WHERE entity_type = 'session' AND entity_id = 's1'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            dirty, 1,
            "a genuine change must still queue search reindexing, not just no-ops getting skipped"
        );
    }

    /// Reproduces the failure mode a real boot smoke turned up: a `_memo.md` that already
    /// carries a frontmatter wrapper (as an external edit or the retired legacy exporter
    /// would leave behind) must not have that wrapper compound with each
    /// automatic rebuild pass. Without `strip_leading_frontmatter` in the read path, each of
    /// these calls would index the *previous* pass's own wrapper verbatim, growing the indexed
    /// body by one nested frontmatter block every time -- exactly what Task 10's automatic
    /// startup/focus rescans would otherwise do to it on every boot.
    #[tokio::test]
    async fn rebuild_of_an_already_wrapped_note_file_does_not_grow_the_indexed_body() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        let dir = vault.path().join("sessions/s1");
        std::fs::write(
            dir.join("_memo.md"),
            "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\nreal content",
        )
        .unwrap();

        store.rebuild_index().await.unwrap();
        let first: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id='s1:note'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(first, "real content");

        store.rebuild_index().await.unwrap();
        store.rebuild_index().await.unwrap();
        let after_three_passes: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id='s1:note'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(after_three_passes, "real content");
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
    async fn rebuild_restores_event_and_folder_columns_from_files() {
        let (store, _vault) = test_store().await;
        let mut m = meta("s1", "One");
        m.event = Some(serde_json::json!({"tracking_id": "evt-1", "meeting_link": ""}));
        m.folder = Some("work".to_string());
        store.write_meta(&m).await.unwrap();

        sqlx::query("DELETE FROM sessions")
            .execute(store.pool())
            .await
            .unwrap();

        store.rebuild_index().await.unwrap();

        let (event_json, folder_path): (String, String) =
            sqlx::query_as("SELECT event_json, folder_path FROM sessions WHERE id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&event_json).unwrap(),
            m.event.unwrap()
        );
        assert_eq!(folder_path, "work");
    }

    /// The change-guard no-op property must hold for the widened columns too: a meta whose
    /// `event` is populated re-serializes to the identical `event_json` string on every
    /// rebuild pass, so an unchanged file must still not requeue search reindexing.
    #[tokio::test]
    async fn rebuild_of_unchanged_event_and_folder_does_not_requeue_search_reindexing() {
        let (store, _vault) = test_store().await;
        let mut m = meta("s1", "One");
        m.event = Some(serde_json::json!({"tracking_id": "evt-1", "meeting_link": "x"}));
        m.folder = Some("work".to_string());
        store.write_meta(&m).await.unwrap();
        store.rebuild_index().await.unwrap();

        sqlx::query("DELETE FROM search_index_dirty")
            .execute(store.pool())
            .await
            .unwrap();

        store.rebuild_index().await.unwrap();

        let dirty: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM search_index_dirty")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(dirty, 0);
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

    /// REGRESSION: `Path::exists()` swallows permission-denied as "false", which used to make
    /// a transiently-unreadable `_meta.json` look identical to a missing one and delete a
    /// live session's index rows. read_meta must now distinguish "not found" from "exists but
    /// unreadable" and rebuild must treat the latter as an error, not a deletion.
    #[cfg(unix)]
    #[tokio::test]
    async fn rebuild_unreadable_meta_leaves_existing_row_and_logs_error() {
        use std::os::unix::fs::PermissionsExt;

        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "Original")).await.unwrap();

        let meta_path = vault.path().join("sessions/s1/_meta.json");
        let original_perms = std::fs::metadata(&meta_path).unwrap().permissions();
        std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let report = store.rebuild_index().await.unwrap();

        // Restore permissions before any assertion can panic, so tempdir cleanup never trips
        // over a file it can't stat/delete.
        std::fs::set_permissions(&meta_path, original_perms).unwrap();

        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(
            title, "Original",
            "existing row must survive a transiently-unreadable file, not just a corrupt one"
        );
        assert!(
            !report.errors.is_empty(),
            "an unreadable file must be reported, not silently treated as absent"
        );
    }

    /// Vaults created before this branch can still hold the retired sync machinery's
    /// conflict backups (`<stem>.conflict-<timestamp>.md`) and, after a crash mid-write,
    /// `.tmp-<pid>-<nonce>-<name>` atomic-write leftovers. Neither is a live document;
    /// rebuild must not index them as one.
    #[tokio::test]
    async fn rebuild_ignores_conflict_backups_and_tmp_leftovers() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "live note").await.unwrap();
        let dir = vault.path().join("sessions/s1");
        std::fs::write(
            dir.join("_memo.conflict-2026-07-23T12-00-00.123Z.md"),
            "stale conflict copy",
        )
        .unwrap();
        std::fs::write(dir.join(".tmp-1234-5678-_memo.md"), "crashed atomic write").unwrap();

        let report = store.rebuild_index().await.unwrap();

        assert_eq!(report.notes, 1, "only the live note should be indexed");
        let kinds: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM session_documents WHERE session_id='s1'")
                .fetch_all(store.pool())
                .await
                .unwrap();
        assert_eq!(kinds, vec!["note".to_string()]);
    }

    fn enhanced_doc(session_id: &str, doc_id: &str) -> crate::session_store::EnhancedDoc {
        crate::session_store::EnhancedDoc {
            id: doc_id.to_string(),
            session_id: session_id.to_string(),
            kind: "template_output".to_string(),
            title: "Customer review".to_string(),
            template_id: "template-1".to_string(),
            sort_order: 2,
            markdown: "# Review\n\n- Point".to_string(),
        }
    }

    /// The whole point of the file home: after a full index wipe, every metadata column
    /// (title/template_id/sort_order/kind) comes back from the frontmatter alone.
    #[tokio::test]
    async fn rebuild_restores_enhanced_doc_metadata_from_frontmatter() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        sqlx::query("DELETE FROM session_documents")
            .execute(store.pool())
            .await
            .unwrap();

        store.rebuild_index().await.unwrap();

        let (kind, template_id, title, sort_order, body): (String, String, String, i64, String) =
            sqlx::query_as(
                "SELECT kind, template_id, title, sort_order, body
                 FROM session_documents WHERE id='doc-1' AND session_id='s1'",
            )
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(kind, "template_output");
        assert_eq!(template_id, "template-1");
        assert_eq!(title, "Customer review");
        assert_eq!(sort_order, 2);
        assert_eq!(body, "# Review\n\n- Point");
    }

    /// The change-guard no-op property must hold for enhanced docs too: an unchanged
    /// `enhanced/<doc>.md` must not requeue search reindexing on the automatic
    /// startup/focus rebuild passes.
    #[tokio::test]
    async fn rebuild_of_unchanged_enhanced_doc_does_not_requeue_search_reindexing() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();
        store.rebuild_index().await.unwrap();

        sqlx::query("DELETE FROM search_index_dirty")
            .execute(store.pool())
            .await
            .unwrap();

        store.rebuild_index().await.unwrap();

        let dirty: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM search_index_dirty")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(dirty, 0);
    }

    #[tokio::test]
    async fn rebuild_prunes_enhanced_doc_row_whose_file_is_gone() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        std::fs::remove_file(vault.path().join("sessions/s1/enhanced/doc-1.md")).unwrap();

        store.rebuild_index().await.unwrap();

        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_documents WHERE id='doc-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    /// Corruption must never look like deletion: an `enhanced/<doc>.md` whose frontmatter
    /// no longer parses is logged, and its existing index row survives the pass -- both
    /// the upsert skip and the prune must respect it.
    #[tokio::test]
    async fn rebuild_unparseable_enhanced_doc_leaves_existing_row_and_logs_error() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        std::fs::write(
            vault.path().join("sessions/s1/enhanced/doc-1.md"),
            "---\ntitle: [unclosed\n---\n\nbody",
        )
        .unwrap();

        let report = store.rebuild_index().await.unwrap();

        let title: String =
            sqlx::query_scalar("SELECT title FROM session_documents WHERE id='doc-1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            title, "Customer review",
            "existing row must survive a corrupt file"
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("enhanced/doc-1.md")),
            "the corrupt doc must be reported: {:?}",
            report.errors
        );
    }

    /// Pre-cutover UUID summary rows never had a file home, and the owner's no-migration
    /// directive means they never get one -- rebuild prunes them exactly as it did before
    /// this task (this test pins that preserved behavior rather than introducing it).
    #[tokio::test]
    async fn rebuild_still_prunes_legacy_index_only_uuid_rows_without_files() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body)
             VALUES ('legacy-uuid', 's1', 'summary', 'prosemirror_json', '{}')",
        )
        .execute(store.pool())
        .await
        .unwrap();

        store.rebuild_index().await.unwrap();

        let legacy: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_documents WHERE id='legacy-uuid'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(legacy, 0, "index-only rows without files stay pruned");
        let file_backed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_documents WHERE id='doc-1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            file_backed, 1,
            "file-backed docs must survive the same prune"
        );
    }

    #[tokio::test]
    async fn rebuild_reports_ghost_sessions_without_indexing_them() {
        let (store, _vault) = test_store().await;
        // A "ghost" session: transcript.json written without ever calling write_meta, matching
        // Task 7's recording_into_unknown_session_still_persists regression.
        store
            .write_transcript("ghost", transcript("t1", "hi"))
            .await
            .unwrap();

        let report = store.rebuild_index().await.unwrap();

        assert_eq!(report.ghost_sessions, vec!["ghost".to_string()]);
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id='ghost'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "ghost sessions must not be indexed");
    }
}
