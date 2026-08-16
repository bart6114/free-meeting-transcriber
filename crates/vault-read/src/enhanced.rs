use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, layout, paths};

pub const ENHANCED_KINDS: [&str; 2] = ["summary", "template_output"];

/// One AI-generated document (`summary` or `template_output`), file-canonical at
/// `sessions/<session_id>/enhanced/<id>.md`. `id` is the same UUID the `session_documents`
/// index row uses, and the frontmatter carries every metadata column that row mirrors --
/// there is deliberately no sidecar file.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct EnhancedDoc {
    pub id: String,
    pub session_id: String,
    /// "summary" (no template) or "template_output".
    pub kind: String,
    pub title: String,
    pub template_id: String,
    pub sort_order: i32,
    /// Body only -- never includes the frontmatter block.
    pub markdown: String,
}

/// The typed frontmatter schema for `enhanced/<id>.md`. Every field defaults so a
/// hand-created or partially-written file still parses (missing kind falls back to
/// "summary" below); unknown keys are ignored by serde. `hypr_frontmatter::Document`'s
/// renderer sorts keys, so the on-disk bytes are deterministic for identical content.
#[derive(Serialize, Deserialize, Debug)]
struct EnhancedFrontmatter {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    template_id: String,
    #[serde(default)]
    sort_order: i32,
}

pub fn render_enhanced_file(doc: &EnhancedDoc) -> Result<String> {
    let frontmatter = EnhancedFrontmatter {
        kind: doc.kind.clone(),
        title: doc.title.clone(),
        template_id: doc.template_id.clone(),
        sort_order: doc.sort_order,
    };
    hypr_frontmatter::Document::new(frontmatter, &doc.markdown)
        .render()
        .map_err(|e| Error::Parse(format!("failed to render enhanced doc: {e}")))
}

/// Inverse of `render_enhanced_file`. A file with no frontmatter at all (an external
/// drop-in) is accepted as a plain summary; a file whose frontmatter is present but
/// malformed is an error -- callers treat that like any other unparseable artifact.
/// An unknown `kind` degrades to "summary" rather than erroring so a hand-edited value
/// can't make the document vanish.
pub fn parse_enhanced_file(id: &str, session_id: &str, raw: &str) -> Result<EnhancedDoc> {
    let (frontmatter, markdown) =
        match hypr_frontmatter::Document::<EnhancedFrontmatter>::from_str(raw) {
            Ok(doc) => (doc.frontmatter, doc.content),
            Err(hypr_frontmatter::Error::MissingOpeningDelimiter) => (
                EnhancedFrontmatter {
                    kind: String::new(),
                    title: String::new(),
                    template_id: String::new(),
                    sort_order: 0,
                },
                raw.to_string(),
            ),
            Err(e) => {
                return Err(Error::Parse(format!(
                    "failed to parse enhanced doc frontmatter: {e}"
                )));
            }
        };

    let kind = if ENHANCED_KINDS.contains(&frontmatter.kind.as_str()) {
        frontmatter.kind
    } else {
        "summary".to_string()
    };

    Ok(EnhancedDoc {
        id: id.to_string(),
        session_id: session_id.to_string(),
        kind,
        title: frontmatter.title,
        template_id: frontmatter.template_id,
        sort_order: frontmatter.sort_order,
        markdown,
    })
}

/// Every parseable `enhanced/<uuid>.md` doc in the session's directory (resolved from
/// the id via layout discovery). Files that fail to parse are skipped (read-only
/// tolerance: one corrupted doc must not hide the rest); a missing `enhanced/`
/// directory is an empty list.
pub fn list_enhanced_docs(vault: &Path, session_id: &str) -> Result<Vec<EnhancedDoc>> {
    list_enhanced_docs_in(vault, &layout::artifact_dir(vault, session_id)?, session_id)
}

/// `list_enhanced_docs` for an already-resolved session directory (vault-relative);
/// `session_id` is stamped into each returned doc.
pub fn list_enhanced_docs_in(
    vault: &Path,
    session_dir: &Path,
    session_id: &str,
) -> Result<Vec<EnhancedDoc>> {
    let dir = vault.join(paths::enhanced_dir_in(session_dir));
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(format!("failed to read enhanced dir: {e}"))),
    };

    let mut docs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::Io(format!("failed to read dir entry: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(doc) = parse_enhanced_file(stem, session_id, &raw) {
            docs.push(doc);
        }
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> EnhancedDoc {
        EnhancedDoc {
            id: "doc-1".to_string(),
            session_id: "s1".to_string(),
            kind: "template_output".to_string(),
            title: "Customer review".to_string(),
            template_id: "template-1".to_string(),
            sort_order: 3,
            markdown: "# Review\n\n- Point".to_string(),
        }
    }

    #[test]
    fn render_parse_round_trip_preserves_every_field() {
        let d = doc();
        let rendered = render_enhanced_file(&d).unwrap();
        let parsed = parse_enhanced_file("doc-1", "s1", &rendered).unwrap();
        assert_eq!(parsed, d);
    }

    #[test]
    fn parse_accepts_a_file_without_frontmatter_as_a_plain_summary() {
        let parsed =
            parse_enhanced_file("d1", "s1", "# Just markdown\n\nDropped in by hand.").unwrap();
        assert_eq!(parsed.kind, "summary");
        assert_eq!(parsed.markdown, "# Just markdown\n\nDropped in by hand.");
    }

    #[test]
    fn parse_degrades_an_unknown_kind_to_summary() {
        let raw = "---\nkind: nonsense\ntitle: T\n---\n\nbody";
        let parsed = parse_enhanced_file("d1", "s1", raw).unwrap();
        assert_eq!(parsed.kind, "summary");
        assert_eq!(parsed.title, "T");
    }

    #[test]
    fn parse_errors_on_malformed_frontmatter() {
        let raw = "---\ntitle: [unclosed\n---\n\nbody";
        assert!(parse_enhanced_file("d1", "s1", raw).is_err());
    }

    #[test]
    fn list_enhanced_docs_reads_all_parseable_docs_and_tolerates_absence() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            list_enhanced_docs(temp.path(), "s1").unwrap().is_empty(),
            "missing enhanced dir must read as no docs"
        );

        let dir = temp.path().join("sessions/s1/enhanced");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("doc-1.md"), render_enhanced_file(&doc()).unwrap()).unwrap();
        std::fs::write(dir.join("broken.md"), "---\ntitle: [unclosed\n---\nbody").unwrap();
        std::fs::write(dir.join("notes.txt"), "not markdown").unwrap();

        let docs = list_enhanced_docs(temp.path(), "s1").unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0], doc());
    }
}
