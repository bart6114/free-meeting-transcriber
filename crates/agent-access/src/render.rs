use std::collections::HashMap;
use std::path::Path;

use hypr_transcript::{
    ChannelProfile, IdentityAssignment, IdentityScope, RenderTranscriptHuman,
    RenderTranscriptInput, RenderTranscriptRequest, RenderTranscriptWordInput,
    render_transcript_segments,
};
use hypr_vault_read::{TranscriptWithData, TranscriptWord};
use serde_json::Value;

/// Renders a meeting's transcripts the way the desktop transcript view does:
/// the shared segment renderer (`crates/transcript`) groups words into speaker
/// turns, resolves names through the people registry, and falls back to
/// "Speaker N". Each turn becomes one `[HH:MM:SS] Name: text` line, timed
/// relative to the earliest transcript start.
pub(crate) fn render_meeting_transcript(
    vault: &Path,
    transcripts: &[TranscriptWithData],
) -> String {
    let humans = hypr_vault_read::read_people(vault)
        .into_iter()
        .filter(|person| !person.name.trim().is_empty())
        .map(|person| RenderTranscriptHuman {
            human_id: person.id,
            name: person.name,
        })
        .collect();
    let self_human_id = transcripts
        .iter()
        .map(|transcript| transcript.user_id.trim())
        .find(|user_id| !user_id.is_empty())
        .map(str::to_string);

    let segments = render_transcript_segments(RenderTranscriptRequest {
        transcripts: transcripts.iter().map(render_input).collect(),
        participant_human_ids: Vec::new(),
        self_human_id,
        humans,
    });

    segments
        .iter()
        .map(|segment| {
            format!(
                "[{}] {}: {}",
                timestamp(segment.start_ms),
                segment.speaker_label,
                segment.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mirrors the desktop's two-pass hint normalization (`render-transcript.ts`):
/// provider hints stamp per-word speaker indexes first, then label hints bind a
/// person id to the anchored word's channel (or channel + speaker index).
fn render_input(transcript: &TranscriptWithData) -> RenderTranscriptInput {
    let mut words = Vec::with_capacity(transcript.words.len());
    let mut index_by_word_id = HashMap::new();
    for (index, word) in transcript.words.iter().enumerate() {
        // Words in older vaults may lack ids; hints only ever reference real ids,
        // so a synthetic id keeps the word renderable without colliding.
        let id = word
            .id
            .clone()
            .unwrap_or_else(|| format!("{}:{index}", transcript.id));
        index_by_word_id.insert(id.clone(), words.len());
        words.push(RenderTranscriptWordInput {
            id,
            text: word.text.clone(),
            start_ms: word.start_ms.round() as i64,
            end_ms: word.end_ms.round() as i64,
            channel: word.channel.round() as i32,
            speaker_index: None,
        });
    }

    for hint in &transcript.speaker_hints {
        if hint.hint_type != "provider_speaker_index" {
            continue;
        }
        let Some(&index) = index_by_word_id.get(hint.word_id.as_str()) else {
            continue;
        };
        let Some(value) = object_hint_value(&hint.value) else {
            continue;
        };
        let Some(speaker_index) = value.get("speaker_index").and_then(Value::as_f64) else {
            continue;
        };
        words[index].speaker_index = Some(speaker_index.round() as i32);
        if let Some(channel) = value.get("channel").and_then(Value::as_f64) {
            words[index].channel = channel.round() as i32;
        }
    }

    let mut assignments = Vec::new();
    for hint in &transcript.speaker_hints {
        if hint.hint_type != "speaker_label" {
            continue;
        }
        let Some(&index) = index_by_word_id.get(hint.word_id.as_str()) else {
            continue;
        };
        let Some(label) = hint.value.as_str().filter(|label| !label.is_empty()) else {
            continue;
        };
        let word = &words[index];
        let channel = ChannelProfile::from(word.channel);
        assignments.push(IdentityAssignment {
            human_id: label.to_string(),
            scope: match word.speaker_index {
                None => IdentityScope::Channel { channel },
                Some(speaker_index) => IdentityScope::ChannelSpeaker {
                    channel,
                    speaker_index,
                },
            },
        });
    }

    let synthetic_timing = transcript.words.iter().any(|word| {
        matches!(
            timing_source(word),
            Some("synthetic_speech" | "synthetic_text")
        )
    });

    RenderTranscriptInput {
        started_at: Some(transcript.started_at.round() as i64),
        words,
        assignments,
        synthetic_timing: synthetic_timing.then_some(true),
    }
}

pub(crate) fn object_hint_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(raw) => serde_json::from_str::<Value>(raw)
            .ok()
            .filter(Value::is_object),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    }
}

fn timing_source(word: &TranscriptWord) -> Option<&str> {
    let metadata = word.metadata.as_ref()?;
    metadata
        .get("timing")
        .and_then(Value::as_object)
        .and_then(|timing| timing.get("source"))
        .or_else(|| metadata.get("timing_source"))
        .and_then(Value::as_str)
}

fn timestamp(ms: i64) -> String {
    let total_seconds = ms.max(0) / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        total_seconds / 3600,
        total_seconds % 3600 / 60,
        total_seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypr_vault_read::TranscriptSpeakerHint;

    fn word(id: &str, text: &str, start_ms: f64, channel: f64) -> TranscriptWord {
        TranscriptWord {
            id: Some(id.to_string()),
            text: text.to_string(),
            start_ms,
            end_ms: start_ms + 100.0,
            channel,
            speaker: None,
            metadata: None,
        }
    }

    fn transcript(
        id: &str,
        user_id: &str,
        words: Vec<TranscriptWord>,
        speaker_hints: Vec<TranscriptSpeakerHint>,
    ) -> TranscriptWithData {
        TranscriptWithData {
            id: id.to_string(),
            user_id: user_id.to_string(),
            created_at: String::new(),
            session_id: "session-1".to_string(),
            started_at: 0.0,
            ended_at: None,
            memo_md: String::new(),
            words,
            speaker_hints,
        }
    }

    #[test]
    fn renders_speaker_turns_with_timestamps_and_registry_names() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(
            vault.path().join("people.json"),
            serde_json::json!({
                "people": [
                    {"id": "me", "name": "Bart"},
                    {"id": "guest", "name": "Ada"},
                ],
            })
            .to_string(),
        )
        .unwrap();

        let rendered = render_meeting_transcript(
            vault.path(),
            &[transcript(
                "t1",
                "me",
                vec![
                    word("w1", " Hello", 16_000.0, 0.0),
                    word("w2", " there.", 16_200.0, 0.0),
                    word("w3", " Hi", 62_000.0, 1.0),
                    word("w4", " back.", 62_200.0, 1.0),
                ],
                vec![TranscriptSpeakerHint {
                    id: None,
                    word_id: "w3".to_string(),
                    hint_type: "speaker_label".to_string(),
                    value: Value::String("guest".to_string()),
                }],
            )],
        );

        assert_eq!(
            rendered,
            "[00:00:16] Bart: Hello there.\n[00:01:02] Ada: Hi back."
        );
    }

    #[test]
    fn words_without_ids_still_render_with_speaker_fallback() {
        let vault = tempfile::tempdir().unwrap();
        let mut anonymous = word("ignored", " hello", 0.0, 0.0);
        anonymous.id = None;

        let rendered = render_meeting_transcript(
            vault.path(),
            &[transcript("t1", "", vec![anonymous], vec![])],
        );

        assert_eq!(rendered, "[00:00:00] Speaker 1: hello");
    }
}
