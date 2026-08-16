use std::path::{Path, PathBuf};

use hypr_vault_read::{SessionLookupError, SessionMeta, find_session};

use crate::path::is_uuid;
use crate::{Error, Result};

/// How a directory participates in the session layout: one holding `_meta.json`
/// is a session directory (even when the meta is unreadable — such a directory is
/// left untouched, never traversed as a folder); anything else is a personal folder.
pub(crate) enum DirClass {
    /// The id is present when `_meta.json` parses; it comes from `_meta.json.id`,
    /// never from the directory basename.
    Session(Option<String>),
    Folder,
}

pub(crate) fn classify_dir(abs_dir: &Path) -> DirClass {
    match std::fs::read(abs_dir.join("_meta.json")) {
        Ok(bytes) => DirClass::Session(
            serde_json::from_slice::<SessionMeta>(&bytes)
                .ok()
                .map(|meta| meta.id),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DirClass::Folder,
        Err(_) => DirClass::Session(None),
    }
}

pub fn find_session_dir(sessions_base: &Path, session_id: &str) -> Result<PathBuf> {
    if !is_uuid(session_id) {
        return Err(Error::Path("session_id_invalid".into()));
    }

    // vault-read's resolver scans `<vault>/sessions`; callers hand us that
    // directory, so the vault root is its parent.
    let vault = sessions_base
        .parent()
        .ok_or_else(|| Error::Path("sessions_base_invalid".into()))?;

    match find_session(vault, session_id) {
        Ok(Some((location, _))) => Ok(vault.join(location.relative_dir)),
        // Not-yet-created sessions keep resolving to the legacy path so callers
        // that create the directory on demand (audio import, attachments) work.
        Ok(None) => Ok(sessions_base.join(session_id)),
        // A corrupt meta still owns its directory: artifact access stays possible
        // and the directory is never treated as absent.
        Err(SessionLookupError::Corrupt { dir, .. }) => Ok(vault.join(dir)),
        Err(error @ SessionLookupError::Ambiguous { .. }) => Err(Error::Path(error.to_string())),
        Err(SessionLookupError::Io(reason)) => Err(Error::Path(reason)),
    }
}

pub fn delete_session_dir(session_dir: &Path) -> std::io::Result<()> {
    if session_dir.exists() {
        std::fs::remove_dir_all(session_dir)?;
    }
    Ok(())
}

pub fn list_uuid_files(dir: &Path, ext: &str) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            if path.extension().and_then(|e| e.to_str()) != Some(ext) {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            if !is_uuid(stem) {
                return None;
            }
            Some((stem.to_string(), path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{TestEnv, UUID_1, UUID_2};
    use assert_fs::TempDir;
    use assert_fs::prelude::*;
    use predicates::prelude::*;

    #[test]
    fn find_session_at_root() {
        let env = TestEnv::new()
            .folder("sessions")
            .session(UUID_1)
            .done_folder()
            .done()
            .build();

        let result = find_session_dir(&env.path().join("sessions"), UUID_1).unwrap();
        assert_eq!(result, env.folder_session_path("sessions", UUID_1));
    }

    #[test]
    fn find_session_in_nested_folder() {
        let env = TestEnv::new()
            .folder("sessions")
            .done()
            .folder("sessions/work")
            .done()
            .folder("sessions/work/project")
            .session(UUID_1)
            .done_folder()
            .done()
            .build();

        let result = find_session_dir(&env.path().join("sessions"), UUID_1).unwrap();
        assert_eq!(
            result,
            env.path().join("sessions/work/project").join(UUID_1)
        );
    }

    #[test]
    fn find_session_by_meta_id_in_readable_dir_at_root() {
        let env = TestEnv::new()
            .folder("sessions")
            .session(UUID_1)
            .dir_name("2026-03-20 — Product planning — 550e84")
            .done_folder()
            .done()
            .build();

        let result = find_session_dir(&env.path().join("sessions"), UUID_1).unwrap();
        assert_eq!(
            result,
            env.path()
                .join("sessions/2026-03-20 — Product planning — 550e84")
        );
    }

    #[test]
    fn find_session_by_meta_id_in_readable_dir_nested_in_folder() {
        let env = TestEnv::new()
            .folder("sessions")
            .done()
            .folder("sessions/work")
            .session(UUID_1)
            .dir_name("2026-04-01 — Retro — 550e84")
            .done_folder()
            .done()
            .build();

        let result = find_session_dir(&env.path().join("sessions"), UUID_1).unwrap();
        assert_eq!(
            result,
            env.path().join("sessions/work/2026-04-01 — Retro — 550e84")
        );
    }

    #[test]
    fn find_session_reads_identity_from_meta_never_from_the_basename() {
        // A directory named like one uuid whose metadata claims another.
        let env = TestEnv::new()
            .folder("sessions")
            .session(UUID_1)
            .dir_name(UUID_2)
            .done_folder()
            .done()
            .build();

        let result = find_session_dir(&env.path().join("sessions"), UUID_1).unwrap();
        assert_eq!(result, env.path().join("sessions").join(UUID_2));
    }

    #[test]
    fn find_session_duplicate_id_claims_error_instead_of_picking_one() {
        let env = TestEnv::new()
            .folder("sessions")
            .session(UUID_1)
            .dir_name("2026-03-20 — Planning — 550e84")
            .done_folder()
            .done()
            .folder("sessions/work")
            .session(UUID_1)
            .dir_name("Planning copy")
            .done_folder()
            .done()
            .build();

        let result = find_session_dir(&env.path().join("sessions"), UUID_1);
        assert!(matches!(result, Err(Error::Path(_))));
    }

    #[test]
    fn find_session_fallback_when_not_found() {
        let temp = TempDir::new().unwrap();
        let sessions = temp.child("sessions");
        sessions.create_dir_all().unwrap();

        let result = find_session_dir(sessions.path(), UUID_1).unwrap();
        assert_eq!(result, sessions.path().join(UUID_1));
    }

    #[test]
    fn find_session_rejects_non_uuid_session_id() {
        let temp = TempDir::new().unwrap();
        let sessions = temp.child("sessions");
        sessions.create_dir_all().unwrap();

        let result = find_session_dir(sessions.path(), "../outside");

        assert!(matches!(result, Err(Error::Path(message)) if message == "session_id_invalid"));
    }

    #[test]
    fn delete_session_dir_removes_directory() {
        let env = TestEnv::new().session(UUID_1).done().build();

        delete_session_dir(&env.session_path(UUID_1)).unwrap();
        env.child(UUID_1).assert(predicate::path::missing());
    }

    #[test]
    fn delete_session_dir_noop_if_missing() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join(UUID_1);

        let result = delete_session_dir(&missing);
        assert!(result.is_ok());
    }

    #[test]
    fn list_uuid_files_nonexistent_dir_returns_empty() {
        let temp = TempDir::new().unwrap();
        let nonexistent = temp.path().join("does_not_exist");

        let result = list_uuid_files(&nonexistent, "md");

        assert!(result.is_empty());
    }

    #[test]
    fn list_uuid_files_empty_dir_returns_empty() {
        let env = TestEnv::new().build();

        let result = list_uuid_files(env.path(), "md");

        assert!(result.is_empty());
    }

    #[test]
    fn list_uuid_files_finds_uuid_files() {
        let env = TestEnv::new()
            .file(&format!("{UUID_1}.md"), "content1")
            .file(&format!("{UUID_2}.md"), "content2")
            .build();

        let result = list_uuid_files(env.path(), "md");

        assert_eq!(result.len(), 2);
        let ids: Vec<_> = result.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&UUID_1));
        assert!(ids.contains(&UUID_2));
    }

    #[test]
    fn list_uuid_files_skips_non_uuid_filenames() {
        let env = TestEnv::new()
            .file(&format!("{UUID_1}.md"), "valid")
            .file("not-a-uuid.md", "skip")
            .file("readme.md", "skip")
            .build();

        let result = list_uuid_files(env.path(), "md");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, UUID_1);
    }

    #[test]
    fn list_uuid_files_skips_wrong_extension() {
        let env = TestEnv::new()
            .file(&format!("{UUID_1}.md"), "valid")
            .file(&format!("{UUID_1}.txt"), "skip")
            .file(&format!("{UUID_1}.json"), "skip")
            .build();

        let result = list_uuid_files(env.path(), "md");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, UUID_1);
    }

    #[test]
    fn list_uuid_files_skips_directories() {
        let env = TestEnv::new()
            .file(&format!("{UUID_1}.md"), "valid")
            .folder(UUID_2)
            .done()
            .build();

        let result = list_uuid_files(env.path(), "md");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, UUID_1);
    }
}
