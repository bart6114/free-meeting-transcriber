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

pub fn enhanced_dir(id: &str) -> PathBuf {
    session_dir(id).join("enhanced")
}

pub fn enhanced_doc_path(id: &str, doc_id: &str) -> PathBuf {
    enhanced_dir(id).join(format!("{}.md", doc_id))
}

pub fn transcript_path(id: &str) -> PathBuf {
    session_dir(id).join("transcript.json")
}

pub fn session_tasks_path(id: &str) -> PathBuf {
    session_dir(id).join("tasks.json")
}

pub fn vault_tasks_path() -> PathBuf {
    PathBuf::from("tasks.json")
}

pub fn audio_dir(id: &str) -> PathBuf {
    session_dir(id).join("audio")
}

pub fn templates_root() -> PathBuf {
    PathBuf::from("templates")
}

pub fn template_path(id: &str) -> PathBuf {
    templates_root().join(format!("{}.json", id))
}

pub fn deleted_default_templates_path() -> PathBuf {
    templates_root().join(".deleted-defaults.json")
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
        assert_eq!(enhanced_dir("s1"), PathBuf::from("sessions/s1/enhanced"));
        assert_eq!(
            enhanced_doc_path("s1", "doc-1"),
            PathBuf::from("sessions/s1/enhanced/doc-1.md")
        );
        assert_eq!(
            transcript_path("s1"),
            PathBuf::from("sessions/s1/transcript.json")
        );
        assert_eq!(
            session_tasks_path("s1"),
            PathBuf::from("sessions/s1/tasks.json")
        );
        assert_eq!(vault_tasks_path(), PathBuf::from("tasks.json"));
        assert_eq!(audio_dir("s1"), PathBuf::from("sessions/s1/audio"));
        assert_eq!(templates_root(), PathBuf::from("templates"));
        assert_eq!(template_path("t-1"), PathBuf::from("templates/t-1.json"));
        assert_eq!(
            deleted_default_templates_path(),
            PathBuf::from("templates/.deleted-defaults.json")
        );
    }
}
