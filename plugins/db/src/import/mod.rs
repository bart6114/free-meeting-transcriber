mod calendars;
mod events;
mod legacy_vault;
mod templates;

use std::path::PathBuf;

use sqlx::SqlitePool;

/// Outcome of a single `sync_from_vault` pass: how many vault files were
/// newly imported / left unchanged / reconciled by the files-win conflict
/// rule, on top of the raw `migration_import_runs` counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub run_id: String,
    pub discovered_count: i64,
    pub imported_count: i64,
    pub matched_count: i64,
    pub skipped_count: i64,
    pub conflict_count: i64,
    /// Conflicts that were force-resolved in favor of the vault file
    /// (already folded into `imported_count` / subtracted from
    /// `conflict_count` above).
    pub reconciled_count: i64,
    /// Sessions soft-hidden (`sessions.deleted_at` set) because their
    /// `_meta.json` was found missing — see `import_paths`'s deletion
    /// semantics. Always 0 for `sync_from_vault`, which doesn't currently
    /// notice a session folder disappearing between startups (out of scope
    /// for Task 14 — see the report).
    pub deleted_count: i64,
}

/// Reconcile `app.db` from the vault's files. Idempotent and safe to call on
/// every startup: unchanged files (matching sha256 of the last successful
/// import) are skipped, new files are imported, and changed files are
/// re-imported with the vault file winning any content conflict. A fresh or
/// empty database (no prior run rows) imports everything — this is the
/// "delete app.db and relaunch" rebuild path.
pub async fn sync_from_vault(
    pool: &SqlitePool,
    vault_base: &std::path::Path,
) -> crate::Result<SyncReport> {
    let run_id = legacy_vault::import_legacy_vault(pool, vault_base, false).await?;
    let reconciled_count =
        legacy_vault::reconcile_vault_conflicts(pool, vault_base, &run_id).await?;

    let (discovered_count, imported_count, matched_count, skipped_count, conflict_count): (
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = sqlx::query_as(
        "SELECT discovered_count, imported_count, matched_count, skipped_count, conflict_count
         FROM migration_import_runs
         WHERE id = ?",
    )
    .bind(&run_id)
    .fetch_one(pool)
    .await?;

    Ok(SyncReport {
        run_id,
        discovered_count,
        imported_count,
        matched_count,
        skipped_count,
        conflict_count,
        reconciled_count,
        deleted_count: 0,
    })
}

/// Imports (or soft-hides, on deletion) an explicit list of vault paths into
/// `app.db` — the watcher's entry point (Task 14). Reuses exactly the same
/// classification (`classify_source`), row application
/// (`hypr_db_app::apply_legacy_import_item`) and files-win conflict
/// resolution (`reconcile_vault_conflicts`) as `sync_from_vault`, just scoped
/// to the given paths instead of a full recursive directory walk — a live
/// edit to one note shouldn't pay for rescanning the whole vault.
///
/// # Loop-prevention (third link)
///
/// Two mechanisms already stop an export write from being re-imported here:
/// `plugins/notify` marks own-writes before they happen and filters them out
/// of `FileChanged` before it's even emitted, and a byte-identical export
/// write is a no-op that never touches the filesystem or fires an event at
/// all (see `vault_export.rs`'s module doc). This function adds a third,
/// independent check: before parsing a changed path at all, if its current
/// sha256 matches the hash of the last successful import of that exact path
/// (`hypr_db_app::legacy_source_already_imported`), it's treated as
/// unchanged and skipped — belt-and-braces against either upstream mechanism
/// somehow missing.
///
/// # Deletion semantics
///
/// Only a missing `_meta.json` — a session's canonical existence marker — is
/// acted on: the session row is soft-hidden (`sessions.deleted_at`), which
/// `list_sessions` already filters out and which the Task 13 export worker
/// already treats as "trash this folder" (tolerant of the folder already
/// being gone). Every other now-missing vault file (a document, a
/// transcript, an attachment, ...) is logged and otherwise ignored: there's
/// no cheap, unambiguous way to map a gone file back to which DB row it was
/// without risking a wrong guess, and this task didn't ask for new granular
/// per-document delete UX.
/// Reads a file's bytes on tokio's blocking thread pool rather than the
/// shared async runtime — the same whole-branch-review fix Task 13 applied
/// to `vault_export.rs`'s filesystem writes (`spawn_blocking!` there): a
/// synchronous `std::fs::read` run directly on an async task blocks
/// whichever runtime worker thread picked it up, which is also servicing
/// the rest of the app's async work (including this same DB pool's other
/// connections) for as long as the read takes. A panic inside the blocking
/// closure surfaces as an `io::Error` (via `JoinError`'s `Display`) instead
/// of silently killing the calling task.
async fn read_file_off_runtime(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    let path = path.to_path_buf();
    match tokio::task::spawn_blocking(move || std::fs::read(&path)).await {
        Ok(result) => result,
        Err(join_error) => Err(std::io::Error::other(join_error.to_string())),
    }
}

pub async fn import_paths(
    pool: &SqlitePool,
    vault_base: &std::path::Path,
    paths: &[PathBuf],
) -> crate::Result<SyncReport> {
    let run_id = uuid::Uuid::new_v4().to_string();
    hypr_db_app::begin_legacy_import_run(pool, &run_id, &vault_base.to_string_lossy(), false)
        .await?;

    let mut deleted_count = 0_i64;

    for path in paths {
        let relative_path = legacy_vault::normalized_relative_path(vault_base, path);
        let Some(kind) = legacy_vault::classify_source(&relative_path) else {
            continue;
        };

        match read_file_off_runtime(path).await {
            Ok(bytes) => {
                let source_sha256 = legacy_vault::sha256(&bytes);
                // Bypass the hash short-circuit for a `_meta.json` whose
                // session is currently soft-deleted — see
                // `session_needs_revival_bypass`'s doc for why: a
                // byte-identical reappearance (the dominant real-world
                // case) would otherwise skip straight past
                // `revive_soft_deleted_session` and wedge the session
                // hidden forever.
                let needs_revival_bypass =
                    legacy_vault::session_needs_revival_bypass(pool, vault_base, path, kind)
                        .await?;

                if !needs_revival_bypass
                    && hypr_db_app::legacy_source_already_imported(
                        pool,
                        &relative_path,
                        &source_sha256,
                    )
                    .await?
                {
                    let item_id = uuid::Uuid::new_v4().to_string();
                    hypr_db_app::record_legacy_import_unchanged(
                        pool,
                        hypr_db_app::LegacyImportItem {
                            id: &item_id,
                            run_id: &run_id,
                            source_path: &relative_path,
                            source_kind: kind.as_str(),
                            source_sha256: &source_sha256,
                        },
                    )
                    .await?;
                    continue;
                }

                let source = legacy_vault::SourceFile {
                    path: path.clone(),
                    relative_path: relative_path.clone(),
                    kind,
                };
                let item_id = uuid::Uuid::new_v4().to_string();
                let item = hypr_db_app::LegacyImportItem {
                    id: &item_id,
                    run_id: &run_id,
                    source_path: &relative_path,
                    source_kind: kind.as_str(),
                    source_sha256: &source_sha256,
                };

                match legacy_vault::parse_source(vault_base, &source, &bytes, &source_sha256) {
                    Ok(batch) => {
                        hypr_db_app::apply_legacy_import_item(pool, item, &batch, false).await?;
                    }
                    Err(error) => {
                        hypr_db_app::record_legacy_import_error(pool, item, &error).await?;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if matches!(kind, legacy_vault::SourceKind::SessionMeta) {
                    if let Ok((session_id, _folder_path)) =
                        legacy_vault::infer_session_id_and_folder(vault_base, path)
                    {
                        // Stamp the external-soft-hide marker into
                        // metadata_json (preserving whatever else is
                        // already there) so the vault export worker knows
                        // NOT to trash this session's remaining files —
                        // the external actor (a sync client, or the user
                        // directly) owns them, and this may just be a
                        // transient blip. Cleared again on revival by
                        // `hypr_db_app::revive_soft_deleted_session`.
                        let existing_metadata_json: Option<String> = sqlx::query_scalar(
                            "SELECT metadata_json FROM sessions WHERE id = ? AND deleted_at IS NULL",
                        )
                        .bind(&session_id)
                        .fetch_optional(pool)
                        .await?;

                        if let Some(existing_metadata_json) = existing_metadata_json {
                            let flagged_metadata_json = hypr_db_app::set_external_soft_hide_flag(
                                &existing_metadata_json,
                                true,
                            );

                            let result = sqlx::query(
                                "UPDATE sessions
                                 SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                                     metadata_json = ?
                                 WHERE id = ? AND deleted_at IS NULL",
                            )
                            .bind(&flagged_metadata_json)
                            .bind(&session_id)
                            .execute(pool)
                            .await?;

                            if result.rows_affected() > 0 {
                                deleted_count += 1;
                                tracing::info!(
                                    session_id,
                                    "vault watch: session folder removed externally; soft-hid the session"
                                );
                            }
                        }
                    }
                } else {
                    tracing::info!(
                        path = %relative_path,
                        kind = kind.as_str(),
                        "vault watch: externally deleted vault file has no soft-hide mapping at this granularity; ignoring"
                    );
                }
            }
            Err(error) => {
                let item_id = uuid::Uuid::new_v4().to_string();
                hypr_db_app::record_legacy_import_error(
                    pool,
                    hypr_db_app::LegacyImportItem {
                        id: &item_id,
                        run_id: &run_id,
                        source_path: &relative_path,
                        source_kind: kind.as_str(),
                        source_sha256: "",
                    },
                    &error.to_string(),
                )
                .await?;
            }
        }
    }

    hypr_db_app::finish_legacy_import_run(pool, &run_id).await?;
    let reconciled_count =
        legacy_vault::reconcile_vault_conflicts(pool, vault_base, &run_id).await?;

    let (discovered_count, imported_count, matched_count, skipped_count, conflict_count): (
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = sqlx::query_as(
        "SELECT discovered_count, imported_count, matched_count, skipped_count, conflict_count
         FROM migration_import_runs
         WHERE id = ?",
    )
    .bind(&run_id)
    .fetch_one(pool)
    .await?;

    Ok(SyncReport {
        run_id,
        discovered_count,
        imported_count,
        matched_count,
        skipped_count,
        conflict_count,
        reconciled_count,
        deleted_count,
    })
}

pub async fn import_legacy_data<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
) -> crate::Result<()> {
    let vault_base = resolve_startup_vault_base(app)?;

    match sync_from_vault(pool, &vault_base).await {
        Ok(_report) => Ok(()),
        Err(crate::Error::Io(error)) => {
            tracing::warn!(
                %error,
                "vault reconcile could not read its source files; continuing with recovery copies intact"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub(crate) async fn legacy_migration_verified(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM storage_migration_state
           WHERE id = 'legacy_v1'
             AND importer_version = ?
             AND parity_verified = 1
         )",
    )
    .bind(hypr_db_app::LEGACY_IMPORTER_VERSION)
    .fetch_one(pool)
    .await
}

pub async fn get_legacy_import_report(
    pool: &SqlitePool,
) -> crate::Result<crate::LegacyImportReport> {
    let state = sqlx::query_as::<_, crate::StorageMigrationState>(
        "SELECT phase, latest_run_id, parity_verified, cutover_at, rollback_until, last_error, updated_at
         FROM storage_migration_state
         WHERE id = 'legacy_v1'",
    )
    .fetch_one(pool)
    .await?;

    let latest_run = if state.latest_run_id.is_empty() {
        None
    } else {
        sqlx::query_as::<_, crate::LegacyImportRun>(
            "SELECT id, importer_version, source_root, dry_run, status, discovered_count,
                    imported_count, matched_count, skipped_count, conflict_count, error_count, started_at,
                    completed_at, error
             FROM migration_import_runs
             WHERE id = ?",
        )
        .bind(&state.latest_run_id)
        .fetch_optional(pool)
        .await?
    };

    let items = if state.latest_run_id.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, crate::LegacyImportItemReport>(
            "SELECT source_path, source_kind, source_sha256, status, discovered_count,
                    imported_count, matched_count, skipped_count, conflict_count, error
             FROM migration_import_items
             WHERE run_id = ?
             ORDER BY source_path",
        )
        .bind(&state.latest_run_id)
        .fetch_all(pool)
        .await?
    };

    let targets = if state.latest_run_id.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, crate::LegacyImportTargetReport>(
            "SELECT source_path, table_name, target_id, status, error
             FROM migration_import_targets
             WHERE run_id = ?
             ORDER BY table_name, target_id, source_path",
        )
        .bind(&state.latest_run_id)
        .fetch_all(pool)
        .await?
    };

    Ok(crate::LegacyImportReport {
        state,
        latest_run,
        items,
        targets,
    })
}

fn resolve_startup_vault_base<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::Result<PathBuf> {
    let bundle_id: &str = app.config().identifier.as_ref();
    let settings_base = hypr_storage::global::compute_default_base(bundle_id)
        .ok_or(std::io::Error::other("settings base unavailable"))?;
    std::fs::create_dir_all(&settings_base)?;

    Ok(hypr_storage::vault::resolve_base(
        &settings_base,
        &settings_base,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migration_verification_uses_current_importer_version() {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();

        assert!(!legacy_migration_verified(db.pool()).await.unwrap());

        sqlx::query(
            "UPDATE storage_migration_state
             SET importer_version = ?, parity_verified = 1
             WHERE id = 'legacy_v1'",
        )
        .bind(hypr_db_app::LEGACY_IMPORTER_VERSION)
        .execute(db.pool())
        .await
        .unwrap();

        assert!(legacy_migration_verified(db.pool()).await.unwrap());

        sqlx::query(
            "UPDATE storage_migration_state
             SET importer_version = importer_version - 1
             WHERE id = 'legacy_v1'",
        )
        .execute(db.pool())
        .await
        .unwrap();

        assert!(!legacy_migration_verified(db.pool()).await.unwrap());
    }

    #[tokio::test]
    async fn explicit_retry_normalizes_legacy_conflict_only_run() {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let sqlite_document = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"SQLite note"}]}]}"#;
        let sqlite_words =
            r#"[{"id":"sqlite-word","text":"SQLite words","start_ms":0,"end_ms":10,"channel":0}]"#;
        sqlx::query(
            "INSERT INTO sessions
             (id, owner_user_id, title, created_at, started_at, ended_at, event_id,
              external_event_id, external_provider, series_id, event_json, folder_path)
             VALUES ('session-1', 'user-1', 'Planning', '2026-07-10T01:00:00Z',
                     '', '', '', '', '', '', '', '')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, body_format, body, created_at, updated_at)
             VALUES ('note-1', 'session-1', 'note', 'prosemirror_json', ?,
                     '2026-07-10T01:00:00Z', '2026-07-10T02:00:00Z')",
        )
        .bind(sqlite_document)
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts
             (id, owner_user_id, session_id, started_at_ms, memo, words_json,
              speaker_hints_json, created_at)
             VALUES ('transcript-1', 'user-1', 'session-1', 0, 'SQLite memo', ?,
                     '[]', '2026-07-10T01:00:00Z')",
        )
        .bind(sqlite_words)
        .execute(db.pool())
        .await
        .unwrap();

        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta = br#"{"id":"session-1","user_id":"user-1","created_at":"2026-07-10T01:00:00Z","title":"Planning"}"#;
        let note = b"---\nid: note-1\nsession_id: session-1\n---\n\nLegacy note";
        let transcript = br#"{"transcripts":[{"id":"transcript-1","user_id":"user-1","session_id":"session-1","created_at":"2026-07-10T01:00:00Z","started_at":0,"memo_md":"Legacy memo","words":[{"text":"Legacy words","start_ms":0,"end_ms":10,"channel":0}],"speaker_hints":[]}] }"#;
        std::fs::write(session_dir.join("_meta.json"), meta).unwrap();
        std::fs::write(session_dir.join("_memo.md"), note).unwrap();
        std::fs::write(session_dir.join("transcript.json"), transcript).unwrap();

        let legacy_run_id = legacy_vault::import_legacy_vault(db.pool(), vault.path(), false)
            .await
            .unwrap();
        let legacy_run: (String, i64, i64, i64) = sqlx::query_as(
            "SELECT status, conflict_count, skipped_count, error_count
             FROM migration_import_runs WHERE id = ?",
        )
        .bind(&legacy_run_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(
            legacy_run,
            ("completed_with_conflicts".to_string(), 2, 0, 0)
        );
        sqlx::query(
            "UPDATE migration_import_runs
             SET status = 'completed_with_issues'
             WHERE id = ?",
        )
        .bind(&legacy_run_id)
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE storage_migration_state
             SET last_error = 'completed_with_issues'
             WHERE id = 'legacy_v1' AND latest_run_id = ?",
        )
        .bind(&legacy_run_id)
        .execute(db.pool())
        .await
        .unwrap();

        assert!(!legacy_migration_verified(db.pool()).await.unwrap());
        let recovery_run_id = legacy_vault::import_legacy_vault(db.pool(), vault.path(), false)
            .await
            .unwrap();

        let recovery_run: (String, i64, i64, i64) = sqlx::query_as(
            "SELECT status, conflict_count, skipped_count, error_count
             FROM migration_import_runs WHERE id = ?",
        )
        .bind(&recovery_run_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(
            recovery_run,
            ("completed_with_conflicts".to_string(), 2, 0, 0)
        );
        let recovery_run_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM migration_import_runs
             WHERE status = 'completed_with_conflicts'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(recovery_run_count, 1);
        let total_run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM migration_import_runs")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(total_run_count, 2);

        let document_body: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id = 'note-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        let transcript_data: (String, String) =
            sqlx::query_as("SELECT memo, words_json FROM transcripts WHERE id = 'transcript-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(document_body, sqlite_document);
        assert_eq!(
            transcript_data,
            ("SQLite memo".to_string(), sqlite_words.to_string())
        );
        assert_eq!(std::fs::read(session_dir.join("_meta.json")).unwrap(), meta);
        assert_eq!(std::fs::read(session_dir.join("_memo.md")).unwrap(), note);
        assert_eq!(
            std::fs::read(session_dir.join("transcript.json")).unwrap(),
            transcript
        );

        assert!(!legacy_migration_verified(db.pool()).await.unwrap());
    }

    #[tokio::test]
    async fn orphaned_session_children_remain_unverified_for_explicit_retry() {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/missing-session");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("_memo.md"),
            "---\nid: orphan-note\nsession_id: missing-session\n---\n\nOrphaned note",
        )
        .unwrap();

        let run_id = legacy_vault::import_legacy_vault(db.pool(), vault.path(), false)
            .await
            .unwrap();
        let run: (String, i64, i64, i64) = sqlx::query_as(
            "SELECT status, skipped_count, conflict_count, error_count
             FROM migration_import_runs WHERE id = ?",
        )
        .bind(&run_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        let target_status: String = sqlx::query_scalar(
            "SELECT status FROM migration_import_targets
             WHERE run_id = ? AND target_id = 'orphan-note'",
        )
        .bind(&run_id)
        .fetch_one(db.pool())
        .await
        .unwrap();

        assert_eq!(run, ("completed_with_issues".to_string(), 1, 0, 1));
        assert_eq!(target_status, "missing_dependency");
        assert!(!legacy_migration_verified(db.pool()).await.unwrap());
    }

    #[tokio::test]
    async fn document_and_transcript_conflicts_preserve_both_stores() {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let sqlite_document = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"SQLite note"}]}]}"#;
        let sqlite_words =
            r#"[{"id":"sqlite-word","text":"SQLite words","start_ms":0,"end_ms":10,"channel":0}]"#;
        sqlx::query(
            "INSERT INTO sessions
             (id, owner_user_id, title, created_at, started_at, ended_at, event_id,
              external_event_id, external_provider, series_id, event_json, folder_path)
             VALUES ('session-1', 'user-1', 'Planning', '2026-07-10T01:00:00Z',
                     '', '', '', '', '', '', '', '')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, body_format, body, created_at, updated_at)
             VALUES ('note-1', 'session-1', 'note', 'prosemirror_json',
                     ?,
                     '2026-07-10T01:00:00Z', '2026-07-10T02:00:00Z')",
        )
        .bind(sqlite_document)
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts
             (id, owner_user_id, session_id, started_at_ms, memo, words_json,
              speaker_hints_json, created_at)
             VALUES ('transcript-1', 'user-1', 'session-1', 0, 'SQLite memo',
                     ?,
                     '[]', '2026-07-10T01:00:00Z')",
        )
        .bind(sqlite_words)
        .execute(db.pool())
        .await
        .unwrap();

        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta = br#"{"id":"session-1","user_id":"user-1","created_at":"2026-07-10T01:00:00Z","title":"Planning"}"#;
        let note = b"---\nid: note-1\nsession_id: session-1\n---\n\nLegacy note";
        let transcript = br#"{"transcripts":[{"id":"transcript-1","user_id":"user-1","session_id":"session-1","created_at":"2026-07-10T01:00:00Z","started_at":0,"memo_md":"Legacy memo","words":[{"text":"Legacy words","start_ms":0,"end_ms":10,"channel":0}],"speaker_hints":[]}]}"#;
        std::fs::write(session_dir.join("_meta.json"), meta).unwrap();
        std::fs::write(session_dir.join("_memo.md"), note).unwrap();
        std::fs::write(session_dir.join("transcript.json"), transcript).unwrap();

        let run_id = legacy_vault::import_legacy_vault(db.pool(), vault.path(), false)
            .await
            .unwrap();
        let run: (String, i64, i64, i64) = sqlx::query_as(
            "SELECT status, conflict_count, skipped_count, error_count
             FROM migration_import_runs WHERE id = ?",
        )
        .bind(&run_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(run, ("completed_with_conflicts".to_string(), 2, 0, 0));

        let targets: Vec<(String, String)> = sqlx::query_as(
            "SELECT table_name, status FROM migration_import_targets
             WHERE run_id = ? ORDER BY table_name",
        )
        .bind(&run_id)
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(
            targets,
            vec![
                ("session_documents".to_string(), "conflict".to_string()),
                ("sessions".to_string(), "matched".to_string()),
                ("transcripts".to_string(), "conflict".to_string()),
            ]
        );

        let document_body: String =
            sqlx::query_scalar("SELECT body FROM session_documents WHERE id = 'note-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        let transcript_memo: String =
            sqlx::query_scalar("SELECT memo FROM transcripts WHERE id = 'transcript-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        let transcript_words: String =
            sqlx::query_scalar("SELECT words_json FROM transcripts WHERE id = 'transcript-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(document_body, sqlite_document);
        assert_eq!(transcript_memo, "SQLite memo");
        assert_eq!(transcript_words, sqlite_words);
        assert_eq!(std::fs::read(session_dir.join("_meta.json")).unwrap(), meta);
        assert_eq!(std::fs::read(session_dir.join("_memo.md")).unwrap(), note);
        assert_eq!(
            std::fs::read(session_dir.join("transcript.json")).unwrap(),
            transcript
        );

        let parity_verified: bool = sqlx::query_scalar(
            "SELECT parity_verified FROM storage_migration_state WHERE id = 'legacy_v1'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(!parity_verified);
        assert!(!legacy_migration_verified(db.pool()).await.unwrap());
    }

    #[tokio::test]
    #[ignore = "exercises legacy vault machinery removed in Task 13; backing tables dropped in Task 3"]
    async fn stale_snapshots_for_preexisting_sqlite_domains_do_not_block_cutover() {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars \
             (id, tracking_id_calendar, name, enabled, provider, source, color, connection_id) \
             VALUES ('calendar-1', 'tracking-1', 'Work', 0, 'google', 'work@example.com', '#123456', 'connection-1')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO events \
             (id, tracking_id_event, calendar_id, title, started_at, ended_at, location, \
              meeting_link, description, note, recurrence_series_id, has_recurrence_rules, \
              is_all_day, provider, participants_json) \
             VALUES ('event-1', 'tracking-event-1', 'calendar-1', 'Updated title', \
                     '2026-07-11T10:00:00Z', '2026-07-11T11:00:00Z', '', '', '', '', '', 0, 0, \
                     'google', '[]')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let vault = tempfile::tempdir().unwrap();
        std::fs::write(
            vault.path().join("calendars.json"),
            r##"{
              "calendar-1": {
                "tracking_id_calendar": "tracking-1",
                "name": "Work",
                "enabled": true,
                "provider": "google",
                "source": "work@example.com",
                "color": "#123456",
                "connection_id": "connection-1"
              }
            }"##,
        )
        .unwrap();
        std::fs::write(
            vault.path().join("events.json"),
            r#"{
              "event-1": {
                "tracking_id_event": "tracking-event-1",
                "calendar_id": "calendar-1",
                "title": "Stale title",
                "started_at": "2026-07-11T09:00:00Z",
                "ended_at": "2026-07-11T10:00:00Z",
                "provider": "google",
                "participants": []
              }
            }"#,
        )
        .unwrap();

        let run_id = legacy_vault::import_legacy_vault(db.pool(), vault.path(), false)
            .await
            .unwrap();

        assert!(legacy_migration_verified(db.pool()).await.unwrap());
        let target_statuses: Vec<String> = sqlx::query_scalar(
            "SELECT status FROM migration_import_targets WHERE run_id = ? ORDER BY target_id",
        )
        .bind(&run_id)
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(
            target_statuses,
            vec!["retained_existing", "retained_existing"]
        );

        let calendar_enabled: bool =
            sqlx::query_scalar("SELECT enabled FROM calendars WHERE id = 'calendar-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        let event_title: String =
            sqlx::query_scalar("SELECT title FROM events WHERE id = 'event-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert!(!calendar_enabled);
        assert_eq!(event_title, "Updated title");
    }

    /// `app.db` is a disposable cache: unlike the old run-once gate, every
    /// startup reconciles from the vault, so an edit made to a vault file
    /// while the app was closed is picked up on the next open — not frozen
    /// in place by whatever was imported first.
    #[tokio::test]
    async fn sync_from_vault_reconciles_vault_edits_on_every_startup() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("app.db");
        let vault = dir.path().join("vault");
        let session_dir = vault.join("sessions/session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join("_meta.json"),
            r#"{"id":"session-1","user_id":"user-1","created_at":"2026-07-10T01:00:00Z","title":"Imported before restart"}"#,
        )
        .unwrap();

        let db = crate::runtime::open_app_db(Some(&db_path)).await.unwrap();
        assert!(db.cloudsync_enabled());

        let first = sync_from_vault(db.pool(), &vault).await.unwrap();
        assert_eq!(first.conflict_count, 0);
        let run_count_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM migration_import_runs")
                .fetch_one(db.pool())
                .await
                .unwrap();
        let stored_title: String =
            sqlx::query_scalar("SELECT title FROM sessions WHERE id = 'session-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(stored_title, "Imported before restart");

        db.pool().close().await;
        drop(db);

        // A user (or another device sharing the vault) edits the file
        // directly while the app is closed.
        std::fs::write(
            session_dir.join("_meta.json"),
            r#"{"id":"session-1","user_id":"user-1","created_at":"2026-07-10T01:00:00Z","title":"Changed after restart"}"#,
        )
        .unwrap();

        let reopened = crate::runtime::open_app_db(Some(&db_path)).await.unwrap();
        assert!(reopened.cloudsync_enabled());

        let second = sync_from_vault(reopened.pool(), &vault).await.unwrap();
        assert_eq!(second.imported_count, 1);
        assert_eq!(second.conflict_count, 0);

        let run_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM migration_import_runs")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
        let stored_title: String =
            sqlx::query_scalar("SELECT title FROM sessions WHERE id = 'session-1'")
                .fetch_one(reopened.pool())
                .await
                .unwrap();

        assert_eq!(run_count_after, run_count_before + 1);
        assert_eq!(stored_title, "Changed after restart");
        assert!(
            std::fs::read_dir(&session_dir)
                .unwrap()
                .filter_map(std::result::Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().contains(".conflict-"))
        );
    }

    async fn test_db() -> hypr_db_core::Db {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        db
    }

    /// Task 14, Step 1: an external editor changed `_memo.md` while the app
    /// was running. `import_paths` is handed just that one changed path (no
    /// full vault rescan) and must update the existing `session_documents`
    /// row via the same files-win conflict path `sync_from_vault` uses.
    #[tokio::test]
    async fn import_paths_reconciles_a_modified_memo_md_into_the_document_row() {
        let db = test_db().await;
        sqlx::query(
            "INSERT INTO sessions
             (id, owner_user_id, title, created_at, started_at, ended_at, event_id,
              external_event_id, external_provider, series_id, event_json, folder_path)
             VALUES ('session-1', 'user-1', 'Planning', '2020-01-01T00:00:00Z',
                     '', '', '', '', '', '', '', '')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, body_format, body, created_at, updated_at)
             VALUES ('note-1', 'session-1', 'note', 'prosemirror_json',
                     '{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"Original body\"}]}]}',
                     '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-1");
        std::fs::create_dir_all(&session_dir).unwrap();
        let memo_path = session_dir.join("_memo.md");
        std::fs::write(
            &memo_path,
            "---\nid: note-1\nsession_id: session-1\n---\n\nUpdated body from external editor",
        )
        .unwrap();

        let report = import_paths(db.pool(), vault.path(), &[memo_path])
            .await
            .unwrap();
        assert_eq!(report.reconciled_count, 1);

        let (body_format, body): (String, String) =
            sqlx::query_as("SELECT body_format, body FROM session_documents WHERE id = 'note-1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(body_format, "markdown");
        assert_eq!(body, "Updated body from external editor");
    }

    /// Adding a brand new session folder externally (not just editing an
    /// existing one) must also flow in — `_meta.json` has no FK dependency,
    /// so this exercises the plain-insert half of the apply path.
    #[tokio::test]
    async fn import_paths_creates_a_new_session_from_a_brand_new_meta_json() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-2");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta_path = session_dir.join("_meta.json");
        std::fs::write(
            &meta_path,
            r#"{"id":"session-2","user_id":"user-1","created_at":"2026-07-20T00:00:00Z","title":"Brand new session"}"#,
        )
        .unwrap();

        let report = import_paths(db.pool(), vault.path(), &[meta_path])
            .await
            .unwrap();
        assert_eq!(report.imported_count, 1);

        let title: String = sqlx::query_scalar("SELECT title FROM sessions WHERE id = 'session-2'")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(title, "Brand new session");
    }

    /// Loop-prevention, third link: even if the watcher's own-write filter
    /// and the export worker's byte-identical write skip both somehow missed
    /// a re-triggered `FileChanged`, `import_paths` must not touch the DB row
    /// again for content whose sha256 already matches the last successful
    /// import of that exact path.
    #[tokio::test]
    async fn import_paths_skips_reimport_when_content_is_byte_identical() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-3");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta_path = session_dir.join("_meta.json");
        let memo_path = session_dir.join("_memo.md");
        std::fs::write(
            &meta_path,
            r#"{"id":"session-3","user_id":"user-1","created_at":"2026-07-20T00:00:00Z","title":"Session three"}"#,
        )
        .unwrap();
        std::fs::write(
            &memo_path,
            "---\nid: note-3\nsession_id: session-3\n---\n\nUnchanged body",
        )
        .unwrap();

        let first = import_paths(
            db.pool(),
            vault.path(),
            &[meta_path.clone(), memo_path.clone()],
        )
        .await
        .unwrap();
        assert_eq!(first.imported_count, 2);

        let updated_at_before: String =
            sqlx::query_scalar("SELECT updated_at FROM session_documents WHERE id = 'note-3'")
                .fetch_one(db.pool())
                .await
                .unwrap();

        let second = import_paths(db.pool(), vault.path(), &[memo_path])
            .await
            .unwrap();

        // The hash short-circuit records the item as `unchanged` (copying
        // forward the prior run's discovered/matched counters, mirroring
        // `sync_from_vault`'s existing unchanged-skip bookkeeping) rather
        // than re-running `parse_source`/`apply_legacy_import_item` — no new
        // insert, no conflict, and nothing for `reconcile_vault_conflicts`
        // to do.
        assert_eq!(second.imported_count, 0);
        assert_eq!(second.matched_count, 1);
        assert_eq!(second.conflict_count, 0);
        assert_eq!(second.reconciled_count, 0);

        let updated_at_after: String =
            sqlx::query_scalar("SELECT updated_at FROM session_documents WHERE id = 'note-3'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(updated_at_before, updated_at_after);
    }

    /// `.trash/` and `.conflict-*` paths must never reach the DB — even if a
    /// watcher event somehow slips one through, `classify_source` rejects
    /// both outright (checked here end-to-end, not just at the unit level).
    #[tokio::test]
    async fn import_paths_rejects_trash_and_conflict_backup_paths() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        let trash_dir = vault.path().join(".trash/2026-07-01/sessions/session-4");
        std::fs::create_dir_all(&trash_dir).unwrap();
        let trash_path = trash_dir.join("_meta.json");
        std::fs::write(
            &trash_path,
            r#"{"id":"session-4","user_id":"user-1","created_at":"2026-07-20T00:00:00Z","title":"Trashed"}"#,
        )
        .unwrap();

        let session_dir = vault.path().join("sessions/session-4");
        std::fs::create_dir_all(&session_dir).unwrap();
        let conflict_path = session_dir.join("_memo.conflict-2026-07-01T00-00-00Z.md");
        std::fs::write(
            &conflict_path,
            "---\nid: note-4\nsession_id: session-4\n---\n\nBackup content",
        )
        .unwrap();

        let report = import_paths(db.pool(), vault.path(), &[trash_path, conflict_path])
            .await
            .unwrap();

        assert_eq!(report.imported_count, 0);
        assert_eq!(report.matched_count, 0);
        assert_eq!(report.conflict_count, 0);
        assert_eq!(report.deleted_count, 0);
        let session_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id = 'session-4'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(session_count, 0);
    }

    /// Deletion semantics: an externally deleted session folder (its
    /// `_meta.json` is gone by the time `import_paths` processes the change)
    /// soft-hides the session rather than cascading a hard delete.
    #[tokio::test]
    async fn import_paths_soft_hides_a_session_whose_meta_json_was_deleted() {
        let db = test_db().await;
        sqlx::query(
            "INSERT INTO sessions
             (id, owner_user_id, title, created_at, started_at, ended_at, event_id,
              external_event_id, external_provider, series_id, event_json, folder_path)
             VALUES ('session-5', 'user-1', 'Gone soon', '2026-07-20T00:00:00Z',
                     '', '', '', '', '', '', '', '')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-5");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta_path = session_dir.join("_meta.json");
        std::fs::write(
            &meta_path,
            r#"{"id":"session-5","user_id":"user-1","created_at":"2026-07-20T00:00:00Z","title":"Gone soon"}"#,
        )
        .unwrap();
        // The watcher saw this path change, but by the time the coalesced
        // batch is processed the file (and likely the whole folder) is gone
        // — simulate that race directly rather than depending on timing.
        std::fs::remove_file(&meta_path).unwrap();

        let report = import_paths(db.pool(), vault.path(), &[meta_path])
            .await
            .unwrap();
        assert_eq!(report.deleted_count, 1);

        let deleted_at: Option<String> =
            sqlx::query_scalar("SELECT deleted_at FROM sessions WHERE id = 'session-5'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert!(deleted_at.is_some());
    }

    /// Scope decision (see the module doc): a deleted document/transcript
    /// file within a session that still exists is logged and ignored, not
    /// soft-deleted — there's no cheap, unambiguous filename-to-row mapping
    /// for that granularity, unlike `_meta.json` which *is* the session.
    #[tokio::test]
    async fn import_paths_ignores_deletion_of_a_non_meta_file_within_a_live_session() {
        let db = test_db().await;
        sqlx::query(
            "INSERT INTO sessions
             (id, owner_user_id, title, created_at, started_at, ended_at, event_id,
              external_event_id, external_provider, series_id, event_json, folder_path)
             VALUES ('session-6', 'user-1', 'Still here', '2026-07-20T00:00:00Z',
                     '', '', '', '', '', '', '', '')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, body_format, body, created_at, updated_at)
             VALUES ('note-6', 'session-6', 'note', 'markdown', 'Still here too',
                     '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-6");
        std::fs::create_dir_all(&session_dir).unwrap();
        let memo_path = session_dir.join("_memo.md");
        std::fs::write(
            &memo_path,
            "---\nid: note-6\nsession_id: session-6\n---\n\nStill here too",
        )
        .unwrap();
        std::fs::remove_file(&memo_path).unwrap();

        let report = import_paths(db.pool(), vault.path(), &[memo_path])
            .await
            .unwrap();
        assert_eq!(report.deleted_count, 0);

        let (session_deleted_at, document_deleted_at): (Option<String>, Option<String>) =
            sqlx::query_as(
                "SELECT
               (SELECT deleted_at FROM sessions WHERE id = 'session-6'),
               (SELECT deleted_at FROM session_documents WHERE id = 'note-6')",
            )
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(session_deleted_at.is_none());
        assert!(document_deleted_at.is_none());
    }

    /// Regression for a real bug found in code review: soft-hiding on a
    /// missing `_meta.json` must be reversible. A sync client can deliver a
    /// delete-then-recreate more than the 2s coalesce window apart (or a
    /// user can `rm` then restore a file), so `import_paths` will see the
    /// deletion and the recreation as two separate calls. Before the
    /// `revive_soft_deleted_session` fix in `crates/db-app`, nothing ever
    /// cleared `deleted_at` again: `row_matches_existing`/`import_target_exists`
    /// both require `deleted_at IS NULL` and `reconcile_content_conflict`
    /// bailed on a soft-deleted row, so re-importing a present `_meta.json`
    /// fell through to `MissingDependency` forever.
    ///
    /// Crucially, this establishes a **baseline import first** (exactly
    /// like the reviewer's reproduction) rather than inserting the session
    /// via raw SQL: without a prior `import_paths`/`sync_from_vault` call
    /// recording `_meta.json`'s hash in `migration_import_items`, the second
    /// bug this test also covers — the hash short-circuit
    /// (`legacy_source_already_imported`) firing on a byte-identical
    /// recreation and skipping straight past `revive_soft_deleted_session`
    /// — would never trigger, since there'd be no recorded hash for it to
    /// match in the first place. A first version of this test made exactly
    /// that mistake and passed for the wrong reason.
    #[tokio::test]
    async fn import_paths_revives_a_soft_deleted_session_when_meta_json_reappears() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-7");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta_path = session_dir.join("_meta.json");
        let meta_content = r#"{"id":"session-7","user_id":"user-1","created_at":"2026-07-20T00:00:00Z","title":"Comes back"}"#;
        std::fs::write(&meta_path, meta_content).unwrap();

        // Baseline: a normal import establishes the session AND records
        // this exact path+hash in `migration_import_items` — precisely
        // what a startup `sync_from_vault` reconcile would already have
        // done long before any blip, in the real app.
        let baseline_report = import_paths(db.pool(), vault.path(), &[meta_path.clone()])
            .await
            .unwrap();
        assert_eq!(baseline_report.imported_count, 1);

        // The watcher saw the deletion (file briefly gone) — soft-hides
        // the session, exactly like
        // `import_paths_soft_hides_a_session_whose_meta_json_was_deleted`.
        std::fs::remove_file(&meta_path).unwrap();
        let deletion_report = import_paths(db.pool(), vault.path(), &[meta_path.clone()])
            .await
            .unwrap();
        assert_eq!(deletion_report.deleted_count, 1);
        let (deleted_at_after_hide, metadata_json_after_hide): (Option<String>, String) =
            sqlx::query_as("SELECT deleted_at, metadata_json FROM sessions WHERE id = 'session-7'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert!(deleted_at_after_hide.is_some());
        assert!(
            hypr_db_app::is_externally_soft_hidden(&metadata_json_after_hide),
            "soft-hiding via a missing _meta.json must flag the session as externally hidden"
        );

        // A sync client (or the user) recreates the file with
        // byte-identical content, more than the watcher's coalesce window
        // later — a second, independent `import_paths` call. This is the
        // dominant real-world case: nothing about the file's content
        // actually changed, only its transient absence.
        std::fs::write(&meta_path, meta_content).unwrap();
        let revival_report = import_paths(db.pool(), vault.path(), &[meta_path])
            .await
            .unwrap();

        let (title, deleted_at_after_revival, metadata_json_after_revival): (
            String,
            Option<String>,
            String,
        ) = sqlx::query_as(
            "SELECT title, deleted_at, metadata_json FROM sessions WHERE id = 'session-7'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(
            deleted_at_after_revival.is_none(),
            "session should be visible again once _meta.json reappears, even with byte-identical content"
        );
        assert_eq!(title, "Comes back");
        assert_eq!(revival_report.imported_count, 1);
        assert!(
            !hypr_db_app::is_externally_soft_hidden(&metadata_json_after_revival),
            "revival must clear the external-soft-hide marker"
        );
    }

    /// Same fix, exercised through the startup-reconcile path
    /// (`sync_from_vault`, not the live watcher's `import_paths`) — the
    /// revival lives in the shared `insert_row_if_missing` ->
    /// `reconcile_content_conflict` machinery both call, so a session
    /// soft-hidden while the app was running must also come back the next
    /// time the app launches and reconciles the whole vault, not just via a
    /// live watcher event.
    ///
    /// Also establishes a baseline import first (via `sync_from_vault`
    /// itself this time) before soft-hiding, then recreates the file with
    /// byte-identical content — otherwise `legacy_source_already_imported`
    /// never gets a chance to defeat the revival in the first place. The
    /// soft-hide itself goes through `import_paths` (not raw SQL) so the
    /// external-soft-hide marker is set exactly like it would be in
    /// production, demonstrating that `import_paths` and `sync_from_vault`
    /// compose correctly across the two entry points.
    #[tokio::test]
    async fn sync_from_vault_revives_a_soft_deleted_session_on_the_next_startup_reconcile() {
        let db = test_db().await;
        let vault = tempfile::tempdir().unwrap();
        let session_dir = vault.path().join("sessions/session-8");
        std::fs::create_dir_all(&session_dir).unwrap();
        let meta_path = session_dir.join("_meta.json");
        let meta_content = r#"{"id":"session-8","user_id":"user-1","created_at":"2026-07-20T00:00:00Z","title":"Also comes back"}"#;
        std::fs::write(&meta_path, meta_content).unwrap();

        // Baseline: a normal startup reconcile imports the session and
        // records its _meta.json hash in migration_import_items.
        let baseline = sync_from_vault(db.pool(), vault.path()).await.unwrap();
        assert_eq!(baseline.imported_count, 1);

        // The live watcher's import_paths sees _meta.json go missing during
        // a prior running session and soft-hides it.
        std::fs::remove_file(&meta_path).unwrap();
        let deletion_report = import_paths(db.pool(), vault.path(), &[meta_path.clone()])
            .await
            .unwrap();
        assert_eq!(deletion_report.deleted_count, 1);

        // The file reappears with byte-identical content before the app's
        // next launch — the next full sync_from_vault reconcile (not
        // import_paths this time) must still revive the session, even
        // though its hash already matches the baseline import's recorded
        // hash.
        std::fs::write(&meta_path, meta_content).unwrap();
        let revival = sync_from_vault(db.pool(), vault.path()).await.unwrap();
        assert_eq!(revival.imported_count, 1);

        let (title, deleted_at, metadata_json): (String, Option<String>, String) = sqlx::query_as(
            "SELECT title, deleted_at, metadata_json FROM sessions WHERE id = 'session-8'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(
            deleted_at.is_none(),
            "startup reconcile should revive a soft-deleted session whose _meta.json is present, even with byte-identical content"
        );
        assert_eq!(title, "Also comes back");
        assert!(
            !hypr_db_app::is_externally_soft_hidden(&metadata_json),
            "revival must clear the external-soft-hide marker"
        );
    }
}
