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

        let removals_before = self
            .catalog_removals
            .load(std::sync::atomic::Ordering::SeqCst);
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
                // the next lookup for any of them is a validated cache hit. The scan
                // ran without the write lock, so if any catalog entry was REMOVED
                // while it ran (a concurrent delete), the whole snapshot is suspect:
                // fill-only cannot tell a never-cached id from a just-deleted one,
                // and re-inserting a trashed directory would let a late write
                // recreate it. Skip the warm (and the hit insert) in that case; the
                // resolved answer itself is still returned.
                let no_concurrent_removal = self
                    .catalog_removals
                    .load(std::sync::atomic::Ordering::SeqCst)
                    == removals_before;
                if no_concurrent_removal && let Some(discovery) = &scan {
                    let mut catalog = self.locations.write().unwrap();
                    for (location, _) in &discovery.sessions {
                        catalog
                            .entry(location.id.clone())
                            .or_insert_with(|| location.relative_dir.clone());
                    }
                }
                Ok(found.map(|(location, _)| {
                    if no_concurrent_removal {
                        self.catalog_insert(id, location.relative_dir.clone());
                    }
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

    /// `lookup_existing_dir` for an id the caller just generated (a fresh random
    /// UUID): the same duplicate rejection and catalog probe, and the same
    /// tolerance for every state of the legacy `sessions/<id>` directory, but a
    /// miss costs one O(1) probe instead of a full discovery walk. The skipped
    /// scan's only remaining job would be finding a readable-named or nested
    /// directory claiming the id — impossible for a just-minted UUID short of an
    /// RNG collision. Never use this for an id that may already exist somewhere:
    /// an unseen claimant would get a duplicate sibling directory here.
    async fn lookup_existing_dir_unscanned(&self, id: &str) -> Result<Option<PathBuf>, StoreError> {
        self.ensure_not_duplicated(id)?;
        if let Some(dir) = self.locations.read().unwrap().get(id) {
            return Ok(Some(dir.clone()));
        }

        let legacy = legacy_session_dir(id);
        let abs = self.vault_base.join(&legacy);
        let id_owned = id.to_string();
        let claimed = tokio::task::spawn_blocking(move || {
            match hypr_vault_read::classify_session_dir(&abs) {
                hypr_vault_read::SessionDirKind::Session(meta) => {
                    hypr_vault_read::layout::eq_nfc(&meta.id, &id_owned)
                }
                // Matches the scanning lookup: a corrupt legacy meta keeps the
                // directory as the id's home instead of minting a sibling.
                hypr_vault_read::SessionDirKind::Corrupt(_) => true,
                hypr_vault_read::SessionDirKind::Folder => false,
            }
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?;
        Ok(claimed.then_some(legacy))
    }

    /// `creation_dir_locked` for a caller-guaranteed-fresh id: identical naming
    /// policy (ghost adoption and occupied-candidate refusal included), but the
    /// cold-catalog lookup stays O(1) in session count — see
    /// `lookup_existing_dir_unscanned` for why that is sound only for
    /// just-generated ids.
    pub(crate) async fn creation_dir_fresh_locked(
        &self,
        _guard: &WriteGuard<'_>,
        meta: &super::SessionMeta,
    ) -> Result<PathBuf, StoreError> {
        match self.lookup_existing_dir_unscanned(&meta.id).await? {
            Some(dir) => Ok(dir),
            None => self.choose_new_session_dir(meta).await,
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
        if self.active_recordings.lock().unwrap().contains_key(id) {
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

    /// Explicit, user-invoked rename of the session's directory to the readable name
    /// derived from its current title. Unlike the one-shot provisional reconcile this
    /// runs whatever the current basename is -- the user asked for it -- but it still
    /// refuses while a recording path lease is held (the recorder writes into the
    /// resolved path), and unlike the reconcile its failures propagate so the UI can
    /// report them. Renaming to a name the directory already carries is a successful
    /// no-op. Returns the basename in effect afterwards.
    pub async fn rename_session_dir_to_title(&self, id: &str) -> Result<String, StoreError> {
        validate_session_id(id)?;
        let guard = self.lock_writes().await;
        if self.active_recordings.lock().unwrap().contains_key(id) {
            return Err(StoreError::Io(
                "a recording is in progress for this session; rename the folder after it stops"
                    .to_string(),
            ));
        }
        let meta = self
            .read_meta(id)
            .await?
            .ok_or_else(|| StoreError::Io(format!("session {id} has no metadata")))?;
        let title = meta.title.trim().to_string();
        if title.is_empty() {
            return Err(StoreError::Io(
                "the session has no title to name the folder after".to_string(),
            ));
        }
        let current_dir = self
            .lookup_existing_dir(id)
            .await?
            .ok_or_else(|| StoreError::Io(format!("no directory found for session {id}")))?;
        let basename = current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                StoreError::Io(format!(
                    "session directory {} has no usable basename",
                    current_dir.display()
                ))
            })?
            .to_string();

        // Keep the basename's established date when it carries one (moving a vault
        // across time zones must not shift dates); a legacy UUID or fully hand-renamed
        // directory derives its date the same way creation would.
        let date = match super::layout_name::leading_date(&basename) {
            Some(date) => date.to_string(),
            None => {
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                let (date, diagnostic) = super::layout_name::session_date(
                    meta.started_at.as_deref(),
                    &meta.created_at,
                    &today,
                );
                if let Some(diagnostic) = diagnostic {
                    tracing::warn!(session_id = %id, %diagnostic, "naming renamed session directory from the current date");
                }
                date
            }
        };
        let parent = current_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let candidates = super::layout_name::session_dir_candidates(&parent, &date, &title, id);
        if candidates
            .iter()
            .any(|candidate| hypr_vault_read::layout::paths_eq_nfc(candidate, &current_dir))
        {
            return Ok(basename);
        }

        let vault_base = self.vault_base.clone();
        let probe = candidates.clone();
        let free = tokio::task::spawn_blocking(move || {
            probe
                .into_iter()
                .find(|candidate| !vault_base.join(candidate).exists())
        })
        .await
        .map_err(|e| StoreError::Io(format!("task join error: {e}")))?;
        let Some(target) = free else {
            return Err(StoreError::Io(
                "every candidate folder name is already occupied".to_string(),
            ));
        };

        self.rename_session_dir_locked(&guard, id, &current_dir, &target)
            .await?;
        tracing::info!(session_id = %id, from = %current_dir.display(), to = %target.display(), "renamed session directory to match its title");
        Ok(target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string())
    }

    /// Startup reconciliation: rename every provisional-`Untitled` directory whose
    /// metadata already carries a non-empty title (covers crashes mid-recording and
    /// title writes from older code paths). Idempotent and read-tolerant.
    pub async fn reconcile_provisional_names(&self) {
        let guard = self.lock_writes().await;
        let Ok(mut scan) = self.scan_session_locations().await else {
            return;
        };
        self.reconcile_provisional_from_scan(&guard, &mut scan)
            .await;
    }

    /// The reconciliation body, working off an already-paid layout snapshot (see
    /// `normalize_startup_layout`). Successful renames update the snapshot's paths
    /// in place via the catalog entry the rename just wrote.
    pub(super) async fn reconcile_provisional_from_scan(
        &self,
        guard: &WriteGuard<'_>,
        scan: &mut super::rebuild::SessionLayoutScan,
    ) {
        for (location, meta) in scan.sessions.iter_mut() {
            let is_provisional = location
                .relative_dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(super::layout_name::is_provisional_untitled_name);
            if !is_provisional || meta.title.trim().is_empty() {
                continue;
            }
            self.reconcile_provisional_name_locked(
                guard,
                &location.id,
                &meta.title,
                &location.relative_dir,
            )
            .await;
            if let Some(dir) = self.locations.read().unwrap().get(&location.id) {
                location.relative_dir = dir.clone();
            }
        }
    }

    /// Synchronous half of the recording-lifecycle guard: ensures the session holds
    /// at least one path lease the moment the `Started` capture event arrives,
    /// before any async meta stamping gets scheduled -- an idempotent defense for
    /// capture callers that bypassed `prepare_recording`. Deliberately does NOT
    /// increment an existing lease: the preparer already holds one for this
    /// recording, and stacking another here would survive `Stopped`'s clear only by
    /// accident. Cleared by `mark_recording_ended` (or `delete_session`).
    pub fn note_recording_active(&self, id: &str) {
        self.active_recordings
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert(1);
    }

    /// Reserve the session's physical directory ahead of a recording: resolves the
    /// current directory under the write lock and takes one path lease before
    /// releasing it, so the returned path cannot be renamed away by a first-title
    /// reconcile between now and recorder open (the pre-start hook and the recorder
    /// both use it). Every prepare is paired with either `release_recording_prepare`
    /// (start failed) or the `Stopped` lifecycle clearing the session's leases.
    pub async fn prepare_recording(&self, id: &str) -> Result<PathBuf, StoreError> {
        validate_session_id(id)?;
        let guard = self.lock_writes().await;
        let dir = match self.lookup_existing_dir(id).await? {
            Some(dir) => dir,
            // No directory claims the id: resolve exactly the way the recorder
            // itself will (fs-sync's discovery adopts a meta-less/corrupt directory
            // named for the id -- a moved recorder ghost -- before falling back to
            // the legacy creation target), so the pre-start hook and the recorder
            // can never disagree about the session's home. Non-UUID legacy ids,
            // which fs-sync rejects, keep the plain legacy fallback.
            None => {
                let vault_base = self.vault_base.clone();
                let id_owned = id.to_string();
                tokio::task::spawn_blocking(move || {
                    hypr_fs_sync_core::find_session_dir(&vault_base.join("sessions"), &id_owned)
                        .ok()
                        .and_then(|abs| abs.strip_prefix(&vault_base).ok().map(Path::to_path_buf))
                })
                .await
                .map_err(|e| StoreError::Io(format!("task join error: {e}")))?
                .unwrap_or_else(|| legacy_session_dir(id))
            }
        };
        *self
            .active_recordings
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert(0) += 1;
        drop(guard);
        Ok(dir)
    }

    /// Release exactly one path lease taken by `prepare_recording`. Only when the
    /// last lease drops does the deferred provisional-title rename get retried --
    /// releasing one of several reservations (a failed duplicate start) must never
    /// unprotect a recording that is still active. Safe to call from paired failure
    /// cleanup even when no lease is held.
    pub async fn release_recording_prepare(&self, id: &str) -> Result<(), StoreError> {
        validate_session_id(id)?;
        let guard = self.lock_writes().await;
        let now_unprotected = {
            let mut leases = self.active_recordings.lock().unwrap();
            match leases.get_mut(id) {
                Some(count) if *count > 1 => {
                    *count -= 1;
                    false
                }
                Some(_) => {
                    leases.remove(id);
                    true
                }
                None => false,
            }
        };
        if now_unprotected
            && let Ok(Some(meta)) = self.read_meta(id).await
            && !meta.title.trim().is_empty()
            && let Ok(dir) = self.session_dir_locked(&guard, id).await
        {
            self.reconcile_provisional_name_locked(&guard, id, &meta.title, &dir)
                .await;
        }
        Ok(())
    }

    /// Catalog writes announce genuine physical-location changes on the index bus
    /// (`IndexEntity::Locations`): every frontend cache holding an absolute session
    /// path invalidates off that one event, wherever the change originated (app
    /// rename, migration, delete/restore, fs-sync move, external rename caught by a
    /// rebuild). Re-inserting the same NFC-normalized path stays silent.
    pub(crate) fn catalog_insert(&self, id: &str, dir: PathBuf) {
        let previous = self
            .locations
            .write()
            .unwrap()
            .insert(id.to_string(), dir.clone());
        let changed =
            previous.is_none_or(|prev| !hypr_vault_read::layout::paths_eq_nfc(&prev, &dir));
        if changed {
            self.notify_index_changed(super::IndexEntity::Locations, vec![id.to_string()]);
        }
    }

    pub(crate) fn catalog_remove(&self, id: &str) {
        self.catalog_removals
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.locations.write().unwrap().remove(id).is_some() {
            self.notify_index_changed(super::IndexEntity::Locations, vec![id.to_string()]);
        }
    }
}
