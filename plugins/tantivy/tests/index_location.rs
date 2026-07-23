// Regression test for Task 11 (move the tantivy search index out of the vault).
// Confirms register_collection() resolves the index under settings().global_base()
// (app-data dir) rather than settings().vault_base() (the vault), using a $HOME
// override so nothing touches the real machine's Application Support folder.
use tauri::Manager;
use tauri_plugin_settings::SettingsPluginExt;
use tauri_plugin_tantivy::{CollectionConfig, IndexState, TantivyPluginExt};

fn build_schema() -> tantivy::schema::Schema {
    let mut builder = tantivy::schema::Schema::builder();
    builder.add_text_field("id", tantivy::schema::STRING | tantivy::schema::STORED);
    builder.build()
}

#[tokio::test]
async fn register_collection_uses_global_base_not_vault_base() {
    let fake_home = tempfile::tempdir().unwrap();
    let vault_dir = tempfile::tempdir().unwrap();

    unsafe {
        std::env::set_var("HOME", fake_home.path());
        // Force a custom vault location distinct from app-data, the same
        // knob `resolve_startup_vault_base()` honors on real startup, so a
        // regression back to vault_base() is actually observable.
        std::env::set_var("CHAR_VAULT_BASE", vault_dir.path());
    }

    // tauri_plugin_tantivy::init() is pinned to tauri::Wry (not generic over
    // R), so it can't be attached via .plugin() to a MockRuntime app. Manage
    // the same IndexState the plugin's setup() would, and drive
    // register_collection() directly through the same TantivyPluginExt path
    // the real app uses.
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_settings::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    app.manage(IndexState::default());

    let global_base = app.settings().global_base().unwrap();
    let vault_base = app.settings().vault_base().unwrap();
    assert_ne!(global_base.as_str(), vault_base.as_str());

    let config = CollectionConfig {
        name: "verify".to_string(),
        path: "search_index".to_string(),
        schema_builder: build_schema,
        schema_version: 1,
    };

    app.tantivy().register_collection(config).await.unwrap();

    let expected_index_dir = global_base.join("search_index");
    assert!(
        expected_index_dir.join("meta.json").exists(),
        "expected tantivy meta.json under global_base at {:?}",
        expected_index_dir
    );

    let stale_vault_index_dir = std::path::Path::new(vault_base.as_str()).join("search_index");
    assert!(
        !stale_vault_index_dir.exists(),
        "search_index must NOT be written under vault_base anymore: {:?}",
        stale_vault_index_dir
    );

    println!("OK: index created at {:?}", expected_index_dir);
    println!("OK: nothing written at {:?}", stale_vault_index_dir);
}
