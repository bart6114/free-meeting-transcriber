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
| D — file homes (no exodus) | D-1, D-3 done; D-2 in progress | 966214b (D-1), aba1625 (D-3) | D-1 fixed title-revert bug; D-3 found rebuild already pruned index-only UUID rows (shadow hack papered over it) — preserved for legacy rows; enhanced docs = YAML frontmatter via hypr-frontmatter; deletion = .trash move + hard row delete (no undo path existed); store-level CAS with "conflict:" typed errors (store-errors.ts). NOTE: any `cargo test -p desktop --lib` run on Linux prettier-relayouts tauri.gen.ts — checkout + hand-apply. |
| E — reactivity core | pending | — | |
| F — search on files | pending | — | |
| G — FE/db decoupling | pending | — | |
| H — SQLite deletion | pending | — | gated on owner-machine exodus verify |

## Owner-machine checklist (accumulating)

Items that require macOS and/or the real vault; commands to be filled in as phases land.

- [ ] Phase A (ready now): on macOS, at branch `refactor/obsidian-model-no-db`:
  - `cargo check -p detect -p tauri-plugin-detect -p template-app -p tauri-plugin-template -p desktop`
  - `cargo test -p detect -p tauri-plugin-detect -p template-app -p tauri-plugin-template`
  - NOTE: the two `export_types` tests will prettier-relayout `plugins/{detect,template}/js/bindings.gen.ts` on some machines — if `git diff` shows wholesale relayout afterwards, `git checkout -- plugins/detect/js/bindings.gen.ts plugins/template/js/bindings.gen.ts` (semantic content is already correct).
  - Optional: `pnpm -F @hypr/desktop tauri:dev` smoke — start/stop a recording; no disclosure toast/setting anywhere; Settings → General shows telemetry toggle but no "Post recording disclosure" row.
- [ ] Phase C: settings UI round-trip across two windows; theme change without restart; dock-icon toggle applied post-restart.
- [ ] Phase D: exodus run against real vault WITH pre-backup of `app.db*` + vault copy.
- [ ] Phase E: multi-window reactivity check (title edit in main → float window updates).
- [ ] Phase H: Obsidian-model verification ritual (plan §9) + cold-boot timing.

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
