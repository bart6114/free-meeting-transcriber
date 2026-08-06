#![forbid(unsafe_code)]

mod search;

pub use search::{
    DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT, SearchHit, SearchKind, SearchMeetingsInput, SearchPage,
    search_meetings,
};

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

pub const DEFAULT_LIST_LIMIT: u32 = 20;
pub const MAX_LIST_LIMIT: u32 = 200;
pub const DEFAULT_TRANSCRIPT_LIMIT: u32 = 200;
pub const MAX_TRANSCRIPT_LIMIT: u32 = 500;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{action} failed: {reason}")]
    Vault {
        action: &'static str,
        reason: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub struct ListMeetingsInput {
    #[schemars(description = "Case-insensitive title or meeting id substring")]
    pub query: Option<String>,
    #[schemars(description = "Maximum results; defaults to 20 and is capped at 200")]
    #[schemars(range(min = 1, max = 200))]
    pub limit: Option<u32>,
    #[schemars(description = "Number of results to skip; defaults to 0")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub struct GetMeetingInput {
    #[schemars(description = "Free Meeting Transcriber meeting id")]
    pub meeting_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "snake_case")]
pub struct GetMeetingTranscriptInput {
    #[schemars(description = "Free Meeting Transcriber meeting id")]
    pub meeting_id: String,
    #[schemars(description = "Word offset; defaults to 0")]
    pub offset: Option<u32>,
    #[schemars(description = "Maximum words; defaults to 200 and is capped at 500")]
    #[schemars(range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct Pagination {
    pub offset: u32,
    pub limit: u32,
    pub returned: usize,
    pub total: Option<usize>,
    pub next_offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct MeetingListItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: String,
    pub ended_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct MeetingPage {
    pub meetings: Vec<MeetingListItem>,
    pub pagination: Pagination,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct TranscriptPage {
    pub meeting_id: String,
    pub text: String,
    pub words: Vec<Value>,
    pub pagination: Pagination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct Document {
    pub id: String,
    pub kind: String,
    pub template_id: String,
    pub title: String,
    pub markdown: String,
    pub sort_order: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct ActionItem {
    pub id: String,
    pub assignee_human_id: String,
    pub status: String,
    pub text: String,
    pub due_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: String,
    pub ended_at: String,
    pub timezone: String,
    pub language: String,
    pub note: Option<Document>,
    pub summaries: Vec<Document>,
    pub action_items: Vec<ActionItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct Transcript {
    pub id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub memo: String,
    pub text: String,
    pub words: Vec<Value>,
    pub speaker_hints: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct MeetingExport {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub transcripts: Vec<Transcript>,
}

pub async fn list_meetings(vault: &Path, input: ListMeetingsInput) -> Result<MeetingPage> {
    run_blocking("list meetings", vault, move |vault| {
        list_meetings_sync(vault, input)
    })
    .await
}

pub async fn get_meeting(vault: &Path, input: GetMeetingInput) -> Result<Meeting> {
    run_blocking("load meeting", vault, move |vault| {
        get_meeting_sync(vault, &input.meeting_id)
    })
    .await
}

pub async fn get_meeting_transcript(
    vault: &Path,
    input: GetMeetingTranscriptInput,
) -> Result<TranscriptPage> {
    run_blocking("load transcript", vault, move |vault| {
        get_meeting_transcript_sync(vault, input)
    })
    .await
}

pub async fn get_meeting_export(vault: &Path, meeting_id: String) -> Result<MeetingExport> {
    run_blocking("export meeting", vault, move |vault| {
        let meeting = get_meeting_sync(vault, &meeting_id)?;
        let transcripts = load_transcripts_sync(vault, &meeting_id)?;
        Ok(MeetingExport {
            meeting,
            transcripts,
        })
    })
    .await
}

async fn run_blocking<T, F>(action: &'static str, vault: &Path, operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Path) -> Result<T> + Send + 'static,
{
    let vault: PathBuf = vault.to_path_buf();
    tokio::task::spawn_blocking(move || operation(&vault))
        .await
        .map_err(|error| Error::Vault {
            action,
            reason: format!("task join error: {error}"),
        })?
}

fn vault_error(action: &'static str) -> impl Fn(hypr_vault_read::Error) -> Error {
    move |error| Error::Vault {
        action,
        reason: error.to_string(),
    }
}

fn list_meetings_sync(vault: &Path, input: ListMeetingsInput) -> Result<MeetingPage> {
    let limit = input
        .limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let offset = input.offset.unwrap_or(0);

    let mut metas =
        hypr_vault_read::meta::list_session_metas(vault).map_err(vault_error("list meetings"))?;

    if let Some(search) = input
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        let search = search.to_lowercase();
        metas.retain(|meta| {
            meta.title.to_lowercase().contains(&search) || meta.id.to_lowercase().contains(&search)
        });
    }

    sort_metas_recent_first(&mut metas);

    let mut meetings = metas
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize + 1)
        .map(|meta| meeting_list_item(vault, meta))
        .collect::<Vec<_>>();
    let has_more = meetings.len() > limit as usize;
    meetings.truncate(limit as usize);
    let pagination = pagination(offset, limit, meetings.len(), None, has_more);

    Ok(MeetingPage {
        meetings,
        pagination,
    })
}

fn get_meeting_sync(vault: &Path, meeting_id: &str) -> Result<Meeting> {
    let meta = hypr_vault_read::meta::read_session_meta(vault, meeting_id)
        .map_err(vault_error("load meeting"))?
        .ok_or_else(|| Error::NotFound(format!("meeting '{meeting_id}'")))?;

    let note = hypr_vault_read::meta::read_note(vault, meeting_id)
        .map_err(vault_error("load meeting"))?
        .map(|markdown| Document {
            id: format!("{meeting_id}:note"),
            kind: "note".to_string(),
            template_id: String::new(),
            title: String::new(),
            markdown,
            sort_order: 0,
            updated_at: file_updated_at(vault, &hypr_vault_read::paths::note_path(meeting_id)),
        });

    let summaries = load_summaries_sync(vault, meeting_id)?;

    let mut tasks = hypr_vault_read::tasks::read_session_tasks(vault, meeting_id)
        .map_err(vault_error("load meeting"))?;
    tasks.sort_by(|a, b| {
        (a.source_order, &a.created_at, &a.id).cmp(&(b.source_order, &b.created_at, &b.id))
    });
    let action_items = tasks
        .into_iter()
        .map(|task| ActionItem {
            id: task.id,
            assignee_human_id: task.assignee,
            status: task.status,
            text: task.text,
            due_at: task.due_at,
            completed_at: None,
        })
        .collect();

    Ok(Meeting {
        updated_at: file_updated_at(vault, &hypr_vault_read::paths::meta_path(meeting_id)),
        id: meta.id,
        title: meta.title,
        kind: "meeting".to_string(),
        status: "active".to_string(),
        created_at: meta.created_at,
        started_at: meta.started_at.unwrap_or_default(),
        ended_at: meta.ended_at.unwrap_or_default(),
        timezone: String::new(),
        language: String::new(),
        note,
        summaries,
        action_items,
    })
}

// The old session_documents read returned both legacy single-slot docs
// (`sessions/<id>/<kind>.md`, indexed with id `<id>:<kind>`) and per-doc enhanced files,
// filtered to the summary/template_output kinds and ordered by (sort_order, id).
fn load_summaries_sync(vault: &Path, meeting_id: &str) -> Result<Vec<Document>> {
    let mut summaries = Vec::new();
    for doc in hypr_vault_read::meta::list_legacy_docs(vault, meeting_id)
        .map_err(vault_error("load meeting"))?
    {
        if !hypr_vault_read::ENHANCED_KINDS.contains(&doc.kind.as_str()) {
            continue;
        }
        summaries.push(Document {
            id: format!("{meeting_id}:{}", doc.kind),
            updated_at: file_updated_at(
                vault,
                &hypr_vault_read::paths::document_path(meeting_id, &doc.kind),
            ),
            kind: doc.kind,
            template_id: String::new(),
            title: String::new(),
            markdown: doc.markdown,
            sort_order: 0,
        });
    }
    for doc in hypr_vault_read::enhanced::list_enhanced_docs(vault, meeting_id)
        .map_err(vault_error("load meeting"))?
    {
        summaries.push(Document {
            updated_at: file_updated_at(
                vault,
                &hypr_vault_read::paths::enhanced_doc_path(meeting_id, &doc.id),
            ),
            id: doc.id,
            kind: doc.kind,
            template_id: doc.template_id,
            title: doc.title,
            markdown: doc.markdown,
            sort_order: i64::from(doc.sort_order),
        });
    }
    summaries.sort_by(|a, b| (a.sort_order, &a.id).cmp(&(b.sort_order, &b.id)));
    Ok(summaries)
}

fn get_meeting_transcript_sync(
    vault: &Path,
    input: GetMeetingTranscriptInput,
) -> Result<TranscriptPage> {
    let exists = hypr_vault_read::meta::read_session_meta(vault, &input.meeting_id)
        .map_err(vault_error("load meeting"))?
        .is_some();
    if !exists {
        return Err(Error::NotFound(format!("meeting '{}'", input.meeting_id)));
    }

    let transcripts = load_transcripts_sync(vault, &input.meeting_id)?;
    Ok(transcript_page(
        &input.meeting_id,
        &transcripts,
        input.offset.unwrap_or(0),
        input
            .limit
            .unwrap_or(DEFAULT_TRANSCRIPT_LIMIT)
            .clamp(1, MAX_TRANSCRIPT_LIMIT),
    ))
}

fn load_transcripts_sync(vault: &Path, meeting_id: &str) -> Result<Vec<Transcript>> {
    let file = hypr_vault_read::transcript::read_transcript_json(vault, meeting_id)
        .map_err(vault_error("load transcript"))?;
    let mut transcripts = file
        .transcripts
        .into_iter()
        .map(Transcript::from)
        .collect::<Vec<_>>();
    transcripts.sort_by(|a, b| (a.started_at_ms, &a.id).cmp(&(b.started_at_ms, &b.id)));
    Ok(transcripts)
}

// Matches the retired SQL ordering: most recent first by started_at (falling back to
// created_at when a session never started), then created_at, then id.
fn sort_metas_recent_first(metas: &mut [hypr_vault_read::SessionMeta]) {
    metas.sort_by(|a, b| {
        let a_key = (occurred_at(a), a.created_at.as_str(), a.id.as_str());
        let b_key = (occurred_at(b), b.created_at.as_str(), b.id.as_str());
        b_key.cmp(&a_key)
    });
}

fn occurred_at(meta: &hypr_vault_read::SessionMeta) -> &str {
    match meta.started_at.as_deref() {
        Some(started_at) if !started_at.is_empty() => started_at,
        _ => meta.created_at.as_str(),
    }
}

fn meeting_list_item(vault: &Path, meta: hypr_vault_read::SessionMeta) -> MeetingListItem {
    MeetingListItem {
        updated_at: file_updated_at(vault, &hypr_vault_read::paths::meta_path(&meta.id)),
        id: meta.id,
        title: meta.title,
        kind: "meeting".to_string(),
        status: "active".to_string(),
        created_at: meta.created_at,
        started_at: meta.started_at.unwrap_or_default(),
        ended_at: meta.ended_at.unwrap_or_default(),
    }
}

/// The retired index's `updated_at` column tracked the last write; the file's mtime is the
/// vault-native equivalent. Empty string when the file can't be inspected.
fn file_updated_at(vault: &Path, relative: &Path) -> String {
    std::fs::metadata(vault.join(relative))
        .and_then(|metadata| metadata.modified())
        .map(|modified| {
            chrono::DateTime::<chrono::Utc>::from(modified)
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        })
        .unwrap_or_default()
}

impl Meeting {
    pub fn to_markdown(&self) -> String {
        let title = if self.title.trim().is_empty() {
            "Untitled meeting"
        } else {
            self.title.trim()
        };
        let mut sections = vec![format!("# {title}"), self.metadata_markdown()];

        if let Some(note) = &self.note {
            push_section(&mut sections, "Notes", &note.markdown);
        }
        for summary in &self.summaries {
            let heading = if summary.title.trim().is_empty() {
                "Summary"
            } else {
                summary.title.trim()
            };
            push_section(&mut sections, heading, &summary.markdown);
        }
        if !self.action_items.is_empty() {
            let body = self
                .action_items
                .iter()
                .map(|item| {
                    let checked = matches!(item.status.as_str(), "done" | "completed");
                    format!("- [{}] {}", if checked { "x" } else { " " }, item.text)
                })
                .collect::<Vec<_>>()
                .join("\n");
            push_section(&mut sections, "Action items", &body);
        }

        sections.join("\n\n").trim().to_string()
    }

    fn metadata_markdown(&self) -> String {
        let occurred_at = if self.started_at.is_empty() {
            &self.created_at
        } else {
            &self.started_at
        };
        let lines = [
            format!("- ID: `{}`", self.id),
            format!("- Date: {occurred_at}"),
        ];
        lines.join("\n")
    }
}

impl MeetingExport {
    pub fn to_markdown(&self) -> String {
        let mut markdown = self.meeting.to_markdown();
        let transcript = render_transcripts(&self.transcripts);
        if !transcript.is_empty() {
            markdown.push_str("\n\n## Transcript\n\n");
            markdown.push_str(&transcript);
        }
        markdown
    }
}

impl From<hypr_vault_read::TranscriptWithData> for Transcript {
    fn from(value: hypr_vault_read::TranscriptWithData) -> Self {
        let words = value
            .words
            .iter()
            .map(|word| serde_json::to_value(word).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        let text = transcript_text(&words);
        Self {
            id: value.id,
            started_at_ms: value.started_at.round() as i64,
            ended_at_ms: value.ended_at.map(|ended_at| ended_at.round() as i64),
            memo: value.memo_md,
            text,
            words,
            speaker_hints: value
                .speaker_hints
                .iter()
                .map(|hint| serde_json::to_value(hint).unwrap_or(Value::Null))
                .collect(),
        }
    }
}

fn transcript_page(
    meeting_id: &str,
    transcripts: &[Transcript],
    offset: u32,
    limit: u32,
) -> TranscriptPage {
    let mut words = Vec::new();
    for transcript in transcripts {
        for word in &transcript.words {
            let mut word = word.clone();
            if let Some(object) = word.as_object_mut() {
                object.insert(
                    "transcript_id".to_string(),
                    Value::String(transcript.id.clone()),
                );
            }
            words.push(word);
        }
    }

    let total_words = words.len();
    let offset_usize = offset as usize;
    let limit = limit.clamp(1, MAX_TRANSCRIPT_LIMIT);
    let words = words
        .into_iter()
        .skip(offset_usize)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let text = transcript_page_text(&words);
    let has_more = offset_usize.saturating_add(words.len()) < total_words;

    TranscriptPage {
        meeting_id: meeting_id.to_string(),
        text,
        pagination: pagination(offset, limit, words.len(), Some(total_words), has_more),
        words,
    }
}

fn render_transcripts(transcripts: &[Transcript]) -> String {
    transcripts
        .iter()
        .filter(|transcript| !transcript.text.trim().is_empty())
        .map(|transcript| transcript.text.trim())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn transcript_text(words: &[Value]) -> String {
    words
        .iter()
        .filter_map(|word| word.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn transcript_page_text(words: &[Value]) -> String {
    let mut segments = Vec::new();
    let mut transcript_id = None;
    let mut segment = Vec::new();

    for word in words {
        let next_transcript_id = word.get("transcript_id").and_then(Value::as_str);
        if transcript_id.is_some() && transcript_id != next_transcript_id {
            segments.push(segment.join(" "));
            segment.clear();
        }
        transcript_id = next_transcript_id;

        if let Some(text) = word.get("text").and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                segment.push(text);
            }
        }
    }

    if !segment.is_empty() {
        segments.push(segment.join(" "));
    }

    segments.join("\n\n")
}

fn push_section(sections: &mut Vec<String>, title: &str, body: &str) {
    if !body.trim().is_empty() {
        sections.push(format!("## {title}\n\n{}", body.trim()));
    }
}

fn pagination(
    offset: u32,
    limit: u32,
    returned: usize,
    total: Option<usize>,
    has_more: bool,
) -> Pagination {
    Pagination {
        offset,
        limit,
        returned,
        total,
        next_offset: has_more.then(|| offset.saturating_add(returned as u32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_session(vault: &Path, id: &str, title: &str, started_at: Option<&str>) {
        let dir = vault.join(format!("sessions/{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            serde_json::json!({
                "id": id,
                "title": title,
                "started_at": started_at,
                "ended_at": null,
                "created_at": "2026-07-01T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
    }

    fn seed_meeting_fixture(vault: &Path) {
        seed_session(vault, "meeting-1", "Planning", Some("2026-07-13"));
        seed_session(vault, "meeting-2", "Prior planning", Some("2026-07-06"));

        let dir = vault.join("sessions/meeting-1");
        std::fs::write(dir.join("_memo.md"), "Launch decision").unwrap();
        std::fs::write(dir.join("summary.md"), "Ship Tuesday").unwrap();
        std::fs::create_dir_all(dir.join("enhanced")).unwrap();
        std::fs::write(
            dir.join("enhanced/doc-1.md"),
            "---\nkind: template_output\ntitle: Customer review\ntemplate_id: template-1\nsort_order: 3\n---\n\n# Review",
        )
        .unwrap();
        std::fs::write(
            dir.join("tasks.json"),
            serde_json::json!({
                "tasks": [{
                    "id": "action-1",
                    "source_type": "session_raw_note",
                    "source_id": "meeting-1",
                    "source_order": 1,
                    "status": "open",
                    "text": "Prepare launch",
                    "body": [],
                    "due_at": "2026-07-20",
                    "assignee": "human-1",
                    "created_at": "2026-07-13T00:00:00Z",
                    "updated_at": "2026-07-13T00:00:00Z",
                }],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join("transcript.json"),
            serde_json::json!({
                "transcripts": [{
                    "id": "transcript-1",
                    "session_id": "meeting-1",
                    "started_at": 0.0,
                    "memo_md": "internal memo",
                    "words": [
                        {"text": "one", "start_ms": 0.0, "end_ms": 1.0, "channel": 0.0},
                        {"text": "two", "start_ms": 1.0, "end_ms": 2.0, "channel": 0.0},
                    ],
                }],
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn transcript_text_uses_word_text() {
        let words = serde_json::json!([
            {"text": " Hello "},
            {"text": "world."},
            {"other": "ignored"}
        ]);
        assert_eq!(transcript_text(words.as_array().unwrap()), "Hello world.");
    }

    #[test]
    fn transcript_page_text_preserves_transcript_boundaries() {
        let words = serde_json::json!([
            {"text": "First segment", "transcript_id": "transcript-1"},
            {"text": "continues.", "transcript_id": "transcript-1"},
            {"text": "Second segment.", "transcript_id": "transcript-2"}
        ]);

        assert_eq!(
            transcript_page_text(words.as_array().unwrap()),
            "First segment continues.\n\nSecond segment."
        );
    }

    #[tokio::test]
    async fn operations_return_curated_meeting_data() {
        let vault = tempfile::tempdir().unwrap();
        seed_meeting_fixture(vault.path());

        let listed = list_meetings(
            vault.path(),
            ListMeetingsInput {
                query: Some("plan".to_string()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed.meetings[0].id, "meeting-1");
        assert_eq!(listed.pagination.next_offset, Some(1));

        let meeting = get_meeting(
            vault.path(),
            GetMeetingInput {
                meeting_id: "meeting-1".to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(meeting.note.as_ref().unwrap().markdown, "Launch decision");
        assert_eq!(meeting.action_items[0].text, "Prepare launch");
        assert_eq!(meeting.action_items[0].assignee_human_id, "human-1");
        assert_eq!(
            meeting
                .summaries
                .iter()
                .map(|summary| summary.id.as_str())
                .collect::<Vec<_>>(),
            vec!["meeting-1:summary", "doc-1"],
            "legacy single-slot and enhanced docs both surface, ordered by (sort_order, id)"
        );
        assert_eq!(meeting.summaries[1].title, "Customer review");
        assert_eq!(meeting.summaries[1].template_id, "template-1");
        let serialized = serde_json::to_value(&meeting).unwrap();
        assert!(serialized.get("workspace_id").is_none());
        assert!(serialized.get("owner_user_id").is_none());
        assert!(serialized.get("metadata_json").is_none());

        let transcript = get_meeting_transcript(
            vault.path(),
            GetMeetingTranscriptInput {
                meeting_id: "meeting-1".to_string(),
                offset: Some(1),
                limit: Some(1),
            },
        )
        .await
        .unwrap();
        assert_eq!(transcript.text, "two");
        assert_eq!(transcript.pagination.total, Some(2));
        assert_eq!(transcript.words[0]["transcript_id"], "transcript-1");
    }

    #[tokio::test]
    async fn list_meetings_orders_by_started_at_then_created_at_and_matches_ids() {
        let vault = tempfile::tempdir().unwrap();
        seed_session(
            vault.path(),
            "alpha-old",
            "Alpha Planning",
            Some("2026-01-01"),
        );
        seed_session(
            vault.path(),
            "alpha-new",
            "ALPHA Review",
            Some("2026-02-01"),
        );
        seed_session(vault.path(), "beta", "Beta Review", Some("2026-03-01"));

        let first = list_meetings(
            vault.path(),
            ListMeetingsInput {
                query: Some(" alpha ".to_string()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let second = list_meetings(
            vault.path(),
            ListMeetingsInput {
                query: Some("alpha".to_string()),
                limit: Some(1),
                offset: Some(1),
            },
        )
        .await
        .unwrap();
        let by_id = list_meetings(
            vault.path(),
            ListMeetingsInput {
                query: Some("old".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(first.meetings[0].id, "alpha-new");
        assert_eq!(second.meetings[0].id, "alpha-old");
        assert_eq!(by_id.meetings[0].id, "alpha-old");
    }

    #[tokio::test]
    async fn missing_meeting_is_not_found() {
        let vault = tempfile::tempdir().unwrap();
        let error = get_meeting(
            vault.path(),
            GetMeetingInput {
                meeting_id: "ghost".to_string(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::NotFound(_)));

        let error = get_meeting_transcript(
            vault.path(),
            GetMeetingTranscriptInput {
                meeting_id: "ghost".to_string(),
                offset: None,
                limit: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::NotFound(_)));
    }
}
