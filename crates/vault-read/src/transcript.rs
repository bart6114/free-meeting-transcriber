use std::path::Path;

use hypr_fs_format::TranscriptJson;

use crate::{Error, Result, paths};

/// Read one session's `transcript.json`; a missing file is an empty transcript list
/// (the "no transcript yet" state), malformed JSON is an error so callers can tell
/// "nothing here" apart from "something here that failed to parse".
pub fn read_transcript_json(vault: &Path, session_id: &str) -> Result<TranscriptJson> {
    let path = vault.join(paths::transcript_path(session_id));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TranscriptJson {
                transcripts: Vec::new(),
            });
        }
        Err(e) => return Err(Error::Io(format!("failed to read transcript file: {e}"))),
    };
    serde_json::from_slice(&bytes)
        .map_err(|e| Error::Parse(format!("failed to deserialize transcript.json: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_transcript_json_reads_words_and_tolerates_absence() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            read_transcript_json(temp.path(), "s1")
                .unwrap()
                .transcripts
                .is_empty()
        );

        let dir = temp.path().join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("transcript.json"),
            serde_json::json!({
                "transcripts": [{
                    "id": "t1",
                    "session_id": "s1",
                    "started_at": 100.0,
                    "words": [
                        {"text": "hello", "start_ms": 0.0, "end_ms": 10.0, "channel": 0.0}
                    ],
                }],
            })
            .to_string(),
        )
        .unwrap();

        let file = read_transcript_json(temp.path(), "s1").unwrap();
        assert_eq!(file.transcripts.len(), 1);
        assert_eq!(file.transcripts[0].words[0].text, "hello");

        std::fs::write(dir.join("transcript.json"), "{ invalid").unwrap();
        assert!(read_transcript_json(temp.path(), "s1").is_err());
    }
}
