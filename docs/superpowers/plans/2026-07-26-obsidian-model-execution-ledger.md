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
| A — meeting-chat total drop | in progress (A1+A2) | — | macOS verification owner-side |
| B — dead DB weight | pending | — | |
| C — config.json | pending | — | |
| D — file homes + exodus | pending | — | |
| E — reactivity core | pending | — | |
| F — search on files | pending | — | |
| G — FE/db decoupling | pending | — | |
| H — SQLite deletion | pending | — | gated on owner-machine exodus verify |

## Owner-machine checklist (accumulating)

Items that require macOS and/or the real vault; commands to be filled in as phases land.

- [ ] Phase A: `cargo test -p detect -p tauri-plugin-detect -p template-app -p tauri-plugin-template` on macOS (meeting_ax deletion is cfg(macos); Linux gives no signal).
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
