use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SessionStore, StoreError, paths};

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
    pub tags: Vec<String>,
    /// Opaque calendar-event envelope (the sessions row's `event_json`). The store never
    /// inspects its interior -- it round-trips whatever JSON the frontend hands it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

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
        let meta_json =
            serde_json::to_vec_pretty(meta).map_err(|e| StoreError::Serialize(e.to_string()))?;

        self.write_file(paths::meta_path(&meta.id), meta_json)
            .await?;

        let pool = self.pool();
        let id = meta.id.clone();
        let title = meta.title.clone();
        let started_at = meta.started_at.as_deref().unwrap_or("");
        let ended_at = meta.ended_at.as_deref().unwrap_or("");
        let created_at = meta.created_at.clone();
        let event_json = event_json_column(meta);
        let folder_path = meta.folder.as_deref().unwrap_or("");

        sqlx::query(
            "INSERT INTO sessions (id, title, started_at, ended_at, created_at, event_json, folder_path, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               started_at = excluded.started_at,
               ended_at = excluded.ended_at,
               created_at = excluded.created_at,
               event_json = excluded.event_json,
               folder_path = excluded.folder_path,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(&id)
        .bind(&title)
        .bind(started_at)
        .bind(ended_at)
        .bind(&created_at)
        .bind(&event_json)
        .bind(folder_path)
        .execute(pool)
        .await
        .map_err(|e| StoreError::Db(e.to_string()))?;

        Ok(())
    }

    /// Read-modify-write partial update of `_meta.json`, then the same file-first dual-write
    /// as `write_meta`. Errors (rather than synthesizing a fresh meta) when the session has no
    /// `_meta.json`: every store-created session has one, and inventing one here would let a
    /// title edit racing a delete quietly resurrect the session folder.
    pub async fn update_meta(&self, id: &str, patch: SessionMetaPatch) -> Result<(), StoreError> {
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

        self.write_meta(&meta).await
    }

    pub async fn read_meta(&self, id: &str) -> Result<Option<SessionMeta>, StoreError> {
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
        let note_bytes = markdown.as_bytes().to_vec();
        self.write_file(paths::note_path(id), note_bytes).await?;

        let pool = self.pool();
        let id = id.to_string();
        let markdown = markdown.to_string();

        sqlx::query(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body, updated_at)
             VALUES (?, ?, 'note', 'md', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               body = excluded.body,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(format!("{}:note", &id))
        .bind(&id)
        .bind(&markdown)
        .execute(pool)
        .await
        .map_err(|e| StoreError::Db(e.to_string()))?;

        Ok(())
    }

    pub async fn read_note(&self, id: &str) -> Result<Option<String>, StoreError> {
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
        let doc_bytes = markdown.as_bytes().to_vec();
        self.write_file(paths::document_path(id, kind), doc_bytes)
            .await?;

        let pool = self.pool();
        let id = id.to_string();
        let kind = kind.to_string();
        let markdown = markdown.to_string();

        sqlx::query(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body, updated_at)
             VALUES (?, ?, ?, 'md', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
               body = excluded.body,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(format!("{}:{}", &id, &kind))
        .bind(&id)
        .bind(&kind)
        .bind(&markdown)
        .execute(pool)
        .await
        .map_err(|e| StoreError::Db(e.to_string()))?;

        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<(), StoreError> {
        let vault_base = self.vault_base.clone();
        let id_str = id.to_string();

        // Move folder to trash first (file operation)
        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            let session_path = vault_base.join(paths::session_dir(&id_str));
            hypr_fs_sync_core::export::move_to_trash(&vault_base, &session_path)
                .map_err(|e| StoreError::Io(format!("failed to move session to trash: {}", e)))?;
            Ok(())
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {}", e)))??;

        // Delete from database in a single transaction
        let pool = self.pool();
        let id = id.to_string();

        let mut tx = pool
            .begin()
            .await
            .map_err(|e| StoreError::Db(format!("failed to start transaction: {}", e)))?;

        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;

        sqlx::query("DELETE FROM session_documents WHERE session_id = ?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;

        sqlx::query("DELETE FROM transcripts WHERE session_id = ?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StoreError::Db(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StoreError::Db(format!("failed to commit transaction: {}", e)))?;

        Ok(())
    }

    /// Undoes a `delete_session` from earlier today: moves the folder back from
    /// `.trash/<today>/sessions/<id>` and reindexes it. Only looks at today's trash dir --
    /// this backs the undo-toast window, not a general-purpose recovery tool. `Ok(false)`
    /// (not an error) when there's nothing to restore, e.g. the toast window already lapsed
    /// past midnight or the session was never deleted.
    pub async fn restore_session(&self, id: &str) -> Result<bool, StoreError> {
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

/// `sessions.event_json` is `TEXT NOT NULL DEFAULT ''`, so an absent `event` maps to the
/// empty string (matching the `started_at`/`ended_at` `unwrap_or("")` convention), never NULL.
/// Serialization goes through `serde_json::Value::to_string`, which is deterministic for the
/// same value -- rebuild's change-guarded upsert depends on that to recognize an unchanged
/// file as a no-op.
pub(super) fn event_json_column(meta: &SessionMeta) -> String {
    meta.event
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_default()
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
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
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
        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(title, "Jury feedback");
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

        let (event_json, folder_path): (String, String) =
            sqlx::query_as("SELECT event_json, folder_path FROM sessions WHERE id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&event_json).unwrap(),
            m.event.clone().unwrap()
        );
        assert_eq!(folder_path, "work/standups");

        // Tags live in the file only at this layer -- the tag-table dual-write stays at the
        // frontend mutation site until Phase E.
        let raw = std::fs::read_to_string(vault.path().join("sessions/s1/_meta.json")).unwrap();
        assert!(raw.contains("planning"));
    }

    /// Old `_meta.json` files (written before `event`/`folder` existed) must keep
    /// deserializing -- the new fields default to absent, and the SQL mirror gets the
    /// schema-default empty strings, not an error.
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
        let (event_json, folder_path): (String, String) =
            sqlx::query_as("SELECT event_json, folder_path FROM sessions WHERE id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(event_json, "");
        assert_eq!(folder_path, "");
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

        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(title, "Renamed", "dual-write must reach the index");
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
        store
            .write_note("s1", "# Meeting notes\n\nDiscussed: X, Y, Z")
            .await
            .unwrap();
        assert!(vault.path().join("sessions/s1/_memo.md").is_file());
        let body: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id='s1:note'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(body, "# Meeting notes\n\nDiscussed: X, Y, Z");
        assert_eq!(
            store.read_note("s1").await.unwrap().unwrap(),
            "# Meeting notes\n\nDiscussed: X, Y, Z"
        );
    }

    #[tokio::test]
    async fn write_document_writes_file_and_index() {
        let (store, vault) = test_store().await;
        store
            .write_document("s1", "summary", "## Summary\n\nKey points: A, B, C")
            .await
            .unwrap();
        assert!(vault.path().join("sessions/s1/summary.md").is_file());
        let body: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id='s1:summary'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(body, "## Summary\n\nKey points: A, B, C");
        let doc_kind: String =
            sqlx::query_scalar("SELECT kind FROM session_documents WHERE id='s1:summary'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(doc_kind, "summary");
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

        // Seed a transcript row
        sqlx::query("INSERT INTO transcripts (id, session_id, words_json) VALUES (?, ?, ?)")
            .bind("t1")
            .bind("s1")
            .bind("[]")
            .execute(store.pool())
            .await
            .unwrap();

        assert!(vault.path().join("sessions/s1").is_dir());
        store.delete_session("s1").await.unwrap();

        assert!(!vault.path().join("sessions/s1").is_dir());

        let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(session_count, 0);

        let doc_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_documents WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(doc_count, 0);

        let transcript_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE session_id='s1'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(transcript_count, 0);

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
    async fn write_note_file_first_survives_index_failure() {
        let (store, vault) = test_store().await;
        // Drop session_documents table to force index failure
        sqlx::query("DROP TABLE session_documents")
            .execute(store.pool())
            .await
            .unwrap();

        // Call write_note; should return Err(StoreError::Db)
        let result = store.write_note("s1", "# Test note").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StoreError::Db(_)));

        // But the file should exist on disk with correct content
        let file_path = vault.path().join("sessions/s1/_memo.md");
        assert!(
            file_path.exists(),
            "file should exist despite index failure"
        );
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "# Test note");
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

        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(title, "Jury feedback");

        let body: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id='s1:note'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(body, "Some notes");
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
