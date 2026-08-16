use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::index::{IndexEntity, SessionEntry, VAULT_TASKS_KEY, apply_map_value, legacy_doc};
use super::{SessionStore, StoreError, paths};

/// Summary of a `rebuild_index`/`refresh_session` pass. Counts reflect entries *derived
/// from files* this pass, not the resulting index size. `errors` never aborts the scan --
/// an unparseable file is logged here and its existing index entry is left untouched (see
/// the hard rule in each match arm below: corruption must never look like deletion).
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct RebuildReport {
    pub sessions: usize,
    /// Documents read this pass -- every `<kind>.md` file including the note (`_memo.md`)
    /// and every `enhanced/<doc_id>.md` doc, not just the note.
    pub notes: usize,
    pub transcripts: usize,
    /// Folder ids that have at least one recognized content file (a `<kind>.md` document or
    /// `transcript.json`) but no `_meta.json` -- left deliberately unindexed; files untouched.
    pub ghost_sessions: Vec<String>,
    pub errors: Vec<String>,
}

impl SessionStore {
    /// One-way: scan sessions/*/ -> reconcile the in-memory index; drop index entries whose
    /// folder is gone. Never writes to the vault -- read-only on the filesystem, write-only
    /// on the index. Also reconciles the vault-root tasks file and the templates folder.
    /// Only the entities whose value actually changed are notified onto the change bus
    /// (`PartialEq` diff), so the automatic startup/focus rescans stay silent over
    /// unchanged files -- the search projection and the frontend both ride that bus, and
    /// a no-op rescan must not re-trigger either.
    pub async fn rebuild_index(&self) -> Result<RebuildReport, StoreError> {
        let mut report = RebuildReport::default();

        // The scan and the catalog swap run under the store write lock: renames
        // (provisional reconcile, migration) also hold it, so the swap can never
        // revert the catalog to a directory a concurrent rename just moved away
        // from -- which would make the next write recreate the old path.
        let guard = self.lock_writes().await;
        let scan = self.scan_session_locations().await?;

        // Ids the prune below must not touch: an id claimed by multiple directories
        // is ambiguous (not gone), and an id whose known directory -- or any parent
        // of it -- is now corrupt/unreadable is broken (not gone). Descendant
        // matching matters for the unreadable case: a permission error on a personal
        // folder must not make every session homed under it look deleted.
        let protected: HashSet<String> = {
            let catalog = self.locations.read().unwrap();
            catalog
                .iter()
                .filter(|(_, dir)| {
                    scan.broken_dirs
                        .iter()
                        .any(|broken| hypr_vault_read::layout::path_starts_with_nfc(dir, broken))
                })
                .map(|(id, _)| id.clone())
                .chain(scan.duplicate_ids.iter().cloned())
                .collect()
        };

        // Refresh the location catalog from discovery before reconciling any content, so
        // every per-session read below resolves against the physical layout just scanned.
        // Corrupt-protected ids keep their previous location (their directory is still
        // there, just unreadable); duplicated ids are dropped so writes block instead of
        // silently picking one claimant.
        {
            let mut catalog = self.locations.write().unwrap();
            let previous = std::mem::take(&mut *catalog);
            for (id, dir) in &scan.locations {
                catalog.insert(id.clone(), dir.clone());
            }
            for id in &protected {
                if scan.duplicate_ids.contains(id) {
                    continue;
                }
                if let Some(dir) = previous.get(id) {
                    catalog.entry(id.clone()).or_insert_with(|| dir.clone());
                }
            }
        }
        // Duplicate claims persist past the rebuild so lazy per-id resolution can't
        // sidestep them (find_session's legacy fast path would otherwise silently
        // pick the canonical claimant and let writes diverge the copies).
        *self.known_duplicates.write().unwrap() = scan.duplicate_ids.iter().cloned().collect();
        drop(guard);

        report.errors.extend(scan.errors.iter().cloned());
        report.ghost_sessions = scan.ghost_dirs.clone();

        for (id, _) in &scan.locations {
            // The per-session raw error (if any) is only useful to refresh_session's single-id
            // caller; rebuild_index already has the full picture in report.errors.
            let _ = self.refresh_one(id, &mut report).await?;
        }

        // Sessions that vanished from disk are removed (the discovery scan succeeded and
        // came back without them -- the only certainty a prune is allowed to act on;
        // ambiguous/corrupt ids are protected above).
        let present: HashSet<&str> = scan.locations.iter().map(|(id, _)| id.as_str()).collect();
        let stale: Vec<String> = {
            let index = self.index.read().unwrap();
            index
                .sessions
                .keys()
                .chain(index.docs.keys())
                .chain(index.transcripts.keys())
                .chain(index.tasks.keys())
                .filter(|id| {
                    *id != VAULT_TASKS_KEY
                        && !present.contains(id.as_str())
                        && !protected.contains(id.as_str())
                })
                .cloned()
                .collect::<HashSet<String>>()
                .into_iter()
                .collect()
        };
        for id in stale {
            self.index_remove_session_and_notify(&id);
        }

        if let Some(tasks) = self.read_index_tasks(paths::vault_tasks_path()).await {
            let changed = {
                let mut index = self.index.write().unwrap();
                apply_map_value(&mut index.tasks, VAULT_TASKS_KEY, tasks)
            };
            if changed {
                self.notify_index_changed(IndexEntity::Tasks, vec![VAULT_TASKS_KEY.to_string()]);
            }
        }

        self.index_refresh_templates().await;
        self.index_refresh_people().await;

        Ok(report)
    }

    /// Watcher + focus entry point: re-read one session's files, refresh its index slice.
    /// Missing `_meta.json` -> remove the session's index entries. Never touches files.
    ///
    /// `Err` does not mean nothing happened: any slices that reconciled before the failing
    /// artifact are already applied to the index. rebuild/refresh are idempotent, so a
    /// caller can simply retry -- the next pass converges on the same result rather than
    /// double-applying anything.
    pub async fn refresh_session(&self, session_id: &str) -> Result<(), StoreError> {
        let mut report = RebuildReport::default();
        let first_error = self.refresh_one(session_id, &mut report).await?;

        if let Some(first_error) = first_error {
            // Propagate the original variant (Io/Serialize) rather than relabeling every
            // per-artifact failure -- callers may want to distinguish, e.g., a transient
            // permission error (Io, worth retrying) from real corruption.
            return Err(first_error);
        }
        Ok(())
    }

    /// Shared by `rebuild_index` (looped over every folder) and `refresh_session` (one id).
    /// A missing `_meta.json` means this id has no session identity in the index -- every
    /// entry for it is removed and we return early without inspecting the other files.
    /// Anything else that fails to read/parse is logged and its existing entry is left
    /// exactly as it was; only the entities whose value actually changed are notified.
    ///
    /// Returns the first raw `StoreError` encountered among the per-artifact reads (already
    /// also logged into `report.errors` as a formatted string) so `refresh_session` can hand
    /// its caller the real error variant. The outer `Result` is reserved for failures that
    /// must abort this session's refresh entirely (task-join failures).
    async fn refresh_one(
        &self,
        id: &str,
        report: &mut RebuildReport,
    ) -> Result<Option<StoreError>, StoreError> {
        let mut first_error: Option<StoreError> = None;

        let meta = match self.read_meta(id).await {
            Ok(None) => {
                // The directory (or at least its identity) is gone: drop the catalog
                // entry too, so a later write re-resolves instead of recreating the
                // stale path.
                self.catalog_remove(id);
                self.index_remove_session_and_notify(id);
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
                report.sessions += 1;
                Some(meta)
            }
            Err(e) => {
                // Unreadable/corrupt: keep the old entry.
                record_error(
                    &mut report.errors,
                    &mut first_error,
                    &format!("{id}: _meta.json"),
                    e,
                );
                None
            }
        };

        let note = match self.read_note(id).await {
            Ok(note) => {
                if note.is_some() {
                    report.notes += 1;
                }
                Some(note)
            }
            Err(e) => {
                record_error(
                    &mut report.errors,
                    &mut first_error,
                    &format!("{id}: _memo.md"),
                    e,
                );
                None
            }
        };

        // Documents: both directory listings must succeed before the docs vec is replaced
        // wholesale -- a failed listing can't tell "gone" from "unlistable", and replacing
        // from a partial scan would make corruption look like deletion. Individual files
        // that fail to parse keep their previous entry by id, same invariant.
        let previous_docs: HashMap<String, super::EnhancedDoc> = {
            let index = self.index.read().unwrap();
            index
                .docs
                .get(id)
                .map(|docs| {
                    docs.iter()
                        .map(|doc| (doc.id.clone(), doc.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut document_scans_succeeded = true;
        let mut collected_docs = Vec::new();
        match self.scan_document_files(id).await {
            Ok(doc_files) => {
                for (kind, content) in doc_files {
                    match content {
                        Ok(body) => {
                            collected_docs.push(legacy_doc(id, &kind, body));
                            report.notes += 1;
                        }
                        Err(e) => {
                            record_error(
                                &mut report.errors,
                                &mut first_error,
                                &format!("{id}: {kind}.md"),
                                e,
                            );
                            if let Some(old) = previous_docs.get(&format!("{id}:{kind}")) {
                                collected_docs.push(old.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                document_scans_succeeded = false;
                record_error(&mut report.errors, &mut first_error, id, e);
            }
        }
        match self.scan_enhanced_doc_files(id).await {
            Ok(enhanced_files) => {
                for (doc_id, parsed) in enhanced_files {
                    match parsed {
                        Ok(doc) => {
                            collected_docs.push(doc);
                            report.notes += 1;
                        }
                        Err(e) => {
                            record_error(
                                &mut report.errors,
                                &mut first_error,
                                &format!("{id}: enhanced/{doc_id}.md"),
                                e,
                            );
                            if let Some(old) = previous_docs.get(&doc_id) {
                                collected_docs.push(old.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                document_scans_succeeded = false;
                record_error(&mut report.errors, &mut first_error, id, e);
            }
        }
        let docs = document_scans_succeeded.then_some(collected_docs);

        let transcripts = match self.read_transcript_json(id).await {
            Ok(file) => {
                report.transcripts += file.transcripts.len();
                // A missing transcript.json parses to an empty list via read_transcript_json,
                // so this also correctly removes the entry when the file itself is gone.
                Some(file.transcripts)
            }
            Err(e) => {
                record_error(
                    &mut report.errors,
                    &mut first_error,
                    &format!("{id}: transcript.json"),
                    e,
                );
                None
            }
        };

        let tasks = match self.session_dir(id).await {
            Ok(dir) => {
                self.read_index_tasks(paths::session_tasks_path_in(&dir))
                    .await
            }
            // Unresolvable (e.g. ambiguous) id: leave the existing tasks entry alone,
            // same keep-on-failure contract as every other artifact here.
            Err(_) => None,
        };

        let mut changes = Vec::new();
        {
            let mut index = self.index.write().unwrap();

            if let Some(new_meta) = meta {
                let old = index.sessions.get(id);
                let note_markdown = match note.clone() {
                    Some(note) => note,
                    None => old.and_then(|entry| entry.note_markdown.clone()),
                };
                let entry = SessionEntry {
                    meta: new_meta,
                    note_markdown,
                };
                if old != Some(&entry) {
                    index.sessions.insert(id.to_string(), entry);
                    changes.push((IndexEntity::Sessions, id.to_string()));
                }
            } else if let Some(Some(note)) = note {
                // Meta unreadable (kept as-is) but the note read fine: still refresh it.
                if let Some(entry) = index.sessions.get_mut(id) {
                    if entry.note_markdown.as_ref() != Some(&note) {
                        entry.note_markdown = Some(note);
                        changes.push((IndexEntity::Sessions, id.to_string()));
                    }
                }
            }

            if let Some(new_docs) = docs {
                if apply_map_value(&mut index.docs, id, new_docs) {
                    changes.push((IndexEntity::Docs, id.to_string()));
                }
            }
            if let Some(new_transcripts) = transcripts {
                if apply_map_value(&mut index.transcripts, id, new_transcripts) {
                    changes.push((IndexEntity::Transcripts, id.to_string()));
                }
            }
            if let Some(new_tasks) = tasks {
                if apply_map_value(&mut index.tasks, id, new_tasks) {
                    changes.push((IndexEntity::Tasks, id.to_string()));
                }
            }
        }
        self.notify_many(changes);

        Ok(first_error)
    }

    // -- filesystem reads (read-only; never writes to the vault) --

    /// Discovery-backed layout scan: `(id, physical directory)` pairs for every healthy
    /// session (identity from `_meta.json.id`, both legacy UUID-named and readable
    /// directories), plus the diagnostics rebuild needs -- formatted layout errors,
    /// the directories whose metadata is unreadable, the ids claimed by more than one
    /// directory, and ghost directories (session-like content with no metadata).
    pub(super) async fn scan_session_locations(&self) -> Result<SessionLayoutScan, StoreError> {
        let vault_base = self.vault_base.clone();
        tokio::task::spawn_blocking(move || -> Result<SessionLayoutScan, StoreError> {
            let discovery = hypr_vault_read::discover_sessions(&vault_base)?;

            let mut scan = SessionLayoutScan {
                locations: discovery
                    .sessions
                    .into_iter()
                    .map(|(location, _)| (location.id, location.relative_dir))
                    .collect(),
                ..Default::default()
            };
            for error in &discovery.errors {
                scan.errors.push(error.to_string());
                match error {
                    hypr_vault_read::SessionDiscoveryError::DuplicateId { id, .. } => {
                        scan.duplicate_ids.push(id.clone());
                    }
                    hypr_vault_read::SessionDiscoveryError::CorruptMeta { dir, .. }
                    | hypr_vault_read::SessionDiscoveryError::Unreadable { dir, .. } => {
                        scan.broken_dirs.push(dir.clone());
                    }
                }
            }

            scan.ghost_dirs = scan_ghost_dirs(&vault_base)?;
            Ok(scan)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// Lists every `<kind>.md` file in the session directory except `_memo.md`. Per-file read
    /// failures are carried in the inner `Result` (unparseable -> caller logs, leaves the
    /// entry alone); an `Err` here means the directory itself couldn't be listed, which the
    /// caller must not treat as "zero files" (that would look like every document vanished).
    pub(super) async fn scan_document_files(
        &self,
        id: &str,
    ) -> Result<Vec<(String, Result<String, StoreError>)>, StoreError> {
        let dir = self.vault_base.join(self.session_dir(id).await?);
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
    /// keeps the entry -- the doc id is still reported so pruning never mistakes
    /// "unparseable" for "gone"); an outer `Err` means the directory listing itself
    /// failed and the caller must not prune. A missing `enhanced/` dir is simply "no
    /// docs" -- most sessions never get one.
    pub(super) async fn scan_enhanced_doc_files(
        &self,
        id: &str,
    ) -> Result<Vec<(String, Result<super::EnhancedDoc, StoreError>)>, StoreError> {
        let dir = self
            .vault_base
            .join(paths::enhanced_dir_in(&self.session_dir(id).await?));
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
        let dir = self.vault_base.join(self.session_dir(id).await?);
        tokio::task::spawn_blocking(move || dir_has_session_content(&dir))
            .await
            .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }
}

/// Result of `scan_session_locations`: the healthy `(id, vault-relative dir)` pairs
/// plus everything rebuild must know about the parts of the layout that are not
/// healthy sessions.
#[derive(Debug, Default)]
pub(super) struct SessionLayoutScan {
    pub locations: Vec<(String, PathBuf)>,
    /// Formatted layout diagnostics (duplicate ids, corrupt/unreadable metadata),
    /// carrying physical paths; surfaced through `RebuildReport.errors`.
    pub errors: Vec<String>,
    /// Ids claimed by more than one directory -- ambiguous, never pruned or resolved.
    pub duplicate_ids: Vec<String>,
    /// Vault-relative directories whose `_meta.json` exists but cannot be read/parsed.
    pub broken_dirs: Vec<PathBuf>,
    /// Directories (relative to `sessions/`) holding recognized session content (a
    /// `<kind>.md` document or `transcript.json`) with no `_meta.json` at all -- left
    /// deliberately unindexed, files untouched.
    pub ghost_dirs: Vec<String>,
}

/// Walks `sessions/` the same way discovery does (skip hidden, never descend past a
/// `_meta.json`) and reports the meta-less directories that directly hold session-like
/// content. Root-level ghosts keep their historical bare-basename form; nested ones
/// carry their `sessions/`-relative path.
fn scan_ghost_dirs(vault_base: &std::path::Path) -> Result<Vec<String>, StoreError> {
    let root = vault_base.join(paths::sessions_root());
    let mut ghosts = Vec::new();
    let mut pending: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let entries = match std::fs::read_dir(root.join(&relative)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            // Unreadable directories are already reported as layout diagnostics by the
            // discovery pass; the ghost sweep just moves on.
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let child_rel = relative.join(&name);
            let child_abs = entry.path();
            if child_abs.join("_meta.json").exists() {
                continue; // a session (or corrupt session) directory, never a parent
            }
            if dir_has_session_content(&child_abs)? {
                // A ghost's own subdirectories (enhanced/, attachments/) are its
                // content, not further ghosts -- report the orphan once, don't
                // descend and double-count its interior.
                ghosts.push(child_rel.to_string_lossy().into_owned());
                continue;
            }
            pending.push(child_rel);
        }
    }
    ghosts.sort();
    Ok(ghosts)
}

fn dir_has_session_content(dir: &std::path::Path) -> Result<bool, StoreError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Ok(false),
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
        let is_transcript = path.file_name().and_then(|n| n.to_str()) == Some("transcript.json");
        if is_md || is_transcript {
            return Ok(true);
        }
    }
    Ok(false)
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
    use hypr_fs_format::TranscriptWithData;

    use super::*;
    use crate::content::SessionMeta;

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
            extra: Default::default(),
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
        let store = SessionStore::new(temp.path().to_path_buf());
        (store, temp)
    }

    /// Physical directory of a session: creation now picks a human-readable name, so
    /// tests resolve it through the store instead of assuming `sessions/<id>`.
    async fn session_path(
        store: &SessionStore,
        vault: &tempfile::TempDir,
        id: &str,
    ) -> std::path::PathBuf {
        vault.path().join(store.session_dir(id).await.unwrap())
    }

    /// A second store over the same vault, with a cold (empty) index -- the startup shape:
    /// everything it knows must come back from the files alone.
    fn cold_store(vault: &tempfile::TempDir) -> SessionStore {
        SessionStore::new(vault.path().to_path_buf())
    }

    /// Drains everything currently queued on the store's change bus. The bus is the
    /// observable that replaced the SQL dirty queue: a no-op rescan must leave it
    /// empty, a genuine change must land on it.
    fn drain_changes(store: &SessionStore) -> Vec<(crate::IndexEntity, Vec<String>)> {
        let mut rx = store
            .take_index_change_receiver()
            .expect("receiver taken once per drain");
        let mut changes = Vec::new();
        while let Ok(change) = rx.try_recv() {
            changes.push(change);
        }
        *store.index_changes_rx.lock().unwrap() = Some(rx);
        changes
    }

    /// The no-op-rescan guarantee (was: "unchanged rebuild must not requeue search
    /// reindexing" against the SQL dirty queue): rebuild_index runs
    /// automatically on every startup and window focus, so a rebuild over unchanged files
    /// must emit nothing on the change bus -- otherwise every boot/focus re-triggers the
    /// search projection and every subscribed webview, forever.
    #[tokio::test]
    async fn rebuild_of_unchanged_files_does_not_notify_the_change_bus() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "# hi").await.unwrap();
        store.rebuild_index().await.unwrap();
        drain_changes(&store);

        store.rebuild_index().await.unwrap();

        assert_eq!(drain_changes(&store), vec![]);
    }

    /// The mirror image of the no-op test above: the diff must never suppress a *genuine*
    /// change, only a spurious re-fire. Simulates an external edit with a raw
    /// `std::fs::write` (bypassing `write_meta` entirely, the way another device or a
    /// hand-edit would) and asserts both halves of "the reconcile actually reconciled":
    /// the index value changed, and the change bus saw the session.
    #[tokio::test]
    async fn rebuild_of_a_genuinely_changed_file_updates_the_index_and_notifies() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.rebuild_index().await.unwrap();
        drain_changes(&store);

        let meta_path = session_path(&store, &vault, "s1").await.join("_meta.json");
        let edited = serde_json::to_vec_pretty(&meta("s1", "Two")).unwrap();
        std::fs::write(&meta_path, edited).unwrap();

        store.rebuild_index().await.unwrap();

        assert_eq!(store.session_get("s1").unwrap().meta.title, "Two");
        let changes = drain_changes(&store);
        assert!(
            changes.iter().any(|(entity, ids)| {
                *entity == crate::IndexEntity::Sessions && ids.contains(&"s1".to_string())
            }),
            "a genuine change must still notify, not just no-ops getting skipped: {changes:?}"
        );
    }

    /// Reproduces the failure mode a real boot smoke turned up: a `_memo.md` that already
    /// carries a frontmatter wrapper (as an external edit or the retired legacy exporter
    /// would leave behind) must not have that wrapper compound with each
    /// automatic rebuild pass. Without `strip_leading_frontmatter` in the read path, each of
    /// these calls would index the *previous* pass's own wrapper verbatim, growing the indexed
    /// body by one nested frontmatter block every time -- exactly what the automatic
    /// startup/focus rescans would otherwise do to it on every boot.
    #[tokio::test]
    async fn rebuild_of_an_already_wrapped_note_file_does_not_grow_the_indexed_body() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        let dir = session_path(&store, &vault, "s1").await;
        std::fs::write(
            dir.join("_memo.md"),
            "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\nreal content",
        )
        .unwrap();

        store.rebuild_index().await.unwrap();
        assert_eq!(
            store.session_get("s1").unwrap().note_markdown.as_deref(),
            Some("real content")
        );

        store.rebuild_index().await.unwrap();
        store.rebuild_index().await.unwrap();
        assert_eq!(
            store.session_get("s1").unwrap().note_markdown.as_deref(),
            Some("real content")
        );
    }

    #[tokio::test]
    async fn rebuild_is_idempotent() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "# hi").await.unwrap();
        store.rebuild_index().await.unwrap();
        let first = store.index.read().unwrap().clone();
        store.rebuild_index().await.unwrap();
        assert_eq!(first, *store.index.read().unwrap());
    }

    /// The whole point of files-as-truth: a brand-new store (cold, empty index -- the
    /// startup shape) rebuilds everything from the vault alone.
    #[tokio::test]
    async fn rebuild_from_a_cold_index_restores_everything_from_files() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "notes").await.unwrap();
        store
            .write_transcript("s1", transcript("t1", "restored-word"))
            .await
            .unwrap();

        let cold = cold_store(&vault);
        assert!(cold.session_get("s1").is_none());
        cold.rebuild_index().await.unwrap();

        let record = cold.session_get("s1").unwrap();
        assert_eq!(record.meta.title, "One");
        assert_eq!(record.note_markdown.as_deref(), Some("notes"));
        assert_eq!(
            cold.transcript_get("t1").unwrap().words[0].text,
            "restored-word"
        );
    }

    #[tokio::test]
    async fn rebuild_restores_event_and_folder_from_files() {
        let (store, vault) = test_store().await;
        let mut m = meta("s1", "One");
        m.event = Some(serde_json::json!({"tracking_id": "evt-1", "meeting_link": ""}));
        m.folder = Some("work".to_string());
        store.write_meta(&m).await.unwrap();

        let cold = cold_store(&vault);
        cold.rebuild_index().await.unwrap();

        let restored = cold.session_get("s1").unwrap().meta;
        assert_eq!(restored.event, m.event);
        assert_eq!(restored.folder.as_deref(), Some("work"));
    }

    /// The no-op property must hold for the widened meta fields too: a meta whose `event`
    /// is populated re-derives to an identical entry on every rebuild pass, so an
    /// unchanged file must still stay silent on the bus.
    #[tokio::test]
    async fn rebuild_of_unchanged_event_and_folder_does_not_notify() {
        let (store, _vault) = test_store().await;
        let mut m = meta("s1", "One");
        m.event = Some(serde_json::json!({"tracking_id": "evt-1", "meeting_link": "x"}));
        m.folder = Some("work".to_string());
        store.write_meta(&m).await.unwrap();
        store.rebuild_index().await.unwrap();
        drain_changes(&store);

        store.rebuild_index().await.unwrap();

        assert_eq!(drain_changes(&store), vec![]);
    }

    #[tokio::test]
    async fn refresh_missing_meta_removes_index_entry_but_no_files() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "keep me").await.unwrap();
        let dir = session_path(&store, &vault, "s1").await;
        std::fs::remove_file(dir.join("_meta.json")).unwrap();
        store.refresh_session("s1").await.unwrap();
        assert!(store.session_get("s1").is_none());
        assert!(dir.join("_memo.md").is_file()); // vault untouched
    }

    #[tokio::test]
    async fn rebuild_unparseable_meta_leaves_existing_entry_and_logs_error() {
        let (store, vault) = test_store().await;
        // Seed a legacy uuid-style directory by hand: a corrupt meta can no longer name
        // its id, so the reported error identifies the session by its directory path.
        let dir = vault.path().join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            serde_json::to_vec_pretty(&meta("s1", "Original")).unwrap(),
        )
        .unwrap();
        store.rebuild_index().await.unwrap();
        std::fs::write(dir.join("_meta.json"), b"{ not json").unwrap();

        let report = store.rebuild_index().await.unwrap();

        assert_eq!(
            store.session_get("s1").unwrap().meta.title,
            "Original",
            "existing entry must survive a corrupt file"
        );
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("s1"));
    }

    #[tokio::test]
    async fn rebuild_removes_entries_for_vanished_folder_across_all_maps() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "notes").await.unwrap();
        store
            .write_document("s1", "summary", "doc body")
            .await
            .unwrap();
        store
            .write_transcript("s1", transcript("t1", "hi"))
            .await
            .unwrap();

        std::fs::remove_dir_all(session_path(&store, &vault, "s1").await).unwrap();

        store.rebuild_index().await.unwrap();

        assert!(store.session_get("s1").is_none());
        assert!(store.session_enhanced_docs("s1").is_empty());
        assert!(store.session_transcripts("s1").is_empty());
    }

    /// REGRESSION: `Path::exists()` swallows read failures as "false", which used to make
    /// a transiently-unreadable `_meta.json` look identical to a missing one and delete a
    /// live session's index entries. read_meta must distinguish "not found" from "exists
    /// but unreadable" and rebuild must treat the latter as an error, not a deletion.
    /// Unreadability is injected by replacing the file with a directory of the same name
    /// (EISDIR on read) -- unlike the previous chmod-0o000 injection, this fails for root
    /// too, so the test holds no matter which user runs it.
    #[tokio::test]
    async fn rebuild_unreadable_meta_leaves_existing_entry_and_logs_error() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "Original")).await.unwrap();

        let meta_path = session_path(&store, &vault, "s1").await.join("_meta.json");
        std::fs::remove_file(&meta_path).unwrap();
        std::fs::create_dir(&meta_path).unwrap();

        let report = store.rebuild_index().await.unwrap();

        assert_eq!(
            store.session_get("s1").unwrap().meta.title,
            "Original",
            "existing entry must survive a transiently-unreadable file, not just a corrupt one"
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
        let dir = session_path(&store, &vault, "s1").await;
        std::fs::write(
            dir.join("_memo.conflict-2026-07-23T12-00-00.123Z.md"),
            "stale conflict copy",
        )
        .unwrap();
        std::fs::write(dir.join(".tmp-1234-5678-_memo.md"), "crashed atomic write").unwrap();

        let report = store.rebuild_index().await.unwrap();

        assert_eq!(report.notes, 1, "only the live note should be indexed");
        assert_eq!(
            store.session_get("s1").unwrap().note_markdown.as_deref(),
            Some("live note")
        );
        let index = store.index.read().unwrap();
        assert!(
            !index.docs.contains_key("s1"),
            "neither leftover may become a document entry: {:?}",
            index.docs.get("s1")
        );
    }

    fn enhanced_doc(session_id: &str, doc_id: &str) -> crate::EnhancedDoc {
        crate::EnhancedDoc {
            id: doc_id.to_string(),
            session_id: session_id.to_string(),
            kind: "template_output".to_string(),
            title: "Customer review".to_string(),
            template_id: "template-1".to_string(),
            sort_order: 2,
            markdown: "# Review\n\n- Point".to_string(),
        }
    }

    /// The whole point of the file home: on a cold index, every metadata field
    /// (title/template_id/sort_order/kind) comes back from the frontmatter alone.
    #[tokio::test]
    async fn rebuild_restores_enhanced_doc_metadata_from_frontmatter() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        let cold = cold_store(&vault);
        cold.rebuild_index().await.unwrap();

        assert_eq!(
            cold.enhanced_doc_get("doc-1"),
            Some(enhanced_doc("s1", "doc-1"))
        );
    }

    /// The no-op property must hold for enhanced docs too: an unchanged
    /// `enhanced/<doc>.md` must stay silent on the bus across the automatic
    /// startup/focus rebuild passes.
    #[tokio::test]
    async fn rebuild_of_unchanged_enhanced_doc_does_not_notify() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();
        store.rebuild_index().await.unwrap();
        drain_changes(&store);

        store.rebuild_index().await.unwrap();

        assert_eq!(drain_changes(&store), vec![]);
    }

    #[tokio::test]
    async fn rebuild_prunes_enhanced_doc_entry_whose_file_is_gone() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        std::fs::remove_file(
            session_path(&store, &vault, "s1")
                .await
                .join("enhanced/doc-1.md"),
        )
        .unwrap();

        store.rebuild_index().await.unwrap();

        assert!(store.enhanced_doc_get("doc-1").is_none());
    }

    /// Corruption must never look like deletion: an `enhanced/<doc>.md` whose frontmatter
    /// no longer parses is logged, and its existing index entry survives the pass -- both
    /// the re-derive and the prune must respect it.
    #[tokio::test]
    async fn rebuild_unparseable_enhanced_doc_leaves_existing_entry_and_logs_error() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        std::fs::write(
            session_path(&store, &vault, "s1")
                .await
                .join("enhanced/doc-1.md"),
            "---\ntitle: [unclosed\n---\n\nbody",
        )
        .unwrap();

        let report = store.rebuild_index().await.unwrap();

        assert_eq!(
            store.enhanced_doc_get("doc-1").unwrap().title,
            "Customer review",
            "existing entry must survive a corrupt file"
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

    /// Pre-cutover UUID summary entries never had a file home, and the owner's
    /// no-migration directive means they never get one -- rebuild prunes them exactly as
    /// it did before this task (this test pins that preserved behavior rather than
    /// introducing it).
    #[tokio::test]
    async fn rebuild_still_prunes_legacy_index_only_uuid_entries_without_files() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_enhanced_doc(&enhanced_doc("s1", "doc-1"))
            .await
            .unwrap();

        {
            let mut index = store.index.write().unwrap();
            index
                .docs
                .entry("s1".to_string())
                .or_default()
                .push(crate::EnhancedDoc {
                    id: "legacy-uuid".to_string(),
                    session_id: "s1".to_string(),
                    kind: "summary".to_string(),
                    title: String::new(),
                    template_id: String::new(),
                    sort_order: 0,
                    markdown: "{}".to_string(),
                });
        }

        store.rebuild_index().await.unwrap();

        assert!(
            store.enhanced_doc_get("legacy-uuid").is_none(),
            "index-only entries without files stay pruned"
        );
        assert!(
            store.enhanced_doc_get("doc-1").is_some(),
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
        assert!(
            store.session_get("ghost").is_none(),
            "ghost sessions must not be indexed"
        );
    }
}
