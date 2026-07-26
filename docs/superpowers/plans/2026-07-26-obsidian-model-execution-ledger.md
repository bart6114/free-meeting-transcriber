# Execution ledger — Obsidian-model plan (meeting-chat drop, config.json, SQLite removal)

**Plan:** `docs/superpowers/plans/2026-07-26-meeting-chat-drop-config-file-sqlite-removal.md`
**Branch:** `refactor/obsidian-model-no-db` (from `main` @ `86d5be9`)
**Mode:** controller + subagent implementers; controller commits after each accepted task.
**Confirmed decisions:** D1 (delete `meeting_ax.rs` wholesale, incl. caller-less
`inspect_meeting_accessibility`), D8 (CLI `--db-path` → `--vault-path`).

**Hard rules in force:**
- Never edit shipped migrations.
- Phase B `drop_dead_tables` is the LAST new SQL migration.
- Do not touch `telemetry_consent` while removing `consent_auto_send_chat`.

**OWNER DIRECTIVE CHANGE (2026-07-26, after C1):** "Drop the legacy solution,
no actual migration needed — start data capture from scratch. Keep going until
completely finished."
- C2 (settings migration) CANCELLED — config.json starts from defaults; the FE
  legacy resolution chain (legacy-snapshots, TinybaseValues, quirk parsing) is
  deleted outright in C3.
- D-6 data exodus CANCELLED — no `.files-canonical-v1` marker, no DB→file
  export. New file homes start empty; DB-only data (templates→reseeded
  defaults, action items, UUID summaries, event_json/folder_path/tags,
  settings, providers) is NOT carried over.
- Safety net retained: Phase H renames `app.db*` → `app.db.pre-files-backup*`
  in app-data on first boot (hand-recoverable, never synced). Providers/API
  keys: keychain entries survive untouched; provider config rows are lost →
  user re-adds providers once.
- No more per-phase review stops — run A→H to completion.

## Phase status

| Phase | Status | Commits | Notes |
|-------|--------|---------|-------|
| Plan spot-verify | done | — | claims hold; drift notes below |
| A — meeting-chat total drop | done (Linux gates green) | fa11943, 9513f62, c01de0b | awaiting owner macOS verify |
| B — dead DB weight | done | 40276b7 | last-ever SQL migration added (20260726120000_drop_dead_tables) |
| C — config.json | done | ed21619 (C1), 9a4c910 (C3; C2 cancelled) | config.json = real JSON arrays, FE boundary stringifies; provider ids deterministic (keychain survives re-add); hasValues heuristic = differs-from-default (residual edge: explicit-default value ≡ unset, only surfaces via boot init). plugins/settings legacy load/save commands now FE-orphaned → delete in H sweep. plugins/importer tinybase-shaped output still live — own follow-up, out of scope. Owner-machine check pending: 2-window settings round-trip, theme, dock toggle. |
| D — file homes (no exodus) | DONE | 966214b (D-1), aba1625 (D-3), 1d6d6a2 (D-2), 994d67c (gen layout), 1ac6d2e (D-4), 21b4d6b (D-5) | crates/vault-read = canonical vault format home; CLI zero SQLite deps; templates/tasks cut straight to files, meta/enhanced dual-write until E/F/H |
| E — reactivity core | DONE | 7671fdd (E1), 4208775 (E2), d95e043 (E3) | VaultIndex + 10ms coalesced index-changed bus + 10 typed commands; all 14 FE subscription sites ported; owner-user deleted (D10). Transcript finding: deleted_at was SQL-only transient bookkeeping — file truth has no tombstones (soft-delete zeroes words); supersede becomes store primitive in E3. FE baseline now 1150/167. | D-1 fixed title-revert bug; D-3 found rebuild already pruned index-only UUID rows (shadow hack papered over it) — preserved for legacy rows; enhanced docs = YAML frontmatter via hypr-frontmatter; deletion = .trash move + hard row delete (no undo path existed); store-level CAS with "conflict:" typed errors (store-errors.ts). NOTE: any `cargo test -p desktop --lib` run on Linux prettier-relayouts tauri.gen.ts — checkout + hand-apply. |
| F — search on files | DONE | d66240a | Rust-side bus taps; DirtyQueue reproduces generation semantics (race test ported); projection_version now a file in disposable search_index/; PROJECTION_VERSION→5. E3 findings: empty-note placeholder deleted (was shadowing note content in search); tag tables dead; supersede = session_replace_transcripts. |
| G — FE/db decoupling | DONE | 5d33c7b | packages/db{,-react,-tauri,-runtime}, plugins/db JS, src/db deleted; write-queue → shared/; db plugin unregistered; only Rust crates left for H. |
| H — SQLite deletion | DONE | af16a7e | ~19.4k lines deleted; 8 crates + plugins/db gone; sqlx out of the workspace; app.db retired in place to .pre-files-backup* (both candidate dirs swept, never clobbered, never fails startup). Watcher ignore generalized (app-data doubles as default vault base). chmod-as-root test finally fixed (unreadability now injected via EISDIR, not chmod) → **176/176 desktop tests pass, zero known failures**. Extra finds: a live sqlx query in a vault_watch test cargo check never compiled; macOS cloudsync framework bundling that would have broken `tauri build`; bitrise.yml for a nonexistent apps/mobile. |

### H scope call needing owner sign-off before merge

`crates/mobile-bridge` is **deleted** by Phase H. Verified: not a dependency of
the desktop binary, already `--exclude`d from CI workspace tests, reachable only
through `cargo xtask mobile-bridge {ios,android,rn}` build tooling. Its entire
function is syncing `app.db` to mobile, so it cannot outlive SQLite — there is no
coherent middle ground that keeps it without also keeping db-core, db-migrate and
cloudsync alive, which defeats the phase. `crates/cloudsync` dies with it
(consumed only by db-core/db-reactive). **If mobile is a product surface you
intend to revive, say so — this is the one Phase H deletion that is a product
decision rather than dead-code removal.** Fully reversible: it lives in git
history on this unmerged branch.

## Whole-branch review (2026-07-26, post-H, three independent read-only reviewers)

Dimensions: reactivity/cache-coherence, data-loss/durability, behavioral parity vs the
replaced SQL. What the reviews **validated** matters as much as what they found: write-through
is uniform with a consistent index-before-notify ordering, the 10ms coalescer provably cannot
drop changes, the search worker's generation/acknowledge logic is sound, every id-scoped
subscription matches the id kind Rust emits, the "corruption never looks like deletion"
invariant holds across all five artifact kinds, `.trash` is written before every destructive
step, and every `ORDER BY`/`WHERE`/`COALESCE` in the retired SQL has a traceable counterpart.
Parity confidence: high on read paths.

### Confirmed defects (fixes in flight)

1. **`useSessionRawMd` silent data loss** (`session/queries.ts:86-108`) — caches `_memo.md`
   under a key nothing ever invalidates, with `staleTime: Infinity`, and *prefers* it over the
   fresh index value. Tab-switch or external edit → editor remounts with stale content → the
   next keystroke persists it back over the file. **Pre-existing** (identical at merge base)
   but it defeats the very bus Phases E–H installed. Flagged by 2 of 3 reviewers.
2. **`write_file` overwrites external edits with no recovery** (`session_store/mod.rs:101-141`)
   — atomic but unconditional; never trashes what it replaces. The repo's own sibling
   `fs_sync_core::export::write_file_atomic` already does this, calling it "the critical fix
   from whole-branch review". Fix trashes only when on-disk bytes differ from the journal hash,
   so normal writes stay trash-free.
3. **`tasks` entity emitted into a void** — Rust notifies, no FE subscriber exists; the
   "Phase E adds store-change events" TODO survived into the final commit. Whole-source
   `replace_tasks` means a stale second window reverts the first's change. Flagged by all 3.
4. **`delete_session` doesn't clear the live transcript buffer** (`content.rs:169`) — an
   in-flight debounced flush recreates `sessions/<id>/`, so undo then fails with ENOTEMPTY and
   the user's undo-delete is permanently broken.
5. **No path validation on session ids** — `delete_session("")` trashes the entire `sessions/`
   tree; an absolute id escapes the vault. Other commands (templates, audio) already guard.
6. Reseed can clobber an edited default template (`is_file()` false negative on stat failure);
   `app.db` retirement can orphan the WAL; read-modify-write lost updates across windows.

### Known residuals after the fix pass (f90d448) — deliberate, flagged not hidden

- **Trash is "zero per editing session", not literally zero.** The journal is in-memory, so
  the first store write to a given file after an app restart has no journal entry and trashes
  one copy of the pre-existing content. Bounded at one per file per run, and it is the safe
  direction (a backup of the pre-session state), but it is not nothing — persisting the
  journal across runs would remove it.
- **`templates.rs::upsert_template`** still uses the unlocked read-modify-write pattern that
  FIX 6 removed elsewhere. Low risk (single-user UI action, identical writes short-circuit),
  but it is a real remaining instance; threading the guard needs `clear_deleted_default` and
  `write_template_file` variants too.
- **`delete_session` residual race.** A transcript flush already past its snapshot and mid-I/O
  when the delete lands can still recreate the folder. The debounce-timer case — the one the
  review described and the one that actually happens — is fully closed; closing the rest needs
  a tombstone/generation scheme.

### Accepted / deferred (not defects to fix now)

- `applyGeneratedSessionTitle` lost transaction atomicity — inherent to dropping the
  transactional store; needs an explicit owner decision, not an accidental fix.
- Transcript ordering dropped its `created_at` tiebreaker (ties only among soft-deleted
  transcripts, which all collapse to `started_at == 0`). Narrow; queued.
- `AGENTS.md` is rewritten at the vault root every boot with no existence check — pre-existing,
  but now materially likelier since the vault is expected to be a pre-existing user directory.
- Audio deletion remains the only user content with no `.trash` recovery path (deliberate
  retention behavior, carried over unchanged).

## Owner-machine checklist (accumulating)

Items that require macOS and/or the real vault; commands to be filled in as phases land.

- [ ] Phase A (ready now): on macOS, at branch `refactor/obsidian-model-no-db`:
  - `cargo check -p detect -p tauri-plugin-detect -p template-app -p tauri-plugin-template -p desktop`
  - `cargo test -p detect -p tauri-plugin-detect -p template-app -p tauri-plugin-template`
  - NOTE: the two `export_types` tests will prettier-relayout `plugins/{detect,template}/js/bindings.gen.ts` on some machines — if `git diff` shows wholesale relayout afterwards, `git checkout -- plugins/detect/js/bindings.gen.ts plugins/template/js/bindings.gen.ts` (semantic content is already correct).
  - Optional: `pnpm -F @hypr/desktop tauri:dev` smoke — start/stop a recording; no disclosure toast/setting anywhere; Settings → General shows telemetry toggle but no "Post recording disclosure" row.
- [ ] Phase C: settings UI round-trip across two windows; theme change without restart; dock-icon toggle applied post-restart.
- [ ] Phase E: multi-window reactivity check (title edit in main → float window updates).

### CRITICAL — macOS-only build risk (cannot be verified on Linux)

- [ ] **`pnpm -F @hypr/desktop tauri:build` on macOS.** Phase H deleted the
  `frameworks` arrays from `tauri.conf.json` + `tauri.conf.macos-intel.json`
  that bundled `crates/cloudsync/vendor/…/cloudsync.dylib`. Keeping them would
  have broken the build outright (the path is gone), but *removing* them is only
  verifiable by actually building on macOS. If the app launches and records, the
  framework was genuinely only needed by the deleted cloudsync stack.

### First-boot data safety (do this on a COPY of the real vault first)

- [ ] **Back up before first launch**: copy the whole vault dir AND
  `app.db*` out of the app-data folder. The exodus was cancelled by owner
  directive — DB-resident data (custom templates, action items, UUID summaries,
  event/folder/tags, settings, provider config) does NOT carry over.
- [ ] Launch once, then confirm `app.db` is now `app.db.pre-files-backup`
  (plus `-wal`/`-shm`) and that the app started normally. Nothing is deleted —
  the rename is in place and reversible by renaming back.
- [ ] Re-add AI providers once (keychain API keys survive; provider config rows
  did not). Confirm enhance/summarize works end to end.

### Obsidian-model verification ritual (plan §9)

- [ ] Cold-boot timing on the real vault (index build is now a full FS scan —
  compare against the old SQLite boot).
- [ ] Edit `_memo.md` in Obsidian/an external editor while the app is running →
  the note updates in-app without a restart, and search finds the new text.
- [ ] Create a session, record, enhance, add action items, then verify every
  artifact is a readable file under `sessions/<id>/`.
- [ ] Delete an enhanced doc and a transcript → confirm the prior content is
  recoverable from `.trash/<date>/`.
- [ ] `fmtr --vault-path <vault> doctor` and `fmtr meetings list` against the
  real vault (the CLI no longer takes `--db-path`).

### Scope decision needing sign-off

- [ ] **`crates/mobile-bridge` was deleted** (see the H scope call above). Confirm
  mobile is not a surface you intend to revive, or say so and I'll restore it
  from git history.

## Phase D task split (scouted 2026-07-26, no-exodus variant)

- **D-1 meta fields**: SessionMeta += event (Option<Value>), folder; wire tags. Widen content.rs:32 + rebuild.rs:326 upserts to event_json/folder_path (DUAL-WRITE REQUIRED — timeline/useSession/useKeywords/welcome-note/search read SQL until E). New `session_update_meta` command; retarget updateSession (session/queries.ts:376-410), createSession event seed (:446), welcome-note meeting_link clear (welcome-note.ts:62). BONUS FIX: title edits are SQL-only today (only createSession calls sessionWriteMeta) → rebuild reverts titles to stale file title; D-1 closes this. owner_user_id: not ported — meeting-float + shared/owner-user.ts use constant DEFAULT_USER_ID come Phase E (D10).
- **D-2 tasks.json**: sessions/<id>/tasks.json + vault-level file for non-session sources; task-storage.ts (raw subscribe :119, 16-col upsert :299-356) is the SOLE consumer; action_items feeds no search trigger → CUT STRAIGHT TO FILES, no dual-write. CLI action-items read ports in D-5.
- **D-3 enhanced docs**: deterministic per-doc files + doc metadata sidecar (document files are frontmatter-free by design — mod.rs:136-176 strips); retarget enhancer/storage.ts (ensureSummaryDocument :11-72, expectedRowsAffected :36/:88, title CAS :143), updateEnhancedNoteContent/deleteEnhancedNote (queries.ts:322/:358); DELETE the enhance-success.ts:156-179 shadow-row tombstone hack; deletion = file removal. DUAL-WRITE REQUIRED (session_documents read by FE + search).
- **D-4 templates**: vault templates/<id>.json; store commands list/get/upsert/delete; re-seed 17 bundled defaults when templates/ missing (replaces repair_missing_core_tables seeding); port templates/queries.ts off drizzle (kills BOTH drizzle live sites + useDrizzleLiveQuery in db/index.ts:8) → CUT STRAIGHT TO FILES, tanstack-query invalidation until E.
- **D-5 CLI retarget (LAST — after D-1/2/3 formats freeze)**: fmtr reads vault files not app.db; --base becomes vault dir, --db-path replaced by --vault-path (D8); update agents-content.md, docs/reference/cli.mdx, skills/fmtr, insta cli_contract snapshot (doc-parity tests apps/cli/src/cli.rs:190-264 enforce all three). Reads live in crates/db-app/src/session_ops.rs (NOT agent-access; agent-access/src/lib.rs wraps).
- Search projection stays SQL-fed (9 triggers + drain) through ALL of Phase D — sessions/session_documents/transcripts upserts must survive until Phase F.
- Phase E porting list = 14 subscription sites (16 today − 2 settings dying in C3): session/queries.ts:134,186,204,225,248,277; sidebar/timeline/queries.ts:15; meeting-float/hooks.ts:35 (raw); shared/owner-user.ts:17; stt/queries.ts:76,94; task-storage.ts:119 (raw, dies in D-2); templates/queries.ts:125,142 (die in D-4). ~24 non-reactive execute sites (CAS at content-mutations.ts:102/163, enhancer/storage.ts:36/88/143); transcript supersede stt/queries.ts:150-157 (E-6).

## Follow-ups / deviations log

**Spot-verify drift notes (2026-07-26, repo @ 86d5be9):**
- Settings schema is `apps/desktop/src/settings/schema.ts` (plan said `components/settings/`).
- Detect mocks live inside `useStartListening.test.ts` (lines ~32/58/79), NOT `test-setup.ts`; test-setup only mocks plugin-db `getMeeting`/`getMeetingTranscript`/`listMeetings` (@63–65, dies in Phase B).
- `meeting_ax.rs` (exactly 5,408 lines) has 3 `cfg(not(macos))` stubs (lines 295/429/1153); `mod meeting_ax;` is ungated; non-macOS inspect command stub at `plugins/detect/src/commands.rs:77–84` also dies in A3.
- Phase B extras: update table-list assertion test `crates/db-app/src/lib.rs:~399–417` when dropping the 4 dead tables; remove unused root `Cargo.toml:64` alias `hypr-db-cli`.
- `pnpm -F desktop typecheck` valid (scope-omission match for `@hypr/desktop`); test runner vitest 4.
- D-2 note: container restart mid-task killed the bindings regen; controller hand-applied bindings (verified identical to regen output), then committed the generated prettier layout for tauri.gen.ts once (994d67c) so future cargo-test regens are diff-free. tasks.json shape: TaskItem {id, source_type ("session_raw_note"|"enhanced_note"), source_id, source_order, status, text, body (real JSON), due_at, assignee, created_at, updated_at}; unknown source types → vault-root tasks.json. Baselines now: FE 1149 tests/168 files; cargo test -p desktop --lib green except 1 known environmental failure (chmod-as-root).

**Phase C scout findings (2026-07-26):**
- 35 schema keys, not 36; `todo_linear_filter`/`todo_github_repository` dead (schema.ts:161/166, zero other refs) → **33 live keys** migrate.
- Split-brain is TOTAL: no FE path ever calls `settingsCommands.save()`; appearance.rs:18–45, detect env.rs:44–55, hooks config.rs:7–19 read a frozen pre-migration `settings.json`. C1's reader switch fixes it (C4 folds into C1).
- `hooks` config key is NOT in schema.ts — config.json must carry it explicitly.
- Onboarding reset call: `apps/desktop/src-tauri/src/lib.rs:413` (plugin `reset()` is ext-only, not a command; plan cited wrong file).
- Legacy chain confirmed: parseSettingRows queries.ts:224–257, quirks 275–351 (audio_retention 5 sources @287–299, normalize in services/audio-retention-policy.ts:18–34), snapshots legacy-snapshots.ts:6–56, TinybaseValues store.rs:8 + commands.rs:79 + ext.rs:66–70 + lib.rs:563.
- Second write path: `syncLocalSttServer` raw UPSERT queries.ts:450–461 bypasses side-effects — must reroute to set_config_values.
- providers.ts: rows `ai_provider:<type>:<id>` in app_settings; optimistic UPDATE retry loop :108–181; keychain scope "ai-provider-api-keys" :19; redact + secure_delete :398–460.
- Vault-move manifest: `crates/storage/src/vault/fs.rs:6–23` — add `config.json` to VAULT_FILES or copy/move loses it.
- `atomic_write` fs.rs:6 / `atomic_write_async` fs.rs:19 confirmed.
- vault_watch only emits Refresh for `sessions/<id>/…`; config.json falls through to Ignore — no watcher change needed (use tmp-prefixed atomic writes).
- Live subscription sites: 14 total (12 useLiveQuery + 2 raw subscribe), of which exactly 2 die in C: settings/queries.ts:55, settings/providers.ts:35.
- No Rust reader of the `app_settings` TABLE (only the file readers above).
- `set_config_values` must not echo `config-changed` to the writing window (or FE dedupes) to avoid render loops; plugins/settings save() merge is shallow — config service needs section-aware merge if keeping `{section:{key}}` shape.
