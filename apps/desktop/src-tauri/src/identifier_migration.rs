const WINDOW_STATE_FILENAME: &str = ".window-state.json";

fn legacy_identifier(identifier: &str) -> Option<&'static str> {
    match identifier {
        "io.loofah.dev" => Some("org.freemeetingtranscriber.dev"),
        "io.loofah.staging" => Some("org.freemeetingtranscriber.staging"),
        "io.loofah.stable" => Some("org.freemeetingtranscriber.stable"),
        _ => None,
    }
}

pub fn plugin<R: tauri::Runtime>(identifier: &str) -> tauri::plugin::TauriPlugin<R> {
    let legacy_identifier = legacy_identifier(identifier);
    tauri::plugin::Builder::new("identifier-migration")
        .setup(move |app, _| {
            let Some(legacy_identifier) = legacy_identifier else {
                return Ok(());
            };
            let Some(config_dir) = dirs::config_dir() else {
                return Ok(());
            };
            if let Err(error) = migrate_file(
                &config_dir.join(legacy_identifier).join(WINDOW_STATE_FILENAME),
                &config_dir
                    .join(&app.config().identifier)
                    .join(WINDOW_STATE_FILENAME),
            ) {
                tracing::warn!(%error, "failed to migrate window state for the new bundle identifier");
            }
            if let Some((legacy_webview_dir, current_webview_dir)) =
                webview_directories(legacy_identifier, &app.config().identifier)
                && let Err(error) = migrate_directory(&legacy_webview_dir, &current_webview_dir)
            {
                tracing::warn!(%error, "failed to migrate webview state for the new bundle identifier");
            }
            Ok(())
        })
        .build()
}

fn migrate_file(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    if target.exists() || !source.is_file() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(source, target)
}

#[cfg(target_os = "macos")]
fn webview_directories(
    legacy_identifier: &str,
    identifier: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let root = dirs::home_dir()?.join("Library/WebKit");
    Some((root.join(legacy_identifier), root.join(identifier)))
}

#[cfg(any(target_os = "linux", windows))]
fn webview_directories(
    legacy_identifier: &str,
    identifier: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let root = dirs::data_local_dir()?;
    Some((root.join(legacy_identifier), root.join(identifier)))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn webview_directories(
    _legacy_identifier: &str,
    _identifier: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    None
}

fn migrate_directory(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    if target.exists() || !source.is_dir() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_new_bundle_identifier_to_the_previous_one() {
        assert_eq!(
            legacy_identifier("io.loofah.dev"),
            Some("org.freemeetingtranscriber.dev")
        );
        assert_eq!(
            legacy_identifier("io.loofah.staging"),
            Some("org.freemeetingtranscriber.staging")
        );
        assert_eq!(
            legacy_identifier("io.loofah.stable"),
            Some("org.freemeetingtranscriber.stable")
        );
    }

    #[test]
    fn migrates_window_state_without_overwriting_new_state() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("old/.window-state.json");
        let target = dir.path().join("new/.window-state.json");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "old").unwrap();

        migrate_file(&source, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");

        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "stale").unwrap();
        std::fs::write(&target, "new").unwrap();
        migrate_file(&source, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn migrates_webview_state_without_overwriting_a_new_profile() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("old");
        let target = dir.path().join("new");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("state"), "old").unwrap();

        migrate_directory(&source, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("state")).unwrap(),
            "old"
        );

        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("state"), "stale").unwrap();
        std::fs::write(target.join("state"), "new").unwrap();
        migrate_directory(&source, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("state")).unwrap(),
            "new"
        );
    }
}
