use std::path::{Path, PathBuf};

pub const VAULT_CONFIG_FILENAME: &str = "global.json";
pub const STAGING_BUNDLE_ID: &str = "org.freemeetingtranscriber.staging";
const RELEASE_APP_FOLDER: &str = "free-meeting-transcriber";
const LEGACY_RELEASE_APP_FOLDERS: [&str; 2] = ["anarlog", "hyprnote"];

pub fn compute_vault_config_path(base: &Path) -> PathBuf {
    base.join(VAULT_CONFIG_FILENAME)
}

pub fn compute_default_base(bundle_id: &str) -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    let app_folder = resolve_app_folder(&data_dir, bundle_id, cfg!(debug_assertions));
    Some(data_dir.join(app_folder))
}

/// Walks the release app-folder ladder: the new `free-meeting-transcriber`
/// folder wins if it exists or if none of the legacy folders have data;
/// otherwise the first legacy folder (in order: `anarlog`, then `hyprnote`)
/// that has data is used, so we never orphan an existing install across a
/// rebrand. Dev builds and the staging channel always use the raw bundle id
/// as the folder name (unchanged from prior behavior).
fn resolve_app_folder<'a>(data_dir: &Path, bundle_id: &'a str, is_debug: bool) -> &'a str {
    if is_debug || bundle_id == STAGING_BUNDLE_ID {
        return bundle_id;
    }

    if has_app_data(&data_dir.join(RELEASE_APP_FOLDER)) {
        return RELEASE_APP_FOLDER;
    }

    for legacy_folder in LEGACY_RELEASE_APP_FOLDERS {
        if has_app_data(&data_dir.join(legacy_folder)) {
            return legacy_folder;
        }
    }

    RELEASE_APP_FOLDER
}

fn has_app_data(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or_else(|_| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const STABLE_BUNDLE_ID: &str = "org.freemeetingtranscriber.stable";

    #[test]
    fn resolve_app_folder_uses_new_folder_for_new_stable_installs() {
        let temp = tempdir().unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), STABLE_BUNDLE_ID, false),
            RELEASE_APP_FOLDER
        );
    }

    #[test]
    fn resolve_app_folder_finds_anarlog_when_only_that_legacy_folder_has_data() {
        let temp = tempdir().unwrap();
        let legacy_base = temp.path().join(LEGACY_RELEASE_APP_FOLDERS[0]);
        std::fs::create_dir_all(&legacy_base).unwrap();
        std::fs::write(legacy_base.join("store.json"), "{}").unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), STABLE_BUNDLE_ID, false),
            LEGACY_RELEASE_APP_FOLDERS[0]
        );
    }

    #[test]
    fn resolve_app_folder_finds_hyprnote_when_only_that_legacy_folder_has_data() {
        let temp = tempdir().unwrap();
        let legacy_base = temp.path().join(LEGACY_RELEASE_APP_FOLDERS[1]);
        std::fs::create_dir_all(&legacy_base).unwrap();
        std::fs::write(legacy_base.join("app.db"), "").unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), STABLE_BUNDLE_ID, false),
            LEGACY_RELEASE_APP_FOLDERS[1]
        );
    }

    #[test]
    fn resolve_app_folder_prefers_anarlog_over_hyprnote_when_both_legacy_folders_have_data() {
        let temp = tempdir().unwrap();
        let anarlog_base = temp.path().join(LEGACY_RELEASE_APP_FOLDERS[0]);
        let hyprnote_base = temp.path().join(LEGACY_RELEASE_APP_FOLDERS[1]);
        std::fs::create_dir_all(&anarlog_base).unwrap();
        std::fs::create_dir_all(&hyprnote_base).unwrap();
        std::fs::write(anarlog_base.join("store.json"), "{}").unwrap();
        std::fs::write(hyprnote_base.join("app.db"), "").unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), STABLE_BUNDLE_ID, false),
            LEGACY_RELEASE_APP_FOLDERS[0]
        );
    }

    #[test]
    fn resolve_app_folder_prefers_new_folder_when_it_has_data() {
        let temp = tempdir().unwrap();
        let legacy_base = temp.path().join(LEGACY_RELEASE_APP_FOLDERS[0]);
        let new_base = temp.path().join(RELEASE_APP_FOLDER);
        std::fs::create_dir_all(&legacy_base).unwrap();
        std::fs::create_dir_all(&new_base).unwrap();
        std::fs::write(legacy_base.join("store.json"), "{}").unwrap();
        std::fs::write(new_base.join("app.db"), "").unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), STABLE_BUNDLE_ID, false),
            RELEASE_APP_FOLDER
        );
    }

    #[test]
    fn resolve_app_folder_ignores_empty_legacy_folders() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(LEGACY_RELEASE_APP_FOLDERS[0])).unwrap();
        std::fs::create_dir_all(temp.path().join(LEGACY_RELEASE_APP_FOLDERS[1])).unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), STABLE_BUNDLE_ID, false),
            RELEASE_APP_FOLDER
        );
    }

    #[test]
    fn resolve_app_folder_uses_new_folder_for_other_release_bundle_ids() {
        let temp = tempdir().unwrap();

        assert_eq!(
            resolve_app_folder(temp.path(), "com.hyprnote.Hyprnote", false),
            RELEASE_APP_FOLDER
        );
    }

    #[test]
    fn resolve_app_folder_returns_bundle_id_for_staging() {
        assert_eq!(
            resolve_app_folder(Path::new("/tmp"), STAGING_BUNDLE_ID, false),
            STAGING_BUNDLE_ID
        );
    }

    #[test]
    fn resolve_app_folder_returns_bundle_id_in_debug_builds() {
        assert_eq!(
            resolve_app_folder(Path::new("/tmp"), STABLE_BUNDLE_ID, true),
            STABLE_BUNDLE_ID
        );
    }
}
