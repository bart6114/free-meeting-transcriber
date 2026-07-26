use std::collections::HashMap;
use std::str::FromStr;

/// The store's note/document writers never write a frontmatter block -- `_memo.md` and every
/// other `sessions/<id>/<kind>.md` file are meant to hold raw markdown only. A file can still
/// gain a leading frontmatter block from outside those writers: an external edit, or the
/// legacy `vault_export` DB-to-vault mirror, which always wrapped a `session_documents` row's
/// body in one on export, and which could nest a wrapper on top of an already-wrapped file.
/// Those wrapped files still exist in real vaults, so the strip stays load-bearing.
///
/// Strips repeatedly, one layer per loop iteration, so a file carrying two or more nested
/// exporter wrappers converges to the true inner content in a single call.
///
/// Each layer is only stripped if it's *recognizable as the exporter's own wrapping* -- its
/// frontmatter has an `id` and/or `position` key, the keys the legacy exporter's
/// `render_session_document` always wrote (see `crates/fs-sync-core/src/export.rs`). A block
/// that parses as well-formed frontmatter but has neither key is treated as genuine user
/// content and returned untouched from that point on. A file with no frontmatter at all, or
/// one that starts with `---` but isn't a well-formed block (e.g. a note opening with a
/// horizontal rule), round-trips completely unchanged.
pub fn strip_leading_frontmatter(content: String) -> String {
    let mut current = content;
    loop {
        let parsed =
            match hypr_frontmatter::Document::<HashMap<String, serde_yaml::Value>>::from_str(
                &current,
            ) {
                Ok(parsed) => parsed,
                Err(_) => return current,
            };
        if !is_exporter_wrapper(&parsed.frontmatter) {
            return current;
        }
        current = parsed.content;
    }
}

/// The specific, narrow signal that a parsed leading frontmatter block is the legacy
/// exporter's own wrapping rather than arbitrary user/third-party frontmatter: either an
/// `id` or a `position` key is present.
fn is_exporter_wrapper(frontmatter: &HashMap<String, serde_yaml::Value>) -> bool {
    frontmatter.contains_key("id") || frontmatter.contains_key("position")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_nested_exporter_layers() {
        let input = "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\n\
                     ---\nid: s1\nposition: 0\nsession_id: s1\n---\n\nreal content";
        assert_eq!(strip_leading_frontmatter(input.to_string()), "real content");
    }

    #[test]
    fn leaves_non_exporter_frontmatter_untouched() {
        let input = "---\ntitle: My Doc\nauthor: me\n---\n\nActual user content.";
        assert_eq!(strip_leading_frontmatter(input.to_string()), input);
    }

    #[test]
    fn leaves_unparseable_leading_dashes_untouched() {
        let input = "---\n\nActual note that opens with a horizontal rule.";
        assert_eq!(strip_leading_frontmatter(input.to_string()), input);
    }

    #[test]
    fn plain_markdown_round_trips_unchanged() {
        let input = "# Meeting notes\n\nDiscussed: X, Y, Z";
        assert_eq!(strip_leading_frontmatter(input.to_string()), input);
    }

    #[test]
    fn empty_exporter_wrapper_strips_to_empty_string() {
        let input = "---\nid: s1:note\nposition: 0\nsession_id: s1\n---\n\n";
        assert_eq!(strip_leading_frontmatter(input.to_string()), "");
    }
}
