//! Render helpers for the DB-to-vault write-through mirror (Task 13).
//!
//! These functions are pure: given plain row data (already fetched from
//! SQLite by the caller — the `vault_export` worker in
//! `apps/desktop/src-tauri/src/vault_export.rs`, or a test), they produce the
//! exact vault file shapes `plugins/db`'s legacy-vault importer
//! (`plugins/db/src/import/legacy_vault.rs::classify_source`/`parse_source`)
//! expects to read back. That importer is the round-trip authority: every
//! shape below was written by reading its `parse_*` functions, not by
//! inventing a new format.
//!
//! This module intentionally has no `sqlx`/DB dependency so it's callable
//! (and unit-testable) without a database or the Tauri command layer — see
//! the brief for Task 13.
//!
//! `write_file_atomic` and the `.trash/` helpers below also back the
//! (otherwise unrelated) `write_document_batch`/`write_json_batch` Tauri
//! commands in `plugins/fs-sync/src/commands.rs`, which used to duplicate
//! this create-parent-dir-and-write logic inline.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::frontmatter::ParsedDocument;
use crate::types::{TranscriptJson, TranscriptSpeakerHint, TranscriptWithData, TranscriptWord};

// ---------------------------------------------------------------------------
// Atomic, Drive-friendly file writes + soft-delete (never hard-delete on a
// synced vault).
// ---------------------------------------------------------------------------

/// Computes the sibling temp-file path `write_file_atomic` will stage
/// through before renaming into place. Exposed so callers that need to mark
/// it as an "own write" *before* the write happens (loop prevention — see
/// `vault_export.rs`'s module doc) can compute it once and pass the same
/// path into `write_file_atomic`, rather than each side independently
/// generating a (different, nonce-based) tmp path. Starts with `.tmp` to
/// match both the repo's tempfile convention
/// (<https://docs.rs/tempfile/latest/tempfile/struct.Builder.html#method.prefix>)
/// and `plugins/notify`'s `should_skip_path`, which ignores any path whose
/// filename starts with `.tmp`.
pub fn tmp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!(".tmp-{}-{nonce}-{file_name}", std::process::id()))
}

/// Writes `content` to `path` via a temp-file-then-rename (`tmp_path`, see
/// `tmp_sibling_path`) so a reader (or a sync client like Google Drive/iCloud)
/// never observes a partially written file. Creates the parent directory if
/// needed.
///
/// Returns `Ok(false)` without touching the filesystem when `path` already
/// holds byte-identical content — this is what breaks the export-worker/
/// vault-watcher feedback loop (see `vault_export.rs`'s module doc).
///
/// When `path` exists with **different** content, the existing file is moved
/// to `<vault_base>/.trash/<date>/...` (via `move_to_trash`) *before* the new
/// content is written — never silently overwritten. Renders are strict
/// subset projections of the DB rows they came from (see `export.rs`'s
/// module doc): a legacy or hand-edited vault file can carry frontmatter
/// keys or JSON fields our render functions don't know how to reproduce, and
/// those would otherwise be destroyed permanently and irrecoverably on the
/// very first export pass. Deletions already get this safety (`move_to_trash`
/// below); overwrites deserve the same.
pub fn write_file_atomic(
    vault_base: &Path,
    path: &Path,
    tmp_path: &Path,
    content: &[u8],
) -> crate::Result<bool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            crate::Error::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "failed to create parent directory {} for {}: {error}",
                    parent.display(),
                    path.display()
                ),
            ))
        })?;
    }

    if let Ok(existing) = std::fs::read(path) {
        if existing == content {
            return Ok(false);
        }
        move_to_trash(vault_base, path)?;
    }

    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    {
        use std::io::Write;
        let mut file = std::fs::File::create(tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    std::fs::rename(tmp_path, path)?;
    Ok(true)
}

/// Moves `path` (a file or a whole directory) to `<vault_base>/.trash/<UTC
/// date>/<relative path>`, creating parent directories as needed and
/// disambiguating with a numeric suffix if something is already there. Used
/// for every vault-export "the DB row is gone" case — deletions must never
/// destroy vault content outright (Drive/iCloud-friendly, and it doubles as
/// an undo buffer). No-ops (returns `Ok(None)`) if `path` doesn't exist.
pub fn move_to_trash(vault_base: &Path, path: &Path) -> crate::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let relative = path.strip_prefix(vault_base).unwrap_or(path);
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut target = vault_base.join(".trash").join(date).join(relative);

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    target = unique_path(target);
    std::fs::rename(path, &target)?;
    Ok(Some(target))
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("item")
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_string);

    let mut counter = 1;
    loop {
        let candidate_name = match &extension {
            Some(extension) => format!("{stem}-{counter}.{extension}"),
            None => format!("{stem}-{counter}"),
        };
        let candidate = path.with_file_name(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

// ---------------------------------------------------------------------------
// sessions/<folder>/<id>/_meta.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    pub id: String,
    pub owner_user_id: String,
    pub title: String,
    pub created_at: String,
    pub started_at: String,
    pub ended_at: String,
    pub event_id: String,
    pub external_event_id: String,
    pub series_id: String,
    /// Raw `sessions.event_json` text; may be empty.
    pub event_json: String,
}

#[derive(Debug, Clone)]
pub struct SessionParticipant {
    pub id: String,
    pub owner_user_id: String,
    pub human_id: String,
    pub source: String,
    /// Not read back by `parse_session_meta` today (only `id`/`user_id`/
    /// `human_id`/`source` are) — kept for forward compatibility so nothing
    /// needs to change here if the importer ever grows to read them.
    pub display_name: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct SessionKeyFacts {
    pub content: String,
    pub source_hash: String,
    pub user_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Builds the `_meta.json` JSON value for one session. `key_facts` is the
/// (at most one) `session_documents` row of `kind = 'key_facts'` for this
/// session, if any — it round-trips through `_meta.json` itself rather than
/// its own file (matching `parse_session_meta`'s `key_facts` object).
pub fn render_session_meta(
    session: &SessionMeta,
    participants: &[SessionParticipant],
    tags: &[String],
    key_facts: Option<&SessionKeyFacts>,
) -> Value {
    let mut root = Map::new();
    root.insert("id".to_string(), json!(session.id));
    root.insert("user_id".to_string(), json!(session.owner_user_id));
    root.insert("title".to_string(), json!(session.title));
    root.insert("created_at".to_string(), json!(session.created_at));
    if !session.event_id.is_empty() {
        root.insert("event_id".to_string(), json!(session.event_id));
    }

    if let Some(event) = render_session_event(session) {
        root.insert("event".to_string(), event);
    }

    root.insert(
        "participants".to_string(),
        Value::Array(
            participants
                .iter()
                .map(|participant| {
                    json!({
                        "id": participant.id,
                        "user_id": participant.owner_user_id,
                        "human_id": participant.human_id,
                        "source": participant.source,
                        "display_name": participant.display_name,
                        "email": participant.email,
                        "role": participant.role,
                    })
                })
                .collect(),
        ),
    );

    if let Some(key_facts) = key_facts {
        root.insert(
            "key_facts".to_string(),
            json!({
                "content": key_facts.content,
                "source_hash": key_facts.source_hash,
                "user_id": key_facts.user_id,
                "created_at": key_facts.created_at,
                "updated_at": key_facts.updated_at,
            }),
        );
    }

    root.insert("tags".to_string(), json!(tags));

    Value::Object(root)
}

/// The nested `event` object embeds `started_at`/`ended_at`/`tracking_id`/
/// `recurrence_series_id` alongside whatever else `event_json` already
/// carries — `parse_session_meta` reads the session's `started_at`/
/// `ended_at`/`series_id`/`external_event_id` columns *from inside* this
/// object, not from top-level keys, so the DB columns (source of truth) are
/// patched in over whatever `event_json` last cached.
fn render_session_event(session: &SessionMeta) -> Option<Value> {
    let mut event = if session.event_json.trim().is_empty() {
        Map::new()
    } else {
        match serde_json::from_str::<Value>(&session.event_json) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        }
    };

    set_or_remove(&mut event, "tracking_id", &session.external_event_id);
    set_or_remove(&mut event, "started_at", &session.started_at);
    set_or_remove(&mut event, "ended_at", &session.ended_at);
    set_or_remove(&mut event, "recurrence_series_id", &session.series_id);

    if event.is_empty() {
        None
    } else {
        Some(Value::Object(event))
    }
}

fn set_or_remove(map: &mut Map<String, Value>, key: &str, value: &str) {
    if value.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), json!(value));
    }
}

// ---------------------------------------------------------------------------
// sessions/<folder>/<id>/{_memo.md,_summary.md,<id>.md}
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SessionDocument {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub template_id: String,
    pub title: String,
    /// `"markdown"` or `"prosemirror_json"`.
    pub body_format: String,
    pub body: String,
    pub sort_order: i64,
}

/// Filename `classify_source`/`parse_session_document` will read this
/// document back from, or `None` if this `kind` has no vault-file slot at
/// all (`meeting_chat` — no vault representation exists; `key_facts` —
/// embedded in `_meta.json` by `render_session_meta` instead, handled by the
/// caller before it ever reaches this function).
///
/// `is_first_of_kind` picks the single canonical `_memo.md`/`_summary.md`
/// slot for the first `note`/`summary` row (by the caller's chosen
/// ordering — mirror `search_index.rs`'s `note`/`enhanced_bodies`
/// ordering); additional rows of the same kind fall back to `<id>.md`, which
/// `classify_source`'s fallback (`else -> summary`) still re-imports as
/// `summary` when `template_id` is empty.
pub fn session_document_filename(doc: &SessionDocument, is_first_of_kind: bool) -> Option<String> {
    match doc.kind.as_str() {
        "note" if is_first_of_kind => Some("_memo.md".to_string()),
        "summary" if is_first_of_kind => Some("_summary.md".to_string()),
        "note" | "summary" | "template_output" => Some(format!("{}.md", doc.id)),
        _ => None,
    }
}

/// Builds the `ParsedDocument` (frontmatter + markdown body) for a document.
/// Prosemirror JSON bodies are converted to markdown — the vault always
/// stores markdown, so re-importing a prosemirror-authored note normalizes
/// `body_format` to `"markdown"` (this is `render_document_body_as_markdown`
/// in `legacy_vault.rs`'s own conflict-backup path, applied consistently
/// here too).
pub fn render_session_document(doc: &SessionDocument) -> crate::Result<ParsedDocument> {
    let content = if doc.body_format == "prosemirror_json" {
        let parsed: Value = serde_json::from_str(&doc.body)
            .map_err(|error| crate::Error::Markdown(format!("invalid prosemirror body: {error}")))?;
        hypr_tiptap::tiptap_json_to_md(&parsed).map_err(crate::Error::Markdown)?
    } else {
        doc.body.clone()
    };

    let mut frontmatter = std::collections::HashMap::new();
    frontmatter.insert("id".to_string(), json!(doc.id));
    frontmatter.insert("session_id".to_string(), json!(doc.session_id));
    if !doc.template_id.is_empty() {
        frontmatter.insert("template_id".to_string(), json!(doc.template_id));
    }
    if !doc.title.is_empty() {
        frontmatter.insert("title".to_string(), json!(doc.title));
    }
    frontmatter.insert("position".to_string(), json!(doc.sort_order));

    Ok(ParsedDocument {
        frontmatter,
        content,
    })
}

// ---------------------------------------------------------------------------
// sessions/<folder>/<id>/transcript.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Transcript {
    pub id: String,
    pub owner_user_id: String,
    pub session_id: String,
    pub created_at: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub memo: String,
    /// Raw `transcripts.words_json` text (a JSON array).
    pub words_json: String,
    /// Raw `transcripts.speaker_hints_json` text (a JSON array).
    pub speaker_hints_json: String,
}

pub fn render_transcripts(rows: &[Transcript]) -> Value {
    let transcripts = rows
        .iter()
        .map(|row| TranscriptWithData {
            id: row.id.clone(),
            user_id: row.owner_user_id.clone(),
            created_at: row.created_at.clone(),
            session_id: row.session_id.clone(),
            started_at: row.started_at_ms as f64,
            ended_at: row.ended_at_ms.map(|value| value as f64),
            memo_md: row.memo.clone(),
            words: serde_json::from_str::<Vec<TranscriptWord>>(&row.words_json)
                .unwrap_or_default(),
            speaker_hints: serde_json::from_str::<Vec<TranscriptSpeakerHint>>(
                &row.speaker_hints_json,
            )
            .unwrap_or_default(),
        })
        .collect();

    serde_json::to_value(TranscriptJson { transcripts }).unwrap_or(json!({ "transcripts": [] }))
}

// ---------------------------------------------------------------------------
// humans/<id>.md, organizations/<id>.md
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Human {
    pub owner_user_id: String,
    pub organization_id: String,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub job_title: String,
    pub linkedin_username: String,
    pub memo: String,
    pub pinned: bool,
    pub pin_order: Option<i64>,
    pub created_at: String,
}

pub fn render_human(human: &Human) -> ParsedDocument {
    let mut frontmatter = std::collections::HashMap::new();
    frontmatter.insert("user_id".to_string(), json!(human.owner_user_id));
    frontmatter.insert("org_id".to_string(), json!(human.organization_id));
    frontmatter.insert("name".to_string(), json!(human.name));
    frontmatter.insert("email".to_string(), json!(human.email));
    frontmatter.insert("phone".to_string(), json!(human.phone));
    frontmatter.insert("job_title".to_string(), json!(human.job_title));
    frontmatter.insert(
        "linkedin_username".to_string(),
        json!(human.linkedin_username),
    );
    frontmatter.insert("pinned".to_string(), json!(human.pinned));
    frontmatter.insert("pin_order".to_string(), json!(human.pin_order));
    frontmatter.insert("created_at".to_string(), json!(human.created_at));

    ParsedDocument {
        frontmatter,
        content: human.memo.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct Organization {
    pub owner_user_id: String,
    pub name: String,
    pub memo: String,
    pub pinned: bool,
    pub pin_order: Option<i64>,
    pub created_at: String,
}

pub fn render_organization(organization: &Organization) -> ParsedDocument {
    let mut frontmatter = std::collections::HashMap::new();
    frontmatter.insert("user_id".to_string(), json!(organization.owner_user_id));
    frontmatter.insert("name".to_string(), json!(organization.name));
    frontmatter.insert("pinned".to_string(), json!(organization.pinned));
    frontmatter.insert("pin_order".to_string(), json!(organization.pin_order));
    frontmatter.insert("created_at".to_string(), json!(organization.created_at));

    ParsedDocument {
        frontmatter,
        content: organization.memo.clone(),
    }
}

// ---------------------------------------------------------------------------
// calendars.json / events.json / daily_notes.json / tasks.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Calendar {
    pub id: String,
    pub tracking_id_calendar: String,
    pub name: String,
    pub enabled: bool,
    pub provider: String,
    pub source: String,
    pub color: String,
    pub connection_id: String,
}

pub fn render_calendars(rows: &[Calendar]) -> Value {
    Value::Object(
        rows.iter()
            .map(|row| {
                (
                    row.id.clone(),
                    json!({
                        "tracking_id_calendar": row.tracking_id_calendar,
                        "name": row.name,
                        "enabled": row.enabled,
                        "provider": row.provider,
                        "source": row.source,
                        "color": row.color,
                        "connection_id": row.connection_id,
                    }),
                )
            })
            .collect(),
    )
}

#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub id: String,
    pub tracking_id_event: String,
    pub calendar_id: String,
    pub title: String,
    pub started_at: String,
    pub ended_at: String,
    pub location: String,
    pub meeting_link: String,
    pub description: String,
    pub note: String,
    pub recurrence_series_id: String,
    pub has_recurrence_rules: bool,
    pub is_all_day: bool,
    pub provider: String,
    /// Raw `events.participants_json` text, if any.
    pub participants_json: Option<String>,
}

pub fn render_events(rows: &[CalendarEvent]) -> Value {
    Value::Object(
        rows.iter()
            .map(|row| {
                let mut entry = Map::new();
                entry.insert("tracking_id_event".to_string(), json!(row.tracking_id_event));
                entry.insert("calendar_id".to_string(), json!(row.calendar_id));
                entry.insert("title".to_string(), json!(row.title));
                entry.insert("started_at".to_string(), json!(row.started_at));
                entry.insert("ended_at".to_string(), json!(row.ended_at));
                entry.insert("location".to_string(), json!(row.location));
                entry.insert("meeting_link".to_string(), json!(row.meeting_link));
                entry.insert("description".to_string(), json!(row.description));
                entry.insert("note".to_string(), json!(row.note));
                entry.insert(
                    "recurrence_series_id".to_string(),
                    json!(row.recurrence_series_id),
                );
                entry.insert(
                    "has_recurrence_rules".to_string(),
                    json!(row.has_recurrence_rules),
                );
                entry.insert("is_all_day".to_string(), json!(row.is_all_day));
                entry.insert("provider".to_string(), json!(row.provider));
                if let Some(participants_json) = &row.participants_json {
                    let value = serde_json::from_str(participants_json).unwrap_or(Value::Null);
                    entry.insert("participants".to_string(), value);
                }
                (row.id.clone(), Value::Object(entry))
            })
            .collect(),
    )
}

#[derive(Debug, Clone)]
pub struct DailyNote {
    pub id: String,
    pub owner_user_id: String,
    pub note_date: String,
    /// Raw `daily_notes.body` text (a serialized prosemirror document).
    pub body: String,
}

pub fn render_daily_notes(rows: &[DailyNote]) -> Value {
    Value::Object(
        rows.iter()
            .map(|row| {
                (
                    row.id.clone(),
                    json!({
                        "user_id": row.owner_user_id,
                        "date": row.note_date,
                        "content": row.body,
                    }),
                )
            })
            .collect(),
    )
}

#[derive(Debug, Clone)]
pub struct ActionItem {
    pub id: String,
    pub owner_user_id: String,
    pub source_type: String,
    pub source_id: String,
    pub source_order: i64,
    pub status: String,
    pub text: String,
    /// Raw `action_items.body_json` text.
    pub body_json: String,
    pub due_at: String,
}

pub fn render_tasks(rows: &[ActionItem]) -> Value {
    Value::Object(
        rows.iter()
            .map(|row| {
                let body = serde_json::from_str(&row.body_json).unwrap_or(Value::Array(Vec::new()));
                (
                    row.id.clone(),
                    json!({
                        "user_id": row.owner_user_id,
                        "source_type": row.source_type,
                        "source_id": row.source_id,
                        "source_order": row.source_order,
                        "status": row.status,
                        "text_preview": row.text,
                        "body": body,
                        "due_date": row.due_at,
                    }),
                )
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// chats/<group>/messages.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ChatGroup {
    pub id: String,
    pub owner_user_id: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub chat_group_id: String,
    pub owner_user_id: String,
    pub role: String,
    pub content: String,
    /// Raw `chat_messages.metadata_json` text.
    pub metadata_json: String,
    /// Raw `chat_messages.parts_json` text.
    pub parts_json: String,
    pub status: String,
    pub created_at: String,
}

pub fn render_chat(group: &ChatGroup, messages: &[ChatMessage]) -> Value {
    json!({
        "chat_group": {
            "id": group.id,
            "user_id": group.owner_user_id,
            "title": group.title,
            "created_at": group.created_at,
        },
        "messages": messages
            .iter()
            .map(|message| {
                json!({
                    "id": message.id,
                    "chat_group_id": message.chat_group_id,
                    "user_id": message.owner_user_id,
                    "role": message.role,
                    "content": message.content,
                    "metadata": serde_json::from_str::<Value>(&message.metadata_json).unwrap_or(json!({})),
                    "parts": serde_json::from_str::<Value>(&message.parts_json).unwrap_or(json!([])),
                    "status": message.status,
                    "created_at": message.created_at,
                })
            })
            .collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------
// settings.json
// ---------------------------------------------------------------------------

/// `parse_settings` only ever produces a single `app_settings` row keyed
/// `legacy_settings_document` holding the *entire* file's parsed JSON — the
/// canonical `app_settings` table is otherwise a genuine multi-row key/value
/// store (provider configs, cloudsync bookkeeping, ...) with no vault
/// representation. So this renders only that one row's `value_json`
/// verbatim; see the Task 13 report for why the rest intentionally isn't
/// mirrored.
pub fn render_settings(value_json: &str) -> Value {
    serde_json::from_str(value_json).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_via_tmp(vault_base: &Path, path: &Path, content: &[u8]) -> crate::Result<bool> {
        let tmp_path = tmp_sibling_path(path);
        write_file_atomic(vault_base, path, &tmp_path, content)
    }

    #[test]
    fn write_file_atomic_creates_parent_dirs_and_writes_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join("dir").join("file.json");

        let wrote = write_via_tmp(temp.path(), &path, b"hello").unwrap();

        assert!(wrote);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn write_file_atomic_skips_byte_identical_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file.json");
        std::fs::write(&path, b"same").unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        let wrote = write_via_tmp(temp.path(), &path, b"same").unwrap();

        assert!(!wrote);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn write_file_atomic_overwrites_changed_content_without_leaving_tmp_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file.json");
        std::fs::write(&path, b"old").unwrap();

        let wrote = write_via_tmp(temp.path(), &path, b"new").unwrap();

        assert!(wrote);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let leftovers = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    /// The critical fix from whole-branch review: renders are strict subset
    /// projections, so a byte-different overwrite must never just discard
    /// whatever was there before — it has to land in `.trash/` first.
    #[test]
    fn write_file_atomic_trashes_the_old_bytes_before_overwriting_changed_content() {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path();
        std::fs::create_dir_all(vault.join("sessions/abc")).unwrap();
        let path = vault.join("sessions/abc/_memo.md");
        std::fs::write(&path, "---\nid: doc-1\ncustom_legacy_key: keep-me\n---\n\nOld body").unwrap();

        let wrote = write_via_tmp(vault, &path, b"---\nid: doc-1\n---\n\nNew body").unwrap();

        assert!(wrote);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "---\nid: doc-1\n---\n\nNew body"
        );
        let trashed = vault
            .join(".trash")
            .join(chrono::Utc::now().format("%Y-%m-%d").to_string())
            .join("sessions/abc/_memo.md");
        assert!(trashed.is_file(), "old bytes should be preserved in .trash");
        let trashed_content = std::fs::read_to_string(&trashed).unwrap();
        assert!(trashed_content.contains("custom_legacy_key: keep-me"));
        assert!(trashed_content.contains("Old body"));
    }

    /// Same fix, phrased exactly as the review's reproduction case: a
    /// pre-existing vault file with a frontmatter key our renderer doesn't
    /// (and can't) model must survive the first export pass, just relocated.
    #[test]
    fn write_file_atomic_preserves_unmodeled_frontmatter_keys_on_first_export() {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path();
        let path = vault.join("humans/human-1.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy_content =
            "---\nname: Ada Lovelace\nlegacy_crm_id: crm-9182\n---\n\nHand-written notes.";
        std::fs::write(&path, legacy_content).unwrap();

        let rendered = super::render_human(&Human {
            owner_user_id: String::new(),
            organization_id: String::new(),
            name: "Ada Lovelace".to_string(),
            email: String::new(),
            phone: String::new(),
            job_title: String::new(),
            linkedin_username: String::new(),
            memo: "Hand-written notes.".to_string(),
            pinned: false,
            pin_order: None,
            created_at: String::new(),
        })
        .render()
        .unwrap();

        let wrote = write_via_tmp(vault, &path, rendered.as_bytes()).unwrap();

        assert!(wrote, "the render doesn't reproduce legacy_crm_id, so bytes differ");
        assert!(!std::fs::read_to_string(&path).unwrap().contains("legacy_crm_id"));
        let trashed = vault
            .join(".trash")
            .join(chrono::Utc::now().format("%Y-%m-%d").to_string())
            .join("humans/human-1.md");
        assert!(trashed.is_file());
        assert!(std::fs::read_to_string(&trashed).unwrap().contains("legacy_crm_id: crm-9182"));
    }

    #[test]
    fn write_file_atomic_error_message_names_parent_and_target() {
        let temp = tempfile::tempdir().unwrap();
        let blocker = temp.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let target = blocker.join("child").join("file.json");

        let error = write_via_tmp(temp.path(), &target, b"x").unwrap_err();

        let message = error.to_string();
        assert!(message.contains("failed to create parent directory"));
        assert!(message.contains(&target.parent().unwrap().display().to_string()));
        assert!(message.contains(&target.display().to_string()));
    }

    #[test]
    fn tmp_sibling_path_starts_with_dot_tmp_matching_notify_skip_convention() {
        let path = Path::new("/vault/sessions/abc/_meta.json");

        let tmp = tmp_sibling_path(path);

        let name = tmp.file_name().and_then(|value| value.to_str()).unwrap();
        assert!(name.starts_with(".tmp"), "got {name}");
        assert_eq!(tmp.parent(), path.parent());
    }

    #[test]
    fn move_to_trash_relocates_under_dated_trash_dir_and_disambiguates() {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path();
        std::fs::create_dir_all(vault.join("sessions/abc")).unwrap();
        std::fs::write(vault.join("sessions/abc/_meta.json"), b"{}").unwrap();

        let moved = move_to_trash(vault, &vault.join("sessions/abc/_meta.json"))
            .unwrap()
            .unwrap();

        assert!(!vault.join("sessions/abc/_meta.json").exists());
        assert!(moved.starts_with(vault.join(".trash")));
        assert!(moved.ends_with("sessions/abc/_meta.json"));

        // A second file trashed at the same relative path the same day must
        // not clobber the first.
        std::fs::create_dir_all(vault.join("sessions/abc")).unwrap();
        std::fs::write(vault.join("sessions/abc/_meta.json"), b"{\"again\":true}").unwrap();
        let moved_again = move_to_trash(vault, &vault.join("sessions/abc/_meta.json"))
            .unwrap()
            .unwrap();

        assert_ne!(moved, moved_again);
        assert!(moved.exists());
        assert!(moved_again.exists());
    }

    #[test]
    fn move_to_trash_missing_path_is_a_noop() {
        let temp = tempfile::tempdir().unwrap();

        let result = move_to_trash(temp.path(), &temp.path().join("missing.json")).unwrap();

        assert_eq!(result, None);
    }

    #[test]
    fn session_document_filename_maps_kinds_to_the_importer_contract() {
        let mut note = SessionDocument {
            id: "doc-1".into(),
            session_id: "session-1".into(),
            kind: "note".into(),
            template_id: String::new(),
            title: String::new(),
            body_format: "markdown".into(),
            body: String::new(),
            sort_order: 0,
        };
        assert_eq!(
            session_document_filename(&note, true).as_deref(),
            Some("_memo.md")
        );
        assert_eq!(
            session_document_filename(&note, false).as_deref(),
            Some("doc-1.md")
        );

        note.kind = "summary".into();
        assert_eq!(
            session_document_filename(&note, true).as_deref(),
            Some("_summary.md")
        );
        assert_eq!(
            session_document_filename(&note, false).as_deref(),
            Some("doc-1.md")
        );

        note.kind = "template_output".into();
        assert_eq!(
            session_document_filename(&note, true).as_deref(),
            Some("doc-1.md")
        );

        note.kind = "meeting_chat".into();
        assert_eq!(session_document_filename(&note, true), None);

        note.kind = "key_facts".into();
        assert_eq!(session_document_filename(&note, true), None);
    }

    #[test]
    fn render_session_document_converts_prosemirror_body_to_markdown() {
        let doc = SessionDocument {
            id: "doc-1".into(),
            session_id: "session-1".into(),
            kind: "note".into(),
            template_id: String::new(),
            title: "Title".into(),
            body_format: "prosemirror_json".into(),
            body: r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hello"}]}]}"#
                .into(),
            sort_order: 2,
        };

        let rendered = render_session_document(&doc).unwrap();

        assert_eq!(rendered.frontmatter["id"], json!("doc-1"));
        assert_eq!(rendered.frontmatter["session_id"], json!("session-1"));
        assert_eq!(rendered.frontmatter["position"], json!(2));
        assert!(rendered.content.contains("hello"));
    }

    #[test]
    fn render_session_meta_patches_event_fields_from_columns_over_event_json() {
        let session = SessionMeta {
            id: "session-1".into(),
            owner_user_id: "user-1".into(),
            title: "Planning".into(),
            created_at: "2026-07-01T00:00:00Z".into(),
            started_at: "2026-07-01T10:00:00Z".into(),
            ended_at: "2026-07-01T10:30:00Z".into(),
            event_id: "event-1".into(),
            external_event_id: "track-1".into(),
            series_id: "series-1".into(),
            event_json: r#"{"stale_field":"kept"}"#.into(),
        };

        let value = render_session_meta(&session, &[], &[], None);

        assert_eq!(value["id"], json!("session-1"));
        assert_eq!(value["event_id"], json!("event-1"));
        assert_eq!(value["event"]["tracking_id"], json!("track-1"));
        assert_eq!(value["event"]["started_at"], json!("2026-07-01T10:00:00Z"));
        assert_eq!(value["event"]["ended_at"], json!("2026-07-01T10:30:00Z"));
        assert_eq!(value["event"]["recurrence_series_id"], json!("series-1"));
        assert_eq!(value["event"]["stale_field"], json!("kept"));
        assert_eq!(value["tags"], json!([]));
    }

    #[test]
    fn render_transcripts_matches_the_importer_field_names() {
        let value = render_transcripts(&[Transcript {
            id: "transcript-1".into(),
            owner_user_id: "user-1".into(),
            session_id: "session-1".into(),
            created_at: "2026-07-01T00:00:00Z".into(),
            started_at_ms: 1000,
            ended_at_ms: Some(2000),
            memo: "memo".into(),
            words_json: r#"[{"id":"w1","text":"hi","start_ms":0,"end_ms":100,"channel":0}]"#
                .into(),
            speaker_hints_json: "[]".into(),
        }]);

        let entry = &value["transcripts"][0];
        assert_eq!(entry["id"], json!("transcript-1"));
        assert_eq!(entry["memo_md"], json!("memo"));
        assert_eq!(entry["started_at"], json!(1000.0));
        assert_eq!(entry["ended_at"], json!(2000.0));
        assert_eq!(entry["words"][0]["text"], json!("hi"));
    }

    #[test]
    fn render_calendars_keys_by_row_id() {
        let value = render_calendars(&[Calendar {
            id: "cal-1".into(),
            tracking_id_calendar: "track-1".into(),
            name: "Work".into(),
            enabled: true,
            provider: "google".into(),
            source: "team".into(),
            color: "#111111".into(),
            connection_id: "conn-1".into(),
        }]);

        assert_eq!(value["cal-1"]["name"], json!("Work"));
        assert_eq!(value["cal-1"]["enabled"], json!(true));
    }

    #[test]
    fn render_settings_parses_the_singleton_value_json() {
        let value = render_settings(r#"{"theme":"dark"}"#);
        assert_eq!(value["theme"], json!("dark"));
    }
}
