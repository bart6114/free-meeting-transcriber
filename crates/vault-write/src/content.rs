use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SessionStore, StoreError, WriteGuard, paths, validate_session_id};

// The `_meta.json` schema is shared with the read-only vault consumers (fmtr CLI/MCP);
// the type lives in `hypr-vault-read` so both sides parse the same shape.
pub use hypr_vault_read::SessionMeta;

/// Partial update for `_meta.json`: `None` means "leave as-is", so callers can patch a single
/// field without knowing the rest. There is deliberately no way to clear a field back to
/// absent -- no mutation site needs that today.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, Default, PartialEq)]
pub struct SessionMetaPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

impl SessionStore {
    pub async fn write_meta(&self, meta: &SessionMeta) -> Result<(), StoreError> {
        validate_session_id(&meta.id)?;
        let guard = self.lock_writes().await;
        self.write_meta_locked(&guard, meta).await
    }

    async fn write_meta_locked(
        &self,
        guard: &WriteGuard<'_>,
        meta: &SessionMeta,
    ) -> Result<(), StoreError> {
        let meta_json =
            serde_json::to_vec_pretty(meta).map_err(|e| StoreError::Serialize(e.to_string()))?;

        self.write_file_locked(guard, paths::meta_path(&meta.id), meta_json)
            .await?;

        // Index write-through directly after the file write (file truth).
        self.index_upsert_meta(meta);
        self.notify_index_changed(super::IndexEntity::Sessions, vec![meta.id.clone()]);

        Ok(())
    }

    /// Read-modify-write partial update of `_meta.json`, then the same file-first dual-write
    /// as `write_meta`. Errors (rather than synthesizing a fresh meta) when the session has no
    /// `_meta.json`: every store-created session has one, and inventing one here would let a
    /// title edit racing a delete quietly resurrect the session folder.
    ///
    /// The write lock is held across the read *and* the write: two concurrent patches that
    /// both read the pre-patch meta would otherwise each write a whole file back, and the
    /// loser's fields would vanish.
    pub async fn update_meta(&self, id: &str, patch: SessionMetaPatch) -> Result<(), StoreError> {
        validate_session_id(id)?;
        let guard = self.lock_writes().await;

        let mut meta = self
            .read_meta(id)
            .await?
            .ok_or_else(|| StoreError::Io(format!("session {id} has no _meta.json to update")))?;

        let SessionMetaPatch {
            title,
            started_at,
            ended_at,
            created_at,
            tags,
            event,
            folder,
        } = patch;

        if let Some(title) = title {
            meta.title = title;
        }
        if let Some(started_at) = started_at {
            meta.started_at = Some(started_at);
        }
        if let Some(ended_at) = ended_at {
            meta.ended_at = Some(ended_at);
        }
        if let Some(created_at) = created_at {
            meta.created_at = created_at;
        }
        if let Some(tags) = tags {
            meta.tags = tags;
        }
        if let Some(event) = event {
            meta.event = Some(event);
        }
        if let Some(folder) = folder {
            meta.folder = Some(folder);
        }

        self.write_meta_locked(&guard, &meta).await
    }

    /// Recording lifecycle stamps for `_meta.json`, driven by capture start/stop events.
    /// `started_at` keeps the first recording's start -- a paused-and-resumed recording
    /// must not shift the session forward on the timeline -- while `ended_at` always
    /// advances to the latest stop, so together they span every take in the session.
    /// Errors on a missing `_meta.json` for the same reason as `update_meta`: a stamp
    /// racing a delete must not resurrect the session folder.
    pub async fn mark_recording_started(&self, id: &str, at: &str) -> Result<(), StoreError> {
        validate_session_id(id)?;
        let guard = self.lock_writes().await;

        let mut meta = self
            .read_meta(id)
            .await?
            .ok_or_else(|| StoreError::Io(format!("session {id} has no _meta.json to update")))?;

        if meta.started_at.is_some() {
            return Ok(());
        }
        meta.started_at = Some(at.to_string());
        self.write_meta_locked(&guard, &meta).await
    }

    pub async fn mark_recording_ended(&self, id: &str, at: &str) -> Result<(), StoreError> {
        validate_session_id(id)?;
        let guard = self.lock_writes().await;

        let mut meta = self
            .read_meta(id)
            .await?
            .ok_or_else(|| StoreError::Io(format!("session {id} has no _meta.json to update")))?;

        meta.ended_at = Some(at.to_string());
        self.write_meta_locked(&guard, &meta).await
    }

    pub async fn read_meta(&self, id: &str) -> Result<Option<SessionMeta>, StoreError> {
        validate_session_id(id)?;
        let vault_base = self.vault_base.clone();
        let id = id.to_string();

        let result =
            tokio::task::spawn_blocking(move || -> Result<Option<SessionMeta>, StoreError> {
                let path = vault_base.join(paths::meta_path(&id));

                // Attempt-then-match, not exists()-then-read: `Path::exists()` swallows
                // permission-denied/stat failures as `false`, which would misreport a
                // transiently-unreadable file as "no session" to callers like rebuild.
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => {
                        return Err(StoreError::Io(format!("failed to read meta file: {}", e)));
                    }
                };

                let meta: SessionMeta = serde_json::from_slice(&bytes).map_err(|e| {
                    StoreError::Serialize(format!("failed to deserialize meta: {}", e))
                })?;

                Ok(Some(meta))
            })
            .await
            .map_err(|e| StoreError::Io(format!("task join error: {}", e)))??;

        Ok(result)
    }

    pub async fn write_note(&self, id: &str, markdown: &str) -> Result<(), StoreError> {
        validate_session_id(id)?;
        let note_bytes = markdown.as_bytes().to_vec();
        self.write_file(paths::note_path(id), note_bytes).await?;

        // Store what `read_note` would return, not the raw bytes: a body that starts with an
        // exporter-shaped frontmatter block would otherwise sit un-stripped in the index and
        // change under the user on the next rescan.
        self.index_set_note(
            id,
            Some(super::strip_leading_frontmatter(markdown.to_string())),
        );
        self.notify_index_changed(super::IndexEntity::Sessions, vec![id.to_string()]);

        Ok(())
    }

    pub async fn read_note(&self, id: &str) -> Result<Option<String>, StoreError> {
        validate_session_id(id)?;
        let vault_base = self.vault_base.clone();
        let id = id.to_string();

        let result = tokio::task::spawn_blocking(move || -> Result<Option<String>, StoreError> {
            let path = vault_base.join(paths::note_path(&id));

            // Same attempt-then-match rationale as read_meta above.
            match std::fs::read_to_string(&path) {
                Ok(content) => Ok(Some(super::strip_leading_frontmatter(content))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(StoreError::Io(format!("failed to read note file: {}", e))),
            }
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))??;

        Ok(result)
    }

    pub async fn write_document(
        &self,
        id: &str,
        kind: &str,
        markdown: &str,
    ) -> Result<(), StoreError> {
        validate_session_id(id)?;
        let doc_bytes = markdown.as_bytes().to_vec();
        self.write_file(paths::document_path(id, kind), doc_bytes)
            .await?;

        self.index_upsert_doc(&super::index::legacy_doc(id, kind, markdown.to_string()));
        self.notify_index_changed(super::IndexEntity::Docs, vec![id.to_string()]);

        Ok(())
    }

    /// Moves the whole `sessions/<id>/` folder to trash (undo-able via `restore_session`).
    ///
    /// The id is validated first: `sessions/<id>` for an empty id resolves to `sessions/`
    /// itself, so an unguarded delete would trash the user's entire session tree in one
    /// call -- and `restore_session` could not put it back.
    pub async fn delete_session(&self, id: &str) -> Result<(), StoreError> {
        validate_session_id(id)?;

        let vault_base = self.vault_base.clone();
        let id_str = id.to_string();

        // Drop the session's live transcript buffer *before* trashing the folder, and keep
        // the `live` lock held across the trash. A debounced flush still holding words for
        // this session would otherwise fire afterwards, and `persist_transcript` ->
        // `write_file` -> `create_dir_all` would recreate `sessions/<id>/` -- resurrecting a
        // ghost session and, worse, making `restore_session` fail with ENOTEMPTY because the
        // destination it renames onto now exists. Any flusher that wakes up during the delete
        // blocks here, then finds no buffer and no-ops. (Recording into a session with no
        // `_meta.json` still persists, deliberately: this only drops buffers for a session
        // that was just deleted.)
        let mut live = self.live.lock().await;
        live.remove(id);

        // Move folder to trash first (file operation)
        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            let session_path = vault_base.join(paths::session_dir(&id_str));
            hypr_fs_sync_core::export::move_to_trash(&vault_base, &session_path)
                .map_err(|e| StoreError::Io(format!("failed to move session to trash: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))??;

        drop(live);

        // The folder is confirmed gone (trashed) -- clear every index map.
        self.index_remove_session_and_notify(id);

        Ok(())
    }

    /// Undoes a `delete_session` from earlier today: moves the folder back from
    /// `.trash/<today>/sessions/<id>` and reindexes it. Only looks at today's trash dir --
    /// this backs the undo-toast window, not a general-purpose recovery tool. `Ok(false)`
    /// (not an error) when there's nothing to restore, e.g. the toast window already lapsed
    /// past midnight or the session was never deleted.
    pub async fn restore_session(&self, id: &str) -> Result<bool, StoreError> {
        validate_session_id(id)?;
        let vault_base = self.vault_base.clone();
        let id_owned = id.to_string();

        let restored = tokio::task::spawn_blocking(move || -> Result<bool, StoreError> {
            let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let trash_sessions_dir = vault_base
                .join(".trash")
                .join(date)
                .join(paths::sessions_root());

            let Some(trashed_path) = latest_trashed_session_path(&trash_sessions_dir, &id_owned)?
            else {
                return Ok(false);
            };

            let restored_path = vault_base.join(paths::session_dir(&id_owned));
            if let Some(parent) = restored_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| StoreError::Io(format!("failed to create sessions dir: {}", e)))?;
            }
            std::fs::rename(&trashed_path, &restored_path).map_err(|e| {
                StoreError::Io(format!("failed to restore session from trash: {}", e))
            })?;
            Ok(true)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))??;

        if restored {
            self.refresh_session(id).await?;
        }

        Ok(restored)
    }
}

/// Finds the most-recently-trashed candidate for `id` under today's `.trash/<date>/sessions/`.
/// `move_to_trash`'s `unique_path` disambiguates same-day repeat trashing of the same id as
/// `<id>`, then `<id>-1`, `<id>-2`, ... in that chronological order (each new trash of the same
/// id picks the first free slot) -- so the highest existing suffix is the most recent deletion,
/// and undo should bring that one back, not the oldest. `None` when nothing matches (including
/// a missing trash dir, e.g. the toast window lapsed past midnight).
fn latest_trashed_session_path(
    trash_sessions_dir: &Path,
    id: &str,
) -> Result<Option<PathBuf>, StoreError> {
    let entries = match std::fs::read_dir(trash_sessions_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(StoreError::Io(format!(
                "failed to read trash sessions dir: {}",
                e
            )));
        }
    };

    let mut best: Option<(i64, PathBuf)> = None;
    for entry in entries {
        let entry =
            entry.map_err(|e| StoreError::Io(format!("failed to read dir entry: {}", e)))?;
        if !entry.path().is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };

        // Bare `id` is the oldest possible match (rank -1, below any real -N suffix); `<id>-N`
        // ranks as N.
        let rank: Option<i64> = if name == id {
            Some(-1)
        } else {
            name.strip_prefix(id)
                .and_then(|rest| rest.strip_prefix('-'))
                .and_then(|suffix| suffix.parse::<i64>().ok())
        };

        let Some(rank) = rank else { continue };
        let is_better = match &best {
            Some((best_rank, _)) => rank > *best_rank,
            None => true,
        };
        if is_better {
            best = Some((rank, entry.path()));
        }
    }

    Ok(best.map(|(_, path)| path))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let store = SessionStore::new(vault);
        (store, temp)
    }

    #[tokio::test]
    async fn write_meta_writes_file_and_index() {
        let (store, vault) = test_store().await;
        store
            .write_meta(&meta("s1", "Jury feedback"))
            .await
            .unwrap();
        assert!(vault.path().join("sessions/s1/_meta.json").is_file());
        assert_eq!(store.session_get("s1").unwrap().meta.title, "Jury feedback");
        assert_eq!(
            store.read_meta("s1").await.unwrap().unwrap().title,
            "Jury feedback"
        );
    }

    #[tokio::test]
    async fn write_meta_round_trips_event_folder_and_tags_through_file_and_index() {
        let (store, vault) = test_store().await;
        let mut m = meta("s1", "Sprint sync");
        m.event = Some(serde_json::json!({
            "tracking_id": "evt-1",
            "meeting_link": "https://example.com/x",
        }));
        m.folder = Some("work/standups".to_string());
        m.tags = vec!["planning".to_string(), "q3".to_string()];
        store.write_meta(&m).await.unwrap();

        assert_eq!(store.read_meta("s1").await.unwrap().unwrap(), m);

        let indexed = store.session_get("s1").unwrap().meta;
        assert_eq!(indexed.event, m.event);
        assert_eq!(indexed.folder.as_deref(), Some("work/standups"));

        let raw = std::fs::read_to_string(vault.path().join("sessions/s1/_meta.json")).unwrap();
        assert!(raw.contains("planning"));
    }

    /// Old `_meta.json` files (written before `event`/`folder` existed) must keep
    /// deserializing -- the new fields default to absent, not an error.
    #[tokio::test]
    async fn read_meta_accepts_pre_event_folder_files() {
        let (store, vault) = test_store().await;
        let dir = vault.path().join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            br#"{"id":"s1","title":"Old","started_at":null,"ended_at":null,"created_at":"2026-07-01T00:00:00Z","tags":[]}"#,
        )
        .unwrap();

        let m = store.read_meta("s1").await.unwrap().unwrap();
        assert_eq!(m.event, None);
        assert_eq!(m.folder, None);

        store.write_meta(&m).await.unwrap();
        let indexed = store.session_get("s1").unwrap().meta;
        assert_eq!(indexed.event, None);
        assert_eq!(indexed.folder, None);
    }

    #[tokio::test]
    async fn update_meta_patches_only_the_given_fields() {
        let (store, _vault) = test_store().await;
        let mut m = meta("s1", "Original");
        m.event = Some(serde_json::json!({"tracking_id": "evt-1"}));
        store.write_meta(&m).await.unwrap();

        store
            .update_meta(
                "s1",
                SessionMetaPatch {
                    title: Some("Renamed".to_string()),
                    tags: Some(vec!["kept".to_string()]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let after = store.read_meta("s1").await.unwrap().unwrap();
        assert_eq!(after.title, "Renamed");
        assert_eq!(after.tags, vec!["kept".to_string()]);
        assert_eq!(after.event, m.event, "unpatched fields must survive");
        assert_eq!(after.created_at, m.created_at);

        assert_eq!(
            store.session_get("s1").unwrap().meta.title,
            "Renamed",
            "write-through must reach the index"
        );
    }

    /// A patch against a session with no `_meta.json` must fail loudly instead of inventing
    /// one -- otherwise a title edit racing a delete would resurrect the session folder.
    #[tokio::test]
    async fn update_meta_errors_when_meta_file_is_missing() {
        let (store, vault) = test_store().await;
        let result = store
            .update_meta(
                "ghost",
                SessionMetaPatch {
                    title: Some("nope".to_string()),
                    ..Default::default()
                },
            )
            .await;
        assert!(result.is_err());
        assert!(!vault.path().join("sessions/ghost").exists());
    }

    #[tokio::test]
    async fn write_note_writes_file_and_index() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_note("s1", "# Meeting notes\n\nDiscussed: X, Y, Z")
            .await
            .unwrap();
        assert!(vault.path().join("sessions/s1/_memo.md").is_file());
        assert_eq!(
            store.session_get("s1").unwrap().note_markdown.as_deref(),
            Some("# Meeting notes\n\nDiscussed: X, Y, Z")
        );
        assert_eq!(
            store.read_note("s1").await.unwrap().unwrap(),
            "# Meeting notes\n\nDiscussed: X, Y, Z"
        );
    }

    #[tokio::test]
    async fn write_note_indexes_what_read_note_would_return() {
        // An exporter-shaped frontmatter block is stripped on read, so it must be stripped on
        // the write-through too -- otherwise the index and the file disagree until the next
        // rescan, at which point the displayed note silently changes under the user.
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store
            .write_note("s1", "---\nid: legacy-1\nposition: 0\n---\n\nReal body")
            .await
            .unwrap();

        let indexed = store.session_get("s1").unwrap().note_markdown;
        let on_read = store.read_note("s1").await.unwrap();
        assert_eq!(
            indexed, on_read,
            "index must hold exactly what read_note returns"
        );
        assert_eq!(indexed.as_deref(), Some("Real body"));
    }

    #[tokio::test]
    async fn write_document_writes_file_and_index() {
        let (store, vault) = test_store().await;
        store
            .write_document("s1", "summary", "## Summary\n\nKey points: A, B, C")
            .await
            .unwrap();
        assert!(vault.path().join("sessions/s1/summary.md").is_file());
        let doc = store.enhanced_doc_get("s1:summary").unwrap();
        assert_eq!(doc.markdown, "## Summary\n\nKey points: A, B, C");
        assert_eq!(doc.kind, "summary");
    }

    #[tokio::test]
    async fn delete_session_moves_folder_to_trash_and_clears_index() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "Test Session")).await.unwrap();
        store.write_note("s1", "Some notes").await.unwrap();
        store
            .write_document("s1", "summary", "Summary content")
            .await
            .unwrap();

        store
            .write_transcript(
                "s1",
                hypr_fs_format::TranscriptWithData {
                    id: "t1".to_string(),
                    user_id: String::new(),
                    created_at: "2026-07-24T00:00:00Z".to_string(),
                    session_id: "s1".to_string(),
                    started_at: 0.0,
                    ended_at: None,
                    memo_md: String::new(),
                    words: vec![],
                    speaker_hints: vec![],
                },
            )
            .await
            .unwrap();

        assert!(vault.path().join("sessions/s1").is_dir());
        store.delete_session("s1").await.unwrap();

        assert!(!vault.path().join("sessions/s1").is_dir());

        assert!(store.session_get("s1").is_none());
        assert!(store.session_enhanced_docs("s1").is_empty());
        assert!(store.session_transcripts("s1").is_empty());

        // Verify trashed content exists under .trash/
        let trash_root = vault.path().join(".trash");
        assert!(trash_root.exists(), "trash directory should exist");
        // Look for the moved session folder: .trash/<date>/sessions/s1/_meta.json
        let mut found_meta = false;
        for date_entry in std::fs::read_dir(&trash_root).unwrap() {
            let date_entry = date_entry.unwrap();
            let date_path = date_entry.path();
            if date_path.is_dir() {
                let sessions_path = date_path.join("sessions");
                if sessions_path.exists() {
                    let s1_path = sessions_path.join("s1");
                    if s1_path.exists() {
                        let meta_path = s1_path.join("_meta.json");
                        if meta_path.exists() {
                            found_meta = true;
                            break;
                        }
                    }
                }
            }
        }
        assert!(
            found_meta,
            "trashed session's _meta.json should exist under .trash/<date>/sessions/s1/"
        );
    }

    /// `sessions/<id>` for an empty id is `sessions/` itself, so an unguarded delete would
    /// move the user's ENTIRE session tree to trash in one call -- and `restore_session("")`
    /// could never bring it back, because it looks for `sessions/` *inside* the trashed
    /// sessions dir.
    #[tokio::test]
    async fn delete_session_with_an_empty_id_is_rejected_and_spares_the_whole_sessions_tree() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "Keep me")).await.unwrap();
        store.write_meta(&meta("s2", "Me too")).await.unwrap();

        assert!(store.delete_session("").await.is_err());

        assert!(vault.path().join("sessions/s1/_meta.json").is_file());
        assert!(vault.path().join("sessions/s2/_meta.json").is_file());
        assert!(!vault.path().join(".trash").exists());
        assert!(store.session_get("s1").is_some());
    }

    /// `Path::join` with an absolute path replaces rather than appends, so an absolute id
    /// escapes the vault entirely -- and `move_to_trash`'s `strip_prefix` fails open on it.
    #[tokio::test]
    async fn delete_session_with_an_absolute_id_is_rejected_and_touches_nothing_outside_the_vault()
    {
        let (store, vault) = test_store().await;
        let outside = vault.path().parent().unwrap().join("precious-documents");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("thesis.md"), b"years of work").unwrap();

        let result = store.delete_session(outside.to_str().unwrap()).await;

        assert!(result.is_err());
        assert!(outside.join("thesis.md").is_file());
    }

    #[tokio::test]
    async fn restore_session_rejects_unsafe_ids() {
        let (store, _vault) = test_store().await;
        for id in ["", "..", "/tmp"] {
            assert!(store.restore_session(id).await.is_err(), "{id:?}");
        }
    }

    /// REGRESSION (reviewer-found): a debounced transcript flush still in flight when a
    /// session is deleted used to call `persist_transcript` -> `write_file` ->
    /// `create_dir_all`, recreating `sessions/<id>/`. That resurrected a ghost session AND
    /// permanently broke undo, because `restore_session`'s rename onto the (now existing)
    /// destination fails with ENOTEMPTY.
    #[tokio::test]
    async fn delete_session_drops_the_live_buffer_so_a_pending_flush_cannot_resurrect_the_folder() {
        let (store, vault) = test_store().await;
        store
            .write_meta(&meta("s1", "Being deleted"))
            .await
            .unwrap();
        store
            .write_note("s1", "notes worth restoring")
            .await
            .unwrap();

        // A dirty buffer with a debounce timer already armed and never flushed.
        store
            .append_transcript(
                "s1",
                super::super::TranscriptDelta {
                    transcript_id: "t1".to_string(),
                    new_words: vec![hypr_fs_format::TranscriptWord {
                        id: Some("w0".to_string()),
                        text: "mid-recording".to_string(),
                        start_ms: 0.0,
                        end_ms: 0.0,
                        channel: 0.0,
                        speaker: None,
                        metadata: None,
                    }],
                    replaced_ids: vec![],
                    new_hints: vec![],
                    started_at_ms: 0.0,
                },
            )
            .await
            .unwrap();

        store.delete_session("s1").await.unwrap();
        assert!(!vault.path().join("sessions/s1").exists());

        // Let the armed debounce timer fire well past its deadline.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        assert!(
            !vault.path().join("sessions/s1").exists(),
            "a pending flush must not recreate the deleted session folder"
        );

        let restored = store.restore_session("s1").await.unwrap();
        assert!(restored, "undo-delete must still work");
        assert_eq!(
            std::fs::read_to_string(vault.path().join("sessions/s1/_memo.md")).unwrap(),
            "notes worth restoring"
        );
    }

    /// Two patches of different fields racing each other must both survive: the write lock
    /// spans the read *and* the write, so neither can compute its new whole-file value from
    /// bytes the other is about to replace.
    #[tokio::test]
    async fn concurrent_update_meta_calls_do_not_lose_each_others_fields() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "Original")).await.unwrap();
        let store = std::sync::Arc::new(store);

        let a = {
            let store = store.clone();
            async move {
                store
                    .update_meta(
                        "s1",
                        SessionMetaPatch {
                            title: Some("Renamed".to_string()),
                            ..Default::default()
                        },
                    )
                    .await
            }
        };
        let b = {
            let store = store.clone();
            async move {
                store
                    .update_meta(
                        "s1",
                        SessionMetaPatch {
                            tags: Some(vec!["tagged".to_string()]),
                            ..Default::default()
                        },
                    )
                    .await
            }
        };
        let (ra, rb) = tokio::join!(a, b);
        ra.unwrap();
        rb.unwrap();

        let after = store.read_meta("s1").await.unwrap().unwrap();
        assert_eq!(after.title, "Renamed");
        assert_eq!(after.tags, vec!["tagged".to_string()]);
    }

    /// REGRESSION: sessions are created with `started_at`/`ended_at` null and nothing on
    /// either side of the IPC boundary ever patched them, so every real recording kept
    /// null timestamps forever. A start/stop cycle must leave both stamped in the
    /// persisted file (not just the in-memory index).
    #[tokio::test]
    async fn recording_start_stop_cycle_persists_non_null_timestamps() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "Standup")).await.unwrap();

        store
            .mark_recording_started("s1", "2026-07-31T10:00:00.000Z")
            .await
            .unwrap();
        store
            .mark_recording_ended("s1", "2026-07-31T10:30:00.000Z")
            .await
            .unwrap();

        let raw: serde_json::Value = serde_json::from_slice(
            &std::fs::read(vault.path().join("sessions/s1/_meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw["started_at"], "2026-07-31T10:00:00.000Z");
        assert_eq!(raw["ended_at"], "2026-07-31T10:30:00.000Z");

        let indexed = store.session_get("s1").unwrap().meta;
        assert_eq!(
            indexed.started_at.as_deref(),
            Some("2026-07-31T10:00:00.000Z"),
            "write-through must reach the index"
        );
        assert_eq!(
            indexed.ended_at.as_deref(),
            Some("2026-07-31T10:30:00.000Z")
        );
    }

    #[tokio::test]
    async fn a_second_recording_keeps_the_first_start_and_advances_the_end() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "Standup")).await.unwrap();

        store
            .mark_recording_started("s1", "2026-07-31T10:00:00.000Z")
            .await
            .unwrap();
        store
            .mark_recording_ended("s1", "2026-07-31T10:30:00.000Z")
            .await
            .unwrap();
        store
            .mark_recording_started("s1", "2026-07-31T11:00:00.000Z")
            .await
            .unwrap();
        store
            .mark_recording_ended("s1", "2026-07-31T11:10:00.000Z")
            .await
            .unwrap();

        let after = store.read_meta("s1").await.unwrap().unwrap();
        assert_eq!(
            after.started_at.as_deref(),
            Some("2026-07-31T10:00:00.000Z")
        );
        assert_eq!(after.ended_at.as_deref(), Some("2026-07-31T11:10:00.000Z"));
    }

    /// Same rationale as `update_meta_errors_when_meta_file_is_missing`: a stamp racing a
    /// delete must fail loudly, not invent a meta and resurrect the session folder.
    #[tokio::test]
    async fn mark_recording_timestamps_error_when_meta_file_is_missing() {
        let (store, vault) = test_store().await;
        assert!(
            store
                .mark_recording_started("ghost", "2026-07-31T10:00:00.000Z")
                .await
                .is_err()
        );
        assert!(
            store
                .mark_recording_ended("ghost", "2026-07-31T10:30:00.000Z")
                .await
                .is_err()
        );
        assert!(!vault.path().join("sessions/ghost").exists());
    }

    #[tokio::test]
    async fn read_meta_returns_none_for_absent_file() {
        let (store, _vault) = test_store().await;
        assert_eq!(store.read_meta("nonexistent").await.unwrap(), None);
    }

    #[tokio::test]
    async fn read_note_returns_none_for_absent_file() {
        let (store, _vault) = test_store().await;
        assert_eq!(store.read_note("nonexistent").await.unwrap(), None);
    }

    /// `write_note` never writes frontmatter, but a file can gain a wrapper from outside it
    /// (an external edit, or -- before Task 13 removed it -- the legacy `vault_export`
    /// mirror, which always wrapped a `session_documents` row on export). `read_note` must strip
    /// a well-formed leading block rather than index it verbatim: otherwise `rebuild_index`,
    /// which Task 10 now runs on every startup and window focus, feeds the wrapper back into
    /// `session_documents.body`, and the next export wraps *that* in another layer -- one more
    /// nested frontmatter block per boot/focus, forever.
    #[tokio::test]
    async fn read_note_strips_a_leading_frontmatter_block() {
        let (store, vault) = test_store().await;
        let dir = vault.path().join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_memo.md"),
            "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\nreal content",
        )
        .unwrap();

        assert_eq!(
            store.read_note("s1").await.unwrap().unwrap(),
            "real content"
        );
    }

    /// The mirror image of the test above: content that merely *starts* with `---` but isn't
    /// a well-formed, closed frontmatter block (here, a legitimate note whose first line is a
    /// markdown horizontal rule) must never be mangled -- `read_note` only strips a block it
    /// can unambiguously parse.
    #[tokio::test]
    async fn read_note_leaves_unparseable_leading_dashes_untouched() {
        let (store, vault) = test_store().await;
        let dir = vault.path().join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        let content = "---\n\nActual note that opens with a horizontal rule.";
        std::fs::write(dir.join("_memo.md"), content).unwrap();

        assert_eq!(store.read_note("s1").await.unwrap().unwrap(), content);
    }

    #[tokio::test]
    async fn delete_session_on_nonexistent_session_succeeds() {
        let (store, _vault) = test_store().await;
        // delete_session on a session that doesn't exist should succeed
        // (trash no-ops since path doesn't exist, deletes affect 0 rows)
        let result = store.delete_session("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn restore_session_moves_folder_back_and_reindexes() {
        let (store, vault) = test_store().await;
        store
            .write_meta(&meta("s1", "Jury feedback"))
            .await
            .unwrap();
        store.write_note("s1", "Some notes").await.unwrap();

        store.delete_session("s1").await.unwrap();
        assert!(!vault.path().join("sessions/s1").is_dir());

        let restored = store.restore_session("s1").await.unwrap();
        assert!(restored);

        assert!(vault.path().join("sessions/s1/_meta.json").is_file());
        assert!(vault.path().join("sessions/s1/_memo.md").is_file());

        let record = store.session_get("s1").unwrap();
        assert_eq!(record.meta.title, "Jury feedback");
        assert_eq!(record.note_markdown.as_deref(), Some("Some notes"));
    }

    #[tokio::test]
    async fn restore_session_returns_false_when_nothing_was_trashed_today() {
        let (store, _vault) = test_store().await;
        let restored = store.restore_session("never-deleted").await.unwrap();
        assert!(!restored);
    }

    /// REGRESSION (reviewer-found): `move_to_trash`'s `unique_path` disambiguates a same-day
    /// repeat trash of the same id as `<id>`, `<id>-1`, ... -- restore must pick the *latest*
    /// (highest suffix) one, not just whichever bare `<id>` entry happens to exist.
    #[tokio::test]
    async fn restore_session_picks_the_most_recently_trashed_same_day_duplicate() {
        let (store, vault) = test_store().await;

        store
            .write_meta(&meta("s1", "First version"))
            .await
            .unwrap();
        store.write_note("s1", "first content").await.unwrap();
        store.delete_session("s1").await.unwrap();

        // Recreate under the same id and delete again the same day: move_to_trash finds the
        // .trash/<date>/sessions/s1 slot already taken and disambiguates to .../s1-1.
        store
            .write_meta(&meta("s1", "Second version"))
            .await
            .unwrap();
        store.write_note("s1", "second content").await.unwrap();
        store.delete_session("s1").await.unwrap();

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let trash_sessions_dir = vault.path().join(".trash").join(&date).join("sessions");
        assert!(trash_sessions_dir.join("s1").is_dir());
        assert!(trash_sessions_dir.join("s1-1").is_dir());

        let restored = store.restore_session("s1").await.unwrap();
        assert!(restored);

        let note = std::fs::read_to_string(vault.path().join("sessions/s1/_memo.md")).unwrap();
        assert_eq!(
            note, "second content",
            "restore must bring back the most recently deleted duplicate, not the oldest"
        );
        // The older duplicate is left alone in trash, not silently consumed or merged.
        assert!(trash_sessions_dir.join("s1").is_dir());
    }

    #[tokio::test]
    async fn read_meta_detects_corrupted_file() {
        let (store, vault) = test_store().await;
        // Write a valid meta first
        store.write_meta(&meta("s1", "Original")).await.unwrap();

        // Corrupt the file on disk
        let meta_path = vault.path().join("sessions/s1/_meta.json");
        std::fs::write(&meta_path, b"{ invalid json").unwrap();

        // read_meta should return Err(StoreError::Serialize)
        let result = store.read_meta("s1").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StoreError::Serialize(_)));
    }
}
