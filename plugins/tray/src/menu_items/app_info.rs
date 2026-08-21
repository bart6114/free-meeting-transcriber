use tauri::{
    AppHandle, Result,
    menu::{MenuItem, MenuItemKind},
};

use super::MenuItemHandler;

pub struct AppInfo;

impl MenuItemHandler for AppInfo {
    const ID: &'static str = "hypr_app_info";

    fn build(app: &AppHandle<tauri::Wry>) -> Result<MenuItemKind<tauri::Wry>> {
        let title = format!("About {}", app.package_info().name);
        let item = MenuItem::with_id(app, Self::ID, title, true, None::<&str>)?;
        Ok(MenuItemKind::MenuItem(item))
    }

    fn handle(app: &AppHandle<tauri::Wry>) {
        use tauri_plugin_windows::{AppWindow, Navigate, WindowsPluginExt};
        if app.windows().show(AppWindow::Main).is_ok() {
            let _ = app.windows().emit_navigate(
                AppWindow::Main,
                Navigate {
                    path: "/app/about".to_string(),
                    search: None,
                },
            );
        }
    }
}
