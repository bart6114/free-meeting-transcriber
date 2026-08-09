use std::path::Path;

use crate::cli::{DocumentKind, ExportFormat, MeetingCommand};
use crate::{Error, Result, output};
use hypr_agent_access::{
    Document, GetMeetingInput, GetMeetingTranscriptInput, ListMeetingsInput, MeetingListItem,
    SearchHit, SearchMeetingsInput, get_meeting, get_meeting_export, get_meeting_transcript,
    list_meetings, search_meetings,
};
use hypr_vault_write::{SessionMeta, SessionStore};

pub async fn run(vault: &Path, command: MeetingCommand, json: bool) -> Result<()> {
    match command {
        MeetingCommand::List {
            query,
            limit,
            offset,
        } => {
            let page = list_meetings(
                vault,
                ListMeetingsInput {
                    query,
                    limit: Some(limit),
                    offset: Some(offset),
                },
            )
            .await?;
            let rendered = if json {
                output::json("meetings.list", &page.meetings, Some(&page.pagination))?
            } else {
                render_list(&page.meetings)
            };
            output::emit(&rendered);
            Ok(())
        }
        MeetingCommand::Search {
            query,
            speaker,
            kind,
            limit,
            offset,
        } => {
            let page = search_meetings(
                vault,
                SearchMeetingsInput {
                    query,
                    speaker,
                    kinds: (!kind.is_empty()).then(|| kind.into_iter().map(Into::into).collect()),
                    limit: Some(limit),
                    offset: Some(offset),
                },
            )
            .await?;
            let rendered = if json {
                output::json("meetings.search", &page.hits, Some(&page.pagination))?
            } else {
                render_search(&page.hits)
            };
            output::emit(&rendered);
            Ok(())
        }
        MeetingCommand::Get { id } => {
            let meeting = get_meeting(vault, GetMeetingInput { meeting_id: id }).await?;
            let rendered = if json {
                output::json("meetings.get", &meeting, None)?
            } else {
                meeting.to_markdown()
            };
            output::emit(&rendered);
            Ok(())
        }
        MeetingCommand::New { title, note } => {
            // Read the body before touching the vault, so a bad --note path creates nothing.
            let body = note.as_deref().map(read_body).transpose()?;
            let store = SessionStore::new(vault.to_path_buf());

            // Same id format the desktop app generates (`crypto.randomUUID()`). A collision
            // is practically impossible, but never clobber an existing session: retry a few
            // times, then give up rather than overwrite.
            let mut session_id = None;
            for _ in 0..5 {
                let candidate = uuid::Uuid::new_v4().to_string();
                let occupied = vault.join("sessions").join(&candidate).exists()
                    || store
                        .read_meta(&candidate)
                        .await
                        .map_err(|error| Error::operation("create meeting", error.to_string()))?
                        .is_some();
                if !occupied {
                    session_id = Some(candidate);
                    break;
                }
            }
            let session_id = session_id.ok_or_else(|| {
                Error::operation("create meeting", "could not generate an unused meeting id")
            })?;

            // Millisecond RFC3339 UTC, matching the desktop's `new Date().toISOString()`.
            let created_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let meta = SessionMeta {
                id: session_id.clone(),
                title,
                started_at: None,
                ended_at: None,
                created_at: created_at.clone(),
                tags: Vec::new(),
                event: None,
                folder: None,
            };
            store
                .write_meta(&meta)
                .await
                .map_err(|error| Error::operation("create meeting", error.to_string()))?;
            if let Some(body) = body {
                // The meta write above already created the session; name it in the
                // error so a partial failure leaves an identifiable meeting instead
                // of an anonymous orphan (recover with `meetings note ID --set`).
                store.write_note(&session_id, &body).await.map_err(|error| {
                    Error::operation(
                        "write note",
                        format!(
                            "meeting {session_id} was created, but writing its note failed: {error}"
                        ),
                    )
                })?;
            }

            let rendered = if json {
                output::json(
                    "meetings.new",
                    &serde_json::json!({
                        "id": session_id,
                        "title": meta.title,
                        "created_at": created_at,
                    }),
                    None,
                )?
            } else {
                session_id
            };
            output::emit(&rendered);
            Ok(())
        }
        MeetingCommand::Note {
            id,
            kind,
            set,
            append,
        } => {
            if set.is_some() || append.is_some() {
                return edit_note(vault, &id, set.as_deref(), append.as_deref(), json).await;
            }

            let meeting = get_meeting(
                vault,
                GetMeetingInput {
                    meeting_id: id.clone(),
                },
            )
            .await?;
            if json {
                match kind {
                    DocumentKind::Note => {
                        output::emit(&output::json("meetings.note", &meeting.note, None)?)
                    }
                    DocumentKind::Summary => {
                        output::emit(&output::json("meetings.note", &meeting.summaries, None)?)
                    }
                    DocumentKind::All => output::emit(&output::json(
                        "meetings.note",
                        &serde_json::json!({
                            "note": meeting.note,
                            "summaries": meeting.summaries,
                        }),
                        None,
                    )?),
                }
                return Ok(());
            }

            let text = match kind {
                DocumentKind::Note => meeting
                    .note
                    .map(|note| note.markdown)
                    .ok_or_else(|| crate::Error::NotFound(format!("note for meeting '{id}'")))?,
                DocumentKind::Summary => render_documents(&meeting.summaries),
                DocumentKind::All => {
                    let mut documents = meeting.note.into_iter().collect::<Vec<_>>();
                    documents.extend(meeting.summaries);
                    render_documents(&documents)
                }
            };
            output::emit(&text);
            Ok(())
        }
        MeetingCommand::Transcript { id } => {
            let transcript =
                get_meeting_transcript(vault, GetMeetingTranscriptInput { meeting_id: id }).await?;
            let rendered = if json {
                output::json("meetings.transcript", &transcript, None)?
            } else {
                transcript.text
            };
            output::emit(&rendered);
            Ok(())
        }
        MeetingCommand::Export {
            id,
            format,
            output: path,
            force,
        } => {
            let meeting = get_meeting_export(vault, id).await?;
            let content = match (format, json) {
                (ExportFormat::Markdown, false) => meeting.to_markdown(),
                (ExportFormat::Json, false) => output::raw_json(&meeting)?,
                (ExportFormat::Markdown, true) => output::json(
                    "meetings.export",
                    &serde_json::json!({
                        "format": "markdown",
                        "content": meeting.to_markdown(),
                    }),
                    None,
                )?,
                (ExportFormat::Json, true) => output::json("meetings.export", &meeting, None)?,
            };
            output::write_or_emit(&content, path.as_deref(), force)
        }
    }
}

async fn edit_note(
    vault: &Path,
    id: &str,
    set: Option<&Path>,
    append: Option<&Path>,
    json: bool,
) -> Result<()> {
    let store = SessionStore::new(vault.to_path_buf());
    let exists = store
        .read_meta(id)
        .await
        .map_err(|error| Error::operation("edit note", error.to_string()))?
        .is_some();
    if !exists {
        return Err(Error::NotFound(format!("meeting '{id}'")));
    }

    let (action, source) = match (set, append) {
        (Some(path), None) => ("set", path),
        (None, Some(path)) => ("append", path),
        _ => unreachable!("clap enforces exactly one of --set/--append"),
    };
    let body = read_body(source)?;

    let markdown = if action == "append" {
        match store
            .read_note(id)
            .await
            .map_err(|error| Error::operation("edit note", error.to_string()))?
        {
            Some(existing) if !existing.is_empty() => {
                if existing.ends_with('\n') {
                    format!("{existing}{body}")
                } else {
                    format!("{existing}\n{body}")
                }
            }
            _ => body,
        }
    } else {
        body
    };

    store
        .write_note(id, &markdown)
        .await
        .map_err(|error| Error::operation("edit note", error.to_string()))?;

    let rendered = if json {
        output::json(
            "meetings.note",
            &serde_json::json!({ "id": id, "action": action, "updated": true }),
            None,
        )?
    } else {
        format!("Updated note for meeting {id}.")
    };
    output::emit(&rendered);
    Ok(())
}

fn read_body(source: &Path) -> Result<String> {
    if source == Path::new("-") {
        let mut body = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut body)
            .map_err(|error| Error::operation("read note body", error.to_string()))?;
        return Ok(body);
    }
    std::fs::read_to_string(source).map_err(|error| {
        Error::operation("read note body", format!("{}: {error}", source.display()))
    })
}

fn render_list(meetings: &[MeetingListItem]) -> String {
    if meetings.is_empty() {
        return "No meetings found.".to_string();
    }

    let title_width = meetings
        .iter()
        .map(|meeting| meeting.title.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(5, 48);
    let mut lines = vec![format!("{:<24}  {:<title_width$}  ID", "DATE", "TITLE")];
    for meeting in meetings {
        let occurred_at = if meeting.started_at.is_empty() {
            &meeting.created_at
        } else {
            &meeting.started_at
        };
        lines.push(format!(
            "{:<24}  {:<title_width$}  {}",
            truncate(occurred_at, 24),
            truncate(
                if meeting.title.is_empty() {
                    "Untitled"
                } else {
                    &meeting.title
                },
                title_width,
            ),
            meeting.id,
        ));
    }
    lines.join("\n")
}

fn render_search(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "No matches found.".to_string();
    }

    let mut lines = vec![format!(
        "{:<10}  {:<10}  {:<26}  SNIPPET",
        "DATE", "KIND", "ID"
    )];
    for hit in hits {
        let speaker = hit
            .speaker
            .as_deref()
            .map(|speaker| format!("{speaker}: "))
            .unwrap_or_default();
        lines.push(format!(
            "{:<10}  {:<10}  {:<26}  {speaker}{}",
            truncate(&hit.occurred_at, 10),
            hit.kind,
            truncate(&hit.meeting_id, 26),
            truncate(&hit.snippet, 100),
        ));
    }
    lines.join("\n")
}

fn render_documents(documents: &[Document]) -> String {
    documents
        .iter()
        .filter(|document| !document.markdown.trim().is_empty())
        .map(|document| {
            let title = if document.title.trim().is_empty() {
                "Summary"
            } else {
                document.title.trim()
            };
            format!("## {title}\n\n{}", document.markdown.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    let mut text = value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    text.push('…');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_render_is_bounded_and_contains_ids() {
        let rendered = render_list(&[MeetingListItem {
            id: "meeting-1".to_string(),
            title: "A very long planning meeting title that should not own the terminal"
                .to_string(),
            kind: "meeting".to_string(),
            status: "active".to_string(),
            created_at: "2026-07-13T09:00:00Z".to_string(),
            updated_at: "2026-07-13T09:00:00Z".to_string(),
            started_at: String::new(),
            ended_at: String::new(),
        }]);
        assert!(rendered.contains("meeting-1"));
        assert!(rendered.contains('…'));
    }
}
