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
| Plan spot-verify | in progress | — | subagent checking file:line ground truth |
| A — meeting-chat total drop | pending | — | macOS verification owner-side |
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

(none yet)
