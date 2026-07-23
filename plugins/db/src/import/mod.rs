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
}

/// Reconcile `app.db` from the vault's files. Idempotent and safe to call on
/// every startup: unchanged files (matching sha256 of the last successful
/// import) are skipped, new files are imported, and changed files are
/// re-imported with the vault file winning any content conflict. A fresh or
/// empty database (no prior run rows) imports everything — this is the
/// "delete app.db and relaunch" rebuild path.
pub async fn sync_from_vault(pool: &SqlitePool, vault_base: &std::path::Path) -> crate::Result<SyncReport> {
    let run_id = legacy_vault::import_legacy_vault(pool, vault_base, false).await?;
    let reconciled_count = legacy_vault::reconcile_vault_conflicts(pool, vault_base, &run_id).await?;

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
}
