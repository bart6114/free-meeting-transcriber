# Meeting-chat total drop, filesystem config, SQLite removal — implementation plan

**Date:** 2026-07-26
**Branch this plan lives on:** continuation of the filesystem-first work (Tasks 1–14 complete through commit `91b9775`).
**Owner directives (2026-07-26):**
1. Drop meeting chat **completely** — including the recording-disclosure consent feature that survived Task 13a.
2. Move all app configuration out of SQLite into a filesystem config file.
3. Then delete SQLite entirely — the goal state is an app with **no database**: files are the only store.

**Status: PLAN — not started.** Implementation is expected to run with
subagent-driven development (fresh implementer per task, per-task review,
controller verification), same as Tasks 1–14.

This plan was produced from three exhaustive read-only investigations of the
repo at commit `91b9775` (meeting-chat/consent inventory; config-storage map;
full SQLite surface inventory). File:line references below are ground truth at
that commit.

---

## 0. Reality check (read before estimating)

Moving config to a file does **not** by itself make SQLite unnecessary —
`app_settings` is the *smallest* live consumer. What actually keeps SQLite
alive:

1. **The live-query reactivity layer.** 16 FE subscription sites run SQL
   through `plugin:db subscribe`; change detection is a sqlx
   update/commit-hook bus (`crates/db-change`), dependency analysis is
   `EXPLAIN QUERY PLAN` scraping (`crates/db-reactive`). Multi-window settings
   and session UI reactivity all ride on this. Replacing it is the dominant
   cost of the whole effort.
2. **The search projection pipeline.** `search_index.rs` drains a
   trigger-fed `search_index_dirty` table into Tantivy. Tantivy itself is
   already SQLite-free; only the projection worker is coupled.
3. **Data that exists ONLY in the DB today** (no file home): templates,
   action items, multi-summary/`template_output` documents,
   `sessions.event_json`/`folder_path`, tags. A rebuild-from-empty already
   silently loses several of these — the current "files are source of truth"
   claim has real gaps that this project closes.

Honest sizing from the inventory: **~8–10 weeks of focused solo work** across
all phases. Phases A–D are days each; Phase E (reactivity) is the ~2–3 week
core; F–H hang off it. Each phase lands independently with green gates, so
the work can pause at any phase boundary with a healthy app.

---

## 1. Decisions

### Made by this plan (implementer follows these)

- **D1 — `meeting_ax.rs` dies wholesale, including `inspect_meeting_accessibility`.**
  The inspection command has **zero** frontend callers (the generated binding
  `plugins/detect/js/bindings.gen.ts:57-63` has no call sites; only the
  registered command and a debug example consume it). Everything shared
  inside `meeting_ax.rs` exists solely to serve inspection. Dropping it turns
  a ~68-function surgical extraction into a clean 5,408-line file delete and
  eliminates the whole macOS-verification risk class for this phase.
  *If the owner wants to keep AX inspection for future features, say so
  before Phase A starts — it changes the shape of task A3 completely.*
- **D2 — config file:** a new flat-keyed **`config.json` at the vault root**,
  written atomically (`hypr_storage::fs::atomic_write`), owned by **Rust**
  (single writer). `global.json` in app-data stays exactly as-is (it is the
  vault *pointer*; a config file inside the vault cannot locate the vault).
  Secrets (AI provider API keys) **stay in the OS keychain** — the vault is
  designed to be cloud-synced; keys must never enter it. The legacy
  `settings.json` is absorbed and deleted (its `hooks` section moves into
  `config.json` as a `hooks` key).
- **D3 — config reactivity:** a `config-changed` Tauri event broadcast to all
  webviews on every write, plus a `get_config` command; FE consumes via a
  `useConfig()` hook exposing the same `{data, isLoading, error}` contract
  the live query gave `SettingsHydrationBoundary`/`useSettingsReady`.
- **D4 — action items** get a per-session `tasks.json`
  (`sessions/<id>/tasks.json`), written through the session store like other
  session content. Full fidelity (`status`, `due_at`, `assignee`,
  `source_order` survive); no lossy markdown round-tripping.
- **D5 — enhanced notes/summaries** get per-document files:
  `sessions/<id>/enhanced/<doc-id>.md` with a small YAML frontmatter
  (`title`, `template_id`, `sort_order`, `kind`, `deleted_at`). This closes
  the known gap where only a single `summary.md` slot exists and
  UUID-id `summary`/`template_output` rows are index-only
  (see `enhance-success.ts:138-178`'s own comment).
- **D6 — templates** become files: `templates/<id>.json` at the vault root.
  The 20 seeded defaults ship as bundled assets written on first run when the
  directory is missing (replacing `20260524000000_default_templates.sql` and
  the `repair_missing_core_tables` safety net — "repair" becomes "re-seed
  missing defaults", same guarantee).
- **D7 — the index becomes in-memory.** A Rust-owned typed index built by
  scanning the vault at startup (the scanning half of `rebuild.rs` —
  `scan_session_ids`/`scan_document_files`/`session_has_content` — becomes
  the loader). **No snapshot cache in v1**: a personal-notes vault is
  thousands of sessions at most; measure cold-start first, add a cache file
  only if real numbers demand it. The store-wide write path stays file-first:
  write file → update in-memory index → notify.
- **D8 — CLI/MCP** (`apps/cli`, `crates/agent-access`) read the vault
  directly. `--db-path` is replaced by `--vault-path` (the generated
  `AGENTS.md` in `agents.rs:22` documents the flag — update it). This is the
  one user-visible contract change; the CLI keeps working with the app
  closed, same as today.
- **D9 — search dirty queue** becomes an in-memory channel fed by the same
  index-change notifications (plus the existing `vault_watch` for external
  edits). Crash recovery = the existing count-mismatch full-rebuild guard
  (`projection_consistency_snapshot`), retargeted from `COUNT(*) FROM
  sessions` to the in-memory index size. The `generation` race semantics
  (`search_index.rs:503-560` regression test) must be preserved in the new
  queue.
- **D10 — `owner_user_id` dies as a concept.** It is empty on every row today
  (store never writes it; Task 13 analysis confirmed). Single-user app:
  `shared/owner-user.ts`, `meeting-float/hooks.ts`'s owner column, and
  `session_ops`' projection drop it rather than porting it to files.
- **D11 — one-time data exodus before the DB dies** (Phase D, gated by a
  vault marker like `.store-migrated-v1` from Task 12): existing
  installations have real data in DB-only homes (templates, action items,
  UUID summaries, `event_json`, `folder_path`, tags, settings). The exodus
  writes all of it into the new file homes exactly once. **Deleting SQLite
  without this step destroys user data.**

### Deferred / explicitly out of scope

- `store.json` (store2 plugin): stays as-is. `PinnedTabs`,
  `RecentlyOpenedSessions`, `DismissedToasts`, `OnboardingNeeded2`,
  updater `LastSeenVersion` are UI/app state, not config. Only the legacy
  `TinybaseValues` key is deleted (after Phase C absorbs its migration
  fallback). Folding store2 into a `state.json` is a possible later cleanup.
- Window state plugin, keychain plugin: untouched.
- Multi-user anything: dead with D10.

---

## 2. Phase A — meeting chat & consent: total drop

Everything below is safe to land from a Linux box **except A3/A4 verification,
which requires macOS** (all of `meeting_ax.rs` is `cfg(target_os = "macos")`;
Linux `cargo check` gives no dead-code signal for it).

### A1. Frontend disclosure machinery (`apps/desktop/src/stt/useStartListening.ts`)
- Delete the contiguous block **lines 39–227**: `MEETING_DISCLOSURE_MESSAGE`,
  `MEETING_DISCLOSURE_MAX_ATTEMPTS`, `MEETING_DISCLOSURE_RETRY_INTERVAL_MS`,
  `SLACK_BUNDLE_IDS`, the three `MeetingDisclosure*` types, the
  `meetingDisclosureTasks` map, `meetingDisclosureFailure`,
  `attemptMeetingRecordingDisclosure`, `sendMeetingRecordingDisclosure`,
  `startMeetingRecordingDisclosure`, `cancelMeetingRecordingDisclosure`.
  (`getPostCaptureAction` at line 229 stays.)
- Scattered wiring: line 4 `detectCommands` import (only used in the deleted
  block); line 251 `getSessionMode` const (only consumer is line 441; the
  store method itself stays — other files use it); lines 259–261
  `useConfigValue("consent_auto_send_chat")`; line 297
  `cancelMeetingRecordingDisclosure(sessionId)` (first statement of
  `onStopped` — leave the rest); lines 438–443 the auto-send call; lines
  459 and 464 in the `useCallback` deps array. `sonnerToast` import stays
  (`.error` used at 282/307/341).
- Tests (`useStartListening.test.ts`, 1,290 lines): 14 whole test cases die
  (lines 937–1290 region). Harness surgery: mock fns
  (`getSessionModeMock`, `listMicUsingApplicationsMock`,
  `sendMeetingChatMessageMock`), the `vi.mock("@hypr/plugin-detect")` block,
  and — **highest-risk edit** — the `consent_auto_send_chat` branches inside
  `useConfigValueMock` ternaries at L269, L328, L847, L876, L905 (and in the
  dying tests): several live inside *surviving* tests; strip only that
  branch of each ternary.

### A2. `consent_auto_send_chat` setting
- `settings/schema.ts:87-91` (entry), `settings/general/index.tsx` lines 36,
  56, 91, 105, and the `form.Field` at 165–166 with its closing tag ~60 lines
  down (**9-deep nested pyramid — remove open+close in lockstep**), plus the
  `meetingDisclosureAutoPost` prop at 207–216.
- `settings/general/app-settings.tsx` lines 26, 40, 105–117 (the whole
  `SettingRow` incl. the `<Trans>` blocks that generate the two msgids).
- Dead tests: `app-settings.test.tsx:51-73` (two tests + harness lines
  15/26/30), `settings/queries.test.tsx:151-170`,
  `shared/config/index.test.tsx:33-40`.
- **Do not touch `telemetry_consent`** — adjacent in schema.ts (82–86) and in
  the form pyramid; a careless `consent` grep cuts the wrong feature.

### A3. plugins/detect + crates/detect — wholesale (per D1)
- `plugins/detect`: `send_meeting_chat_message` command
  (`commands.rs:57-75`) **and** its private helper
  `intersect_mic_active_bundle_ids` (`commands.rs:3-22`) and the whole
  `#[cfg(test)]` module (`commands.rs:165-215` — all three tests exercise
  only that helper); `inspect_meeting_accessibility` command
  (`commands.rs:51-55`); both from `lib.rs` `collect_commands!` (lines 71 and
  the inspect entry), `build.rs` `COMMANDS`, `permissions/default.toml`;
  delete `permissions/autogenerated/commands/send_meeting_chat_message.toml`
  (+ the inspect one); let build.rs regenerate reference.md/schema.json.
  Known pre-existing quirk: `build.rs` `COMMANDS` omits
  `set_included_bundle_ids` though lib.rs registers it — do not "fix" while
  editing the list.
- `crates/detect`: **delete `src/meeting_ax.rs` entirely** (5,408 lines: the
  send path, the already-dead capture path — 44 orphaned fns from `93c3f5e`
  — and the caller-less inspection path). Drop `mod meeting_ax;` +
  `pub use meeting_ax::*;` (`lib.rs:8,34`). Delete
  `examples/meeting_ax_probe.rs`. Drop the `url` dependency from
  `crates/detect/Cargo.toml` (only meeting_ax used it;
  `macos_accessibility_client` STAYS — `zoom.rs` uses it).
- Bindings: regenerate via `cargo test -p tauri-plugin-detect export_types`
  — `sendMeetingChatMessage`, `inspectMeetingAccessibility`,
  `MeetingChatSendResult`, `MeetingAccessibilityInspection`,
  `MeetingParticipantStream`, `MeetingApp`, `MeetingPlatform`,
  `MeetingSurface`, `AxRect` all leave `bindings.gen.ts`. **Watch for the
  known Linux-prettier relayout of gen files** (repo precedent: restore
  checked-in style, apply semantic edits by hand, or accept the relayout in
  one dedicated commit).
- FE `test-setup.ts` and any detect-command mocks referencing the deleted
  commands.

### A4. Dormant template-app field
- `crates/template-app/src/chat.rs:11` `meeting_chat` field; fixture at
  74–77; **the inline snapshot literal containing the `Meeting Chat:`
  section must be hand-updated in the same edit**;
  `assets/_macros.jinja:155-159` conditional block;
  regenerate `plugins/template` bindings
  (`cargo test -p tauri-plugin-template export_types`) —
  `SessionContext.meetingChat` leaves. (`crates/listener-core` has an
  unrelated same-named `SessionContext` — do not touch.)

### A5. i18n + fossils
- Two msgids die ("Post recording disclosure in meeting chat" + the long
  description). Regenerate catalogs (`lingui extract --clean && lingui
  compile`, works on Node 22) as a **separate commit** (~180 files churn).
- Optional cosmetic: the fossil negative assertion in
  `shared/main/session-status-banner.test.tsx:31`.
- Leave alone: `crates/db-app/src/lib.rs:733` test-fixture row and the
  `WHERE kind NOT IN ('key_facts','meeting_chat')` lines in the
  **already-shipped** migration `20260725120000` (never edit shipped
  migrations); changelog files; transcript test fixtures whose spoken words
  coincidentally contain "consent".

### Phase A gates
- `cargo check -p desktop -p tauri-plugin-detect -p detect -p template-app
  -p tauri-plugin-template`; `cargo test` for those packages **on macOS**
  (mandatory — Linux cannot see into the deleted cfg-gated code);
  `pnpm -F desktop typecheck && pnpm -F desktop test`; dprint.
- End-state grep (excluding docs/, .superpowers/, locales, shipped
  migrations, changelog, transcript fixtures):
  `meeting_chat|meetingChat|MeetingChat|meeting-chat|disclosure|consent_auto_send_chat|meeting_ax|inspect_meeting_accessibility`
  → zero hits. (`telemetry_consent` and editor Slack-link parsing survive by
  design.)

---

## 3. Phase B — pure subtraction (dead DB weight, do immediately)

All zero-risk deletes; keeps the app healthy while later phases proceed.

- **Migration** `drop_dead_tables.sql`: `DROP TABLE IF EXISTS chat_groups,
  chat_messages, daily_notes, entity_mentions` (all four have zero
  readers/writers — chat feature died in `93c3f5e`). Update
  `migrations_apply_cleanly`'s expected list. NOTE: this is the **last** new
  SQL migration this project should write; later phases remove data classes
  by exodus + infra deletion, not by migration.
- Delete `crates/db-cli` (zero dependents).
- Delete the three unused `plugin:db` Tauri commands `list_meetings` /
  `get_meeting` / `get_meeting_transcript` (`plugins/db/src/commands.rs:7-29`)
  + their `js/index.ts` exports + `test-setup.ts` mocks (no production FE
  caller; the real consumers go through `apps/cli`, untouched here).
- Drizzle mirror (`packages/db/src/schema.ts`): drop the four dead tables.
- `daily_notes` appears as a *fixture name* in `crates/db-reactive` tests —
  rename the fixture, don't chase it as a real dependency.

Gates: standard suite + migration data-integrity test extension.

---

## 4. Phase C — `config.json` (settings out of SQLite)

### C1. Rust config service (new: `apps/desktop/src-tauri/src/config/` or a
`plugins/config`)
- Owns `<vault>/config.json`: typed struct (serde) with the **36 live keys**
  from `settings/schema.ts` (drop dead `todo_linear_filter` /
  `todo_github_repository` — providers were removed), plus:
  `ai_providers: { "<llm|stt>:<id>": { type, base_url } }` (keys stay in
  keychain), plus the `hooks` section absorbed from legacy `settings.json`
  (its only remaining live key; `plugins/hooks/src/config.rs:7-19` retargets
  here).
- API: `get_config` command (whole doc), `set_config_values` command
  (partial update, serialized writer, atomic write via
  `hypr_storage::fs::atomic_write`), `config-changed` event broadcast to all
  webviews after every successful write. Concurrency: single Rust-side
  mutex; FE write queue (`db/write-queue.ts` pattern) is replaced by the
  Rust serialization; exit flush becomes unnecessary (writes are
  synchronous-at-commit).
- Journal the write in the session-store journal? **No** — config.json is at
  the vault root; add it to `vault_watch`'s ignore set the same way markers
  are ignored, OR let the watcher classify it `Ignore` by path (it only
  watches `sessions/**` today — verify and document).

### C2. One-time settings migration (part of the Phase D exodus, but
implemented here behind the same marker)
- Run the **exact legacy resolution chain** the FE runs today
  (`parseSettingRows`: direct `app_settings` row → `legacy_settings_document`
  → `legacy_main_values_document`, with the per-key quirks — `audio_retention`
  5 legacy shapes, `current_stt_model` normalization, comma-string→array
  coercion, `queries.ts:275-351`) once, in Rust or via a bootstrap TS step,
  writing the resolved values into `config.json`. Then delete the whole
  legacy chain: `settings/legacy-snapshots.ts`, the two snapshot rows, the
  `TinybaseValues` store2 key + `getTinybaseValues` command.

### C3. FE rewiring
- `useConfigValue`/`useConfigValues` (`shared/config/index.ts`) re-implemented
  over `get_config` + `config-changed` listen (a small module-level store +
  `useSyncExternalStore`; same `{data, isLoading, error}` contract so
  `SettingsHydrationBoundary`, `useSettingsReady`, and
  `use-settings-theme-ready` keep working).
- `useSetSettingValue`/`setSettingValues` → `set_config_values`.
  `applySettingSideEffects` (`queries.ts:387-441`) is lifted unchanged —
  except `syncLocalSttServer`'s inline SQL UPSERT (`queries.ts:452-461`)
  which becomes a `set_config_values` call.
- `settings/providers.ts`: provider CRUD moves to config keys; the
  optimistic-concurrency UPDATE and `redactPlaintextProviderApiKeys` +
  `PRAGMA secure_delete` machinery dies (keys never touch the file);
  keychain flow (`ai-provider-api-keys` scope) unchanged.
- Boot sequence (`main.tsx:124-136`) simplifies: snapshot refresh dies;
  `initializeApplicationSettings` ports to config calls;
  `bootstrapThemeFromSettings` reads config (localStorage `hypr-theme`
  remains a FOUC cache only).

### C4. Fix the split-brain (the standing bug this phase kills)
- `appearance.rs:18-44` (`show_app_in_dock`/`show_tray_icon`) and
  `plugins/detect/src/env.rs:44-55` (`notification.detect`) currently read a
  `settings.json` that **nothing has written since the vault-export triggers
  were dropped** — stale-config bug. Both retarget to the Rust config
  service (in-process read, no IPC).
- `plugins/settings` slims to: vault-path management (`global.json`,
  `copy_vault`/`move_vault`/`set_vault_base`, `StartupSnapshot`) — its
  `settings.json` load/save/reset surface dies with the file. Onboarding
  reset (`lib.rs:410-415`) resets `config.json` instead.
- Remove `settings.json` from the vault-move manifest
  (`crates/storage/src/vault/fs.rs:22` list) and add `config.json`.

### Phase C gates
- Full suites + a migration test (seed `app_settings` incl. legacy snapshot
  rows → boot path → assert `config.json` values + quirk keys resolved).
- **Real-app check (owner machine):** settings UI round-trip across two
  windows (main + settings), theme change reflected without restart, dock
  icon toggle actually applied post-restart (the split-brain regression
  test).
- After C, `app_settings` has zero readers/writers — but the table drop
  waits for Phase H (no point writing another migration).

---

## 5. Phase D — file homes + data exodus for everything else

Order matters: file homes first (D1–D5 write-through), exodus last (D6).

- **D-1. `_meta.json` grows** `event_json` (opaque envelope — welcome-note
  tracking + keyword extraction + timeline read it), `folder_path`, `tags`
  (field already exists in `SessionMeta`, hardcoded `vec![]` at
  `content.rs:318` / `rebuild.rs:541` — wire it). Writers: `updateSession`'s
  event/folder paths and `content-mutations.ts`'s tag upserts move to store
  commands (`session_update_meta`-style). The write-through keeps updating
  the SQLite rows until Phase H (dual-write during transition — same
  pattern Tasks 5–9 used in the other direction).
- **D-2. `sessions/<id>/tasks.json`** for action items (per D4): store
  read/write commands; `editor-bridge/task-storage.ts`'s 16-column upsert +
  live subscription port to store commands + index events (full reactivity
  arrives in Phase E; until then a tanstack-query invalidation on write is
  acceptable because the only writer is the same editor surface that reads).
- **D-3. `sessions/<id>/enhanced/<doc-id>.md`** (per D5) for
  `summary`/`template_output` docs: `ensureSummaryDocument`/enhance-success
  flows write files through the store; `deleted_at` tombstone becomes
  frontmatter (or file move to `enhanced/.trash/` — pick one, document it);
  single-slot `summary.md` is absorbed (existing files migrate in D-6).
- **D-4. `templates/<id>.json`** (per D6) + bundled default seeding +
  re-seed-on-missing replacing `repair_missing_core_tables`.
- **D-5. CLI/agent-access retarget** (per D8): `session_ops.rs` reads become
  vault scans (a shared `vault-read` crate that both `apps/cli` and
  `agent-access` use; sort/paginate in memory); `--db-path` → `--vault-path`;
  regenerate `agents-content.md` text (`agents.rs`).
- **D-6. Exodus migration** (per D11), marker `.files-canonical-v1` at vault
  root, runs at startup before the index build: for each data class with a
  new file home and no file yet — templates, action items, UUID summary
  docs, `event_json`/`folder_path`/tags per session — export the DB rows to
  files. Idempotent, loud on partial failure (Task 12's `MigrateReport`
  pattern: per-item errors never abort the sweep, unexported items block the
  marker only for their class). Also exports resolved settings if Phase C's
  migration hasn't already run.

### Phase D gates
- Store tests per new content class (write-through, rebuild round-trip,
  watcher refresh); exodus test seeded with every class incl. tombstoned
  docs; **owner-machine run of the exodus against the real vault with a
  pre-backup** (copy `app.db` + vault aside first — the plan's one
  non-negotiable safety step).

---

## 6. Phase E — reactivity replacement (the core)

Replace `db-change`/`db-reactive`/`plugin:db subscribe` with an in-memory
index + event bus. This is ~2–3 weeks alone; nothing in F–H can finish
without it.

- **E-1. In-memory index** (Rust, inside `session_store`): typed maps for
  sessions (id → meta summary incl. title/created_at/event_json/folder_path/
  tags), documents (session → docs incl. enhanced), transcripts (session →
  transcript summaries + has-words flag), tasks. Built at startup by the
  `rebuild.rs` scanning half (which stops writing SQL and starts populating
  the maps); updated synchronously by every store write; `vault_watch`'s
  `Refresh(id)` repoints at it. Snapshot cache: deferred per D7.
- **E-2. Change bus:** every index mutation emits a typed
  `index-changed { entity, ids }` Tauri event (coalesced ~10ms like today's
  dispatcher). Table-level granularity is enough — that is exactly what the
  SQLite hook gives today.
- **E-3. Typed query commands** replacing each subscription's SQL. The 16
  sites and their non-trivial semantics (from the inventory — port each
  deliberately, don't transliterate SQL):
  1. `useSession`'s 30-line `COALESCE(store_note, legacy_note)` join →
     an index method returning the note doc with the same fallback.
  2. `useSessionSummary` / `useSessionSummaries` → simple index reads.
  3. `useSessionHasTranscript` (`json_array_length > 0`) → has-words flag.
  4. `useEnhancedNoteRecords` / `useEnhancedNote` (kind IN + tombstone +
     sort_order ordering).
  5. `useSessionTranscripts` / `useTranscript` (tombstone-filtered).
  6. Timeline (`sidebar/timeline/queries.ts` — event_json + folder grouping).
  7. Settings/providers (already file-backed after Phase C — these two
     subscriptions die there, not here).
  8. Templates (2 drizzle sites → template file service).
  9. `task-storage.ts` per-source dynamic subscriptions → task index
     queries keyed by (source_type, source_id).
  10. `meeting-float/hooks.ts` raw subscribe → index query (owner column
      dies per D10; `shared/owner-user.ts` deleted).
- **E-4. `useIndexQuery` hook** (`packages/` or `apps/desktop/src/db/`):
  command + event-driven refetch via `useSyncExternalStore`, same dedupe
  behavior as `createUseLiveQuery`, same loading/error contract.
- **E-5. The ~30 non-reactive `execute`/`executeTransaction` sites** become
  store commands. The optimistic-concurrency semantics must be preserved
  per-site: `expectedRowsAffected` guards and `WHERE body = ?`
  compare-and-swap (enhancer/storage.ts, content-mutations.ts) map to
  store-level compare-and-swap (the store already owns a write lock; add
  `expected_hash`/`expected_updated_at` parameters and a typed conflict
  error the FE treats like the current row-count rejection).
- **E-6. Transcript supersede** (`stt/queries.ts:151` tombstone-others
  UPDATE): becomes a store primitive (`transcript_replace_session` — the
  store currently has no such primitive; Task 13 analysis called this out).

### Phase E gates
- Per-site behavioral tests (the existing FE tests largely cover these
  surfaces — they must pass unmodified where behavior is unchanged);
  multi-window reactivity check on the owner machine (edit title in main →
  float window updates; settings already covered by C).

---

## 7. Phase F — search projection on files

- `search_index.rs` rewires: dirty source = index change bus (E-2) +
  `vault_watch` external refreshes; queue = in-memory (dedup by entity id,
  `generation` counter preserved — port the `search_index.rs:503-560`
  regression test); content source = the in-memory index / files (same
  flatten logic, `PROJECTION_VERSION` bump to force one rebuild);
  consistency guard compares Tantivy doc count to index size; crash
  recovery = that guard (documented tradeoff per D9).
- `search_index_dirty`/`search_index_state` tables + the 9 triggers die with
  the DB in Phase H (the worker just stops reading them in F).

Gates: search e2e (create session → searchable; edit → reindexed; delete →
gone; kill-during-index → recovered by count guard on next boot).

---

## 8. Phase G — final FE/db decoupling sweep

- Kill remaining `@hypr/plugin-db` imports; delete `packages/db`,
  `packages/db-react`, `packages/db-tauri`, `packages/db-runtime`,
  `drizzle-orm` dependency, `apps/desktop/src/db/` (write-queue + client).
- `test-setup.ts` DB mocks die; FE tests get index-command mocks instead.

---

## 9. Phase H — delete SQLite infra

Only after E–G are green on the owner's machine:

- Delete crates: `db-core` (+ its `cloudsync/` remnant), `db-change`,
  `db-execute`, `db-migrate`, `db-reactive`, `db-app` (33 migrations die
  with it — history lives in git), `plugins/db` entirely.
- `sqlx` leaves every `Cargo.toml` (12 today; verify none remain).
- `apps/desktop/src-tauri/src/db.rs` (pool setup) dies; `lib.rs` setup drops
  `prepare_schema`/pool wiring; `session_store` loses its `pool` field
  (`mod.rs:22`) and every SQL statement (content.rs lines 32/92/144/185,
  transcript.rs:289, rebuild.rs's upsert half — already repurposed in E-1);
  `vault_watch.rs`'s `is_app_db_path` ignore rule dies.
- Startup deletes/ignores the orphaned `app.db*` files? **Leave them on
  disk** (cheap, reversible); log once. A later release can clean up.
- `migrate.rs` (words_json repair) is obsolete once transcripts are
  file-only — verify the repair already ran (marker) and delete the module;
  keep the marker files themselves ignored by the watcher.
- End-state greps (empty outside docs/.superpowers/git history):
  `sqlx|SqlitePool|app_settings|search_index_dirty|plugin:db|@hypr/plugin-db|drizzle|hypr_db_app|hypr-db-`
- Final whole-branch review + the full QA pass (qa-critical-ux minus
  calendar), incl. the original incident scenario (record → quit → relaunch
  → transcript present) and a cold boot timed on the owner's real vault
  (D7's measurement).

---

## 10. Sequencing & landability

```
A (meeting-chat drop)          — independent, land first, needs macOS verify
B (dead-weight subtraction)    — independent, land any time
C (config.json)                — independent of A/B; kills 2 of 16 live queries
D (file homes + exodus)        — after C (exodus includes settings); D-5 CLI
                                 retarget can trail
E (reactivity core)            — after D (needs the file homes to query)
F (search)                     — after E
G (FE decoupling)              — after E/F
H (SQLite deletion)            — last, after real-vault verification
```

Each phase = one or more subagent tasks with the Task 1–14 discipline:
fresh implementer, controller review, real-app verification where mandated.
Phases A–C are safe to run concurrently on separate worktrees if desired;
D onward is strictly sequential.

## 11. Risks

1. **Data loss at the exodus boundary (Phase D-6/H).** Mitigation: marker
   gating, per-class loud failure, mandatory pre-backup of `app.db` + vault
   on the owner machine, and H only proceeds when the exodus report is
   clean.
2. **Cold-start scan cost with no persistent index** (D7). Mitigation:
   measure on the real vault at Phase E; snapshot cache is a contained
   follow-up if needed.
3. **Reactivity regressions across webviews** (E). Mitigation: keep
   table-level granularity (no cleverness), port the 16 sites one by one
   with their existing FE tests as the contract.
4. **macOS-only code paths** (A3, and any AX regressions). Mitigation: A
   lands only with a macOS `cargo test -p detect -p tauri-plugin-detect`
   run in evidence.
5. **CLI contract change** (`--db-path` → `--vault-path`, D-5). User-visible;
   release-note it. The generated vault `AGENTS.md` regenerates itself at
   startup.
6. **Concurrent-writer semantics on files** (E-5). The store's write lock +
   compare-and-swap parameters replace `BEGIN IMMEDIATE` +
   `expectedRowsAffected`; every ported site needs its conflict test.

## 12. What the next agent should do first

1. Read this plan top to bottom, then the three referenced investigations'
   subjects in the repo itself (spot-verify a handful of file:line claims —
   the codebase moves).
2. Confirm D1 (inspection drop) and D8 (CLI flag change) with the owner if
   any doubt remains — both were inferred from "drop meeting chat
   completely" + "delete that infra", not stated verbatim.
3. Start Phase A as subagent tasks (A1+A2 one implementer, A3+A4 a second,
   A5 controller), with the macOS verification requirement stated in every
   Rust-touching brief.
