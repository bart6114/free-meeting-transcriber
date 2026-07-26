use serde::{Deserialize, Serialize};

use super::{SessionStore, StoreError, paths};

/// One summary template, file-canonical at `templates/<id>.json`. Mirrors the live columns
/// of the legacy `templates` table; `icon`/`targets`/`sections` are stored as real JSON
/// (the old `icon_json`/`targets_json`/`sections_json` columns held them stringified).
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct TemplateItem {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub pin_order: Option<i32>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub icon: serde_json::Value,
    #[serde(default)]
    pub targets: Option<serde_json::Value>,
    #[serde(default)]
    pub sections: serde_json::Value,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// What the frontend sends on a write: timestamps are managed store-side (`created_at`
/// survives for a template that already exists).
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq)]
pub struct TemplateInput {
    pub id: String,
    pub title: String,
    pub description: String,
    pub pinned: bool,
    #[serde(default)]
    pub pin_order: Option<i32>,
    #[serde(default)]
    pub category: Option<String>,
    pub icon: serde_json::Value,
    #[serde(default)]
    pub targets: Option<serde_json::Value>,
    pub sections: serde_json::Value,
}

/// Deleting a bundled default must stick across restarts even though startup re-seeds any
/// default whose file is missing, so deleted default ids are tombstoned here
/// (`templates/.deleted-defaults.json`). Re-creating the id via upsert clears the tombstone.
#[derive(Serialize, Deserialize, Debug, Default)]
struct DeletedDefaultsFile {
    #[serde(default)]
    deleted_ids: Vec<String>,
}

/// The 17 bundled defaults, transcribed from the retired
/// `20260524000000_default_templates.sql` seed migration.
const DEFAULT_TEMPLATES_JSON: &str = include_str!("default_templates.json");

#[derive(Deserialize)]
struct DefaultTemplateSeed {
    id: String,
    title: String,
    description: String,
    category: String,
    targets: serde_json::Value,
    sections: serde_json::Value,
}

/// The `icon_json` column default every seeded row carried (the icon migration postdates
/// the seed migration, so all 17 defaults had exactly this value).
fn default_icon() -> serde_json::Value {
    serde_json::json!({ "type": "icon", "value": "notebook-tabs", "color": "#9ca3af" })
}

fn default_template_seeds() -> Result<Vec<DefaultTemplateSeed>, StoreError> {
    serde_json::from_str(DEFAULT_TEMPLATES_JSON).map_err(|e| {
        StoreError::Serialize(format!("failed to parse bundled default templates: {e}"))
    })
}

fn now_iso() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// The id becomes the file name under `templates/`, so it must be a single safe path
/// segment. Dot-prefixed names are reserved (`.deleted-defaults.json`).
fn validate_template_id(id: &str) -> Result<(), StoreError> {
    if id.is_empty()
        || id.starts_with('.')
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
    {
        return Err(StoreError::Io(format!("invalid template id: {id:?}")));
    }
    Ok(())
}

fn same_content(existing: &TemplateItem, next: &TemplateItem) -> bool {
    existing.title == next.title
        && existing.description == next.description
        && existing.pinned == next.pinned
        && existing.pin_order == next.pin_order
        && existing.category == next.category
        && existing.icon == next.icon
        && existing.targets == next.targets
        && existing.sections == next.sections
}

impl SessionStore {
    /// Every parseable `templates/*.json`, sorted by id ascending (parity with the old
    /// `ORDER BY id` read). An unparseable file is logged and skipped rather than failing
    /// the whole list -- templates are user-editable files.
    pub async fn list_templates(&self) -> Result<Vec<TemplateItem>, StoreError> {
        let vault_base = self.vault_base.clone();
        let mut templates =
            tokio::task::spawn_blocking(move || -> Result<Vec<TemplateItem>, StoreError> {
                let entries = match std::fs::read_dir(vault_base.join(paths::templates_root())) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                    Err(e) => {
                        return Err(StoreError::Io(format!("failed to read templates dir: {e}")));
                    }
                };
                let mut templates = Vec::new();
                for entry in entries {
                    let entry = entry
                        .map_err(|e| StoreError::Io(format!("failed to read dir entry: {e}")))?;
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if name.starts_with('.') {
                        continue;
                    }
                    let Some(id) = name.strip_suffix(".json") else {
                        continue;
                    };
                    match read_template_file(&entry.path(), id) {
                        Ok(template) => templates.push(template),
                        Err(e) => {
                            tracing::warn!(template_id = id, "skipping unparseable template: {e}");
                        }
                    }
                }
                Ok(templates)
            })
            .await
            .map_err(|e| StoreError::Io(format!("task join error: {e}")))??;

        templates.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(templates)
    }

    pub async fn get_template(&self, id: &str) -> Result<Option<TemplateItem>, StoreError> {
        validate_template_id(id)?;
        let path = self.vault_base.join(paths::template_path(id));
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<TemplateItem>, StoreError> {
            if !path.is_file() {
                return Ok(None);
            }
            read_template_file(&path, &id).map(Some)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// Create-or-replace. `created_at` survives for an existing template; a
    /// content-identical write is a no-op that never touches the file (so the write journal
    /// only records real changes). Upserting a tombstoned default id clears its tombstone.
    pub async fn upsert_template(&self, input: TemplateInput) -> Result<(), StoreError> {
        validate_template_id(&input.id)?;
        let existing = self.get_template(&input.id).await?;
        let now = now_iso();

        let mut item = TemplateItem {
            id: input.id,
            title: input.title,
            description: input.description,
            pinned: input.pinned,
            pin_order: input.pin_order,
            category: input.category,
            icon: input.icon,
            targets: input.targets,
            sections: input.sections,
            created_at: existing
                .as_ref()
                .map(|e| e.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        if let Some(existing) = &existing {
            if same_content(existing, &item) {
                item.updated_at = existing.updated_at.clone();
            }
        }

        self.clear_deleted_default(&item.id).await?;
        if existing.as_ref() == Some(&item) {
            return Ok(());
        }
        self.write_template_file(&item).await
    }

    /// File to `.trash/<date>/templates/<id>.json` (hand-recoverable, never synced).
    /// Idempotent. Deleting a bundled default id also tombstones it so the startup
    /// seed-on-missing pass doesn't resurrect it.
    pub async fn delete_template(&self, id: &str) -> Result<(), StoreError> {
        validate_template_id(id)?;

        let vault_base = self.vault_base.clone();
        let relative = paths::template_path(id);
        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            let abs = vault_base.join(relative);
            hypr_fs_sync_core::export::move_to_trash(&vault_base, &abs)
                .map_err(|e| StoreError::Io(format!("failed to move template to trash: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))??;

        self.index_remove_template(id);
        self.notify_index_changed(super::IndexEntity::Templates, vec![id.to_string()]);

        let is_default = default_template_seeds()?.iter().any(|seed| seed.id == id);
        if is_default {
            let mut file = self.read_deleted_defaults().await?;
            if !file.deleted_ids.iter().any(|deleted| deleted == id) {
                file.deleted_ids.push(id.to_string());
                self.write_deleted_defaults(&file).await?;
            }
        }
        Ok(())
    }

    /// Seed every bundled default whose file is missing and whose id isn't tombstoned.
    /// Runs at store startup; this replaces the retired SQL-era seed migration and
    /// `repair_missing_core_tables` re-seeding guarantee. Returns how many were written.
    pub async fn seed_default_templates(&self) -> Result<usize, StoreError> {
        let deleted = self.read_deleted_defaults().await?;
        let mut seeded = 0;

        for seed in default_template_seeds()? {
            if deleted.deleted_ids.contains(&seed.id) {
                continue;
            }
            let path = self.vault_base.join(paths::template_path(&seed.id));
            if path.is_file() {
                continue;
            }
            let now = now_iso();
            self.write_template_file(&TemplateItem {
                id: seed.id,
                title: seed.title,
                description: seed.description,
                pinned: false,
                pin_order: None,
                category: Some(seed.category),
                icon: default_icon(),
                targets: Some(seed.targets),
                sections: seed.sections,
                created_at: now.clone(),
                updated_at: now,
            })
            .await?;
            seeded += 1;
        }
        Ok(seeded)
    }

    async fn write_template_file(&self, item: &TemplateItem) -> Result<(), StoreError> {
        let bytes =
            serde_json::to_vec_pretty(item).map_err(|e| StoreError::Serialize(e.to_string()))?;
        self.write_file(paths::template_path(&item.id), bytes)
            .await?;

        self.index_upsert_template(item);
        self.notify_index_changed(super::IndexEntity::Templates, vec![item.id.clone()]);
        Ok(())
    }

    async fn read_deleted_defaults(&self) -> Result<DeletedDefaultsFile, StoreError> {
        let path = self
            .vault_base
            .join(paths::deleted_default_templates_path());
        tokio::task::spawn_blocking(move || -> Result<DeletedDefaultsFile, StoreError> {
            let raw = match std::fs::read(&path) {
                Ok(raw) => raw,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(DeletedDefaultsFile::default());
                }
                Err(e) => {
                    return Err(StoreError::Io(format!(
                        "failed to read deleted-defaults file: {e}"
                    )));
                }
            };
            serde_json::from_slice(&raw).map_err(|e| {
                StoreError::Serialize(format!("failed to parse deleted-defaults file: {e}"))
            })
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    async fn write_deleted_defaults(&self, file: &DeletedDefaultsFile) -> Result<(), StoreError> {
        let bytes =
            serde_json::to_vec_pretty(file).map_err(|e| StoreError::Serialize(e.to_string()))?;
        self.write_file(paths::deleted_default_templates_path(), bytes)
            .await
    }

    async fn clear_deleted_default(&self, id: &str) -> Result<(), StoreError> {
        let mut file = self.read_deleted_defaults().await?;
        let before = file.deleted_ids.len();
        file.deleted_ids.retain(|deleted| deleted != id);
        if file.deleted_ids.len() != before {
            self.write_deleted_defaults(&file).await?;
        }
        Ok(())
    }
}

/// The file name is the authoritative id (same rule as `sessions/<id>/`): a file whose
/// embedded `id` field disagrees with its name is read under its name.
fn read_template_file(path: &std::path::Path, id: &str) -> Result<TemplateItem, StoreError> {
    let raw = std::fs::read(path)
        .map_err(|e| StoreError::Io(format!("failed to read template file: {e}")))?;
    let mut item: TemplateItem = serde_json::from_slice(&raw)
        .map_err(|e| StoreError::Serialize(format!("failed to parse template file: {e}")))?;
    item.id = id.to_string();
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
        (store, temp)
    }

    fn input(id: &str, title: &str) -> TemplateInput {
        TemplateInput {
            id: id.to_string(),
            title: title.to_string(),
            description: "desc".to_string(),
            pinned: false,
            pin_order: None,
            category: Some("Engineering".to_string()),
            icon: serde_json::json!({ "type": "emoji", "value": "🎯" }),
            targets: Some(serde_json::json!(["Tech Lead"])),
            sections: serde_json::json!([{ "title": "Notes", "description": "Capture" }]),
        }
    }

    #[tokio::test]
    async fn seed_writes_all_defaults_and_is_idempotent() {
        let (store, vault) = test_store().await;

        assert_eq!(store.seed_default_templates().await.unwrap(), 17);
        assert!(
            vault
                .path()
                .join("templates/default-daily-standup.json")
                .is_file()
        );
        let listed = store.list_templates().await.unwrap();
        assert_eq!(listed.len(), 17);
        assert!(listed.iter().all(|t| !t.pinned));
        assert!(
            listed
                .iter()
                .all(|t| t.icon["value"] == serde_json::json!("notebook-tabs"))
        );

        // second pass: nothing is missing, nothing is rewritten
        assert_eq!(store.seed_default_templates().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn seed_restores_only_missing_defaults_and_leaves_edits_alone() {
        let (store, _vault) = test_store().await;
        store.seed_default_templates().await.unwrap();

        // edit one default, hand-remove another (external delete, no tombstone)
        let mut edited = input("default-daily-standup", "My Standup");
        edited.pinned = true;
        store.upsert_template(edited).await.unwrap();
        std::fs::remove_file(_vault.path().join("templates/default-board-meeting.json")).unwrap();

        assert_eq!(store.seed_default_templates().await.unwrap(), 1);
        let standup = store
            .get_template("default-daily-standup")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(standup.title, "My Standup", "seed must not clobber edits");
        assert!(
            store
                .get_template("default-board-meeting")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn deleting_a_default_sticks_across_reseed() {
        let (store, vault) = test_store().await;
        store.seed_default_templates().await.unwrap();

        store
            .delete_template("default-daily-standup")
            .await
            .unwrap();
        assert!(
            !vault
                .path()
                .join("templates/default-daily-standup.json")
                .exists()
        );

        assert_eq!(store.seed_default_templates().await.unwrap(), 0);
        assert!(
            store
                .get_template("default-daily-standup")
                .await
                .unwrap()
                .is_none(),
            "a deleted default must not be resurrected by the next seed pass"
        );
    }

    #[tokio::test]
    async fn upserting_a_deleted_default_clears_the_tombstone() {
        let (store, _vault) = test_store().await;
        store.seed_default_templates().await.unwrap();
        store
            .delete_template("default-daily-standup")
            .await
            .unwrap();

        store
            .upsert_template(input("default-daily-standup", "Standup Again"))
            .await
            .unwrap();

        // still present after another seed pass, with the user's content
        store.seed_default_templates().await.unwrap();
        let template = store
            .get_template("default-daily-standup")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(template.title, "Standup Again");
    }

    #[tokio::test]
    async fn upsert_get_list_round_trip_sorted_by_id() {
        let (store, vault) = test_store().await;
        store.upsert_template(input("t-b", "Second")).await.unwrap();
        store.upsert_template(input("t-a", "First")).await.unwrap();

        assert!(vault.path().join("templates/t-a.json").is_file());
        let listed = store.list_templates().await.unwrap();
        assert_eq!(
            listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t-a", "t-b"],
            "list must sort by id"
        );
        let got = store.get_template("t-a").await.unwrap().unwrap();
        assert_eq!(got.title, "First");
        assert_eq!(got.category.as_deref(), Some("Engineering"));
        assert_eq!(got.targets, Some(serde_json::json!(["Tech Lead"])));
        assert!(got.sections.is_array());
        assert!(!got.created_at.is_empty());
        assert!(store.journal_matches_current_file("templates/t-a.json"));
    }

    #[tokio::test]
    async fn upsert_preserves_created_at_and_skips_identical_writes() {
        let (store, vault) = test_store().await;
        store.upsert_template(input("t-1", "Title")).await.unwrap();
        let before = store.get_template("t-1").await.unwrap().unwrap();
        let path = vault.path().join("templates/t-1.json");
        let bytes_before = std::fs::read(&path).unwrap();

        // identical content: file bytes must not change (updated_at included)
        store.upsert_template(input("t-1", "Title")).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes_before);

        store.upsert_template(input("t-1", "Edited")).await.unwrap();
        let after = store.get_template("t-1").await.unwrap().unwrap();
        assert_eq!(after.title, "Edited");
        assert_eq!(
            after.created_at, before.created_at,
            "created_at must survive an update"
        );
    }

    #[tokio::test]
    async fn delete_moves_to_trash_and_is_idempotent() {
        let (store, vault) = test_store().await;
        store.upsert_template(input("t-1", "Title")).await.unwrap();

        store.delete_template("t-1").await.unwrap();
        assert!(!vault.path().join("templates/t-1.json").exists());
        assert!(store.get_template("t-1").await.unwrap().is_none());
        // user templates never gain a tombstone
        assert!(
            !vault
                .path()
                .join("templates/.deleted-defaults.json")
                .exists()
        );

        store.delete_template("t-1").await.unwrap();
    }

    #[tokio::test]
    async fn list_skips_unparseable_and_dot_files() {
        let (store, vault) = test_store().await;
        store.upsert_template(input("t-1", "Title")).await.unwrap();
        std::fs::write(vault.path().join("templates/broken.json"), b"{not json").unwrap();
        std::fs::write(vault.path().join("templates/.hidden.json"), b"{}").unwrap();
        std::fs::write(vault.path().join("templates/readme.txt"), b"hi").unwrap();

        let listed = store.list_templates().await.unwrap();
        assert_eq!(
            listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t-1"]
        );
    }

    #[tokio::test]
    async fn path_escaping_ids_are_rejected() {
        let (store, _vault) = test_store().await;
        for id in ["", "../evil", "a/b", ".deleted-defaults"] {
            assert!(store.get_template(id).await.is_err(), "{id:?}");
            assert!(store.delete_template(id).await.is_err(), "{id:?}");
        }
    }

    #[tokio::test]
    async fn bundled_defaults_asset_parses_with_17_unique_ids() {
        let seeds = default_template_seeds().unwrap();
        assert_eq!(seeds.len(), 17);
        let ids: std::collections::HashSet<&str> = seeds.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), 17);
        assert!(seeds.iter().all(|s| s.id.starts_with("default-")));
        assert!(seeds.iter().all(|s| s.sections.is_array()));
        assert!(seeds.iter().all(|s| s.targets.is_array()));
    }
}
