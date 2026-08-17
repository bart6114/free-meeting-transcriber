//! Filesystem identity rules for session directories.
//!
//! A directory is a session directory when it contains a parseable `_meta.json`;
//! `_meta.json.id` is the logical identity. The directory basename is presentation
//! only and must never be parsed to recover the id, so both legacy UUID-named
//! directories (`sessions/<uuid>`) and human-readable ones
//! (`sessions/2026-03-20 — Planning — 550e84`), nested in personal folders or not,
//! resolve identically.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use unicode_normalization::{UnicodeNormalization, is_nfc};

use crate::{Error, Result, SessionMeta, paths};

/// Physical home of one session: the logical id (from `_meta.json.id`) plus the
/// vault-relative directory holding its artifacts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionLocation {
    pub id: String,
    pub relative_dir: PathBuf,
}

/// Diagnostics from a vault scan. Discovery is read-only and tolerant: one corrupt
/// or duplicated session never hides the healthy ones, it lands here instead.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SessionDiscoveryError {
    /// Two or more directories claim the same `_meta.json.id`. Which one is "real"
    /// must never be resolved by traversal order; none of them are listed as healthy.
    #[error(
        "session id '{id}' is claimed by multiple directories: {}",
        format_dirs(dirs)
    )]
    DuplicateId { id: String, dirs: Vec<PathBuf> },
    /// `_meta.json` exists but cannot be read or parsed. The directory is left
    /// untouched and is never treated as deleted, migrated, or a parent folder.
    #[error("corrupt session metadata in '{}': {reason}", dir.display())]
    CorruptMeta { dir: PathBuf, reason: String },
    #[error("failed to scan '{}': {reason}", dir.display())]
    Unreadable { dir: PathBuf, reason: String },
}

#[derive(Debug, Default)]
pub struct SessionDiscovery {
    pub sessions: Vec<(SessionLocation, SessionMeta)>,
    pub errors: Vec<SessionDiscoveryError>,
}

/// Outcome of resolving one full id, distinguishing "no such session" (`Ok(None)`)
/// from "something is there but unusable" (corrupt or ambiguous).
#[derive(Debug, thiserror::Error)]
pub enum SessionLookupError {
    #[error(
        "session id '{id}' is claimed by multiple directories: {}",
        format_dirs(dirs)
    )]
    Ambiguous { id: String, dirs: Vec<PathBuf> },
    #[error("corrupt session metadata in '{}': {reason}", dir.display())]
    Corrupt { dir: PathBuf, reason: String },
    #[error("I/O error: {0}")]
    Io(String),
}

fn format_dirs(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .map(|dir| format!("'{}'", dir.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// NFC-normalize a name for comparison. macOS APFS preserves whatever form it is
/// given, but sync providers and cross-machine copies can return a differently
/// composed form of the same name, so byte-for-byte comparison is a live bug once
/// names contain user text.
pub fn nfc(value: &str) -> Cow<'_, str> {
    if is_nfc(value) {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(value.nfc().collect())
    }
}

/// NFC-normalized equality for file/directory names and identifiers.
pub fn eq_nfc(a: &str, b: &str) -> bool {
    nfc(a) == nfc(b)
}

/// Component-wise NFC-normalized prefix test: whether `path` is `prefix` itself or
/// nested somewhere under it (non-UTF-8 components fall back to exact comparison).
pub fn path_starts_with_nfc(path: &Path, prefix: &Path) -> bool {
    let mut path_components = path.components();
    for prefix_component in prefix.components() {
        let Some(path_component) = path_components.next() else {
            return false;
        };
        let equal = match (
            path_component.as_os_str().to_str(),
            prefix_component.as_os_str().to_str(),
        ) {
            (Some(a), Some(b)) => eq_nfc(a, b),
            _ => path_component == prefix_component,
        };
        if !equal {
            return false;
        }
    }
    true
}

/// Component-wise NFC-normalized path equality (non-UTF-8 components fall back to
/// exact comparison).
pub fn paths_eq_nfc(a: &Path, b: &Path) -> bool {
    let mut left = a.components();
    let mut right = b.components();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => {
                let equal = match (x.as_os_str().to_str(), y.as_os_str().to_str()) {
                    (Some(x), Some(y)) => eq_nfc(x, y),
                    _ => x == y,
                };
                if !equal {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// The one session-boundary rule, shared by every layout consumer: `_meta.json`
/// absent → personal folder; present and parseable → session; present but
/// unreadable/corrupt → still a session boundary, never a folder (its content
/// must not be traversed, misread as nested sessions, or treated as deletable).
pub enum SessionDirKind {
    Session(Box<SessionMeta>),
    /// `_meta.json` exists but cannot be read or parsed; carries the reason.
    Corrupt(String),
    Folder,
}

pub fn classify_session_dir(abs_dir: &Path) -> SessionDirKind {
    match std::fs::read(abs_dir.join("_meta.json")) {
        Ok(bytes) => match serde_json::from_slice::<SessionMeta>(&bytes) {
            Ok(meta) => SessionDirKind::Session(Box::new(meta)),
            Err(e) => SessionDirKind::Corrupt(format!("failed to deserialize meta: {e}")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SessionDirKind::Folder,
        Err(e) => SessionDirKind::Corrupt(format!("failed to read meta file: {e}")),
    }
}

/// Cheap boundary probe for traversals that only need "may recurse" versus "must
/// stop": stats `_meta.json` without parsing it. `NotFound` is the only answer that
/// makes a folder; any other stat error is conservatively a boundary — the same
/// rule as `classify_session_dir`, and deliberately not `Path::exists()`, which
/// would hide a permission error as absence and let a traversal walk into a
/// session directory it cannot actually judge.
pub fn has_session_boundary(abs_dir: &Path) -> bool {
    match std::fs::metadata(abs_dir.join("_meta.json")) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

/// Recursively scan `sessions/` for session directories. Hidden (dot-prefixed)
/// directories are skipped, and a directory containing `_meta.json` — even a corrupt
/// one — is never descended into, so session content (`enhanced/`, `attachments/`)
/// is never misread as nested sessions. A missing `sessions/` directory is an empty
/// vault, not an error.
pub fn discover_sessions(vault: &Path) -> Result<SessionDiscovery> {
    let root = vault.join(paths::sessions_root());
    match std::fs::metadata(&root) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionDiscovery::default());
        }
        Err(e) => return Err(Error::Io(format!("failed to read sessions dir: {e}"))),
    }

    let mut found: Vec<(SessionLocation, SessionMeta)> = Vec::new();
    let mut errors = Vec::new();
    let mut pending = vec![paths::sessions_root()];
    while let Some(relative_dir) = pending.pop() {
        let entries = match std::fs::read_dir(vault.join(&relative_dir)) {
            Ok(entries) => entries,
            Err(e) => {
                errors.push(SessionDiscoveryError::Unreadable {
                    dir: relative_dir,
                    reason: e.to_string(),
                });
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            // file_type() does not follow symlinks; a symlinked directory is
            // deliberately not traversed (symlink layouts are unsupported).
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let child = relative_dir.join(&name);
            match classify_session_dir(&entry.path()) {
                SessionDirKind::Session(meta) => found.push((
                    SessionLocation {
                        id: meta.id.clone(),
                        relative_dir: child,
                    },
                    *meta,
                )),
                SessionDirKind::Corrupt(reason) => {
                    errors.push(SessionDiscoveryError::CorruptMeta { dir: child, reason })
                }
                SessionDirKind::Folder => pending.push(child),
            }
        }
    }

    // Group by NFC-normalized id: an id claimed by more than one directory is an
    // explicit ambiguity — report every claimant and list none of them as healthy.
    let mut by_id: BTreeMap<String, Vec<(SessionLocation, SessionMeta)>> = BTreeMap::new();
    for (location, meta) in found {
        by_id
            .entry(nfc(&location.id).into_owned())
            .or_default()
            .push((location, meta));
    }
    let mut sessions = Vec::new();
    for (_, mut claims) in by_id {
        if claims.len() == 1 {
            sessions.push(claims.pop().expect("one claim"));
        } else {
            let mut dirs: Vec<PathBuf> = claims
                .iter()
                .map(|(location, _)| location.relative_dir.clone())
                .collect();
            dirs.sort();
            errors.push(SessionDiscoveryError::DuplicateId {
                id: claims[0].0.id.clone(),
                dirs,
            });
        }
    }
    sessions.sort_by(|a, b| a.0.relative_dir.cmp(&b.0.relative_dir));
    errors.sort_by_key(|error| error.to_string());
    Ok(SessionDiscovery { sessions, errors })
}

/// Resolve one full id to its physical location.
///
/// The legacy `sessions/<id>` path is probed first: when its metadata confirms the
/// id, that canonical location wins without a vault scan (this keeps per-session
/// reads O(1) for legacy vaults; `discover_sessions` still reports a duplicate copy
/// elsewhere as a diagnostic). Otherwise a full discovery scan resolves the id by
/// `_meta.json.id`, wherever and whatever the directory is named.
pub fn find_session(
    vault: &Path,
    id: &str,
) -> std::result::Result<Option<(SessionLocation, SessionMeta)>, SessionLookupError> {
    find_session_and_scan(vault, id).map(|(found, _)| found)
}

/// `find_session`, additionally handing back the discovery scan when one was
/// performed, so a caller maintaining a location cache can warm every healthy
/// location from the walk it already paid for. `None` for the scan means the
/// legacy `sessions/<id>` fast path answered without scanning.
pub fn find_session_and_scan(
    vault: &Path,
    id: &str,
) -> std::result::Result<
    (
        Option<(SessionLocation, SessionMeta)>,
        Option<SessionDiscovery>,
    ),
    SessionLookupError,
> {
    let legacy_dir = paths::sessions_root().join(id);
    let mut legacy_corrupt = None;
    match classify_session_dir(&vault.join(&legacy_dir)) {
        SessionDirKind::Session(meta) if eq_nfc(&meta.id, id) => {
            return Ok((
                Some((
                    SessionLocation {
                        id: meta.id.clone(),
                        relative_dir: legacy_dir,
                    },
                    *meta,
                )),
                None,
            ));
        }
        SessionDirKind::Corrupt(reason) => legacy_corrupt = Some((legacy_dir, reason)),
        _ => {}
    }

    let discovery = discover_sessions(vault).map_err(|e| SessionLookupError::Io(e.to_string()))?;
    for error in &discovery.errors {
        if let SessionDiscoveryError::DuplicateId { id: claimed, dirs } = error
            && eq_nfc(claimed, id)
        {
            return Err(SessionLookupError::Ambiguous {
                id: claimed.clone(),
                dirs: dirs.clone(),
            });
        }
    }
    if let Some(found) = discovery
        .sessions
        .iter()
        .find(|(location, _)| eq_nfc(&location.id, id))
        .cloned()
    {
        return Ok((Some(found), Some(discovery)));
    }
    match legacy_corrupt {
        Some((dir, reason)) => Err(SessionLookupError::Corrupt { dir, reason }),
        None => Ok((None, Some(discovery))),
    }
}

/// Vault-relative directory that artifact reads for `id` should use: the discovered
/// location when the id resolves, otherwise the legacy `sessions/<id>` path. The
/// fallback preserves the historical tolerance of artifact readers — a directory
/// whose metadata is corrupt or absent still has its note/transcript/tasks readable
/// at the legacy path, and a genuinely missing session reads as empty there.
pub fn artifact_dir(vault: &Path, id: &str) -> Result<PathBuf> {
    match find_session(vault, id) {
        Ok(Some((location, _))) => Ok(location.relative_dir),
        Ok(None) => Ok(paths::sessions_root().join(id)),
        Err(SessionLookupError::Corrupt { dir, .. }) => Ok(dir),
        Err(error @ SessionLookupError::Ambiguous { .. }) => Err(Error::Parse(error.to_string())),
        Err(SessionLookupError::Io(reason)) => Err(Error::Io(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID_1: &str = "550e8400-e29b-41d4-a716-446655440000";
    const UUID_2: &str = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
    const UUID_3: &str = "6ba7b811-9dad-11d1-80b4-00c04fd430c8";
    const UUID_4: &str = "6ba7b812-9dad-11d1-80b4-00c04fd430c8";
    const UUID_5: &str = "6ba7b814-9dad-11d1-80b4-00c04fd430c8";

    fn seed_session_at(vault: &Path, relative_dir: &str, id: &str) {
        let dir = vault.join(relative_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            serde_json::json!({
                "id": id,
                "title": format!("Session {id}"),
                "started_at": null,
                "ended_at": null,
                "created_at": "2026-07-01T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
    }

    fn discovered_dir(discovery: &SessionDiscovery, id: &str) -> Option<PathBuf> {
        discovery
            .sessions
            .iter()
            .find(|(location, _)| location.id == id)
            .map(|(location, _)| location.relative_dir.clone())
    }

    #[test]
    fn discovers_uuid_readable_nested_and_renamed_layouts() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(vault.path(), &format!("sessions/{UUID_1}"), UUID_1);
        seed_session_at(
            vault.path(),
            "sessions/2026-03-20 — Product planning — 6ba7b8",
            UUID_2,
        );
        seed_session_at(vault.path(), &format!("sessions/Work/{UUID_3}"), UUID_3);
        seed_session_at(
            vault.path(),
            "sessions/Work/2026-04-01 — Retro — 6ba7b8",
            UUID_4,
        );
        seed_session_at(vault.path(), "sessions/My renamed notes", UUID_5);

        let discovery = discover_sessions(vault.path()).unwrap();
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.sessions.len(), 5);
        assert_eq!(
            discovered_dir(&discovery, UUID_1).unwrap(),
            PathBuf::from(format!("sessions/{UUID_1}"))
        );
        assert_eq!(
            discovered_dir(&discovery, UUID_2).unwrap(),
            PathBuf::from("sessions/2026-03-20 — Product planning — 6ba7b8")
        );
        assert_eq!(
            discovered_dir(&discovery, UUID_3).unwrap(),
            PathBuf::from(format!("sessions/Work/{UUID_3}"))
        );
        assert_eq!(
            discovered_dir(&discovery, UUID_4).unwrap(),
            PathBuf::from("sessions/Work/2026-04-01 — Retro — 6ba7b8")
        );
        assert_eq!(
            discovered_dir(&discovery, UUID_5).unwrap(),
            PathBuf::from("sessions/My renamed notes")
        );

        // Every layout resolves identically by full id.
        for (id, dir) in [
            (UUID_1, format!("sessions/{UUID_1}")),
            (
                UUID_2,
                "sessions/2026-03-20 — Product planning — 6ba7b8".to_string(),
            ),
            (UUID_3, format!("sessions/Work/{UUID_3}")),
            (
                UUID_4,
                "sessions/Work/2026-04-01 — Retro — 6ba7b8".to_string(),
            ),
            (UUID_5, "sessions/My renamed notes".to_string()),
        ] {
            let (location, meta) = find_session(vault.path(), id).unwrap().unwrap();
            assert_eq!(location.relative_dir, PathBuf::from(dir), "id {id}");
            assert_eq!(location.id, id);
            assert_eq!(meta.id, id);
        }
    }

    #[test]
    fn identity_comes_from_meta_never_from_the_basename() {
        let vault = tempfile::tempdir().unwrap();
        // A directory named like one uuid whose metadata claims another.
        seed_session_at(vault.path(), &format!("sessions/{UUID_1}"), UUID_2);

        let discovery = discover_sessions(vault.path()).unwrap();
        assert_eq!(discovery.sessions.len(), 1);
        assert_eq!(discovery.sessions[0].0.id, UUID_2);

        let (location, _) = find_session(vault.path(), UUID_2).unwrap().unwrap();
        assert_eq!(
            location.relative_dir,
            PathBuf::from(format!("sessions/{UUID_1}"))
        );
        assert!(
            find_session(vault.path(), UUID_1).unwrap().is_none(),
            "the basename must never be parsed as an identity"
        );
    }

    #[test]
    fn corrupt_meta_is_reported_without_hiding_healthy_sessions() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(vault.path(), &format!("sessions/{UUID_1}"), UUID_1);
        let broken = vault.path().join("sessions/broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("_meta.json"), "{ invalid").unwrap();

        let discovery = discover_sessions(vault.path()).unwrap();
        assert_eq!(discovery.sessions.len(), 1);
        assert_eq!(discovery.sessions[0].0.id, UUID_1);
        assert!(matches!(
            &discovery.errors[..],
            [SessionDiscoveryError::CorruptMeta { dir, .. }] if dir == &PathBuf::from("sessions/broken")
        ));

        assert!(matches!(
            find_session(vault.path(), "broken"),
            Err(SessionLookupError::Corrupt { .. })
        ));
    }

    #[test]
    fn duplicate_ids_are_ambiguous_never_resolved_by_traversal_order() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(
            vault.path(),
            "sessions/2026-03-20 — Planning — 6ba7b8",
            UUID_2,
        );
        seed_session_at(vault.path(), "sessions/Work/Planning copy", UUID_2);
        seed_session_at(vault.path(), &format!("sessions/{UUID_1}"), UUID_1);

        let discovery = discover_sessions(vault.path()).unwrap();
        assert_eq!(
            discovery.sessions.len(),
            1,
            "neither duplicate claimant may be listed as healthy"
        );
        assert_eq!(discovery.sessions[0].0.id, UUID_1);
        let [SessionDiscoveryError::DuplicateId { id, dirs }] = &discovery.errors[..] else {
            panic!(
                "expected one duplicate-id error, got {:?}",
                discovery.errors
            );
        };
        assert_eq!(id, UUID_2);
        assert_eq!(
            dirs,
            &vec![
                PathBuf::from("sessions/2026-03-20 — Planning — 6ba7b8"),
                PathBuf::from("sessions/Work/Planning copy"),
            ]
        );

        assert!(matches!(
            find_session(vault.path(), UUID_2),
            Err(SessionLookupError::Ambiguous { dirs, .. }) if dirs.len() == 2
        ));
        assert!(artifact_dir(vault.path(), UUID_2).is_err());
    }

    #[test]
    fn hidden_and_temp_directories_are_skipped() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(vault.path(), &format!("sessions/{UUID_1}"), UUID_1);
        seed_session_at(
            vault.path(),
            &format!("sessions/.trash/2026-08-01/{UUID_2}"),
            UUID_2,
        );
        seed_session_at(vault.path(), "sessions/.tmp-copy", UUID_3);
        // Stray files next to session dirs are ignored.
        std::fs::write(vault.path().join("sessions/.DS_Store"), b"junk").unwrap();
        std::fs::write(vault.path().join("sessions/stray.md"), b"note").unwrap();

        let discovery = discover_sessions(vault.path()).unwrap();
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        assert_eq!(discovery.sessions.len(), 1);
        assert_eq!(discovery.sessions[0].0.id, UUID_1);
        assert!(find_session(vault.path(), UUID_2).unwrap().is_none());
    }

    #[test]
    fn a_session_directory_is_never_scanned_as_a_parent_folder() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(vault.path(), &format!("sessions/{UUID_1}"), UUID_1);
        // Session content — even a directory that itself looks like a session —
        // must not be discovered once the parent has `_meta.json`.
        std::fs::create_dir_all(vault.path().join(format!("sessions/{UUID_1}/enhanced"))).unwrap();
        seed_session_at(
            vault.path(),
            &format!("sessions/{UUID_1}/attachments/inner"),
            UUID_2,
        );
        // The same rule holds when the parent's meta is corrupt.
        let corrupt = vault.path().join("sessions/corrupt-parent");
        std::fs::create_dir_all(&corrupt).unwrap();
        std::fs::write(corrupt.join("_meta.json"), "{ invalid").unwrap();
        seed_session_at(vault.path(), "sessions/corrupt-parent/nested", UUID_3);

        let discovery = discover_sessions(vault.path()).unwrap();
        assert_eq!(discovery.sessions.len(), 1);
        assert_eq!(discovery.sessions[0].0.id, UUID_1);
        assert!(find_session(vault.path(), UUID_2).unwrap().is_none());
        assert!(find_session(vault.path(), UUID_3).unwrap().is_none());
    }

    #[test]
    fn boundary_probe_and_classifier_agree_on_the_session_boundary_rule() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(vault.path(), "sessions/healthy", UUID_1);
        let corrupt = vault.path().join("sessions/corrupt");
        std::fs::create_dir_all(&corrupt).unwrap();
        std::fs::write(corrupt.join("_meta.json"), "{ invalid").unwrap();
        let folder = vault.path().join("sessions/folder");
        std::fs::create_dir_all(&folder).unwrap();

        let healthy = vault.path().join("sessions/healthy");
        assert!(has_session_boundary(&healthy));
        assert!(matches!(
            classify_session_dir(&healthy),
            SessionDirKind::Session(meta) if meta.id == UUID_1
        ));
        // A corrupt meta is a boundary in both views -- never a folder.
        assert!(has_session_boundary(&corrupt));
        assert!(matches!(
            classify_session_dir(&corrupt),
            SessionDirKind::Corrupt(_)
        ));
        assert!(!has_session_boundary(&folder));
        assert!(matches!(
            classify_session_dir(&folder),
            SessionDirKind::Folder
        ));
    }

    #[test]
    fn missing_sessions_root_is_an_empty_vault() {
        let vault = tempfile::tempdir().unwrap();
        let discovery = discover_sessions(vault.path()).unwrap();
        assert!(discovery.sessions.is_empty());
        assert!(discovery.errors.is_empty());
        assert!(find_session(vault.path(), UUID_1).unwrap().is_none());
    }

    #[test]
    fn artifact_dir_prefers_the_discovered_location_and_falls_back_to_legacy() {
        let vault = tempfile::tempdir().unwrap();
        seed_session_at(vault.path(), "sessions/My renamed notes", UUID_1);

        assert_eq!(
            artifact_dir(vault.path(), UUID_1).unwrap(),
            PathBuf::from("sessions/My renamed notes")
        );
        // Unknown ids keep resolving to the legacy path so absent-artifact reads
        // stay "empty", exactly as before.
        assert_eq!(
            artifact_dir(vault.path(), "ghost").unwrap(),
            PathBuf::from("sessions/ghost")
        );
        // A corrupt legacy meta doesn't block artifact reads from that directory.
        let corrupt = vault.path().join("sessions/broken");
        std::fs::create_dir_all(&corrupt).unwrap();
        std::fs::write(corrupt.join("_meta.json"), "{ invalid").unwrap();
        assert_eq!(
            artifact_dir(vault.path(), "broken").unwrap(),
            PathBuf::from("sessions/broken")
        );
    }

    #[test]
    fn path_starts_with_nfc_matches_prefix_and_nested_paths_across_compositions() {
        let dir = Path::new("sessions").join("2026-03-20 — Caf\u{e9} sync — 6ba7b8");
        let nfd_file = Path::new("sessions")
            .join("2026-03-20 — Cafe\u{301} sync — 6ba7b8")
            .join("_memo.md");
        assert!(path_starts_with_nfc(&dir, &dir));
        assert!(path_starts_with_nfc(&nfd_file, &dir));
        assert!(!path_starts_with_nfc(&dir, &nfd_file));
        assert!(!path_starts_with_nfc(
            Path::new("sessions/other/_memo.md"),
            &dir
        ));
        // A sibling whose name merely extends the prefix's last component must not match.
        assert!(!path_starts_with_nfc(
            Path::new("sessions/abc-extended/_memo.md"),
            Path::new("sessions/abc"),
        ));
    }

    #[test]
    fn nfc_and_nfd_forms_of_the_same_name_compare_equal() {
        let nfc_name = "2026-03-20 — Caf\u{e9} sync — 6ba7b8";
        let nfd_name = "2026-03-20 — Cafe\u{301} sync — 6ba7b8";
        assert_ne!(nfc_name, nfd_name);
        assert!(eq_nfc(nfc_name, nfd_name));
        assert!(!eq_nfc(nfc_name, "2026-03-20 — Cafe sync — 6ba7b8"));
        assert!(paths_eq_nfc(
            &Path::new("sessions").join(nfd_name),
            &Path::new("sessions").join(nfc_name),
        ));
        assert!(!paths_eq_nfc(
            &Path::new("sessions").join(nfc_name),
            Path::new(nfc_name),
        ));
    }

    #[test]
    fn discovery_resolves_sessions_in_nfd_named_directories() {
        let vault = tempfile::tempdir().unwrap();
        let nfd_name = "2026-03-20 — Cafe\u{301} sync — 6ba7b8";
        seed_session_at(vault.path(), &format!("sessions/{nfd_name}"), UUID_2);

        let (location, meta) = find_session(vault.path(), UUID_2).unwrap().unwrap();
        assert_eq!(meta.id, UUID_2);
        // The filesystem may hand back either composition of the stored name;
        // comparisons must treat them as the same directory.
        assert!(paths_eq_nfc(
            &location.relative_dir,
            &Path::new("sessions").join("2026-03-20 — Caf\u{e9} sync — 6ba7b8"),
        ));
    }
}
