use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, paths, strip_leading_frontmatter};

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
    pub tags: Vec<String>,
    /// Opaque calendar-event envelope (the sessions row's `event_json`). The store never
    /// inspects its interior -- it round-trips whatever JSON the frontend hands it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// A legacy single-slot document file `sessions/<id>/<kind>.md` (e.g. `summary.md`),
/// predating the per-doc `enhanced/<uuid>.md` layout. `kind` is the file stem.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyDoc {
    pub kind: String,
    pub markdown: String,
}

/// Read one session's `_meta.json`. `Ok(None)` only for a genuinely absent file --
/// a transiently unreadable or corrupt file is an error, never "no session".
pub fn read_session_meta(vault: &Path, id: &str) -> Result<Option<SessionMeta>> {
    let path = vault.join(paths::meta_path(id));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(format!("failed to read meta file: {e}"))),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| Error::Parse(format!("failed to deserialize meta: {e}")))
}

/// Scan `sessions/` and return every session's parsed `_meta.json`. Entries without a
/// parseable meta file are skipped (read-only tolerance: one corrupted session must not
/// hide the rest); a missing `sessions/` directory is an empty vault, not an error.
pub fn list_session_metas(vault: &Path) -> Result<Vec<SessionMeta>> {
    let root = vault.join(paths::sessions_root());
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(format!("failed to read sessions dir: {e}"))),
    };

    let mut metas = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::Io(format!("failed to read dir entry: {e}")))?;
        if !entry.path().is_dir() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if let Ok(Some(meta)) = read_session_meta(vault, &id) {
            metas.push(meta);
        }
    }
    Ok(metas)
}

/// Read the user's note (`_memo.md`), stripping any legacy exporter frontmatter wrapper.
pub fn read_note(vault: &Path, id: &str) -> Result<Option<String>> {
    let path = vault.join(paths::note_path(id));
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Some(strip_leading_frontmatter(content))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(format!("failed to read note file: {e}"))),
    }
}

/// Legacy single-slot `<kind>.md` docs directly inside the session dir. Underscore-prefixed
/// files (`_memo.md`) are the note/meta namespace, not documents. Unreadable files are
/// skipped.
pub fn list_legacy_docs(vault: &Path, id: &str) -> Result<Vec<LegacyDoc>> {
    let dir = vault.join(paths::session_dir(id));
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(format!("failed to read session dir: {e}"))),
    };

    let mut docs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::Io(format!("failed to read dir entry: {e}")))?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.starts_with('_') {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        docs.push(LegacyDoc {
            kind: stem.to_string(),
            markdown: strip_leading_frontmatter(content),
        });
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_session(vault: &Path, id: &str, title: &str) {
        let dir = vault.join(format!("sessions/{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            serde_json::json!({
                "id": id,
                "title": title,
                "started_at": null,
                "ended_at": null,
                "created_at": "2026-07-01T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn list_session_metas_scans_and_skips_corrupt_entries() {
        let temp = tempfile::tempdir().unwrap();
        assert!(list_session_metas(temp.path()).unwrap().is_empty());

        seed_session(temp.path(), "s1", "Planning");
        seed_session(temp.path(), "s2", "Review");
        let broken = temp.path().join("sessions/broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("_meta.json"), "{ invalid").unwrap();
        let no_meta = temp.path().join("sessions/no-meta");
        std::fs::create_dir_all(&no_meta).unwrap();

        let mut ids: Vec<String> = list_session_metas(temp.path())
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["s1", "s2"]);
    }

    #[test]
    fn read_session_meta_distinguishes_absent_from_corrupt() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(read_session_meta(temp.path(), "ghost").unwrap(), None);

        seed_session(temp.path(), "s1", "Planning");
        assert_eq!(
            read_session_meta(temp.path(), "s1").unwrap().unwrap().title,
            "Planning"
        );

        std::fs::write(temp.path().join("sessions/s1/_meta.json"), "{ invalid").unwrap();
        assert!(read_session_meta(temp.path(), "s1").is_err());
    }

    #[test]
    fn read_note_strips_exporter_frontmatter() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(read_note(temp.path(), "s1").unwrap(), None);

        let dir = temp.path().join("sessions/s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_memo.md"),
            "---\nid: s1:note\nposition: 0\n---\n\nreal content",
        )
        .unwrap();
        assert_eq!(
            read_note(temp.path(), "s1").unwrap().unwrap(),
            "real content"
        );
    }

    #[test]
    fn list_legacy_docs_returns_non_underscore_markdown_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("sessions/s1");
        std::fs::create_dir_all(dir.join("enhanced")).unwrap();
        std::fs::write(dir.join("_memo.md"), "note").unwrap();
        std::fs::write(dir.join("summary.md"), "## Summary\n\nBody").unwrap();
        std::fs::write(dir.join("transcript.json"), "{}").unwrap();

        let docs = list_legacy_docs(temp.path(), "s1").unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].kind, "summary");
        assert_eq!(docs[0].markdown, "## Summary\n\nBody");
    }
}
