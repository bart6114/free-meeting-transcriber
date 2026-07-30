use tauri_plugin_settings::SettingsPluginExt;

#[derive(Clone, Copy)]
pub struct AppAppearanceSettings {
    pub show_app_in_dock: bool,
    pub show_tray_icon: bool,
}

pub fn load_app_appearance_settings<R, M>(manager: &M) -> AppAppearanceSettings
where
    R: tauri::Runtime,
    M: tauri::Manager<R>,
{
    // Missing config.json (or missing keys) falls back to AppConfig defaults,
    // which are `true` for both — same as the old settings.json reader.
    let config = manager.settings().config();

    AppAppearanceSettings {
        show_app_in_dock: config.show_app_in_dock,
        show_tray_icon: config.show_tray_icon,
    }
}
