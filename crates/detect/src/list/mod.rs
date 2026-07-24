#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn list_installed_apps() -> Vec<InstalledApp> {
    Vec::new()
}

const SELF_BUNDLE_IDS: &[&str] = &[
    "org.freemeetingtranscriber.dev",
    "org.freemeetingtranscriber.stable",
    "org.freemeetingtranscriber.staging",
];

const SELF_APP_NAMES: &[&str] = &[
    "free meeting transcriber",
    "free meeting transcriber staging",
];

const SELF_APP_PATH_SEGMENTS: &[&str] = &[
    "/free meeting transcriber.app/",
    "/free meeting transcriber staging.app/",
];

fn is_self_app(app: &InstalledApp) -> bool {
    let id = app.id.to_lowercase();
    let name = app.name.to_lowercase();

    SELF_BUNDLE_IDS.contains(&id.as_str())
        || SELF_APP_NAMES.contains(&name.as_str())
        || SELF_APP_PATH_SEGMENTS
            .iter()
            .any(|segment| id.contains(segment))
}

pub fn list_mic_using_apps() -> Result<Vec<InstalledApp>, crate::Error> {
    let apps = {
        #[cfg(target_os = "macos")]
        {
            macos::list_mic_using_apps()?
        }
        #[cfg(target_os = "linux")]
        {
            linux::list_mic_using_apps()?
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Vec::<InstalledApp>::new()
        }
    };

    Ok(apps.into_iter().filter(|app| !is_self_app(app)).collect())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> InstalledApp {
        InstalledApp {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn test_is_self_app_matches_known_bundle_ids() {
        assert!(is_self_app(&app(
            "org.freemeetingtranscriber.stable",
            "Free Meeting Transcriber"
        )));
    }

    #[test]
    fn test_is_self_app_matches_renamed_app_names() {
        assert!(is_self_app(&app("pid:41", "Free Meeting Transcriber")));
        assert!(is_self_app(&app(
            "pid:45",
            "Free Meeting Transcriber Staging"
        )));
    }

    #[test]
    fn test_is_self_app_matches_path_fallbacks() {
        assert!(is_self_app(&app(
            "/Applications/Free Meeting Transcriber.app/Contents/MacOS/free-meeting-transcriber",
            "Unknown",
        )));
    }

    #[test]
    fn test_is_self_app_does_not_match_unrelated_char_apps() {
        assert!(!is_self_app(&app(
            "com.adobe.character-animator",
            "Character Animator"
        )));
        assert!(!is_self_app(&app(
            "/Applications/Chart.app/Contents/MacOS/Chart",
            "Chart"
        )));
    }
}
