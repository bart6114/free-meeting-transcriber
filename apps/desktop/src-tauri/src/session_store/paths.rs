use std::path::PathBuf;

pub fn sessions_root() -> PathBuf {
    PathBuf::from("sessions")
}

pub fn session_dir(id: &str) -> PathBuf {
    sessions_root().join(id)
}

pub fn meta_path(id: &str) -> PathBuf {
    session_dir(id).join("_meta.json")
}

pub fn note_path(id: &str) -> PathBuf {
    session_dir(id).join("_memo.md")
}

pub fn document_path(id: &str, kind: &str) -> PathBuf {
    session_dir(id).join(format!("{}.md", kind))
}

pub fn transcript_path(id: &str) -> PathBuf {
    session_dir(id).join("transcript.json")
}

pub fn audio_dir(id: &str) -> PathBuf {
    session_dir(id).join("audio")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_relative_and_correct() {
        assert_eq!(sessions_root(), PathBuf::from("sessions"));
        assert_eq!(session_dir("s1"), PathBuf::from("sessions/s1"));
        assert_eq!(meta_path("s1"), PathBuf::from("sessions/s1/_meta.json"));
        assert_eq!(note_path("s1"), PathBuf::from("sessions/s1/_memo.md"));
        assert_eq!(
            document_path("s1", "notes"),
            PathBuf::from("sessions/s1/notes.md")
        );
        assert_eq!(
            transcript_path("s1"),
            PathBuf::from("sessions/s1/transcript.json")
        );
        assert_eq!(audio_dir("s1"), PathBuf::from("sessions/s1/audio"));
    }
}
