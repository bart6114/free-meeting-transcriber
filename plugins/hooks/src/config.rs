use tauri_plugin_settings::SettingsPluginExt;

use hypr_hooks::HooksConfig;

use crate::error::{Error, Result};

pub async fn load_config<R: tauri::Runtime>(app: &impl tauri::Manager<R>) -> Result<HooksConfig> {
    let Some(hooks_value) = app.settings().config().hooks else {
        return Ok(HooksConfig::empty());
    };

    HooksConfig::from_value(hooks_value).map_err(Error::from)
}
