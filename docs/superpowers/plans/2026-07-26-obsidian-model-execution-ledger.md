# Execution ledger — Obsidian-model plan (meeting-chat drop, config.json, SQLite removal)

**Plan:** `docs/superpowers/plans/2026-07-26-meeting-chat-drop-config-file-sqlite-removal.md`
**Branch:** `refactor/obsidian-model-no-db` (from `main` @ `86d5be9`)
**Mode:** controller + subagent implementers; controller commits after each accepted task.
**Confirmed decisions:** D1 (delete `meeting_ax.rs` wholesale, incl. caller-less
`inspect_meeting_accessibility`), D8 (CLI `--db-path` → `--vault-path`).

**Hard rules in force:**
- Never edit shipped migrations.
- Phase B `drop_dead_tables` is the LAST new SQL migration.
- Phase D-6 exodus (marker `.files-canonical-v1`) must land + verify before any Phase H SQLite infra deletion.
- Do not touch `telemetry_consent` while removing `consent_auto_send_chat`.

## Phase status

| Phase | Status | Commits | Notes |
|-------|--------|---------|-------|
| Plan spot-verify | done | — | claims hold; drift notes below |
| A — meeting-chat total drop | done (Linux gates green) | fa11943, 9513f62, c01de0b | awaiting owner macOS verify |
| B — dead DB weight | in progress | — | |
| C — config.json | scouted | — | ground truth verified; task split C1→C2→C3 strict |
| D — file homes + exodus | pending | — | |
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
