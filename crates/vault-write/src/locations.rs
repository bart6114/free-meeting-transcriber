//! The store's authoritative session-location catalog: logical id -> vault-relative
//! physical directory. Directory basenames are presentation only (`_meta.json.id` is
//! identity, see `hypr_vault_read::layout`), so every session-scoped read and write
//! resolves through here instead of assuming `sessions/<id>`.

use std::path::{Path, PathBuf};

use super::{SessionStore, StoreError, WriteGuard, validate_session_id};

/// One recent `delete_session`, retained in memory only to back the current process's
/// undo toast: the exact trash path `move_to_trash` returned plus the original
/// vault-relative directory to restore to. Neither path is ever rebuilt from the
/// logical id -- a readable directory name would not survive that round trip.
#[derive(Clone, Debug)]
pub(crate) struct DeletedSession {
    pub original_relative_dir: PathBuf,
    pub trash_path: PathBuf,
}

pub(crate) fn legacy_session_dir(id: &str) -> PathBuf {
    hypr_vault_read::paths::sessions_root().join(id)
}

impl SessionStore {
    /// Vault-relative physical directory for `id`: the catalog answer when warm,
    /// otherwise a targeted discovery (which caches a hit). An id no directory claims
    /// resolves to the legacy `sessions/<id>` path -- that is where a new session is
    /// created and where absent-artifact reads correctly find nothing -- and is
    /// deliberately not cached, since the answer changes the moment the session is
    /// created. An id claimed by more than one directory is an error: writes must
    /// block on a duplicate, never pick a winner.
    pub async fn session_dir(&self, id: &str) -> Result<PathBuf, StoreError> {
        validate_session_id(id)?;
        self.resolve_session_dir(id).await
    }

    /// Cache-only synchronous lookup for callers that must stay O(1) in session
    /// count (the fs-sync plugin's per-artifact resolution): a validated catalog
    /// hit or `Ok(None)`, never a discovery scan. A hit is verified against the
    /// filesystem with at most one metadata read before it is trusted, because an
    /// external rename can leave the catalog stale until the watcher/focus rebuild:
    ///
    /// - directory or `_meta.json` gone → `None` (the caller's fallback re-resolves);
    /// - parseable meta claiming a different id → `None` (stale entry);
    /// - parseable matching meta → the cataloged vault-relative directory;
    /// - unreadable/corrupt meta → still the cataloged directory, matching the
    ///   corruption tolerance of every other artifact-access path.
    ///
    /// Duplicate-claimed ids error exactly like the async resolver: writes and
    /// reads must block on a duplicate, never pick a winner.
    pub fn session_dir_cached(&self, id: &str) -> Result<Option<PathBuf>, StoreError> {
        validate_session_id(id)?;
        self.ensure_not_duplicated(id)?;
        let Some(dir) = self.locations.read().unwrap().get(id).cloned() else {
            return Ok(None);
        };
        match hypr_vault_read::classify_session_dir(&self.vault_base.join(&dir)) {
            hypr_vault_read::SessionDirKind::Session(meta)
                if hypr_vault_read::layout::eq_nfc(&meta.id, id) =>
            {
                Ok(Some(dir))
            }
            hypr_vault_read::SessionDirKind::Session(_) => Ok(None),
            hypr_vault_read::SessionDirKind::Corrupt(_) => Ok(Some(dir)),
            // Covers both a vanished directory and a vanished `_meta.json`.
            hypr_vault_read::SessionDirKind::Folder => Ok(None),
        }
    }

    /// `session_dir` for write paths: the guard is a proof token that the store write
    /// lock is held, so the resolved location cannot be renamed out from under the
    /// write that follows (renames also run under the write lock).
    pub(crate) async fn session_dir_locked(
        &self,
        _guard: &WriteGuard<'_>,
        id: &str,
    ) -> Result<PathBuf, StoreError> {
        self.resolve_session_dir(id).await
    }

    /// A duplicate claim recorded by the last rebuild blocks resolution outright:
    /// `find_session`'s legacy fast path can't see the second claimant, so without
    /// this check a lazy lookup would quietly re-adopt one copy after rebuild
    /// deliberately unindexed the id.
    fn ensure_not_duplicated(&self, id: &str) -> Result<(), StoreError> {
        if self.known_duplicates.read().unwrap().contains(id) {
            return Err(StoreError::Io(format!(
                "session id '{id}' is claimed by multiple directories; refusing to pick one"
            )));
        }
        Ok(())
    }

    /// The one existing-location lookup shared by artifact resolution and creation:
    /// duplicate rejection, catalog hit, full discovery on a cold miss (which warms
    /// the catalog), and corrupt/ambiguous error mapping. `Ok(None)` means no
    /// directory anywhere claims the id -- the two callers alone decide what that
    /// means (legacy fallback path vs. a fresh readable name).
    async fn lookup_existing_dir(&self, id: &str) -> Result<Option<PathBuf>, StoreError> {
        self.ensure_not_duplicated(id)?;
        if let Some(dir) = self.locations.read().unwrap().get(id) {
            return Ok(Some(dir.clone()));
        }

        let vault_base = self.vault_base.clone();
        let id_owned = id.to_string();
        let found = tokio::task::spawn_blocking(move || {
            hypr_vault_read::find_session_and_scan(&vault_base, &id_owned)
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?;

        match found {
            Ok((found, scan)) => {
                // A cold miss pays for a full discovery walk; warm every healthy
                // non-duplicate location from it (fill-only -- an entry a concurrent
                // rename just updated must not be clobbered with pre-scan data), so
                // the next lookup for any of them is a validated cache hit.
                if let Some(discovery) = scan {
                    let mut catalog = self.locations.write().unwrap();
                    for (location, _) in &discovery.sessions {
                        catalog
                            .entry(location.id.clone())
                            .or_insert_with(|| location.relative_dir.clone());
                    }
                }
                Ok(found.map(|(location, _)| {
                    self.catalog_insert(id, location.relative_dir.clone());
                    location.relative_dir
                }))
            }
            // A corrupt legacy meta doesn't unhome the directory: artifact reads keep
            // their historical tolerance there, and read_meta surfaces the parse error
            // itself.
            Err(hypr_vault_read::SessionLookupError::Corrupt { dir, .. }) => Ok(Some(dir)),
            Err(error @ hypr_vault_read::SessionLookupError::Ambiguous { .. }) => {
                Err(StoreError::Io(error.to_string()))
            }
            Err(hypr_vault_read::SessionLookupError::Io(reason)) => Err(StoreError::Io(reason)),
        }
    }

    async fn resolve_session_dir(&self, id: &str) -> Result<PathBuf, StoreError> {
        match self.lookup_existing_dir(id).await? {
            Some(dir) => Ok(dir),
            None => Ok(legacy_session_dir(id)),
        }
    }

    /// Directory a `_meta.json` write should target. An existing session keeps its
    /// directory wherever (and whatever) it is; a brand-new id receives a
    /// collision-free human-readable directory name (`YYYY-MM-DD — title — shortid`).
    /// Callers never supply a physical folder name -- the policy lives entirely here.
    pub(crate) async fn creation_dir_locked(
        &self,
        _guard: &WriteGuard<'_>,
        meta: &super::SessionMeta,
    ) -> Result<PathBuf, StoreError> {
        match self.lookup_existing_dir(&meta.id).await? {
            Some(dir) => Ok(dir),
            None => self.choose_new_session_dir(meta).await,
        }
    }

    async fn choose_new_session_dir(
        &self,
        meta: &super::SessionMeta,
    ) -> Result<PathBuf, StoreError> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let (date, diagnostic) =
            super::layout_name::session_date(meta.started_at.as_deref(), &meta.created_at, &today);
        if let Some(diagnostic) = diagnostic {
            tracing::warn!(session_id = %meta.id, %diagnostic, "naming new session directory from the current date");
        }
        let mut candidates: Vec<PathBuf> = super::layout_name::session_dir_candidates(
            &hypr_vault_read::paths::sessions_root(),
            &date,
            &meta.title,
            &meta.id,
        );
        // A meta-less directory already named exactly for this id (the recorder's
        // ghost fallback: transcript/audio can land before the first meta write) is
        // adopted rather than orphaned beside a readable-named sibling.
        let legacy = legacy_session_dir(&meta.id);
        let vault_base = self.vault_base.clone();
        let id = meta.id.clone();
        tokio::task::spawn_blocking(move || {
            if vault_base.join(&legacy).is_dir() {
                return Ok(legacy);
            }
            // Never merge onto an occupied target: each candidate that exists belongs
            // to some other directory tree (a same-id claimant would have resolved
            // above), so keep widening the suffix and fail rather than reuse one.
            candidates
                .iter()
                .position(|candidate| !vault_base.join(candidate).exists())
                .map(|index| candidates.swap_remove(index))
                .ok_or_else(|| {
                    StoreError::Io(format!(
                        "no free directory name for new session {id}: every candidate is occupied"
                    ))
                })
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
    }

    /// Logical session id owning a vault-relative path (the path itself, or anything
    /// nested under a cataloged session directory), by longest-prefix match with
    /// NFC-normalized component comparison -- the watcher's classification lookup.
    /// `None` for paths outside every cataloged session directory (including a
    /// not-yet-cataloged brand-new directory, which callers treat as structural).
    pub fn session_id_for_relative_path(&self, relative: &Path) -> Option<String> {
        let catalog = self.locations.read().unwrap();
        catalog
            .iter()
            .filter(|(_, dir)| hypr_vault_read::layout::path_starts_with_nfc(relative, dir))
            .max_by_key(|(_, dir)| dir.components().count())
            .map(|(id, _)| id.clone())
    }

    /// Rename a session's physical directory (presentation only -- the logical id
    /// never changes). Runs under the write lock so no artifact write can race into
    /// the old location; the target must not exist (never merge, never overwrite);
    /// the catalog and the write journal are re-homed atomically with the rename so
    /// late filesystem events for old paths are not mistaken for current writes.
    pub(crate) async fn rename_session_dir_locked(
        &self,
        _guard: &WriteGuard<'_>,
        id: &str,
        from: &Path,
        to: &Path,
    ) -> Result<(), StoreError> {
        let vault_base = self.vault_base.clone();
        let from_abs = vault_base.join(from);
        let to_abs = vault_base.join(to);
        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            if to_abs.exists() {
                return Err(StoreError::Io(format!(
                    "rename target {} is already occupied",
                    to_abs.display()
                )));
            }
            std::fs::rename(&from_abs, &to_abs)
                .map_err(|e| StoreError::Io(format!("failed to rename session directory: {e}")))
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))??;

        self.catalog_insert(id, to.to_path_buf());
        if let (Some(from_str), Some(to_str)) = (from.to_str(), to.to_str()) {
            self.journal.remap_prefix(from_str, to_str);
        }
        Ok(())
    }

    /// One-shot provisional-to-final rename: if the session's current basename is the
    /// app's provisional `Untitled` form and it now has a non-empty title, rename it
    /// once to the final readable name (widening the short-id suffix on collision,
    /// preserving the established date prefix). Deferred while the session is
    /// recording; failures are logged, never propagated -- the title is user data
    /// and must not be rolled back because its presentation rename failed. Startup
    /// reconciliation retries anything left over.
    pub(crate) async fn reconcile_provisional_name_locked(
        &self,
        guard: &WriteGuard<'_>,
        id: &str,
        title: &str,
        current_dir: &Path,
    ) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        // A title that sanitizes to the provisional word itself (literal "Untitled",
        // or pure punctuation) would produce another provisional-shaped name: the
        // rename would ping-pong between suffix widths on every meta write.
        if super::layout_name::sanitize_title(title) == super::layout_name::UNTITLED {
            return;
        }
        if self.active_recordings.lock().unwrap().contains(id) {
            return;
        }
        let Some(basename) = current_dir.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if !super::layout_name::is_provisional_untitled_name(basename) {
            return;
        }
        // The provisional form always opens with its date; keep it -- moving a vault
        // across time zones must not rename established directories.
        let date = &basename[..10];
        let Some(parent) = current_dir.parent() else {
            return;
        };

        let candidates: Vec<PathBuf> =
            super::layout_name::session_dir_candidates(parent, date, title, id)
                .into_iter()
                .filter(|target| !hypr_vault_read::layout::paths_eq_nfc(target, current_dir))
                .collect();

        let vault_base = self.vault_base.clone();
        let probe = candidates.clone();
        let free = tokio::task::spawn_blocking(move || {
            probe
                .into_iter()
                .find(|candidate| !vault_base.join(candidate).exists())
        })
        .await
        .ok()
        .flatten();
        let Some(target) = free else {
            tracing::warn!(session_id = %id, "every readable-name candidate is occupied; keeping the provisional directory name");
            return;
        };

        match self
            .rename_session_dir_locked(guard, id, current_dir, &target)
            .await
        {
            Ok(()) => {
                tracing::info!(session_id = %id, from = %current_dir.display(), to = %target.display(), "renamed provisional session directory to its final readable name");
            }
            Err(error) => {
                tracing::warn!(session_id = %id, %error, "provisional session directory rename failed; will retry at next startup");
            }
        }
    }

    /// Startup reconciliation: rename every provisional-`Untitled` directory whose
    /// metadata already carries a non-empty title (covers crashes mid-recording and
    /// title writes from older code paths). Idempotent and read-tolerant.
    pub async fn reconcile_provisional_names(&self) {
        let vault_base = self.vault_base.clone();
        let discovered = tokio::task::spawn_blocking(move || {
            hypr_vault_read::discover_sessions(&vault_base)
                .map(|discovery| discovery.sessions)
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        for (location, meta) in discovered {
            let is_provisional = location
                .relative_dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(super::layout_name::is_provisional_untitled_name);
            if !is_provisional || meta.title.trim().is_empty() {
                continue;
            }
            let guard = self.lock_writes().await;
            self.reconcile_provisional_name_locked(
                &guard,
                &meta.id,
                &meta.title,
                &location.relative_dir,
            )
            .await;
        }
    }

    /// Synchronous half of the recording-lifecycle guard: registers the session as
    /// actively recording the moment the capture event arrives, before any async
    /// meta stamping gets scheduled. Narrows the window in which a first-title
    /// directory rename could race the recorder's already-captured absolute paths.
    /// Cleared by `mark_recording_ended` (or `delete_session`).
    pub fn note_recording_active(&self, id: &str) {
        self.active_recordings
            .lock()
            .unwrap()
            .insert(id.to_string());
    }

    pub(crate) fn catalog_insert(&self, id: &str, dir: PathBuf) {
        self.locations.write().unwrap().insert(id.to_string(), dir);
    }

    pub(crate) fn catalog_remove(&self, id: &str) {
        self.locations.write().unwrap().remove(id);
    }
}
