//! Integration tests for `sync_from_vault`: the continuous, idempotent
//! reconcile-from-vault path that replaced the old run-once legacy import
//! gate. `app.db` is a disposable cache — these tests exercise the "delete
//! app.db and relaunch" rebuild path (a fresh pool imports everything from
//! the vault) plus the ongoing per-file hash comparison and the
//! files-win conflict rule.

use std::path::Path;

use tauri_plugin_db::sync_from_vault;

async fn fresh_pool() -> hypr_db_core::Db {
    let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
    hypr_db_app::prepare_schema(&db).await.unwrap();
    db
}

fn write_session(vault: &Path, session_id: &str, title: &str, memo: &str) {
    let session_dir = vault.join("sessions").join(session_id);
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(
        session_dir.join("_meta.json"),
        format!(
            r#"{{"id":"{session_id}","user_id":"user-1","created_at":"2026-07-10T01:00:00Z","title":"{title}"}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        session_dir.join("_memo.md"),
        format!("---\nid: note-{session_id}\nsession_id: {session_id}\n---\n\n{memo}"),
    )
    .unwrap();
}

/// Brief Step 1: a fresh pool (the "delete app.db" rebuild path) imports
/// everything on the first call; an unchanged vault reports zero imports on
/// the second call (per-file sha256 skip); editing exactly one file
/// re-imports exactly that one document on the third call.
#[tokio::test]
async fn sync_from_vault_is_idempotent_and_reimports_only_changed_files() {
    let db = fresh_pool().await;
    let vault = tempfile::tempdir().unwrap();
    write_session(vault.path(), "session-1", "Planning", "Original memo body");

    let first = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(first.imported_count, 2, "session + note document");
    assert_eq!(first.conflict_count, 0);

    let session_title: String =
        sqlx::query_scalar("SELECT title FROM sessions WHERE id = 'session-1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(session_title, "Planning");
    let document_body: String = sqlx::query_scalar(
        "SELECT body FROM session_documents WHERE session_id = 'session-1'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(document_body, "Original memo body");

    let second = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(second.imported_count, 0, "unchanged vault: hash skip");
    assert_eq!(second.conflict_count, 0);
    assert_eq!(second.discovered_count, first.discovered_count);

    let session_dir = vault.path().join("sessions/session-1");
    std::fs::write(
        session_dir.join("_memo.md"),
        "---\nid: note-session-1\nsession_id: session-1\n---\n\nUpdated memo body",
    )
    .unwrap();

    let third = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(third.imported_count, 1, "only the edited document");
    assert_eq!(third.conflict_count, 0);

    let document_body: String = sqlx::query_scalar(
        "SELECT body FROM session_documents WHERE session_id = 'session-1'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(document_body, "Updated memo body");
}

/// A fresh/empty database (no prior run rows at all) imports every vault
/// file in one pass — this is the exact equivalent of the manual
/// "delete app.db and relaunch" drill.
#[tokio::test]
async fn sync_from_vault_rebuilds_everything_from_an_empty_database() {
    let db = fresh_pool().await;
    let vault = tempfile::tempdir().unwrap();
    write_session(vault.path(), "session-1", "Standup", "Standup notes");
    write_session(vault.path(), "session-2", "Retro", "Retro notes");

    let report = sync_from_vault(db.pool(), vault.path()).await.unwrap();

    assert_eq!(report.imported_count, 4);
    let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let document_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_documents")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(session_count, 2);
    assert_eq!(document_count, 2);
}

/// Conflict rule: if a vault file changes AND the DB row's `updated_at` is
/// newer than the file's mtime, the file still wins — but the DB's current
/// content is exported to a `.conflict-<timestamp>.<ext>` backup beside the
/// file first, and a warning is logged.
#[tokio::test]
async fn sync_from_vault_exports_conflict_backup_when_db_row_is_newer_than_file() {
    let db = fresh_pool().await;
    let vault = tempfile::tempdir().unwrap();
    write_session(vault.path(), "session-1", "Planning", "Original memo body");

    let first = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(first.conflict_count, 0);

    // Simulate the DB row being touched *after* the file was last imported
    // (e.g. by normal app usage), stamped further in the future than
    // anything a filesystem mtime could read as "now".
    sqlx::query(
        "UPDATE session_documents
         SET body = 'DB-only edit',
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+1 day')
         WHERE session_id = 'session-1'",
    )
    .execute(db.pool())
    .await
    .unwrap();

    // The vault file changes too, with content that diverges from both the
    // original import and the DB-only edit above.
    let session_dir = vault.path().join("sessions/session-1");
    std::fs::write(
        session_dir.join("_memo.md"),
        "---\nid: note-session-1\nsession_id: session-1\n---\n\nFile edit wins",
    )
    .unwrap();

    let second = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(second.reconciled_count, 1);
    assert_eq!(second.conflict_count, 0, "the conflict was force-resolved");

    let document_body: String = sqlx::query_scalar(
        "SELECT body FROM session_documents WHERE session_id = 'session-1'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(document_body, "File edit wins", "the vault file wins content");

    let backups = std::fs::read_dir(&session_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".conflict-"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1, "exactly one conflict backup was written");
    let backup_content = std::fs::read_to_string(session_dir.join(&backups[0])).unwrap();
    assert!(
        backup_content.contains("DB-only edit"),
        "the backup preserves the DB's content before it was overwritten"
    );
}

/// Regression: a `.conflict-<timestamp>.md` backup carries the SAME
/// frontmatter `id` as the live document it was exported from. If it were
/// ever discovered as a live source on a later scan, the duplicate-id dedup
/// pass could let the (lexically earlier-sorting) backup keep the original
/// id, orphan the live file under a "recovered" id, and then
/// `reconcile_vault_conflicts` would force the canonical row back to the
/// stale backup content — silently reverting the exact conflict this
/// feature exists to preserve. `classify_source` must reject `.conflict-`
/// filenames outright so the backup is never scanned again.
#[tokio::test]
async fn sync_from_vault_never_reimports_its_own_conflict_backups() {
    let db = fresh_pool().await;
    let vault = tempfile::tempdir().unwrap();
    write_session(vault.path(), "session-1", "Planning", "Original memo body");
    sync_from_vault(db.pool(), vault.path()).await.unwrap();

    sqlx::query(
        "UPDATE session_documents
         SET body = 'DB-only edit',
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+1 day')
         WHERE session_id = 'session-1'",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let session_dir = vault.path().join("sessions/session-1");
    std::fs::write(
        session_dir.join("_memo.md"),
        "---\nid: note-session-1\nsession_id: session-1\n---\n\nFile edit wins",
    )
    .unwrap();

    // Second run: creates the conflict backup and force-resolves in favor
    // of the file, exactly like the test above.
    let second = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(second.reconciled_count, 1);
    let backups_after_second = std::fs::read_dir(&session_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".conflict-"))
        .collect::<Vec<_>>();
    assert_eq!(backups_after_second.len(), 1);

    // Third run: nothing on disk changed since the second run (the backup
    // already existed when it finished). The backup must not be rediscovered,
    // reforked, or used to revert the canonical row.
    let third = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(third.imported_count, 0, "the backup is not a live source");
    assert_eq!(third.conflict_count, 0);
    assert_eq!(third.reconciled_count, 0);

    let document_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM session_documents WHERE session_id = 'session-1'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(document_count, 1, "no orphaned duplicate document row");

    let document_body: String = sqlx::query_scalar(
        "SELECT body FROM session_documents WHERE session_id = 'session-1'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(
        document_body, "File edit wins",
        "the canonical row must keep the file-won content, not silently revert to the backup"
    );

    let backups_after_third = std::fs::read_dir(&session_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".conflict-"))
        .collect::<Vec<_>>();
    assert_eq!(
        backups_after_third.len(),
        1,
        "no second backup is written for an already-resolved conflict"
    );
}

/// Deletion propagation is out of scope (Task 14's watcher): a vault file
/// that disappears must never delete the corresponding DB row.
#[tokio::test]
async fn sync_from_vault_never_deletes_rows_for_missing_files() {
    let db = fresh_pool().await;
    let vault = tempfile::tempdir().unwrap();
    write_session(vault.path(), "session-1", "Planning", "Memo body");

    sync_from_vault(db.pool(), vault.path()).await.unwrap();

    std::fs::remove_dir_all(vault.path().join("sessions/session-1")).unwrap();
    let report = sync_from_vault(db.pool(), vault.path()).await.unwrap();

    assert_eq!(report.imported_count, 0);
    let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(session_count, 1, "the row survives the file's disappearance");
}

/// When the DB copy being backed up is app-authored tiptap prosemirror JSON
/// (the normal in-app editor format, as opposed to plain vault markdown),
/// the conflict backup renders it to markdown via `hypr_tiptap` rather than
/// dumping raw JSON, so the backup reads like the rest of the vault.
#[tokio::test]
async fn conflict_backup_renders_prosemirror_json_bodies_as_markdown() {
    let db = fresh_pool().await;
    let vault = tempfile::tempdir().unwrap();
    write_session(vault.path(), "session-1", "Planning", "Original memo body");
    sync_from_vault(db.pool(), vault.path()).await.unwrap();

    let prosemirror_body = serde_json::json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{ "type": "text", "text": "Rich text from the in-app editor" }]
        }]
    })
    .to_string();
    sqlx::query(
        "UPDATE session_documents
         SET body = ?, body_format = 'prosemirror_json',
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+1 day')
         WHERE session_id = 'session-1'",
    )
    .bind(&prosemirror_body)
    .execute(db.pool())
    .await
    .unwrap();

    let session_dir = vault.path().join("sessions/session-1");
    std::fs::write(
        session_dir.join("_memo.md"),
        "---\nid: note-session-1\nsession_id: session-1\n---\n\nFile edit wins",
    )
    .unwrap();

    let report = sync_from_vault(db.pool(), vault.path()).await.unwrap();
    assert_eq!(report.reconciled_count, 1);

    let backups = std::fs::read_dir(&session_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".conflict-"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup_content = std::fs::read_to_string(session_dir.join(&backups[0])).unwrap();
    assert!(
        backup_content.contains("Rich text from the in-app editor"),
        "backup should contain rendered markdown, not raw JSON: {backup_content}"
    );
    assert!(
        !backup_content.contains("\"type\":\"doc\""),
        "backup should not contain the raw tiptap JSON: {backup_content}"
    );
}
