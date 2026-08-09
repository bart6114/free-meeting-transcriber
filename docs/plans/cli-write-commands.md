# Plan: shared vault write path + CLI write commands

Status: proposed (2026-08-09)

## Goal

Let the `fmtr` CLI create and modify vault data — new notes/memos, audio imports —
by reusing the desktop app's write path instead of duplicating it. Today the CLI is
strictly read-only (`doctor`, `meetings list/search/get/note/transcript/export`, `mcp`);
all writes live in `apps/desktop/src-tauri/src/session_store/`.

## Why extraction is cheap

An audit of `session_store` shows the Tauri coupling is confined to the edges:

- Core modules (`content.rs`, `audio.rs`, `transcript.rs`, `enhanced.rs`, `tasks.rs`,
  `people.rs`, `templates.rs`, `journal.rs`, `paths.rs`, `rebuild.rs`) have **zero**
  Tauri references. External deps are only `hypr-vault-read`, `hypr-fs-format`,
  `serde`, `tokio`, and std.
- `commands.rs` holds all the Tauri IPC glue (~116 references) — it stays in the
  desktop app either way.
- `index.rs` touches Tauri twice: the `specta::Type + tauri_specta::Event` derive on
  `IndexChanged`, and `spawn_dispatcher(app: tauri::AppHandle)`, which is already a
  thin wrapper over the callback-based `run_index_change_dispatcher`.
- `SessionStore::new()` takes only a vault path.

Concurrent access is already handled by design: files are the source of truth, the
desktop rescans on external file changes (`vault_watch.rs`), and writes go through
atomic replace plus compare-and-swap guards (`StoreError::Conflict`). A CLI write is
indistinguishable from any external edit. Note the store's `write_lock` is in-process
only — cross-process safety rests on atomic writes + CAS, not on the mutex.

## Phase 1 — extract `crates/vault-write`

Create a workspace crate `crates/vault-write/` (package `hypr-vault-write`), the write
sibling of `crates/vault-read/`.

1. Move `session_store/` module tree into the crate, minus `commands.rs`.
2. `IndexChanged` event derive: keep the struct in the crate and put the
   `specta::Type` / `tauri_specta::Event` derives behind a `tauri-events` cargo
   feature (`cfg_attr`), enabled only by the desktop. Fallback if feature-gating
   fights the derive macros: desktop-side newtype wrapper carrying the derives.
3. `spawn_dispatcher` moves to the desktop app (it is desktop-specific glue over
   `run_index_change_dispatcher`, which stays in the crate).
4. Desktop `src-tauri` re-exports the crate as `session_store` so `commands.rs`,
   `vault_watch.rs`, `search_index.rs`, etc. keep compiling with minimal churn.
5. Verify: `cargo check`, existing `session_store` tests run from the new crate,
   desktop builds, no behavior change.

Phase 1 is a pure refactor and ships on its own.

## Phase 2 — CLI: create notes/memos

Add write commands to `fmtr` on top of `SessionStore`:

- `fmtr meetings new --title <t> [--note <file|->]` — create a session and write the
  note body (stdin or file).
- `fmtr meetings note <id> --set <file|->` (or `--append`) — edit an existing note.

Follow the existing CLI architecture (`apps/cli/src/commands/`, lightest structure
that fits — these are one-shot commands, no reducer/effect split needed). Respect
`--json` for machine-readable results (print the new session id).

Contract upkeep (enforced by tests in `apps/cli/src/cli.rs`): every new command and
flag must be added to `docs/reference/cli.mdx` and `skills/fmtr/references/cli.md`,
and the `cli_contract` insta snapshot must be regenerated.

Decide and document whether the MCP server also gains write tools; default is to
keep MCP read-only for now (it advertises itself as read-only).

## Phase 3 — CLI: audio import

Two sub-steps with very different weight:

1. **Store the audio** — `fmtr import <audio-file> [--title <t>]`: create a session,
   copy/convert audio into `sessions/<id>/` via the crate's `audio.rs` path. Cheap
   once Phase 1 lands. The desktop picks the session up via `vault_watch` and can
   transcribe it there.
2. **Transcribe in the CLI (optional, later)** — wire the shared STT crates
   (`transcribe-core`, `transcribe-whisper-local`, `model-downloader`) so
   `fmtr import --transcribe` produces a transcript headlessly. Bigger integration:
   model download/location, progress reporting, feature parity questions. Split into
   its own plan when Phase 3.1 is done; do not block import on it.

## Non-goals

- Live recording from the CLI.
- Moving `commands.rs` (Tauri IPC) or any UI-facing logic out of the desktop app.
- Changing the vault format.

## Verification per phase

- `cargo check` and crate tests after Rust changes; `pnpm -r typecheck` where the
  desktop frontend is touched (Phase 1 should not touch it).
- `pnpm exec dprint fmt` after edits.
- Phase 2/3: CLI contract snapshot + docs/skill coverage tests, plus a manual
  round-trip — create via CLI, confirm the running desktop app indexes the new
  session (`desktop-dev-loop` skill).
