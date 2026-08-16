//! Every id-based reader must behave identically whether a session lives in a
//! legacy UUID-named directory or a human-readable (possibly nested, possibly
//! manually renamed) one — the full id from `_meta.json` is the only identity.

use std::path::Path;

use vault_read::{enhanced, meta, tasks, transcript};

const LEGACY_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const READABLE_ID: &str = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
const READABLE_DIR: &str = "sessions/Work/2026-03-20 — Product planning — 6ba7b8";

fn seed_full_session(vault: &Path, relative_dir: &str, id: &str, title: &str) {
    let dir = vault.join(relative_dir);
    std::fs::create_dir_all(dir.join("enhanced")).unwrap();
    std::fs::write(
        dir.join("_meta.json"),
        serde_json::json!({
            "id": id,
            "title": title,
            "started_at": "2026-03-20T09:00:00Z",
            "ended_at": null,
            "created_at": "2026-03-20T08:00:00Z",
            "tags": [],
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("_memo.md"), format!("note for {title}")).unwrap();
    std::fs::write(
        dir.join("summary.md"),
        format!("legacy summary for {title}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("enhanced/doc-1.md"),
        format!("---\nkind: summary\ntitle: Recap\nsort_order: 1\n---\n\nenhanced for {title}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("transcript.json"),
        serde_json::json!({
            "transcripts": [{
                "id": "t1",
                "session_id": id,
                "started_at": 0.0,
                "words": [
                    {"text": "hello", "start_ms": 0.0, "end_ms": 10.0, "channel": 0.0}
                ],
            }],
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("tasks.json"),
        serde_json::json!({
            "tasks": [{
                "id": "task-1",
                "source_type": "session_raw_note",
                "source_id": id,
                "source_order": 1,
                "status": "todo",
                "text": format!("task for {title}"),
                "body": [],
                "created_at": "2026-03-20T08:00:00Z",
                "updated_at": "2026-03-20T08:00:00Z",
            }],
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn all_readers_resolve_both_layouts_identically_by_full_id() {
    let vault = tempfile::tempdir().unwrap();
    seed_full_session(
        vault.path(),
        &format!("sessions/{LEGACY_ID}"),
        LEGACY_ID,
        "Legacy",
    );
    seed_full_session(vault.path(), READABLE_DIR, READABLE_ID, "Readable");

    let mut ids: Vec<String> = meta::list_session_metas(vault.path())
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    ids.sort();
    let mut expected = vec![LEGACY_ID.to_string(), READABLE_ID.to_string()];
    expected.sort();
    assert_eq!(ids, expected);

    for (id, title) in [(LEGACY_ID, "Legacy"), (READABLE_ID, "Readable")] {
        let session_meta = meta::read_session_meta(vault.path(), id).unwrap().unwrap();
        assert_eq!(session_meta.id, id);
        assert_eq!(session_meta.title, title);

        assert_eq!(
            meta::read_note(vault.path(), id).unwrap().unwrap(),
            format!("note for {title}")
        );

        let legacy_docs = meta::list_legacy_docs(vault.path(), id).unwrap();
        assert_eq!(legacy_docs.len(), 1);
        assert_eq!(
            legacy_docs[0].markdown,
            format!("legacy summary for {title}")
        );

        let docs = enhanced::list_enhanced_docs(vault.path(), id).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].session_id, id);
        assert_eq!(docs[0].markdown, format!("enhanced for {title}"));

        let transcripts = transcript::read_transcript_json(vault.path(), id)
            .unwrap()
            .transcripts;
        assert_eq!(transcripts.len(), 1);
        assert_eq!(transcripts[0].words[0].text, "hello");

        let task_items = tasks::read_session_tasks(vault.path(), id).unwrap();
        assert_eq!(task_items.len(), 1);
        assert_eq!(task_items[0].text, format!("task for {title}"));
    }

    // Looking a readable-layout session up by its directory basename must fail:
    // names are presentation, not identity.
    assert!(
        meta::read_session_meta(vault.path(), "2026-03-20 — Product planning — 6ba7b8")
            .unwrap()
            .is_none()
    );
}
