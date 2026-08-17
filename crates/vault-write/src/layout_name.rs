//! Pure naming policy for human-readable session directories:
//! `YYYY-MM-DD — <sanitized title> — <short id>`. Nothing here touches the
//! filesystem or the clock -- callers resolve dates and check collisions
//! themselves -- and nothing here is ever an identity API: `_meta.json.id`
//! stays authoritative, directory names are presentation only.

use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

/// The separator between the date, title, and short-id segments. Titles may
/// themselves contain an em dash; no code may recover identity by splitting on it.
pub const SEGMENT_SEPARATOR: &str = " — ";

/// Title used when a session has no usable title yet; `is_provisional_untitled_name`
/// recognizes directories carrying it so the first real title can rename them once.
pub const UNTITLED: &str = "Untitled";

/// Maximum byte length of a full directory component. Common filesystems cap
/// components at 255 bytes; 180 leaves comfortable headroom for sync-provider
/// conflict suffixes and future decorations.
pub const MAX_COMPONENT_BYTES: usize = 180;

const HOSTILE: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Plan rules 1-7: trim, collapse whitespace, replace filesystem-hostile and control
/// characters with `-`, collapse the separators that replacement introduced, drop
/// cross-platform-hostile trailing periods/spaces, fall back to `Untitled`, and
/// normalize to NFC. Deliberately does NOT truncate -- byte budgeting needs the full
/// component, so it lives in `format_session_dir_name`.
pub fn sanitize_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| {
            // Whitespace first: newlines/tabs are also control characters, and the
            // plan collapses them to spaces rather than replacing them with `-`.
            if c.is_whitespace() {
                ' '
            } else if HOSTILE.contains(&c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();

    let mut collapsed = String::with_capacity(replaced.len());
    let mut previous: Option<char> = None;
    for c in replaced.chars() {
        if (c == ' ' || c == '-') && previous == Some(c) {
            continue;
        }
        collapsed.push(c);
        previous = Some(c);
    }

    let trimmed = collapsed
        .trim_matches([' ', '-'])
        .trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return UNTITLED.to_string();
    }
    trimmed.nfc().collect()
}

/// Deliberately permissive (32 hex chars, hyphens stripped wherever they sit)
/// rather than `Uuid::try_parse`: legacy ids with noncanonical dash placement
/// already have directories named from their truncated hex, and tightening the
/// check would silently reroute them to the hashed fallback -- renaming their
/// future candidates for no correctness benefit.
fn is_uuid_shaped(id: &str) -> bool {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn id_hex(id: &str) -> String {
    if is_uuid_shaped(id) {
        id.chars()
            .filter(|c| *c != '-')
            .collect::<String>()
            .to_ascii_lowercase()
    } else {
        // Legacy non-UUID id: a stable hash keeps the suffix hexadecimal instead of
        // interpolating unsafe id text into a directory name.
        use sha2::{Digest, Sha256};
        use std::fmt::Write;
        Sha256::digest(id.as_bytes())
            .iter()
            .fold(String::with_capacity(64), |mut out, byte| {
                write!(out, "{byte:02x}").expect("writing to String cannot fail");
                out
            })
    }
}

/// Short-id suffixes to try in order when choosing a directory name: 6, then 8, then
/// 12 hex characters, then the full hex form -- the caller takes the first candidate
/// whose target directory is free and must never merge onto an occupied one.
pub fn short_id_candidates(id: &str) -> Vec<String> {
    let hex = id_hex(id);
    let mut candidates: Vec<String> = [6, 8, 12, hex.len()]
        .into_iter()
        .filter(|len| *len <= hex.len())
        .map(|len| hex[..len].to_string())
        .collect();
    candidates.dedup();
    candidates
}

/// Readable directory candidates for one session, in the order a caller must try
/// them: `<parent>/<date — title — suffix>` with the suffix widening
/// 6 → 8 → 12 → full hex. This owns the whole candidate-construction invariant;
/// callers keep only their genuinely different occupancy rules (legacy ghost
/// adoption, current-directory self-filter, migration's preflight claim set).
pub fn session_dir_candidates(parent: &Path, date: &str, title: &str, id: &str) -> Vec<PathBuf> {
    short_id_candidates(id)
        .into_iter()
        .map(|suffix| parent.join(format_session_dir_name(date, title, &suffix)))
        .collect()
}

/// `YYYY-MM-DD — <title> — <suffix>`, with the title sanitized and truncated on a
/// character boundary so the whole component stays within `MAX_COMPONENT_BYTES`.
/// `date` must already be resolved (see `session_date`); this function never reads
/// the clock so tests stay deterministic.
pub fn format_session_dir_name(date: &str, title: &str, id_suffix: &str) -> String {
    let sanitized = sanitize_title(title);
    let fixed = date.len() + 2 * SEGMENT_SEPARATOR.len() + id_suffix.len();
    let title_budget = MAX_COMPONENT_BYTES.saturating_sub(fixed);

    let mut truncated = String::new();
    for c in sanitized.chars() {
        if truncated.len() + c.len_utf8() > title_budget {
            break;
        }
        truncated.push(c);
    }
    // Truncation can strand a trailing space/dash/dot; re-trim (never to empty --
    // the budget always fits "Untitled", and a fully-stranded title falls back).
    let truncated = truncated
        .trim_matches([' ', '-'])
        .trim_end_matches(['.', ' ']);
    let title_segment = if truncated.is_empty() {
        UNTITLED
    } else {
        truncated
    };

    format!("{date}{SEGMENT_SEPARATOR}{title_segment}{SEGMENT_SEPARATOR}{id_suffix}")
}

/// The session's readable-name date, per the plan's date semantics: prefer
/// `started_at`, else `created_at`; parse RFC3339 and convert to the machine's local
/// calendar date; else take the first valid `YYYY-MM-DD` prefix of the stored value;
/// else fall back to `today_local` (the caller resolves "today" so this stays pure)
/// and report the malformed value as a diagnostic.
pub fn session_date(
    started_at: Option<&str>,
    created_at: &str,
    today_local: &str,
) -> (String, Option<String>) {
    // A malformed started_at falls through to created_at rather than straight to
    // "today": a legacy session with junk in one field but a valid timestamp in the
    // other must not be permanently named with the migration date.
    let sources = [
        started_at.map(str::trim).filter(|s| !s.is_empty()),
        Some(created_at.trim()).filter(|s| !s.is_empty()),
    ];
    for source in sources.iter().flatten() {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(source) {
            let local = parsed.with_timezone(&chrono::Local);
            return (local.format("%Y-%m-%d").to_string(), None);
        }
        if let Some(prefix) = valid_date_prefix(source) {
            return (prefix.to_string(), None);
        }
    }
    (
        today_local.to_string(),
        Some(format!(
            "unusable session timestamps (started_at {started_at:?}, created_at {created_at:?})"
        )),
    )
}

fn valid_date_prefix(value: &str) -> Option<&str> {
    let prefix = value.get(..10)?;
    let bytes = prefix.as_bytes();
    let shape_ok = bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 => *b == b'-',
        _ => b.is_ascii_digit(),
    });
    (shape_ok && chrono::NaiveDate::parse_from_str(prefix, "%Y-%m-%d").is_ok()).then_some(prefix)
}

/// Whether a directory basename is exactly the app's own provisional
/// `YYYY-MM-DD — Untitled — <hex suffix>` form. A lifecycle hint only -- it decides
/// whether the first real title may rename the directory once -- never an identity
/// check, and a user-renamed directory that happens to not match simply keeps its
/// name.
pub fn is_provisional_untitled_name(basename: &str) -> bool {
    let Some(rest) = strip_date_prefix(basename) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(SEGMENT_SEPARATOR) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(UNTITLED) else {
        return false;
    };
    let Some(suffix) = rest.strip_prefix(SEGMENT_SEPARATOR) else {
        return false;
    };
    (6..=64).contains(&suffix.len())
        && suffix
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn strip_date_prefix(value: &str) -> Option<&str> {
    let prefix = value.get(..10)?;
    valid_date_prefix(prefix)?;
    value.get(10..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_normal_and_unicode_titles_readable() {
        assert_eq!(sanitize_title("Product planning"), "Product planning");
        assert_eq!(sanitize_title("Café sync 🚀"), "Café sync 🚀");
        assert_eq!(
            sanitize_title("Retro — Q3 planning"),
            "Retro — Q3 planning",
            "titles may contain the segment separator's em dash"
        );
    }

    #[test]
    fn sanitize_replaces_hostile_and_control_characters() {
        assert_eq!(
            sanitize_title("a/b\\c:d*e?f\"g<h>i|j"),
            "a-b-c-d-e-f-g-h-i-j"
        );
        assert_eq!(sanitize_title("bell\u{7}title"), "bell-title");
    }

    #[test]
    fn sanitize_collapses_whitespace_and_repeated_separators() {
        assert_eq!(
            sanitize_title("  line one\n\tline   two  "),
            "line one line two"
        );
        assert_eq!(sanitize_title("a//b??c"), "a-b-c");
        assert_eq!(
            sanitize_title("weird / path // here"),
            "weird - path - here"
        );
    }

    #[test]
    fn sanitize_strips_trailing_periods_and_falls_back_to_untitled() {
        assert_eq!(sanitize_title("Notes..."), "Notes");
        assert_eq!(sanitize_title(""), "Untitled");
        assert_eq!(sanitize_title("  ??? ///  "), "Untitled");
        assert_eq!(sanitize_title("..."), "Untitled");
    }

    #[test]
    fn sanitize_emits_nfc() {
        let nfd = "Cafe\u{301}";
        assert_eq!(sanitize_title(nfd), "Caf\u{e9}");
    }

    #[test]
    fn short_id_candidates_expand_from_six_to_full() {
        let candidates = short_id_candidates("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(
            candidates,
            vec![
                "550e84".to_string(),
                "550e8400".to_string(),
                "550e8400e29b".to_string(),
                "550e8400e29b41d4a716446655440000".to_string(),
            ]
        );
    }

    #[test]
    fn short_id_candidates_hash_a_legacy_non_uuid_id() {
        let candidates = short_id_candidates("legacy/id with junk");
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0].len(), 6);
        assert_eq!(candidates[3].len(), 64);
        for candidate in &candidates {
            assert!(
                candidate
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{candidate:?} must be lowercase hex, never raw id text"
            );
        }
        assert_eq!(
            candidates,
            short_id_candidates("legacy/id with junk"),
            "the hash must be stable"
        );
    }

    #[test]
    fn session_dir_candidates_walk_the_suffix_ladder_under_the_parent() {
        let candidates = session_dir_candidates(
            Path::new("sessions/Work"),
            "2026-03-20",
            "Planning",
            "550e8400-e29b-41d4-a716-446655440000",
        );
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("sessions/Work/2026-03-20 — Planning — 550e84"),
                PathBuf::from("sessions/Work/2026-03-20 — Planning — 550e8400"),
                PathBuf::from("sessions/Work/2026-03-20 — Planning — 550e8400e29b"),
                PathBuf::from(
                    "sessions/Work/2026-03-20 — Planning — 550e8400e29b41d4a716446655440000"
                ),
            ]
        );
    }

    #[test]
    fn format_builds_the_readable_component() {
        assert_eq!(
            format_session_dir_name("2026-03-20", "Product planning", "550e84"),
            "2026-03-20 — Product planning — 550e84"
        );
        assert_eq!(
            format_session_dir_name("2026-03-20", "", "550e84"),
            "2026-03-20 — Untitled — 550e84"
        );
    }

    #[test]
    fn format_truncates_multibyte_titles_on_character_boundaries_within_budget() {
        let long_title = "é".repeat(200);
        let name = format_session_dir_name("2026-03-20", &long_title, "550e84");
        assert!(name.len() <= MAX_COMPONENT_BYTES, "{} bytes", name.len());
        assert!(name.starts_with("2026-03-20 — é"));
        assert!(name.ends_with(" — 550e84"));
        assert!(std::str::from_utf8(name.as_bytes()).is_ok());
    }

    #[test]
    fn format_with_the_full_suffix_still_fits_the_budget() {
        let name = format_session_dir_name(
            "2026-03-20",
            &"x".repeat(300),
            "550e8400e29b41d4a716446655440000",
        );
        assert!(name.len() <= MAX_COMPONENT_BYTES);
    }

    #[test]
    fn provisional_recognition_matches_only_the_exact_untitled_form() {
        assert!(is_provisional_untitled_name(
            "2026-03-20 — Untitled — 550e84"
        ));
        assert!(is_provisional_untitled_name(
            "2026-03-20 — Untitled — 550e8400e29b41d4a716446655440000"
        ));
        for name in [
            "2026-03-20 — Product planning — 550e84",
            "2026-03-20 — Untitled — ZZZZZZ",
            "2026-03-20 — Untitled — 550e",
            "Untitled — 550e84",
            "2026-13-99 — Untitled — 550e84",
            "550e8400-e29b-41d4-a716-446655440000",
            "2026-03-20 — Untitled extra — 550e84",
        ] {
            assert!(!is_provisional_untitled_name(name), "{name:?}");
        }
    }

    #[test]
    fn stable_rename_between_provisional_and_final_keeps_the_date_and_suffix() {
        let provisional = format_session_dir_name("2026-03-20", "", "550e84");
        assert!(is_provisional_untitled_name(&provisional));
        let final_name = format_session_dir_name("2026-03-20", "Roadmap review", "550e84");
        assert_eq!(final_name, "2026-03-20 — Roadmap review — 550e84");
        assert!(!is_provisional_untitled_name(&final_name));
    }

    #[test]
    fn session_date_prefers_started_at_then_prefix_then_fallback() {
        let (date, diag) = session_date(None, "2026-07-01T00:00:00Z", "2026-08-16");
        assert!(diag.is_none());
        assert_eq!(date.len(), 10);

        let (date, diag) = session_date(Some("2026-03-20"), "2026-07-01T00:00:00Z", "2026-08-16");
        assert_eq!(date, "2026-03-20");
        assert!(
            diag.is_none(),
            "a bare date prefix is usable without parsing"
        );

        let (date, diag) = session_date(Some("not a time"), "also junk", "2026-08-16");
        assert_eq!(date, "2026-08-16");
        assert!(diag.is_some());

        let (date, diag) = session_date(Some("unknown"), "2026-03-01T00:00:00Z", "2026-08-16");
        assert_eq!(date.len(), 10);
        assert_ne!(
            date, "2026-08-16",
            "a malformed started_at must fall through to created_at, not to today"
        );
        assert!(diag.is_none());

        let (date, _) = session_date(Some("   "), "2026-05-05T10:00:00+02:00", "2026-08-16");
        assert_eq!(
            date.len(),
            10,
            "blank started_at falls through to created_at"
        );
    }
}
