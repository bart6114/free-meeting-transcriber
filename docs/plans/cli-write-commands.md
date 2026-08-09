# Plan: shared vault write path + CLI write commands

Status: proposed (2026-08-09, revised after codebase audit)

## Goal

Let the `fmtr` CLI create and modify vault data — new notes/memos, audio imports,
and headless transcription — by reusing the desktop app's write path instead of
duplicating it. Today the CLI is strictly read-only (`doctor`, `meetings
list/search/get/note/transcript/export`, `mcp`); all writes live in
`apps/desktop/src-tauri/src/session_store/`.

Summary generation ("enhance") is explicitly **out of scope** — see Non-goals for
why.

## Why extraction is cheap (audited)

An audit of `session_store` confirms the Tauri coupling is confined to the edges:

- Only `commands.rs` and `index.rs` reference `tauri` at all. `commands.rs`
  (~418 lines) is pure IPC glue — every command is a thin delegation to a store
  method — and stays in the desktop app.
- `index.rs` has exactly two Tauri touchpoints: the `tauri_specta::Event` derive on
  `IndexChanged` (index.rs:49) and `spawn_dispatcher(app: tauri::AppHandle)`
  (index.rs:664), already a thin wrapper over the callback-based
  `run_index_change_dispatcher` (index.rs:645).
- `specta::Type` derives appear across most core modules (content, enhanced, tasks,
  templates, people, transcript, index) — but this is a non-issue:
  `hypr-vault-read` already depends on `specta` unconditionally, so the new crate
  does the same. Only the `tauri_specta::Event` derive needs gating.
- Remaining external deps are just `hypr-vault-read`, `hypr-fs-format`, `serde`,
  `tokio`, `sha2`, and std. No `use crate::` references outside the module tree.
- `SessionStore::new()` takes only a vault path (mod.rs:79).

Concurrent access is already handled by design: files are the source of truth, the
desktop rescans on external file changes (`vault_watch.rs` — its tests explicitly
cover writes that bypass the store, "an external editor/sync client would too"),
and writes go through atomic replace plus compare-and-swap guards
(`StoreError::Conflict`). A CLI write is indistinguishable from any external edit.
Note the store's `write_lock` is in-process only — cross-process safety rests on
atomic writes + CAS, not on the mutex.

## Phase 1 — extract `crates/vault-write`

Create a workspace crate `crates/vault-write/` (package `hypr-vault-write`), the write
sibling of `crates/vault-read/`.

1. Move `session_store/` module tree into the crate, minus `commands.rs`.
2. Depend on `specta` unconditionally (matching `vault-read`); put only the
   `tauri_specta::Event` derive on `IndexChanged` behind a `tauri-events` cargo
   feature (`cfg_attr`), enabled only by the desktop. Fallback if feature-gating
   fights the derive macro: desktop-side newtype wrapper carrying the derive.
3. `spawn_dispatcher` moves to the desktop app (it is desktop-specific glue over
   `run_index_change_dispatcher`, which stays in the crate).
4. Desktop `src-tauri` keeps a `session_store` module that re-exports the crate and
   declares `commands.rs`, so `commands.rs`, `vault_watch.rs`, `search_index.rs`,
   `recording_meta.rs`, `lib.rs` keep compiling with minimal churn.
5. Verify: `cargo check`, existing `session_store` tests run from the new crate,
   desktop builds, no behavior change.

Phase 1 is a pure refactor and ships on its own.

## Phase 2 — CLI: create notes/memos

There is no `create_session` in the store — the desktop frontend generates the id
and calls `session_write_meta`. The CLI mirrors that: generate an id (must pass
`validate_session_id` — non-empty path segment, no leading dot, no `/`), build a
full `SessionMeta` (`id`, `title`, `created_at` = now, empty `tags`), then
`write_meta` + `write_note`.

Commands, on top of `SessionStore` (the CLI already has a multi-threaded tokio
runtime, so the async store API is fine):

- `fmtr meetings new --title <t> [--note <file|->]` — create a session and write the
  note body (stdin or file).
- `fmtr meetings note <id> --set <file|->` (or `--append`) — edit an existing note.

Follow the existing CLI architecture (`apps/cli/src/commands/`, lightest structure
that fits — these are one-shot commands, no reducer/effect split needed). Respect
`--json` for machine-readable results (print the new session id).

Contract upkeep (enforced by tests in `apps/cli/src/cli.rs` — the docs-coverage
check and the `cli_contract` insta snapshot): every new command and flag must be
added to `docs/reference/cli.mdx` and `skills/fmtr/references/cli.md`, and the
snapshot regenerated.

MCP stays read-only (it advertises itself as read-only); revisit separately if
write tools are ever wanted there.

## Phase 3 — CLI: audio import + headless transcription

### 3.1 Import — `fmtr import <audio-file> [--title <t>]`

Create a session (as in Phase 2), then land the audio. Two constraints found in the
audit shape this:

- `store_audio` hard-rejects any extension other than `.mp3`/`.wav`/`.ogg`
  (session_store/audio.rs) and does move-or-copy only — no conversion.
- The desktop import path converts first: `hypr_audio_norm::normalize_file`
  re-encodes to 16 kHz MP3 (rodio/symphonia decode, `afconvert` fallback on macOS;
  accepts wav/mp3/ogg/mp4/m4a/flac/webm/aac). See
  `crates/fs-sync-core/src/audio/mod.rs` `import_to_session`.

So the CLI import is: normalize via `hypr-audio-norm` (tauri-free) into
`sessions/<id>/audio.mp3`, matching the desktop byte-for-byte. A running desktop
picks the session up via `vault_watch`.

### 3.2 Transcribe — `fmtr import --transcribe` / `fmtr transcribe <id>`

The batch engine is already factored for headless use: `crates/listener2-core`'s
`run_batch(Arc<dyn BatchRuntime>, BatchParams) -> BatchRunOutput` has no Tauri
dependency (only an optional `tauri-specta` event derive). The desktop's Tauri
layer discards the return value and forwards events to the frontend; the CLI
instead implements `BatchRuntime::emit` as a progress printer and consumes the
returned response directly.

Work items:

1. **Provider/model resolution.** Read `current_stt_provider` /
   `current_stt_model` (flat keys in vault-root `config.json`;
   `hypr_storage::vault::compute_config_path` is tauri-free — the `AppConfig`
   struct lives in `plugins/settings`, so either extract or duplicate the few
   fields the CLI needs). The desktop today only offers on-device `soniqo`
   (CoreML via Swift bridge, macOS/aarch64 only — fine, the CLI targets the same
   machine as the app). Models live in the Swift-side cache, not the vault.
2. **Response → transcript mapping.** The `batch::Response` →
   `TranscriptWord`/`TranscriptSpeakerHint` mapping (word ids, speaker hints,
   `provider_speaker_index`) currently lives only in TS
   (`apps/desktop/src/stt/useRunBatch.ts`). Port it to Rust — a helper beside
   `listener2-core/src/batch/accumulator.rs` or in `vault-write` — then persist
   via the store's `replace_session_transcripts`.
3. **Clean failures** (explicit requirement): no STT provider/model configured,
   model not yet downloaded (tell the user to open the desktop app once), or
   unsupported platform → clear error message, non-zero exit. Do not attempt
   model downloads in v1.
4. Binary size: this pulls the STT stack (incl. the Swift/CoreML bridge) into
   `fmtr`. Accept it, or feature-gate transcription if it becomes a problem.

### Round trip

`fmtr import meeting.m4a --transcribe` → normalized audio + transcript in the
vault → running desktop indexes it via `vault_watch` → user opens the session and
triggers a summary in the app.

## Non-goals

- **Summary generation ("enhance") from the CLI.** Audited and deliberately cut:
  the entire enhance orchestration — prompt assembly, the streaming LLM call,
  output validation/retry, length policy, title generation, tag extraction —
  lives in the TypeScript frontend (`apps/desktop/src/services/enhancer/`,
  `.../ai-task/task-configs/enhance-*.ts`). Only the prompt templates
  (`crates/template-app`, tauri-free) and the enhanced-doc write path are Rust.
  A headless `fmtr enhance` would mean building a Rust LLM client layer
  (provider config + keychain API keys + HTTP/streaming) and porting all the TS
  post-processing — a separate project if ever wanted. Consequence: CLI-imported
  sessions do not auto-summarize (auto-enhance triggers are in-app TS); the user
  triggers the summary from the app.
- Live recording from the CLI.
- MCP write tools.
- Moving `commands.rs` (Tauri IPC) or any UI-facing logic out of the desktop app.
- Changing the vault format.

## Verification per phase

- `cargo check` and crate tests after Rust changes; `pnpm -r typecheck` where the
  desktop frontend is touched (Phases 1–3 should not touch it).
- `pnpm exec dprint fmt` after edits.
- Phase 2/3: CLI contract snapshot + docs/skill coverage tests, plus a manual
  round-trip — create/import via CLI, confirm the running desktop app indexes the
  new session (`desktop-dev-loop` skill). For 3.2, transcribe a real `.m4a` and
  compare the transcript file shape against one produced by the desktop.
