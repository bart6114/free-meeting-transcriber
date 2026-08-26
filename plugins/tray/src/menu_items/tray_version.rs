use tauri::{
    AppHandle, Result,
    menu::{MenuItem, MenuItemKind},
};

use super::MenuItemHandler;

pub struct TrayVersion;

impl TrayVersion {
    fn get_channel(identifier: &str, app_name: &str) -> &'static str {
        match identifier {
            "io.loofah.stable" => "stable",
            "io.loofah.staging" => "staging",
            "io.loofah.dev" => "dev",
            "org.freemeetingtranscriber.stable" => "stable",
            "org.freemeetingtranscriber.staging" => "staging",
            "org.freemeetingtranscriber.dev" => "dev",
            _ => match app_name {
                "Loofah" => "stable",
                "Loofah Staging" => "staging",
                "Free Meeting Transcriber" => "stable",
                "Free Meeting Transcriber Staging" => "staging",
                _ => "dev",
            },
        }
    }
}

impl MenuItemHandler for TrayVersion {
    const ID: &'static str = "hypr_tray_version";

    fn build(app: &AppHandle<tauri::Wry>) -> Result<MenuItemKind<tauri::Wry>> {
        let identifier = &app.config().identifier;
        let app_name = &app.package_info().name;
        let app_version = app.package_info().version.to_string();
        let channel = Self::get_channel(identifier, app_name);

        let text = format!("v{} ({})", app_version, channel);
        let item = MenuItem::with_id(app, Self::ID, text, false, None::<&str>)?;
        Ok(MenuItemKind::MenuItem(item))
    }

    fn handle(_app: &AppHandle<tauri::Wry>) {}
}

#[cfg(test)]
mod tests {
    use super::TrayVersion;

    #[test]
    fn gets_channel_from_identifier() {
        assert_eq!(
            TrayVersion::get_channel("io.loofah.stable", "Loofah"),
            "stable"
        );
        assert_eq!(
            TrayVersion::get_channel("io.loofah.staging", "Loofah Staging"),
            "staging"
        );
        assert_eq!(
            TrayVersion::get_channel("io.loofah.dev", "Loofah Dev"),
            "dev"
        );
    }

    #[test]
    fn falls_back_to_product_name_for_unknown_identifier() {
        assert_eq!(TrayVersion::get_channel("unknown", "Loofah"), "stable");
        assert_eq!(
            TrayVersion::get_channel("unknown", "Loofah Staging"),
            "staging"
        );
        assert_eq!(TrayVersion::get_channel("unknown", "Loofah Dev"), "dev");
    }
}
