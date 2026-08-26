mod error;
mod events;
mod ext;
mod path;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use notify::RecommendedWatcher;
use notify_debouncer_full::{Debouncer, NoCache};
use tauri::Manager;

pub use error::*;
pub use events::*;
pub use ext::*;

const PLUGIN_NAME: &str = "notify";

pub struct WatcherState {
    pub(crate) debouncer: Mutex<Option<Debouncer<RecommendedWatcher, NoCache>>>,
    pub(crate) own_writes: Arc<Mutex<HashMap<String, Instant>>>,
}

impl WatcherState {
    /// Constructs the same (unstarted) state `init()`'s `setup()` hook would
    /// `.manage()`, for tests that need `app.notify()` to resolve without a
    /// real webview. `init()` itself is pinned to `tauri::Wry`, so it can't
    /// be attached via `.plugin()` to a `tauri::test::mock_builder()` app
    /// (`MockRuntime`) — the same constraint `tauri-plugin-tantivy`'s test
    /// documents for its own `IndexState`. Callers `.manage()` this directly
    /// instead of going through the plugin's setup at all.
    pub fn empty() -> Self {
        Self {
            debouncer: Mutex::new(None),
            own_writes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn make_specta_builder<R: tauri::Runtime>() -> tauri_specta::Builder<R> {
    tauri_specta::Builder::<R>::new()
        .plugin_name(PLUGIN_NAME)
        .events(tauri_specta::collect_events![FileChanged,])
}

pub struct InitOptions {
    pub start_on_webview_ready: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            start_on_webview_ready: true,
        }
    }
}

pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    init_with_options(InitOptions::default())
}

pub fn init_with_options(options: InitOptions) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let specta_builder = make_specta_builder();
    let start_on_webview_ready = options.start_on_webview_ready;

    tauri::plugin::Builder::new(PLUGIN_NAME)
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app, _api| {
            specta_builder.mount_events(app);

            app.manage(WatcherState {
                debouncer: Mutex::new(None),
                own_writes: Arc::new(Mutex::new(HashMap::new())),
            });

            Ok(())
        })
        .on_webview_ready(move |webview| {
            if start_on_webview_ready && let Err(e) = webview.app_handle().notify().start() {
                tracing::error!("failed_to_start_watcher: {}", e);
            }
        })
        .on_drop(|app| {
            if let Err(e) = app.notify().stop() {
                tracing::error!("failed_to_stop_watcher: {}", e);
            }
        })
        .build()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn export_types() {
        const OUTPUT_FILE: &str = "./js/bindings.gen.ts";

        make_specta_builder::<tauri::Wry>()
            .export(
                specta_typescript::Typescript::default()
                    .formatter(specta_typescript::formatter::prettier)
                    .bigint(specta_typescript::BigIntExportBehavior::Number),
                OUTPUT_FILE,
            )
            .unwrap();

        let content = std::fs::read_to_string(OUTPUT_FILE).unwrap();
        std::fs::write(OUTPUT_FILE, format!("// @ts-nocheck\n{content}")).unwrap();
    }
}
