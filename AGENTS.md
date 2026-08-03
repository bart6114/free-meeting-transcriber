# Overview

Tauri desktop note-taking app (`apps/desktop/`) and a CLI (`apps/cli/`).
Uses pnpm workspaces.
Files in the user's vault directory are the only source of truth — there is no database. The vault format lives in `crates/vault-read/`; the desktop write path and in-memory index are `apps/desktop/src-tauri/src/session_store/`. App settings are a `config.json` in the vault, not rows. Zustand is used for UI state, and TipTap powers the editor. Sessions are the core entity — all notes are backed by sessions, stored under `sessions/<id>/`.

## Commands

- Format: `pnpm exec dprint fmt`
- Typecheck (TS): `pnpm -r typecheck`
- Typecheck (Rust): `cargo check`
- Desktop dev: `pnpm -F @hypr/desktop tauri:dev`
- Dev docs: `docs/` (Mintlify project source; not currently published)

## Guidelines

- Format via dprint after making changes.
- JavaScript/TypeScript formatting runs through `oxfmt` via dprint's exec plugin.
- Run `pnpm -r typecheck` after TypeScript changes, `cargo check` after Rust changes.
- After editing files, run the relevant verification commands before finishing.
- For `apps/desktop/` TypeScript changes, prefer `pnpm -F desktop typecheck` to match CI.
- After edits, run `pnpm exec dprint fmt`.
- Use `useForm` (tanstack-form) and `useQuery`/`useMutation` (tanstack-query) for form/mutation state. Avoid manual state management (e.g. `setError`).
- Keep file I/O, atomic writes, and index maintenance on the Rust side. TypeScript reads through the typed store commands and subscribes to changes via `useIndexQuery` (`src/shared/index-query.ts`), which fans out the coalesced `index-changed` event — never read or write vault files directly from the frontend.
- Branch naming: `fix/`, `chore/`, `refactor/` prefixes.

## Releases & Versioning

- Every push to `main` auto-releases: `.github/workflows/release.yaml` bumps the version, commits `chore(release): vX.Y.Z [skip ci]`, tags, and creates a GitHub release with generated notes. No binary is built at this point.
- Bump size comes from conventional-commit keywords across the commits since the last `v*` tag (largest wins): `feat!:`/`BREAKING CHANGE:` → major, `feat:` → minor, everything else → patch.
- The version's source of truth is the root `package.json`; `apps/desktop/package.json` is kept in sync by the workflow, and `tauri.conf.json` reads it (`"version": "../package.json"`) — never bump versions by hand.
- Signed + notarized stable DMGs are built on demand: run the `desktop-release` workflow (`gh workflow run desktop-release`, optional `tag` input, defaults to the latest release) and it attaches the DMG + sha256 to that release.
- That same workflow also builds the in-app updater artifact (`.app.tar.gz` + minisign `.sig`, signed with the `TAURI_SIGNING_PRIVATE_KEY` repo secret) and refreshes `latest.json` on the rolling `updater` prerelease — the endpoint stable builds poll (`tauri.conf.stable.json`). Never delete the `updater` release; the feed only moves forward (version-guarded against rebuilding old tags).
- `desktop_build.yaml` is the separate staging lane: unversioned DMG artifact on every push to `main`.
- Each push leaves a bot `chore(release)` commit on `main`, so local `main` is behind after every push — `git pull --rebase origin main` before pushing.

## Code Style

- Avoid creating types/interfaces unless shared. Inline function props.
- Do not write comments unless code is non-obvious. Comments should explain "why", not "what".
- Use `cn` from `@hypr/utils` for conditional classNames. Always pass an array, split by logical grouping.
- Use `motion/react` instead of `framer-motion`.

## CLI TUI Command Architecture

Choose the lightest command structure that fits the workflow.

Use the full reducer/effect/runtime split only when the command has async orchestration, a multi-step workflow, or substantial state transitions that benefit from reducer-style tests.

```
commands/<name>/
  mod.rs        -- Screen impl, Args, run()          [glue]
  app.rs        -- App or screen-local state          [optional]
  action.rs     -- Action enum                        [optional]
  effect.rs     -- Effect enum                        [optional]
  runtime.rs    -- Runtime, RuntimeEvent              [async I/O]
  ui.rs         -- draw(frame, app)                   [rendering]
```

Naming rules:

- Types drop the command prefix: `App`, `Action`, `Effect`, `Runtime`, `RuntimeEvent`
- `app.rs` → `app/mod.rs` with private submodules when state is complex
- `ui.rs` → `ui/mod.rs` with sub-files when rendering is complex
- `action.rs`/`effect.rs` are siblings of `mod.rs` when they exist; do not create them by default for simple list/detail screens
- `app.rs` contains no rendering logic, no API calls, no async code when using the reducer pattern
- Prefer `screen.rs` plus a small local state struct for simple browse/select flows
- Do not add parent-level action/effect translation layers that proxy child workflows through another command's reducer

## Misc

- Do not create summary docs or example code files unless requested.
