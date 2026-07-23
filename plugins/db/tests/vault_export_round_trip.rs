//! Round-trip contract for Task 13 (DB-to-vault write-through export).
//!
//! `crates/fs-sync-core/src/export.rs`'s render functions are pure (no DB,
//! no Tauri) — these tests are the seam where both sides meet: fetch rows
//! from a source DB, render them with `export::render_*`, write the result
//! into a temp vault with `export::write_file_atomic`, then feed the vault
//! back through the *actual* importer via the public `sync_from_vault` (which
//! internally calls `legacy_vault.rs`'s private `classify_source`/
//! `parse_source` — the round-trip authority) against a **fresh** second
//! database, and assert the re-imported rows match the originals.
//!
//! A fresh second DB (rather than re-importing into the source DB) is
//! deliberate: `insert_row_if_missing` is `ON CONFLICT(id) DO NOTHING` for
//! every row kind exercised here, so re-importing into the same DB would
//! trivially "pass" without ever exercising the insert path.

use std::path::Path;

use hypr_fs_sync_core::export;
use sqlx::Row;
use tauri_plugin_db::sync_from_vault;

async fn fresh_db() -> hypr_db_core::Db {
    let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
    hypr_db_app::prepare_schema(&db).await.unwrap();
    db
}

fn vault() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Imports `vault` into a **fresh, empty** database and hands it back so
/// callers can query the re-imported rows. Fresh (not the source DB) so
/// `ON CONFLICT(id) DO NOTHING` inserts actually run instead of no-oping.
async fn reimport(vault: &Path) -> hypr_db_core::Db {
    let target = fresh_db().await;
    sync_from_vault(target.pool(), vault).await.unwrap();
    target
}

// ---------------------------------------------------------------------------
// Brief Step 2: session + note (prosemirror) + transcript + summary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_note_transcript_and_summary_round_trip() {
    let source = fresh_db().await;
    sqlx::query(
        "INSERT INTO sessions
           (id, owner_user_id, title, created_at, started_at, ended_at,
            event_id, external_event_id, series_id, event_json)
         VALUES ('session-1', 'user-1', 'Planning', '2026-07-01T09:00:00Z',
                 '2026-07-01T10:00:00Z', '2026-07-01T10:30:00Z',
                 'event-1', 'track-1', 'series-1', '{}')",
    )
    .execute(source.pool())
    .await
    .unwrap();

    let note_body_json_value = serde_json::json!({
        "type": "doc",
        "content": [{"type": "paragraph", "content": [{"type": "text", "text": "hello world"}]}]
    });
    let note_body_json = note_body_json_value.to_string();
    sqlx::query(
        "INSERT INTO session_documents
           (id, session_id, kind, title, body_format, body, sort_order)
         VALUES ('doc-note', 'session-1', 'note', 'My note', 'prosemirror_json', ?, 0)",
    )
    .bind(&note_body_json)
    .execute(source.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session_documents
           (id, session_id, kind, title, body_format, body, sort_order)
         VALUES ('doc-summary', 'session-1', 'summary', 'Summary', 'markdown', 'Summary body', 1)",
    )
    .execute(source.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO transcripts
           (id, owner_user_id, session_id, created_at, started_at_ms, ended_at_ms,
            memo, words_json, speaker_hints_json)
         VALUES ('transcript-1', 'user-1', 'session-1', '2026-07-01T09:00:00Z', 1000, 2000,
                 'transcript memo',
                 '[{\"id\":\"w1\",\"text\":\"hi\",\"start_ms\":0,\"end_ms\":100,\"channel\":0}]',
                 '[]')",
    )
    .execute(source.pool())
    .await
    .unwrap();

    let vault = vault();
    let session_dir = vault.path().join("sessions/session-1");

    let session_row = sqlx::query(
        "SELECT id, owner_user_id, title, created_at, started_at, ended_at,
                event_id, external_event_id, series_id, event_json
         FROM sessions WHERE id = 'session-1'",
    )
    .fetch_one(source.pool())
    .await
    .unwrap();
    let session_meta = export::SessionMeta {
        id: session_row.get("id"),
        owner_user_id: session_row.get("owner_user_id"),
        title: session_row.get("title"),
        created_at: session_row.get("created_at"),
        started_at: session_row.get("started_at"),
        ended_at: session_row.get("ended_at"),
        event_id: session_row.get("event_id"),
        external_event_id: session_row.get("external_event_id"),
        series_id: session_row.get("series_id"),
        event_json: session_row.get("event_json"),
    };
    let meta_value = export::render_session_meta(&session_meta, &[], &[], None);
    write_json(vault.path(), &session_dir.join("_meta.json"), &meta_value);

    // _meta.json round-trips the exact vault-carried fields the importer
    // reads (title/timestamps/event ids) — see the module doc for why
    // kind/status/timezone/etc. aren't part of this contract at all.
    assert_eq!(meta_value["title"], serde_json::json!("Planning"));

    let note_row = sqlx::query(
        "SELECT id, session_id, kind, template_id, title, body_format, body, sort_order
         FROM session_documents WHERE id = 'doc-note'",
    )
    .fetch_one(source.pool())
    .await
    .unwrap();
    write_document(vault.path(), &session_dir, &session_row_to_document(&note_row), true);

    let summary_row = sqlx::query(
        "SELECT id, session_id, kind, template_id, title, body_format, body, sort_order
         FROM session_documents WHERE id = 'doc-summary'",
    )
    .fetch_one(source.pool())
    .await
    .unwrap();
    write_document(vault.path(), &session_dir, &session_row_to_document(&summary_row), true);

    let transcript_row = sqlx::query(
        "SELECT id, owner_user_id, session_id, created_at, started_at_ms, ended_at_ms,
                memo, words_json, speaker_hints_json
         FROM transcripts WHERE id = 'transcript-1'",
    )
    .fetch_one(source.pool())
    .await
    .unwrap();
    let transcript = export::Transcript {
        id: transcript_row.get("id"),
        owner_user_id: transcript_row.get("owner_user_id"),
        session_id: transcript_row.get("session_id"),
        created_at: transcript_row.get("created_at"),
        started_at_ms: transcript_row.get("started_at_ms"),
        ended_at_ms: transcript_row.get("ended_at_ms"),
        memo: transcript_row.get("memo"),
        words_json: transcript_row.get("words_json"),
        speaker_hints_json: transcript_row.get("speaker_hints_json"),
    };
    let transcript_value = export::render_transcripts(&[transcript]);
    write_json(vault.path(), &session_dir.join("transcript.json"), &transcript_value);

    assert!(session_dir.join("_meta.json").is_file());
    assert!(session_dir.join("_memo.md").is_file());
    assert!(session_dir.join("_summary.md").is_file());
    assert!(session_dir.join("transcript.json").is_file());
    let memo_contents = std::fs::read_to_string(session_dir.join("_memo.md")).unwrap();
    assert!(memo_contents.contains("id: doc-note"));
    assert!(memo_contents.contains("session_id: session-1"));
    assert!(memo_contents.contains("hello world"));

    let target = reimport(vault.path()).await;

    let (title, started_at, ended_at, event_id, external_event_id, series_id): (
        String,
        String,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT title, started_at, ended_at, event_id, external_event_id, series_id
         FROM sessions WHERE id = 'session-1'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(title, "Planning");
    assert_eq!(started_at, "2026-07-01T10:00:00Z");
    assert_eq!(ended_at, "2026-07-01T10:30:00Z");
    assert_eq!(event_id, "event-1");
    assert_eq!(external_event_id, "track-1");
    assert_eq!(series_id, "series-1");

    // Prosemirror-authored note round-trips to its rendered markdown, with
    // body_format normalized to "markdown" (the vault always stores
    // markdown — see `render_session_document`'s doc comment).
    let (reimported_body_format, reimported_body): (String, String) = sqlx::query_as(
        "SELECT body_format, body FROM session_documents WHERE id = 'doc-note'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(reimported_body_format, "markdown");
    let expected_markdown = hypr_tiptap::tiptap_json_to_md(&note_body_json_value).unwrap();
    assert_eq!(reimported_body.trim(), expected_markdown.trim());

    let summary_body: String =
        sqlx::query_scalar("SELECT body FROM session_documents WHERE id = 'doc-summary'")
            .fetch_one(target.pool())
            .await
            .unwrap();
    assert_eq!(summary_body, "Summary body");

    let (memo, started_at_ms, ended_at_ms): (String, i64, Option<i64>) = sqlx::query_as(
        "SELECT memo, started_at_ms, ended_at_ms FROM transcripts WHERE id = 'transcript-1'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(memo, "transcript memo");
    assert_eq!(started_at_ms, 1000);
    assert_eq!(ended_at_ms, Some(2000));
}

/// Whole-branch-review gap: every other `_meta.json` test in this file
/// passes `participants: &[]` and `key_facts: None`, so the participants
/// array and the `key_facts` object in `render_session_meta`'s output were
/// never actually exercised end to end through the real importer. This
/// covers both: a session with two real participants (each backed by a
/// `humans` row) and a `key_facts`-kind `session_documents` row.
#[tokio::test]
async fn session_meta_participants_and_key_facts_round_trip() {
    let source = fresh_db().await;
    sqlx::query(
        "INSERT INTO sessions (id, owner_user_id, title, created_at)
         VALUES ('session-1', 'user-1', 'Planning', '2026-07-01T00:00:00Z')",
    )
    .execute(source.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO humans (id, name) VALUES ('human-1', 'Ada Lovelace'), ('human-2', 'Alan Turing')",
    )
    .execute(source.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session_participants (id, owner_user_id, session_id, human_id, source)
         VALUES ('participant-1', 'user-1', 'session-1', 'human-1', 'calendar'),
                ('participant-2', 'user-1', 'session-1', 'human-2', 'manual')",
    )
    .execute(source.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session_documents
           (id, session_id, kind, title, body_format, body, source_hash, created_by, created_at, updated_at)
         VALUES ('session-1:key_facts', 'session-1', 'key_facts', 'Key facts', 'markdown',
                 'Discussed Q3 roadmap.', 'hash-abc123', 'user-1',
                 '2026-07-01T00:00:00Z', '2026-07-01T00:05:00Z')",
    )
    .execute(source.pool())
    .await
    .unwrap();

    let session = export::SessionMeta {
        id: "session-1".to_string(),
        owner_user_id: "user-1".to_string(),
        title: "Planning".to_string(),
        created_at: "2026-07-01T00:00:00Z".to_string(),
        started_at: String::new(),
        ended_at: String::new(),
        event_id: String::new(),
        external_event_id: String::new(),
        series_id: String::new(),
        event_json: String::new(),
    };
    let participants = vec![
        export::SessionParticipant {
            id: "participant-1".to_string(),
            owner_user_id: "user-1".to_string(),
            human_id: "human-1".to_string(),
            source: "calendar".to_string(),
            display_name: "Ada Lovelace".to_string(),
            email: String::new(),
            role: String::new(),
        },
        export::SessionParticipant {
            id: "participant-2".to_string(),
            owner_user_id: "user-1".to_string(),
            human_id: "human-2".to_string(),
            source: "manual".to_string(),
            display_name: "Alan Turing".to_string(),
            email: String::new(),
            role: String::new(),
        },
    ];
    let key_facts = export::SessionKeyFacts {
        content: "Discussed Q3 roadmap.".to_string(),
        source_hash: "hash-abc123".to_string(),
        user_id: "user-1".to_string(),
        created_at: "2026-07-01T00:00:00Z".to_string(),
        updated_at: "2026-07-01T00:05:00Z".to_string(),
    };
    let meta_value = export::render_session_meta(&session, &participants, &[], Some(&key_facts));

    let vault = vault();
    write_json(
        vault.path(),
        &vault.path().join("sessions/session-1/_meta.json"),
        &meta_value,
    );

    let target = reimport(vault.path()).await;

    let mut reimported_participants: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, owner_user_id, human_id, source
         FROM session_participants WHERE session_id = 'session-1' ORDER BY id",
    )
    .fetch_all(target.pool())
    .await
    .unwrap();
    reimported_participants.sort();
    assert_eq!(
        reimported_participants,
        vec![
            (
                "participant-1".to_string(),
                "user-1".to_string(),
                "human-1".to_string(),
                "calendar".to_string(),
            ),
            (
                "participant-2".to_string(),
                "user-1".to_string(),
                "human-2".to_string(),
                "manual".to_string(),
            ),
        ]
    );

    let (key_facts_body, key_facts_source_hash): (String, String) = sqlx::query_as(
        "SELECT body, source_hash FROM session_documents
         WHERE session_id = 'session-1' AND kind = 'key_facts'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(key_facts_body, "Discussed Q3 roadmap.");
    assert_eq!(key_facts_source_hash, "hash-abc123");
}

// Small helpers kept out of the main test body for readability.

struct DocumentRow {
    id: String,
    session_id: String,
    kind: String,
    template_id: String,
    title: String,
    body_format: String,
    body: String,
    sort_order: i64,
}

fn session_row_to_document(row: &sqlx::sqlite::SqliteRow) -> DocumentRow {
    DocumentRow {
        id: row.get("id"),
        session_id: row.get("session_id"),
        kind: row.get("kind"),
        template_id: row.get("template_id"),
        title: row.get("title"),
        body_format: row.get("body_format"),
        body: row.get("body"),
        sort_order: row.get("sort_order"),
    }
}

fn write_document(vault_base: &Path, session_dir: &Path, row: &DocumentRow, is_first_of_kind: bool) {
    let document = export::SessionDocument {
        id: row.id.clone(),
        session_id: row.session_id.clone(),
        kind: row.kind.clone(),
        template_id: row.template_id.clone(),
        title: row.title.clone(),
        body_format: row.body_format.clone(),
        body: row.body.clone(),
        sort_order: row.sort_order,
    };
    let Some(filename) = export::session_document_filename(&document, is_first_of_kind) else {
        return;
    };
    let rendered = export::render_session_document(&document).unwrap();
    let content = rendered.render().unwrap();
    write_file(vault_base, &session_dir.join(filename), content.as_bytes());
}

fn write_json(vault_base: &Path, path: &Path, value: &serde_json::Value) {
    let content = hypr_fs_sync_core::json::serialize(value.clone()).unwrap();
    write_file(vault_base, path, content.as_bytes());
}

/// Test-local equivalent of `vault_export.rs`'s `write_tracked` minus the
/// Tauri notify marking (no `AppHandle` in these tests) — computes the tmp
/// path the same way the real worker does and delegates to the same
/// `write_file_atomic`.
fn write_file(vault_base: &Path, path: &Path, content: &[u8]) {
    let tmp_path = export::tmp_sibling_path(path);
    export::write_file_atomic(vault_base, path, &tmp_path, content).unwrap();
}

// ---------------------------------------------------------------------------
// humans / organizations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn human_round_trip() {
    let source = fresh_db().await;
    sqlx::query(
        "INSERT INTO humans
           (id, owner_user_id, organization_id, name, email, phone, job_title,
            linkedin_username, memo, pinned, pin_order, created_at)
         VALUES ('human-1', 'user-1', 'org-1', 'Ada Lovelace', 'ada@example.com',
                 '+1-555-0100', 'Mathematician', 'ada-lovelace', 'Wrote the first program.',
                 1, 3, '2026-07-01T00:00:00Z')",
    )
    .execute(source.pool())
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT owner_user_id, organization_id, name, email, phone, job_title,
                linkedin_username, memo, pinned, pin_order, created_at
         FROM humans WHERE id = 'human-1'",
    )
    .fetch_one(source.pool())
    .await
    .unwrap();
    let human = export::Human {
        owner_user_id: row.get("owner_user_id"),
        organization_id: row.get("organization_id"),
        name: row.get("name"),
        email: row.get("email"),
        phone: row.get("phone"),
        job_title: row.get("job_title"),
        linkedin_username: row.get("linkedin_username"),
        memo: row.get("memo"),
        pinned: row.get("pinned"),
        pin_order: row.get("pin_order"),
        created_at: row.get("created_at"),
    };
    let rendered = export::render_human(&human).render().unwrap();

    let vault = vault();
    let path = vault.path().join("humans/human-1.md");
    write_file(vault.path(), &path, rendered.as_bytes());

    let target = reimport(vault.path()).await;
    let (name, email, phone, job_title, linkedin, memo, pinned, pin_order): (
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT name, email, phone, job_title, linkedin_username, memo, pinned, pin_order
         FROM humans WHERE id = 'human-1'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();

    assert_eq!(name, "Ada Lovelace");
    assert_eq!(email, "ada@example.com");
    assert_eq!(phone, "+1-555-0100");
    assert_eq!(job_title, "Mathematician");
    assert_eq!(linkedin, "ada-lovelace");
    assert_eq!(memo, "Wrote the first program.");
    assert!(pinned);
    assert_eq!(pin_order, Some(3));
}

#[tokio::test]
async fn organization_round_trip() {
    let source = fresh_db().await;
    sqlx::query(
        "INSERT INTO organizations (id, owner_user_id, name, memo, pinned, pin_order, created_at)
         VALUES ('org-1', 'user-1', 'Acme Corp', 'A fine organization.', 0, NULL, '2026-07-01T00:00:00Z')",
    )
    .execute(source.pool())
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT owner_user_id, name, memo, pinned, pin_order, created_at
         FROM organizations WHERE id = 'org-1'",
    )
    .fetch_one(source.pool())
    .await
    .unwrap();
    let organization = export::Organization {
        owner_user_id: row.get("owner_user_id"),
        name: row.get("name"),
        memo: row.get("memo"),
        pinned: row.get("pinned"),
        pin_order: row.get("pin_order"),
        created_at: row.get("created_at"),
    };
    let rendered = export::render_organization(&organization).render().unwrap();

    let vault = vault();
    write_file(
        vault.path(),
        &vault.path().join("organizations/org-1.md"),
        rendered.as_bytes(),
    );

    let target = reimport(vault.path()).await;
    let (name, memo, pinned): (String, String, bool) =
        sqlx::query_as("SELECT name, memo, pinned FROM organizations WHERE id = 'org-1'")
            .fetch_one(target.pool())
            .await
            .unwrap();
    assert_eq!(name, "Acme Corp");
    assert_eq!(memo, "A fine organization.");
    assert!(!pinned);
}

// ---------------------------------------------------------------------------
// calendars.json / events.json
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendars_and_events_round_trip() {
    let value = export::render_calendars(&[export::Calendar {
        id: "cal-1".to_string(),
        tracking_id_calendar: "track-cal-1".to_string(),
        name: "Work".to_string(),
        enabled: true,
        provider: "google".to_string(),
        source: "team".to_string(),
        color: "#123456".to_string(),
        connection_id: "conn-1".to_string(),
    }]);
    let events_value = export::render_events(&[export::CalendarEvent {
        id: "event-1".to_string(),
        tracking_id_event: "track-event-1".to_string(),
        calendar_id: "cal-1".to_string(),
        title: "Standup".to_string(),
        started_at: "2026-07-01T09:00:00Z".to_string(),
        ended_at: "2026-07-01T09:30:00Z".to_string(),
        location: String::new(),
        meeting_link: "https://meet.example/1".to_string(),
        description: "Daily sync".to_string(),
        note: String::new(),
        recurrence_series_id: "series-1".to_string(),
        has_recurrence_rules: true,
        is_all_day: false,
        provider: "google".to_string(),
        participants_json: Some(r#"[{"email":"a@example.com"}]"#.to_string()),
    }]);

    let vault = vault();
    write_json(vault.path(), &vault.path().join("calendars.json"), &value);
    write_json(vault.path(), &vault.path().join("events.json"), &events_value);

    let target = reimport(vault.path()).await;

    let (name, enabled, provider, color): (String, bool, String, String) = sqlx::query_as(
        "SELECT name, enabled, provider, color FROM calendars WHERE id = 'cal-1'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(name, "Work");
    assert!(enabled);
    assert_eq!(provider, "google");
    assert_eq!(color, "#123456");

    let (title, calendar_id, participants_json): (String, String, Option<String>) =
        sqlx::query_as(
            "SELECT title, calendar_id, participants_json FROM events WHERE id = 'event-1'",
        )
        .fetch_one(target.pool())
        .await
        .unwrap();
    assert_eq!(title, "Standup");
    assert_eq!(calendar_id, "cal-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&participants_json.unwrap()).unwrap(),
        serde_json::json!([{"email": "a@example.com"}])
    );
}

// ---------------------------------------------------------------------------
// daily_notes.json / tasks.json
// ---------------------------------------------------------------------------

#[tokio::test]
async fn daily_notes_round_trip() {
    let value = export::render_daily_notes(&[export::DailyNote {
        id: "note-1".to_string(),
        owner_user_id: "user-1".to_string(),
        note_date: "2026-07-01".to_string(),
        body: r#"{"type":"doc","content":[]}"#.to_string(),
    }]);

    let vault = vault();
    write_json(vault.path(), &vault.path().join("daily_notes.json"), &value);

    let target = reimport(vault.path()).await;
    let (note_date, body_format, body): (String, String, String) = sqlx::query_as(
        "SELECT note_date, body_format, body FROM daily_notes WHERE id = 'note-1'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(note_date, "2026-07-01");
    assert_eq!(body_format, "prosemirror_json");
    assert_eq!(body, r#"{"type":"doc","content":[]}"#);
}

#[tokio::test]
async fn tasks_round_trip() {
    let value = export::render_tasks(&[export::ActionItem {
        id: "task-1".to_string(),
        owner_user_id: "user-1".to_string(),
        source_type: "session".to_string(),
        source_id: "session-1".to_string(),
        source_order: 2,
        status: "done".to_string(),
        text: "Send follow-up".to_string(),
        body_json: r#"[{"type":"text","text":"Send follow-up"}]"#.to_string(),
        due_at: "2026-07-05".to_string(),
    }]);

    let vault = vault();
    // `action_items` rows insert via a `JOIN sessions` (they're scoped to an
    // existing session), so the referenced session must exist in the vault
    // too — matching this test's `source_id`.
    let session = export::SessionMeta {
        id: "session-1".to_string(),
        owner_user_id: "user-1".to_string(),
        title: "Planning".to_string(),
        created_at: "2026-07-01T00:00:00Z".to_string(),
        started_at: String::new(),
        ended_at: String::new(),
        event_id: String::new(),
        external_event_id: String::new(),
        series_id: String::new(),
        event_json: String::new(),
    };
    let meta_value = export::render_session_meta(&session, &[], &[], None);
    write_json(
        vault.path(),
        &vault.path().join("sessions/session-1/_meta.json"),
        &meta_value,
    );
    write_json(vault.path(), &vault.path().join("tasks.json"), &value);

    let target = reimport(vault.path()).await;
    let (source_type, source_id, source_order, status, text, due_at): (
        String,
        String,
        i64,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT source_type, source_id, source_order, status, text, due_at
         FROM action_items WHERE id = 'task-1'",
    )
    .fetch_one(target.pool())
    .await
    .unwrap();
    assert_eq!(source_type, "session");
    assert_eq!(source_id, "session-1");
    assert_eq!(source_order, 2);
    assert_eq!(status, "done");
    assert_eq!(text, "Send follow-up");
    assert_eq!(due_at, "2026-07-05");
}

// ---------------------------------------------------------------------------
// chats/<group>/messages.json
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_round_trip() {
    let value = export::render_chat(
        &export::ChatGroup {
            id: "chat-1".to_string(),
            owner_user_id: "user-1".to_string(),
            title: "Coach chat".to_string(),
            created_at: "2026-07-01T00:00:00Z".to_string(),
        },
        &[export::ChatMessage {
            id: "message-1".to_string(),
            chat_group_id: "chat-1".to_string(),
            owner_user_id: "user-1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            metadata_json: r#"{"source":"test"}"#.to_string(),
            parts_json: "[]".to_string(),
            status: "ready".to_string(),
            created_at: "2026-07-01T00:00:01Z".to_string(),
        }],
    );

    let vault = vault();
    write_json(vault.path(), &vault.path().join("chats/chat-1/messages.json"), &value);

    let target = reimport(vault.path()).await;
    let title: String = sqlx::query_scalar("SELECT title FROM chat_groups WHERE id = 'chat-1'")
        .fetch_one(target.pool())
        .await
        .unwrap();
    assert_eq!(title, "Coach chat");

    let (chat_group_id, role, content, metadata_json): (String, String, String, String) =
        sqlx::query_as(
            "SELECT chat_group_id, role, content, metadata_json
             FROM chat_messages WHERE id = 'message-1'",
        )
        .fetch_one(target.pool())
        .await
        .unwrap();
    assert_eq!(chat_group_id, "chat-1");
    assert_eq!(role, "user");
    assert_eq!(content, "Hello");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&metadata_json).unwrap(),
        serde_json::json!({"source": "test"})
    );
}

// ---------------------------------------------------------------------------
// settings.json (scoped to the single `legacy_settings_document` row — see
// `export::render_settings`'s doc comment for why the rest of `app_settings`
// intentionally isn't mirrored)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn settings_round_trip() {
    let value = export::render_settings(r#"{"theme":"dark","locale":"en"}"#);

    let vault = vault();
    write_json(vault.path(), &vault.path().join("settings.json"), &value);

    let target = reimport(vault.path()).await;
    let value_json: String =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE id = 'legacy_settings_document'")
            .fetch_one(target.pool())
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&value_json).unwrap(),
        serde_json::json!({"theme": "dark", "locale": "en"})
    );
}

// ---------------------------------------------------------------------------
// tags embedded in _meta.json
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_tags_round_trip_through_meta_json() {
    let session = export::SessionMeta {
        id: "session-1".to_string(),
        owner_user_id: "user-1".to_string(),
        title: "Planning".to_string(),
        created_at: "2026-07-01T00:00:00Z".to_string(),
        started_at: String::new(),
        ended_at: String::new(),
        event_id: String::new(),
        external_event_id: String::new(),
        series_id: String::new(),
        event_json: String::new(),
    };
    let value = export::render_session_meta(
        &session,
        &[],
        &["urgent".to_string(), "follow-up".to_string()],
        None,
    );

    let vault = vault();
    write_json(vault.path(), &vault.path().join("sessions/session-1/_meta.json"), &value);

    let target = reimport(vault.path()).await;
    let mut tag_ids: Vec<String> =
        sqlx::query_scalar("SELECT tag_id FROM session_tags WHERE session_id = 'session-1' ORDER BY tag_id")
            .fetch_all(target.pool())
            .await
            .unwrap();
    tag_ids.sort();
    assert_eq!(tag_ids, vec!["follow-up".to_string(), "urgent".to_string()]);

    let mut tag_names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM tags WHERE id IN ('urgent', 'follow-up') ORDER BY name",
    )
    .fetch_all(target.pool())
    .await
    .unwrap();
    tag_names.sort();
    assert_eq!(tag_names, vec!["follow-up".to_string(), "urgent".to_string()]);
}

// ---------------------------------------------------------------------------
// Controller re-drill: "empty aggregates write no file at all" — confirms
// this is the *intended*, importer-consistent behavior, not an omission.
// `vault_export.rs`'s `export_calendars_file`/`export_events_file`/
// `export_daily_notes_file`/`export_tasks_file`/`export_settings_file` all
// write nothing (and trash any stale file) when their table is empty, rather
// than writing an empty `{}`/`[]` placeholder. That's only safe if the
// *importer* treats a missing aggregate file identically to an empty one —
// this test proves it does, directly against `sync_from_vault`, for every
// aggregate kind at once: `discover_sources` walks the files that actually
// exist on disk, so a `calendars.json` that was never written is simply
// never discovered, never classified, and never parsed — the same "zero
// rows" outcome as parsing an empty `{}`. No special missing-file handling
// exists (or is needed) on the reconcile side.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn missing_aggregate_files_import_as_empty_not_as_an_error() {
    let vault = vault();
    // A vault with session content but none of the five aggregate files at
    // all — the common case for a fresh install that has never used
    // calendars, daily notes, tasks, or hit the legacy settings sentinel.
    let session = export::SessionMeta {
        id: "session-1".to_string(),
        owner_user_id: "user-1".to_string(),
        title: "Planning".to_string(),
        created_at: "2026-07-01T00:00:00Z".to_string(),
        started_at: String::new(),
        ended_at: String::new(),
        event_id: String::new(),
        external_event_id: String::new(),
        series_id: String::new(),
        event_json: String::new(),
    };
    let meta_value = export::render_session_meta(&session, &[], &[], None);
    write_json(
        vault.path(),
        &vault.path().join("sessions/session-1/_meta.json"),
        &meta_value,
    );
    assert!(!vault.path().join("calendars.json").exists());
    assert!(!vault.path().join("events.json").exists());
    assert!(!vault.path().join("daily_notes.json").exists());
    assert!(!vault.path().join("tasks.json").exists());
    assert!(!vault.path().join("settings.json").exists());

    let target = fresh_db().await;
    let report = tauri_plugin_db::sync_from_vault(target.pool(), vault.path())
        .await
        .expect("sync_from_vault must not error when aggregate files are simply absent");
    assert_eq!(report.conflict_count, 0);

    let session_title: String =
        sqlx::query_scalar("SELECT title FROM sessions WHERE id = 'session-1'")
            .fetch_one(target.pool())
            .await
            .unwrap();
    assert_eq!(session_title, "Planning");

    for (table, query) in [
        ("calendars", "SELECT COUNT(*) FROM calendars"),
        ("events", "SELECT COUNT(*) FROM events"),
        ("daily_notes", "SELECT COUNT(*) FROM daily_notes"),
        ("action_items", "SELECT COUNT(*) FROM action_items"),
        (
            "app_settings (legacy_settings_document)",
            "SELECT COUNT(*) FROM app_settings WHERE id = 'legacy_settings_document'",
        ),
    ] {
        let count: i64 = sqlx::query_scalar(query).fetch_one(target.pool()).await.unwrap();
        assert_eq!(count, 0, "{table} should have zero rows when its vault file is absent");
    }
}
