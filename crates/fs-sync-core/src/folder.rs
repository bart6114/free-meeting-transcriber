use std::path::Path;

use crate::path::get_parent_folder_path;
use crate::session::{DirClass, classify_dir};
use crate::types::{FolderInfo, FolderSessionUpdate, ListFoldersResult};

/// Returns whether `current_path` transitively contains a session directory, so a
/// folder is listed even when its only sessions have unreadable metas.
pub fn scan_directory_recursive(
    sessions_dir: &Path,
    current_path: &str,
    result: &mut ListFoldersResult,
) -> bool {
    let full_path = if current_path.is_empty() {
        sessions_dir.to_path_buf()
    } else {
        sessions_dir.join(current_path)
    };

    let entries = match std::fs::read_dir(&full_path) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    let mut contains_sessions = false;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let entry_path = if current_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", current_path, name)
        };

        match classify_dir(&path) {
            DirClass::Session(id) => {
                contains_sessions = true;
                if let Some(id) = id {
                    result
                        .session_folder_map
                        .insert(id, current_path.to_string());
                }
            }
            DirClass::Folder => {
                if scan_directory_recursive(sessions_dir, &entry_path, result) {
                    contains_sessions = true;
                    result.folders.insert(
                        entry_path.clone(),
                        FolderInfo {
                            name,
                            parent_folder_id: get_parent_folder_path(&entry_path),
                        },
                    );
                }
            }
        }
    }

    contains_sessions
}

pub fn collect_session_updates(
    sessions_dir: &Path,
    current_path: &str,
    result: &mut Vec<FolderSessionUpdate>,
) {
    let full_path = if current_path.is_empty() {
        sessions_dir.to_path_buf()
    } else {
        sessions_dir.join(current_path)
    };

    let entries = match std::fs::read_dir(&full_path) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let entry_path = if current_path.is_empty() {
            name
        } else {
            format!("{}/{}", current_path, name)
        };

        match classify_dir(&path) {
            DirClass::Session(Some(id)) => result.push(FolderSessionUpdate {
                session_id: id,
                folder_id: get_parent_folder_path(&entry_path).unwrap_or_default(),
            }),
            // Unreadable meta: still a session directory, never traversed as a folder.
            DirClass::Session(None) => {}
            DirClass::Folder => collect_session_updates(sessions_dir, &entry_path, result),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{TestEnv, UUID_1, UUID_2};
    use assert_fs::prelude::*;
    use std::collections::HashMap;

    #[test]
    fn scan_directory_detects_sessions_with_meta() {
        let env = TestEnv::new()
            .session(UUID_1)
            .done()
            .session(UUID_2)
            .no_meta()
            .done()
            .build();

        let mut result = ListFoldersResult {
            folders: HashMap::new(),
            session_folder_map: HashMap::new(),
        };
        scan_directory_recursive(env.path(), "", &mut result);

        assert_eq!(result.session_folder_map.len(), 1);
        assert!(result.session_folder_map.contains_key(UUID_1));
        assert!(!result.session_folder_map.contains_key(UUID_2));
    }

    #[test]
    fn scan_directory_tracks_folders_with_sessions() {
        let env = TestEnv::new()
            .folder("work")
            .session(UUID_1)
            .done_folder()
            .done()
            .build();

        let mut result = ListFoldersResult {
            folders: HashMap::new(),
            session_folder_map: HashMap::new(),
        };
        scan_directory_recursive(env.path(), "", &mut result);

        assert!(result.folders.contains_key("work"));
        assert_eq!(result.folders["work"].name, "work");
    }

    #[test]
    fn scan_directory_maps_readable_sessions_by_meta_id() {
        let env = TestEnv::new()
            .session(UUID_1)
            .dir_name("2026-03-20 — Planning — 550e84")
            .done()
            .folder("work")
            .session(UUID_2)
            .dir_name("2026-04-01 — Retro — 550e84")
            .done_folder()
            .done()
            .build();

        let mut result = ListFoldersResult {
            folders: HashMap::new(),
            session_folder_map: HashMap::new(),
        };
        scan_directory_recursive(env.path(), "", &mut result);

        assert_eq!(result.session_folder_map.get(UUID_1), Some(&"".to_string()));
        assert_eq!(
            result.session_folder_map.get(UUID_2),
            Some(&"work".to_string())
        );
        assert!(
            !result
                .session_folder_map
                .contains_key("2026-03-20 — Planning — 550e84")
        );
        assert!(result.folders.contains_key("work"));
        assert!(
            !result
                .folders
                .contains_key("2026-03-20 — Planning — 550e84")
        );
    }

    #[test]
    fn scan_directory_never_descends_into_a_session_directory() {
        let env = TestEnv::new()
            .session(UUID_1)
            .dir_name("Readable notes")
            .done()
            .build();
        let inner = env.child("Readable notes/attachments/inner");
        inner.create_dir_all().unwrap();
        inner
            .child("_meta.json")
            .write_str(&crate::test_fixtures::session_meta_json(UUID_2))
            .unwrap();

        let mut result = ListFoldersResult {
            folders: HashMap::new(),
            session_folder_map: HashMap::new(),
        };
        scan_directory_recursive(env.path(), "", &mut result);

        assert!(result.session_folder_map.contains_key(UUID_1));
        assert!(!result.session_folder_map.contains_key(UUID_2));
        assert!(result.folders.is_empty());
    }

    #[test]
    fn collect_session_updates_tracks_nested_sessions() {
        let env = TestEnv::new()
            .folder("work")
            .session(UUID_1)
            .done_folder()
            .done()
            .folder("work/project")
            .session(UUID_2)
            .done_folder()
            .done()
            .build();

        let mut updates = Vec::new();
        collect_session_updates(env.path(), "work", &mut updates);
        updates.sort_by(|a, b| a.session_id.cmp(&b.session_id));

        assert_eq!(
            updates,
            vec![
                FolderSessionUpdate {
                    session_id: UUID_1.into(),
                    folder_id: "work".into(),
                },
                FolderSessionUpdate {
                    session_id: UUID_2.into(),
                    folder_id: "work/project".into(),
                }
            ]
        );
    }

    #[test]
    fn collect_session_updates_reads_full_ids_from_meta() {
        let env = TestEnv::new()
            .folder("work")
            .session(UUID_1)
            .dir_name("2026-03-20 — Planning — 550e84")
            .done_folder()
            .done()
            .build();

        let mut updates = Vec::new();
        collect_session_updates(env.path(), "work", &mut updates);

        assert_eq!(
            updates,
            vec![FolderSessionUpdate {
                session_id: UUID_1.into(),
                folder_id: "work".into(),
            }]
        );
    }
}
