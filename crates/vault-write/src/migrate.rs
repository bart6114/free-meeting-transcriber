//! One-way migration of legacy UUID-named session directories to the readable
//! `YYYY-MM-DD — title — shortid` form. Idempotent: the physical layout is the
//! truth (no "migration complete" marker), so a re-run -- or a legacy session
//! copied into the vault later -- simply migrates whatever still matches.

use std::collections::HashSet;
use std::path::PathBuf;

use super::{SessionStore, StoreError, layout_name};

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct MigrationReport {
    /// `(from, to)` vault-relative directory pairs actually renamed.
    pub renamed: Vec<(String, String)>,
    /// Directories left untouched on purpose: custom/readable names, corrupt or
    /// duplicated identities, or no free collision target.
    pub skipped: Vec<String>,
    /// Rename attempts that failed with an OS error; retried on the next run.
    pub failed: Vec<String>,
}

impl SessionStore {
    /// Rename every session directory whose basename is exactly its full
    /// `_meta.json.id` to the readable form, preserving its parent personal folder
    /// and never touching directory contents. The complete source-to-target set is
    /// preflighted before any rename so intra-batch name collisions widen their
    /// suffix instead of racing for the same target. Intended to run at desktop
    /// startup before the first index rebuild and before the vault watcher starts.
    pub async fn migrate_legacy_session_directories(&self) -> Result<MigrationReport, StoreError> {
        let vault_base = self.vault_base.clone();
        let discovery =
            tokio::task::spawn_blocking(move || hypr_vault_read::discover_sessions(&vault_base))
                .await
                .map_err(|e| StoreError::Io(format!("task join error: {e}")))??;

        let mut report = MigrationReport::default();
        for error in &discovery.errors {
            report.skipped.push(error.to_string());
        }

        // Preflight: choose every target first. Claimed targets are tracked so two
        // same-day same-title sessions in one batch cannot pick the same name.
        let guard = self.lock_writes().await;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut claimed: HashSet<PathBuf> = HashSet::new();
        let mut renames: Vec<(String, PathBuf, PathBuf)> = Vec::new();
        for (location, meta) in &discovery.sessions {
            let Some(basename) = location.relative_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Only exact `basename == full id` directories migrate; custom or
            // already-readable names are the user's (or this migration's) choice.
            if basename != meta.id {
                continue;
            }
            let Some(parent) = location.relative_dir.parent() else {
                continue;
            };
            let (date, _) =
                layout_name::session_date(meta.started_at.as_deref(), &meta.created_at, &today);
            let vault_base = self.vault_base.clone();
            let target = layout_name::session_dir_candidates(parent, &date, &meta.title, &meta.id)
                .into_iter()
                .find(|candidate| {
                    !claimed.contains(candidate) && !vault_base.join(candidate).exists()
                });
            match target {
                Some(target) => {
                    claimed.insert(target.clone());
                    renames.push((meta.id.clone(), location.relative_dir.clone(), target));
                }
                None => report.skipped.push(format!(
                    "{}: no collision-free readable name",
                    location.relative_dir.display()
                )),
            }
        }

        for (id, from, to) in renames {
            match self
                .rename_session_dir_locked(&guard, &id, &from, &to)
                .await
            {
                Ok(()) => report
                    .renamed
                    .push((from.display().to_string(), to.display().to_string())),
                Err(error) => report.failed.push(format!("{}: {error}", from.display())),
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::SessionStore;

    const UUID_A: &str = "550e8400-e29b-41d4-a716-446655440000";
    const UUID_B: &str = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
    const UUID_C: &str = "6ba7b811-9dad-11d1-80b4-00c04fd430c8";

    fn seed(vault: &Path, relative_dir: &str, id: &str, title: &str, started_at: &str) {
        let dir = vault.join(relative_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_meta.json"),
            serde_json::json!({
                "id": id,
                "title": title,
                "started_at": started_at,
                "ended_at": null,
                "created_at": "2026-03-01T00:00:00Z",
                "tags": [],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("_memo.md"), format!("note {id}")).unwrap();
    }

    #[tokio::test]
    async fn migration_renames_uuid_dirs_preserves_folders_and_is_idempotent() {
        let vault = tempfile::tempdir().unwrap();
        seed(
            vault.path(),
            &format!("sessions/{UUID_A}"),
            UUID_A,
            "Planning",
            "2026-03-20",
        );
        seed(
            vault.path(),
            &format!("sessions/Work/{UUID_B}"),
            UUID_B,
            "Retro",
            "2026-03-21",
        );
        seed(
            vault.path(),
            "sessions/My custom name",
            UUID_C,
            "Custom",
            "2026-03-22",
        );

        let store = SessionStore::new(vault.path().to_path_buf());
        let report = store.migrate_legacy_session_directories().await.unwrap();

        assert_eq!(report.renamed.len(), 2);
        assert!(report.failed.is_empty());
        let planning = vault.path().join("sessions/2026-03-20 — Planning — 550e84");
        assert!(planning.is_dir(), "root uuid dir must gain a readable name");
        assert_eq!(
            std::fs::read_to_string(planning.join("_memo.md")).unwrap(),
            format!("note {UUID_A}"),
            "contents move untouched"
        );
        assert!(
            vault
                .path()
                .join("sessions/Work/2026-03-21 — Retro — 6ba7b8")
                .is_dir(),
            "the personal parent folder is preserved, never flattened"
        );
        assert!(
            vault.path().join("sessions/My custom name").is_dir(),
            "custom names are left alone"
        );

        // Idempotent: a second run over the migrated vault renames nothing.
        let again = SessionStore::new(vault.path().to_path_buf())
            .migrate_legacy_session_directories()
            .await
            .unwrap();
        assert!(again.renamed.is_empty(), "{:?}", again.renamed);

        // Logical ids are unchanged and resolvable through the store.
        for id in [UUID_A, UUID_B, UUID_C] {
            assert_eq!(store.read_meta(id).await.unwrap().unwrap().id, id);
        }
    }

    #[tokio::test]
    async fn migration_skips_corrupt_and_duplicate_identities_and_mismatched_basenames() {
        let vault = tempfile::tempdir().unwrap();
        seed(
            vault.path(),
            &format!("sessions/{UUID_A}"),
            UUID_A,
            "Healthy",
            "2026-03-20",
        );
        // Basename doesn't equal the metadata id: not migratable (identity rule).
        seed(
            vault.path(),
            &format!("sessions/{UUID_B}"),
            UUID_C,
            "Mismatch",
            "2026-03-20",
        );
        let corrupt = vault.path().join("sessions/corrupt");
        std::fs::create_dir_all(&corrupt).unwrap();
        std::fs::write(corrupt.join("_meta.json"), "{ invalid").unwrap();

        let store = SessionStore::new(vault.path().to_path_buf());
        let report = store.migrate_legacy_session_directories().await.unwrap();

        assert_eq!(report.renamed.len(), 1);
        assert!(report.renamed[0].0.ends_with(UUID_A));
        assert!(
            vault.path().join(format!("sessions/{UUID_B}")).is_dir(),
            "a mismatched basename is not migrated"
        );
        assert!(corrupt.is_dir(), "corrupt directories are never touched");
        assert!(
            report.skipped.iter().any(|s| s.contains("corrupt")),
            "{:?}",
            report.skipped
        );
    }

    #[tokio::test]
    async fn migration_preflight_widens_suffixes_for_intra_batch_collisions() {
        let vault = tempfile::tempdir().unwrap();
        // Two sessions, same title and same date: their 6-char-suffix names differ
        // only by suffix, but two sessions sharing the first 6 hex chars would
        // collide -- craft ids that do.
        let id_1 = "aaaaaaaa-0000-0000-0000-000000000001";
        let id_2 = "aaaaaaaa-0000-0000-0000-000000000002";
        seed(
            vault.path(),
            &format!("sessions/{id_1}"),
            id_1,
            "Standup",
            "2026-03-20",
        );
        seed(
            vault.path(),
            &format!("sessions/{id_2}"),
            id_2,
            "Standup",
            "2026-03-20",
        );

        let store = SessionStore::new(vault.path().to_path_buf());
        let report = store.migrate_legacy_session_directories().await.unwrap();

        assert_eq!(report.renamed.len(), 2, "{report:?}");
        assert!(report.failed.is_empty());
        let names: Vec<String> = std::fs::read_dir(vault.path().join("sessions"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names.iter().collect::<std::collections::HashSet<_>>().len(),
            2,
            "both sessions keep distinct directories: {names:?}"
        );
    }
}
