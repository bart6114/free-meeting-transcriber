# Plan: session-layout follow-ups (perf, races, dedup)

Status: proposed

## Context

The readable-session-folders feature (PR #29, v0.16.0; see
`docs/plans/human-readable-personal-session-folders.md`, now marked implemented) shipped
with a set of review findings deliberately deferred: none are correctness bugs, but one
is a recurring user-facing performance regression that got strictly worse the moment the
one-way migration ran, two are narrow-but-real races, and the rest are duplication that
will drift if left. This plan sequences all of them. Every claim below was re-verified
against the code on `main` at v0.16.0 (`03c07b60e`).

Phases are independent — each lands (and is verifiable) on its own. Phase 1 is the only
one with day-to-day user impact; Phases 3–4 are pure refactors that must not change any
observable behavior and should each be a small, separately revertable commit series.

## Phase 1: stop paying a full vault scan per UI interaction

### The problem

`plugins/fs-sync/src/commands.rs` has one resolver helper (`resolve_session_dir`,
~line 27) used by `audio_exist`, `audio_peaks`, `audio_path`, `audio_import`,
`session_dir` (which also backs Show in Finder and the TS `getSessionResourcePath`),
attachment path commands, and `load_session_content`. It calls
`hypr_fs_sync_core::session::find_session_dir`, whose fast path probes the legacy
`sessions/<id>` directory. Since the v0.16.0 migration renamed every directory to the
readable form, that probe now always misses, so **every one of these commands runs
`hypr_vault_read::find_session`'s full recursive discovery — reading and JSON-parsing
every `_meta.json` in the vault**. Opening a note fires several of these (audio exists +
peaks + path at minimum). The same cost applies to `FsSyncCore`'s *internal* resolutions
(`crates/fs-sync-core/src/lib.rs` calls `self.resolve_session_dir` at ~lines 67, 209,
225, 274, 282 for move/attachment/audio operations reached through `app.fs_sync()`).
On top of that, an identity miss now also triggers the `find_unclaimed_dir_named`
recursive walk added by the review hardening (`crates/fs-sync-core/src/session.rs`).

Meanwhile `SessionStore` holds a warm, always-current location catalog
(`crates/vault-write/src/locations.rs`, `session_dir()` is `pub`), maintained by every
write, rename, delete, restore, and rebuild — and the desktop manages
`Arc<hypr_vault_write::SessionStore>` in Tauri state. The original plan even prescribed
this: "Where a component already has access to SessionStore, expose a store-backed
`session_dir` operation so it uses the warm catalog." The fs-sync plugin already depends
on `hypr-vault-write` (added for the post-move catalog refresh), so no new dependency is
needed.

### Design

1. In `plugins/fs-sync/src/commands.rs`, make the `resolve_session_dir` helper async and
   store-first:
   - `app.try_state::<Arc<hypr_vault_write::SessionStore>>()` present →
     `store.session_dir(id).await` (O(1) catalog hit; misses fall through to the
     store's own targeted discovery, which caches). Map `StoreError` to the command's
     `String` error.
   - store not managed (plugin tests, hypothetical headless use) → current
     `find_session_dir` behavior, unchanged.
   Note the store's answer is vault-relative — join it onto the vault base exactly as
   the current helper joins under `sessions/`. Behavioral deltas to preserve/accept:
   the store errors on rebuild-recorded duplicate ids (`known_duplicates`) where the
   core resolver errors on discovery-detected ones — same intent, keep the store's
   answer authoritative.
2. For `FsSyncCore`-internal resolutions, inject the fast path instead of duplicating
   it: give `FsSyncCore` an optional resolver override, e.g.
   `FsSyncCore::with_resolver(base_dir, resolver: Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>)`,
   consulted first by `FsSyncCore::resolve_session_dir` (fall through to
   `find_session_dir` on `None`). The plugin's `ext.rs` `core()` builds it from the
   managed store when available. The closure must be synchronous — the core is sync —
   so it can only serve *catalog hits*: expose a small sync
   `SessionStore::session_dir_cached(&self, id) -> Option<PathBuf>` (a read-lock map
   lookup, no discovery) in `crates/vault-write/src/locations.rs` for this purpose.
   A cache miss then costs what it costs today, which is fine: after startup rebuild
   the catalog contains every session.
3. Do NOT route the resolver through the store for `move_session` — its source
   resolution participates in fs-sync's own move semantics and the command already
   rebuilds the store catalog afterward. Leave it on the core path (the injected
   cached-hit resolver still speeds it up harmlessly).

### Also in this phase: audio-path query staleness on title-driven rename

`apps/desktop/src/session/index.tsx` caches `fsSyncCommands.audioPath(tab.id)` under
`queryKey: ["audio", tab.id, "url"]` and only invalidates on the capturing→idle
transition (~line 74). The provisional-to-final directory rename triggered by a *title*
write (session titled while not recording, e.g. an untitled session with imported audio)
leaves that cached absolute path stale until refetch. Fix: invalidate
`{ queryKey: ["audio", sessionId] }` when a title update mutation succeeds. Find the
mutation site in `apps/desktop/src/session/queries.ts` (~line 253, the store-canonical
title write) / its tanstack-mutation wrapper and add the invalidation there — mirror how
the existing capturing-transition invalidation is written. Cheap; do not build anything
rename-aware in the frontend (renames are invisible to it by design; the id is stable).

### Tests

- Plugin: a readable-layout session resolved through the command path with a managed
  store hits the catalog (assert via behavior: seed a vault, warm the store, remove
  read permission from... — impractical; instead unit-test
  `session_dir_cached` in vault-write and the `with_resolver` override in fs-sync-core:
  resolver returning a dir is used verbatim; returning `None` falls through).
- Existing `tauri-plugin-fs-sync` `export_types` binding-stability test must stay green
  (no command signature changes).
- Frontend: extend the existing audio-query test coverage (vitest) if a natural seam
  exists; otherwise the invalidation is covered by typecheck + manual QA (title an
  untitled session with imported audio → player still resolves).

### Acceptance

Opening a note in a migrated vault performs zero full-vault scans on the audio/session
path commands (verify by tracing/instrumenting locally or by test on
`session_dir_cached` usage). No behavior change for headless/core users.

## Phase 2: sequence the stop-time rename against the hooks path (optional)

### The problem

`apps/desktop/src/store/zustand/listener/general-live.ts` (~line 578) resolves
`resource_dir` for the `AfterListeningStopped` hook via `getSessionResourcePath`
concurrently with the Rust side reacting to the same `CaptureLifecycleEvent::Stopped`:
`apps/desktop/src-tauri/src/recording_meta.rs` spawns `mark_recording_ended`, whose
meta write reconciles the deferred provisional rename
(`reconcile_provisional_name_locked`). A hook script can therefore receive a directory
path that is renamed milliseconds later. Preconditions: user hooks configured AND the
session was untitled at record-start AND titled during recording — narrow, but the
failure (hook writes into a vanished path, or reads nothing) is confusing when hit.

### Design

Deterministic ordering, not retries: after `mark_recording_ended` completes (success or
failure), `recording_meta.rs` emits a dedicated Tauri event, e.g.
`RecordingMetaSettled { session_id }` (tauri-specta event alongside the existing ones).
The JS stop path awaits that event for the session id — with a short timeout fallback
(~2s) so a missing event can never wedge the stop flow — *before* resolving
`resource_dir`. After the event, the store-backed `sessionDir` command returns the
post-rename directory. `BeforeListeningStarted` (~line 332) needs no change: no rename
can happen between its resolution and recording start (the synchronous
`note_recording_active` guard covers the start race).

If this proves noisier than it is worth during implementation (event plumbing through
tauri-specta bindings), the documented fallback position is acceptable: keep it a
release-notes caveat. Do not ship a sleep-based "fix".

### Tests

- Rust: none new (event emission is glue); keep `recording_meta` behavior covered by
  existing store tests.
- TS: unit-test the await-with-timeout helper (event arrives → resolves after it;
  event never arrives → resolves after timeout).

## Phase 3: deduplicate the layout invariants (pure refactor)

No observable behavior may change in this phase; the existing suites are the oracle.
Order within the phase is by drift-risk of the duplicated rule.

1. **`classify_dir` exists twice** — `crates/vault-read/src/layout.rs` (~line 118,
   private `DirKind`: `Session(Box<SessionMeta>) / Corrupt(String) / Folder`) and
   `crates/fs-sync-core/src/session.rs` (~line 18, `DirClass::Session(Option<String>) /
   Folder`). Same core identity rule ("a dir holding `_meta.json` is a session even
   when unreadable; only meta-less dirs are folders") encoded independently.
   Fix: make vault-read's `classify_dir`/`DirKind` `pub` (it carries strictly more
   information) and reimplement fs-sync-core's `DirClass` as a thin mapping over it.
   fs-sync-core already depends on `hypr-vault-read`.
2. **Traversal divergence (review S5)**: fs-sync-core's folder traversal
   (`folder.rs::scan_directory_recursive`) and file scan (`scan.rs`) have **no
   dot-directory skip** (verified: no `starts_with('.')` in either file), while
   vault-read discovery and the watcher both skip hidden entries. A sync client's
   `.stversions/` under `sessions/` is invisible to the index but shows in the folders
   sidebar. Fix while touching these files for (1): skip dot-prefixed directories in
   both traversals, with a test (hidden folder is not listed; hidden dir contents not
   scanned).
3. **Collision-policy block written three times** — the
   `short_id_candidates → format_session_dir_name → first free target` loop appears in
   `crates/vault-write/src/locations.rs` `choose_new_session_dir` (~line 126, plus a
   legacy ghost-dir adoption check), `reconcile_provisional_name_locked` (~line 239
   region, plus a `paths_eq_nfc` self-filter), and `crates/vault-write/src/migrate.rs`
   (~line 62, plus a `claimed`-set predicate). This is the invariant the whole naming
   feature hangs on. Fix: one helper in `layout_name.rs` or `locations.rs`, e.g.
   `first_free_dir_name(parent, date, title, id, is_taken: impl Fn(&Path) -> bool) -> Option<PathBuf>`,
   with each caller supplying its extra predicate (claimed-set, self-equality, ghost
   adoption stays caller-side). Side effect: removes the `position + swap_remove` dance
   (plain `find` returning owned works once the helper owns the vec).
4. **`creation_dir_locked` duplicates `resolve_session_dir`**
   (`crates/vault-write/src/locations.rs` ~lines 61–124): identical
   catalog-check + `spawn_blocking find_session` + error arms, differing only in the
   `Ok(None)` arm. Fix: private
   `lookup_existing_dir(id) -> Result<Option<PathBuf>, StoreError>` shared by both;
   `resolve_session_dir` maps `None → legacy_session_dir(id)`, `creation_dir_locked`
   maps `None → choose_new_session_dir(meta)`. Both keep `ensure_not_duplicated` first.
5. **Retention copy-paste (review S6)**:
   `crates/fs-sync-core/src/audio/mod.rs::delete_orphaned_expired_in_dir` — after the
   hardening, the parseable-meta arm is a no-op (`DirClass::Session(_) => {}`), so only
   the meta-less `is_uuid(name)` arm deletes; the duplication mostly evaporated.
   Verify and simplify residue only.
6. **Small fry** (all in `crates/vault-write/src/layout_name.rs`):
   `short_id_candidates`' collect-then-fold is a hand-rolled `Vec::dedup` — use
   `dedup()`. `is_uuid_shaped`/`id_hex` hand-roll what `uuid::Uuid::try_parse` +
   `.simple()` provide — acceptable to keep IF documented as deliberate looseness
   (dash placement) — decide in-implementation; if switching to `uuid`, add the crate
   dep from the workspace and keep the hashed-fallback branch for non-UUID ids intact.
7. **Test-fixture seeding is re-rolled in four places** (review M7):
   `migrate.rs::tests::seed`, `dual_layout_tests.rs::seed_session_at`, vault-read
   `layout.rs` tests, `vault-read/tests/dual_layout.rs`. Optional: a
   `#[cfg(test)]`-shared helper in vault-read (usable by vault-write via dev-dep is NOT
   currently wired — likely not worth cross-crate plumbing; unify within each crate
   only).

## Phase 4: single-scan startup and rebuild (efficiency)

Only worthwhile once vault sizes make it measurable (1000+ sessions) or if startup
metrics complain; land after Phase 3 so the code being optimized is already deduped.

1. **Triple startup scan** (`apps/desktop/src-tauri/src/lib.rs` ~line 332 region):
   `migrate_legacy_session_directories` → `reconcile_provisional_names` →
   `rebuild_index`, each running its own full `discover_sessions`, all under `block_on`
   before the UI proceeds. Fix: let migration and reconciliation *return/accept* a
   discovery snapshot, or fold both into a store-level
   `startup_normalize_layout()` that scans once, migrates, reconciles (renames update
   the in-memory snapshot), then hands the final snapshot to `rebuild_index` (which
   needs a parameterized variant taking a pre-computed `SessionLayoutScan`). Preserve
   the write-lock discipline established in the hardening pass: the scan+catalog-swap
   stays under the store write lock; renames stay under the same guard.
2. **Rebuild double-walk + meta re-read** (review M1):
   `crates/vault-write/src/rebuild.rs` `scan_session_locations` (~line 389) runs
   discovery, then `scan_ghost_dirs` (~line 583) re-walks the same tree, then
   `refresh_one` (~line 175) re-reads every `_meta.json` discovery just parsed. Fix:
   emit ghost candidates from the discovery walk (requires extending vault-read
   discovery to optionally report meta-less content-bearing dirs — keep it opt-in so
   CLI listing semantics don't change) and thread the parsed metas into `refresh_one`
   so the meta read is skipped when the scan already has it (keep the read-fallback
   for `refresh_session`'s single-id path).
3. **Micro items** (review M3, L3, L4 — take only if touching the files anyway):
   `scan.rs` classification only needs the session-vs-folder bit; a
   `_meta.json` existence stat is cheaper than read+parse — but after Phase 3.1 the
   classification is shared, so add a cheap existence-only variant rather than forking
   the rule. vault-write `resolve_session_dir` cold misses run full discovery but cache
   only the queried id — have the miss path warm the whole catalog from the discovery
   it already paid for (careful: only insert ids that are not duplicates). Watcher
   reverse lookup is O(sessions) per path — fine at current scale; skip unless
   profiling says otherwise.

## Explicitly not in scope

- Any behavior change to naming, migration, identity rules, or delete/undo.
- Caching in `FsSyncCore` itself (state in a per-command-constructed struct is a trap;
  the store catalog is the cache).
- The remaining accepted-race residue after Phase 2's ordering fix (a hook script that
  itself holds the path across minutes was never guaranteed stability — directory
  renames are a documented property of the vault now).
- Frontend rename-awareness beyond the audio-query invalidation (ids are stable; paths
  are resolved on demand).

## Verification

Per phase:

```sh
cargo test -p vault-read -p vault-write -p fs-sync-core -p tauri-plugin-fs-sync -p fmtr-cli -p listener-core -p tauri-plugin-transcription
cargo test -p desktop --lib
cargo check
cargo clippy --locked -p agent-access -p fmtr-cli -p tiptap --all-targets --no-deps -- -D warnings
corepack pnpm -F desktop typecheck   # after TS changes; plain pnpm may be missing from PATH
pnpm exec dprint fmt
```

Phases 3–4 additionally: the full suites must pass **unchanged** (no test edits except
new coverage) — any test needing modification in a "pure refactor" phase is a signal
the refactor changed behavior.

Manual spot-checks: Phase 1 — open a note with audio in a migrated vault and confirm
player/attachments/Finder-reveal; title an untitled session that has imported audio and
confirm the player still resolves. Phase 2 — with a hook configured, title a session
mid-recording, stop, confirm the hook receives the final (post-rename) directory.

## Suggested implementation order and sizing

| Phase | Size | Urgency |
| --- | --- | --- |
| 1 (catalog-first resolution + audio invalidation) | S–M, one PR | Now — recurring UI-path cost in every migrated vault |
| 3 (dedup) | M, one PR of small commits | Soon, low risk, do before anyone edits the naming policy |
| 2 (stop-event ordering) | S–M | Opportunistic; skip if event plumbing balloons |
| 4 (single-scan startup/rebuild) | M–L | Only on measured need |

If time-boxed to one session: do Phase 1 fully, then Phase 3 items 1–4.
