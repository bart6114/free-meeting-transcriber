use std::path::{Path, PathBuf};

pub const VAULT_CONFIG_FILENAME: &str = "global.json";
pub const STAGING_BUNDLE_ID: &str = "org.freemeetingtranscriber.staging";
const RELEASE_APP_FOLDER: &str = "free-meeting-transcriber";

pub fn compute_vault_config_path(base: &Path) -> PathBuf {
    base.join(VAULT_CONFIG_FILENAME)
}

pub fn compute_default_base(bundle_id: &str) -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    let app_folder = resolve_app_folder(bundle_id, cfg!(debug_assertions));
    Some(data_dir.join(app_folder))
}

/// Dev builds and the staging channel use the raw bundle id as the folder
/// name; release builds always use the `free-meeting-transcriber` folder.
fn resolve_app_folder<'a>(bundle_id: &'a str, is_debug: bool) -> &'a str {
    if is_debug || bundle_id == STAGING_BUNDLE_ID {
        return bundle_id;
    }

    RELEASE_APP_FOLDER
}

#[cfg(test)]
mod tests {
    use super::*;

    const STABLE_BUNDLE_ID: &str = "org.freemeetingtranscriber.stable";

    #[test]
    fn resolve_app_folder_uses_new_folder_for_new_stable_installs() {
        assert_eq!(
            resolve_app_folder(STABLE_BUNDLE_ID, false),
            RELEASE_APP_FOLDER
        );
    }

    #[test]
    fn resolve_app_folder_returns_bundle_id_for_staging() {
        assert_eq!(
            resolve_app_folder(STAGING_BUNDLE_ID, false),
            STAGING_BUNDLE_ID
        );
    }

    #[test]
    fn resolve_app_folder_returns_bundle_id_in_debug_builds() {
        assert_eq!(resolve_app_folder(STABLE_BUNDLE_ID, true), STABLE_BUNDLE_ID);
    }
}
