# Free Meeting Transcriber — Fork Transformation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform the fastrepl/anarlog fork into "Free Meeting Transcriber" — a fully local, account-free meeting transcriber whose canonical data lives as plain files in a user-chosen vault folder (Google Drive), with local STT models only, BYO-key LLM providers, and no cloud/subscription/telemetry code paths.

**Architecture:** Six sequential phases, each leaving the app building and runnable: (1) strip all cloud/account/billing/telemetry, (2) prune AI providers to local STT + BYO-key LLM, (3) rebrand identity (name, bundle id, CLI, strings, icons), (4) restore the user-settable storage-dir setting, (5) invert storage authority — vault files become the source of truth, SQLite (`app.db`) is demoted to a disposable, rebuildable cache that never holds data the vault doesn't, (6) personal data migration + local release build. Phase 5 deliberately does NOT delete the SQLite layer: the reactive query engine (`db-reactive`), ~31 UI query files, and tantivy search all read from SQLite, and replacing them is a many-month rewrite for zero user-visible gain. Instead, files win: every canonical write is mirrored to the vault, external file edits are imported back, and deleting `app.db` is always safe (full rebuild from vault on next launch).

**Tech Stack:** Tauri 2 (Rust + webview), React/TypeScript (Vite, TanStack Query, Lingui i18n), pnpm + Turborepo monorepo, Rust workspace (`crates/*`, `plugins/*`), SQLite via sqlx (STRICT, WAL), tantivy search, local STT (Soniqo Parakeet / Argmax / whisper.cpp on Apple Silicon).

## Global Constraints

- **Product name:** `Free Meeting Transcriber` (dev: `Free Meeting Transcriber Dev`, staging: `Free Meeting Transcriber Staging`).
- **Bundle identifiers:** `org.freemeetingtranscriber.stable`, `org.freemeetingtranscriber.dev`, `org.freemeetingtranscriber.staging`.
- **Main binary name:** `free-meeting-transcriber` (`-dev` / `-staging` variants).
- **CLI/MCP command name:** `fmtr` (`fmtr-staging`, `fmtr-dev`); managed dir `.fmtr-cli`. (NOT `fmt` — collides with `/usr/bin/fmt`.)
- **Deep-link scheme:** `freemeetingtranscriber` (replaces `hyprnote` / `char`).
- **App-data folder:** `free-meeting-transcriber`, with a read-migration ladder that still finds existing `anarlog` and `hyprnote` dirs (never orphan user data).
- **STT:** on-device models only (Soniqo Parakeet + Argmax Parakeet/Whisper). ALL cloud STT providers removed from UI and connection routing.
- **LLM:** keep the full BYO-key provider list (openrouter, openai, anthropic, google_generative_ai, azure_openai, azure_ai, mistral, cloudflare_workers_ai, ollama, lmstudio, custom). Remove ONLY the hosted `hyprnote` provider and every auth/entitlement gate.
- **No accounts, no billing, no sharing, no cloud sync, no telemetry, no auto-updater.** CloudSync/E2EE crates stay in-tree but permanently inert (config hardwired `None`) — do not attempt to excise them from `db-core`/`db-app` (§ cascade risk).
- **Storage:** vault files are canonical; `app.db` lives in the OS app-data dir (never inside the vault) and must be deletable at any time with zero data loss. Tantivy index moves OUT of the vault into app-data.
- **Vault file layout** (must round-trip with the existing importer's `classify_source`): `sessions/<uuid>/{_meta.json,_memo.md,_summary.md,transcript.json,audio.mp3,attachments/*}`, `humans/<uuid>.md`, `organizations/<uuid>.md`, `chats/<group>/messages.json`, `calendars.json`, `events.json`, `daily_notes.json`, `tasks.json`, `settings.json`.
- **Internal names stay:** `@hypr/*` JS packages, `hypr-*` crate aliases, `hyprnote_desktop_lib` — not user-visible, renaming is churn with no value.
- **Keep** the SQLite migration chain append-only (`crates/db-app/migrations/*` are never edited or deleted; new behavior = new migration files).
- **Model-download URLs on `hyprnote.s3.us-east-1.amazonaws.com` stay** (public asset host for Whisper/Parakeet weights). Mirroring them is out of scope (noted in risk register).
- **MIT compliance:** root `LICENSE` keeps the Fastrepl copyright line; add `Copyright (c) 2026 Bart Smeets` above it; README credits the upstream project.
- **Verification gates (run after every task):**
  - `cargo check -p desktop` (from repo root) — Rust compiles.
  - `pnpm --dir apps/desktop typecheck` — TS compiles.
  - `pnpm --dir apps/desktop test` — vitest suite passes.
  - Phase-end additionally: `pnpm --dir apps/desktop build` and a manual `pnpm --dir apps/desktop tauri:dev` smoke run.
- **Commit after every task** (conventional commits, one task = at least one commit). Work directly on `main` of `bart6114/free-meeting-transcriber`; tag `fork-base` before Task 1.

---

## Phase overview

| Phase | Deliverable | Ships alone? |
|---|---|---|
| 0 | Baseline recorded, `fork-base` tag | yes |
| 1 | Zero cloud: no server code, no accounts/billing/sharing/telemetry/updater; app builds + runs | yes |
| 2 | AI: local STT only; LLM list minus hosted provider | yes |
| 3 | Full rebrand (name, ids, CLI, strings, icons, docs) | yes |
| 4 | Settings → Storage "Change folder" restored | yes |
| 5 | Vault-canonical storage (mirror, rebuild, watch, authority flip, index relocation) | yes |
| 6 | Personal data migrated; local release DMG | yes |

---

### Task 0: Baseline and safety tag

**Files:**
- No source changes. Creates git tag.

- [ ] **Step 1: Record the pristine baseline**

```bash
cd /Users/bartsmeets/src/free-meeting-transcriber
pnpm install
cargo check -p desktop 2>&1 | tail -5
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test 2>&1 | tail -5
```

Expected: all green (this is upstream v1.3.7 state). Record any pre-existing failures in `docs/superpowers/plans/baseline-notes.md` — those are exempt from later gates.

- [ ] **Step 2: Tag**

```bash
git tag fork-base && git push origin fork-base
```

---

## Phase 1 — De-cloud

### Task 1: Delete server-side code and deploy configs

**Files:**
- Delete: `supabase/`, `apps/api/`, `apps/stripe/`, `apps/web/`, `crates/api-subscription/`, `crates/api-sync/`, `crates/api-auth/`, `crates/api-bot/`, `crates/observability/`, `crates/openstatus/`, `crates/loops/`, `crates/supabase-storage/`, `crates/transcribe-proxy/`, `crates/pyannote-cloud/`, `crates/llm-proxy/`, `crates/soniox/`, `crates/openai-transcription/`, `otel/`, `render.yaml`, `openstatus.yaml`, `openstatus.lock`, `doxxer.api.toml`, `doxxer.stripe.toml`, `doxxer.web.toml`, `.infisical.json`, `OBSERVABILITY.md`
- Modify: root `Cargo.toml` (workspace `members` + `[workspace.dependencies]` aliases for every deleted crate), `pnpm-workspace.yaml`, `turbo.json`, root `package.json` (scripts referencing web/stripe), `Taskfile.yaml` (`web:`, `stripe:`, `supabase*:` tasks)

- [ ] **Step 1: Verify each crate is desktop-unreachable before deleting**

```bash
cd /Users/bartsmeets/src/free-meeting-transcriber
for c in api-subscription api-sync api-auth api-bot observability openstatus loops supabase-storage transcribe-proxy pyannote-cloud llm-proxy soniox openai-transcription; do
  echo "== $c"; rg -l "hypr-$c|\"$c\"" --glob 'Cargo.toml' crates plugins apps | grep -v "crates/$c/" | grep -v apps/api || true
done
```

Expected: no hits outside `apps/api`, `apps/web`, and the crates being deleted together. If a crate IS referenced by a surviving crate (research says `soniox`/`transcribe-proxy` are api-only, but verify), keep it and note it in the commit message instead of deleting.

- [ ] **Step 2: Delete the directories and files listed above** (`git rm -r`).

- [ ] **Step 3: Fix workspace manifests** — remove deleted paths from root `Cargo.toml` `members` and every `hypr-<deleted> = { path = ... }` alias in `[workspace.dependencies]`; remove `apps/api`, `apps/web`, `apps/stripe` from `pnpm-workspace.yaml` and their pipeline entries from `turbo.json`.

- [ ] **Step 4: Gate**

```bash
cargo check -p desktop && pnpm install && pnpm --dir apps/desktop typecheck
```

Expected: PASS. Any `unresolved import hypr_...` error identifies a missed reference — fix by removing that import path (it can only be server code).

- [ ] **Step 5: Commit** — `git commit -m "chore: remove server-side apps, backend crates, and deploy configs"`

### Task 2: Remove telemetry (PostHog, feature flags, Sentry)

**Files:**
- Delete: `crates/analytics/`, `plugins/flag/`
- Modify: `plugins/analytics/src/lib.rs` (gut to no-op, keep command surface), `apps/desktop/src-tauri/src/lib.rs` (remove Sentry init blocks + `tauri_plugin_flag::init()`), `apps/desktop/src-tauri/Cargo.toml` (drop `sentry`, `tauri-plugin-sentry`, `tauri-plugin-flag`), `apps/desktop/package.json` (drop `@sentry/react`, `@hypr/plugin-flag`), `apps/desktop/src/env.ts` (drop `VITE_SENTRY_DSN`), root `Cargo.toml` (members/aliases)

**Interfaces:**
- Produces: `plugins/analytics` keeps exporting the same Tauri commands (`event`, etc.) as no-ops so the ~dozens of `analyticsCommands.*` TS call sites keep compiling untouched.

- [ ] **Step 1: Gut the analytics plugin.** In `plugins/analytics/src/lib.rs`, remove the `hypr-analytics`/PostHog client usage and the `POSTHOG_API_KEY` env read; make every command body `Ok(())` (or return empty). Remove `hypr-analytics` from `plugins/analytics/Cargo.toml`. Do NOT change the command names or the permission files.

- [ ] **Step 2: Delete `crates/analytics/` and `plugins/flag/`;** remove from root `Cargo.toml`. Find flag-plugin JS consumers: `rg -l "plugin-flag" apps/desktop/src` — replace each `flagCommands.*` read with the constant the flag defaulted to (inspect each call site; these are boolean feature gates — hardcode `true` for shipped features).

- [ ] **Step 3: Strip Sentry.** In `apps/desktop/src-tauri/src/lib.rs` delete the `sentry::init` block, `tauri_plugin_sentry::minidump::init`, `init_with_no_injection` plugin registration, and the `capture_message` startup-failure call (replace with `tracing::error!`). Drop the two deps from `apps/desktop/src-tauri/Cargo.toml`. Remove `@sentry/react` usage: `rg -l "@sentry/react" apps/desktop/src` → delete the init file + imports.

- [ ] **Step 4: Gate** (standard three commands) and smoke-run `pnpm --dir apps/desktop tauri:dev` — app must start with no panic from missing plugins.

- [ ] **Step 5: Commit** — `git commit -m "chore: remove telemetry (posthog, flags, sentry)"`

### Task 3: Remove sharing and cloud attachment upload

**Files:**
- Delete: `apps/desktop/src/session-sharing/`, `apps/desktop/src/shared-notes/`, `apps/desktop/src/sidebar/shared-notes.tsx`, `apps/desktop/src/attachment-sync/`, `plugins/attachment-sync/`
- Modify (remove imports/usages of the deleted modules): `apps/desktop/src/session/components/outer-header/index.tsx` (Share button), `apps/desktop/src/session/index.tsx`, `apps/desktop/src/session/window.ts`, `apps/desktop/src/session/components/note-input/index.tsx`, `apps/desktop/src/session/components/note-input/raw.tsx`, `apps/desktop/src/session/components/note-input/enhanced/editor.tsx`, `apps/desktop/src/session/hooks/useDeleteSession.ts`, `apps/desktop/src/sidebar/timeline/index.tsx`, `apps/desktop/src/shared/open-note-dialog.tsx`, `apps/desktop/src/shared/hooks/useDeeplinkHandler.ts`, `apps/desktop/src/shared/main/tab-content.tsx`, `apps/desktop/src/shared/desktop-tab-lifecycle.ts`, `apps/desktop/src/main/lifecycle.tsx`, `apps/desktop/src/devtools-panel/host.tsx`, `apps/desktop/src-tauri/src/lib.rs` (`tauri_plugin_attachment_sync::init()`), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/package.json` (`@hypr/plugin-attachment-sync`), root `Cargo.toml`

- [ ] **Step 1: Delete the four TS dirs/files and the plugin.** Keep local attachments working: session-local attachment save/read goes through `plugins/fs-sync` (`attachmentSave/List/Read/Remove`) — untouched.

- [ ] **Step 2: Sweep importers.** `rg -l "session-sharing|shared-notes|attachment-sync" apps/desktop/src` must go to zero: in each listed file remove the import and the JSX/handler that used it (Share button, shared-note tab type, deeplink share-URL branch, attachment-sync lifecycle mount). Where a tab-type union loses a member, remove the corresponding `case`.

- [ ] **Step 3: De-register the plugin** in `lib.rs` + both manifests. Local SQLite tables (`shared_session_cache`, `attachment_transfer_jobs`, …) remain — migrations are append-only; nothing reads them anymore.

- [ ] **Step 4: Gate + smoke run** (note header renders without Share button; recording + local attachment still work).

- [ ] **Step 5: Commit** — `git commit -m "chore: remove note sharing and cloud attachment upload"`

### Task 4: Remove accounts and billing; stub the gates

**Files:**
- Delete: `apps/desktop/src/billing/`, `apps/desktop/src/onboarding/account/`, `apps/desktop/src/settings/general/account.tsx`, `apps/desktop/src/settings/general/e2ee-setup.tsx` (+ its `.test`), `apps/desktop/src/main/sync-status.tsx` (+ test), `apps/desktop/src/auth/cloudsync.ts`, `apps/desktop/src/auth/cloudsync-progress.ts` (+ tests), `apps/desktop/src/auth/client.ts`, `apps/desktop/src/auth/useConnections.ts`, `packages/pricing/`, `packages/supabase/`, `plugins/auth/`, `crates/supabase-auth/`
- Modify: `apps/desktop/src/auth/context.tsx`, `apps/desktop/src/auth/billing.tsx` → replaced by stubs (below); `apps/desktop/src/shared/main-app-layout.tsx`, `apps/desktop/src/settings/index.tsx` (drop `account` tab), `apps/desktop/src/settings/general/index.tsx` (drop Cloud Sync toggle, E2EE dialog, account bits), `apps/desktop/src/onboarding/{index,config,final}.tsx` (drop account/trial steps), `apps/desktop/src/main/shell-frame.tsx` (drop `SyncStatusIndicator`), `apps/desktop/src-tauri/src/lib.rs` (drop `tauri_plugin_auth::init()`, `AuthPluginExt`, `clear_auth()`), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/package.json` (drop `@hypr/plugin-auth`, `@hypr/pricing`, `@hypr/supabase`, `@supabase/supabase-js`; fix `tauri`/`tauri:dev` scripts to drop `-f ../../.env.supabase`), `apps/desktop/src/env.ts` (drop `VITE_SUPABASE_URL`, `VITE_SUPABASE_ANON_KEY`, `VITE_PRO_PRODUCT_ID`, `VITE_API_URL`, `VITE_APP_URL`), root `Cargo.toml`, `pnpm-workspace.yaml`

**Interfaces:**
- Produces: `useAuth()` → `{ session: null, isSignedIn: false, signOut: () => {} }`; `useBillingAccess()` → `{ isPro: true, isPaid: true, isTrialing: false, plan: "local", canStartTrial: false, upgradeToPro: () => {} }`. Every existing consumer keeps compiling; every gated feature is unlocked-local.

- [ ] **Step 1: Write the stubs.** Replace `apps/desktop/src/auth/context.tsx` and `billing.tsx` (keep file paths so imports resolve; match the exact property names currently consumed — enumerate with `rg "useBillingAccess\(\)" -A3 apps/desktop/src` and `rg "useAuth\(\)" -A3 apps/desktop/src` first, and include every destructured key in the stub type):

```tsx
// apps/desktop/src/auth/billing.tsx (whole file)
import { createContext, useContext, type ReactNode } from "react";

const BILLING = {
  isPro: true, isPaid: true, isTrialing: false, plan: "local" as const,
  canStartTrial: false, upgradeToPro: () => {}, refresh: async () => {},
};
const BillingContext = createContext(BILLING);
export const useBillingAccess = () => useContext(BillingContext);
export function BillingProvider({ children }: { children: ReactNode }) {
  return <BillingContext.Provider value={BILLING}>{children}</BillingContext.Provider>;
}
```

```tsx
// apps/desktop/src/auth/context.tsx (whole file)
import { createContext, useContext, type ReactNode } from "react";

const AUTH = { session: null, isSignedIn: false as const, signIn: async () => {}, signOut: async () => {} };
const AuthContext = createContext(AUTH);
export const useAuth = () => useContext(AuthContext);
export function AuthProvider({ children }: { children: ReactNode }) {
  return <AuthContext.Provider value={AUTH}>{children}</AuthContext.Provider>;
}
```

Adjust the stub fields to the real consumed surface found by the `rg` sweep (the two files above are the minimum; add missing keys rather than editing consumers).

- [ ] **Step 2: Run typecheck; walk the error list.** `pnpm --dir apps/desktop typecheck` — every remaining error is a consumer of a deleted symbol (trial dialogs, account screens, sync-status, e2ee): delete the dead import/JSX in each. Settings AI gating (`settings/ai/shared/eligibility.ts`, `hypr-cloud-button.tsx`) is handled in Phase 2 — for now only fix compile errors.

- [ ] **Step 3: De-register plugin + deps; fix scripts** (`tauri`/`tauri:dev` in `apps/desktop/package.json` lose `-f ../../.env.supabase`; delete `.env.supabase` reference everywhere: `rg -l "env.supabase"`).

- [ ] **Step 4: Onboarding flow** — in `apps/desktop/src/onboarding/config.tsx` remove the `account`/`trial` step ids from the step union + sequence; in `index.tsx` remove their imports/renders. Onboarding now: welcome → permissions → folder-location → model download → done.

- [ ] **Step 5: Gate + smoke run.** App starts signed-out forever; Settings has no Account tab; no trial toasts.

- [ ] **Step 6: Commit** — `git commit -m "chore: remove accounts and billing; unlock all features locally"`

### Task 5: Hardwire CloudSync off; disable auto-updater; prune CI

**Files:**
- Modify: `apps/desktop/src-tauri/src/db.rs`, `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/tauri.conf.stable.json` (updater endpoint), `apps/desktop/src-tauri/tauri.conf.json` (updater pubkey/plugin)
- Delete: `.github/workflows/{desktop_cd.yaml,desktop_publish.yaml,legacy_desktop_cd.yaml,handle_release.yaml,handle_staging.yaml,download_staging.yaml,submit_flathub.yaml,sign_passthrough.yaml}`, `apps/desktop/flatpak/`
- Keep: `desktop_ci.yaml`, `desktop_e2e.yaml`, `cli_ci.yaml` (trim jobs that referenced deleted apps if any)

- [ ] **Step 1: CloudSync off at the root.** In `apps/desktop/src-tauri/src/db.rs` replace the body of `cloudsync_runtime_config_from_env` with `None` (keep signature; delete its env-parsing tests). In `lib.rs`, keep `init_with_cloudsync(db, None)` or switch to the plain `init(db)` variant if exported — whichever compiles with `plugins/db` as-is. The cloudsync/e2ee commands remain registered but permanently return "not configured".

- [ ] **Step 2: Updater off.** `tauri.conf.stable.json`: set `plugins.updater.active: false`, delete the `endpoints` entry. Remove the updater pubkey from `tauri.conf.json` (`plugins.updater.pubkey`). Leave `plugins/updater2` code in place (inert without endpoint) — deletion optional later.

- [ ] **Step 3: Delete release/distribution workflows + flatpak dir** (private fork builds locally; CrabNebula slug `fastrepl/hyprnote2` is theirs).

- [ ] **Step 4: Full-phase gate**

```bash
cargo check -p desktop && pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop test && pnpm --dir apps/desktop build
rg -n "VITE_SUPABASE|VITE_API_URL|VITE_APP_URL|posthog|sentry" apps/desktop/src apps/desktop/src-tauri/src ; # expect: no hits
```

Smoke run: record a short session end-to-end (mic → local transcript → note edit).

- [ ] **Step 5: Commit** — `git commit -m "chore: hardwire cloudsync off, disable updater, prune release CI"`

---

## Phase 2 — Local models only (AI providers)

### Task 6: STT — local on-device only

**Files:**
- Modify: `apps/desktop/src/settings/ai/stt/shared.tsx`, `apps/desktop/src/settings/ai/stt/select.tsx`, `apps/desktop/src/stt/useSTTConnection.ts`, `apps/desktop/src/stt/capabilities.ts`, `apps/desktop/src/stt/model-selection.ts`, `apps/desktop/src/shared/config/configure-paid-settings.ts`, `apps/desktop/src/settings/ai/shared/eligibility.ts`
- Delete: `apps/desktop/src/settings/ai/shared/hypr-cloud-button.tsx`

- [ ] **Step 1: Trim the provider registry.** In `stt/shared.tsx` reduce `_PROVIDERS` to the single `hyprnote` entry (it hosts the on-device models; rename its display label to `On-device` here — brand-neutral ahead of Phase 3). Delete the entries for `deepgram, assemblyai, openai, cartesia, cloudflare_workers_ai, gladia, soniox, elevenlabs, mistral, pyannote, aquavoice, custom, fireworks`. The `ProviderId` union narrows automatically.

- [ ] **Step 2: Fix the fallout, guided by typecheck.**
  - `stt/select.tsx` (~L645): remove the `{ id: "cloud", isDownloaded: billing.isPaid }` model entry so only soniqo/am on-device models list.
  - `stt/useSTTConnection.ts`: keep only the on-device branch (`localSttCommands.getServerForModel` → localhost URL); delete the `hyprnote`-cloud and generic `baseUrl+apiKey` branches.
  - `stt/model-selection.ts`: `DEFAULT_EXTERNAL_STT_MODELS = {}` (or delete + fix imports).
  - `stt/capabilities.ts`: `isHyprnoteCloudSttModel` → dead; remove it and its call sites.
  - `configure-paid-settings.ts`: default `current_stt_provider: "hyprnote"`, `current_stt_model: "soniqo-parakeet-batch"` (verify exact model id against `crates/local-stt-core/src/lib.rs::SUPPORTED_MODELS`).
  - `eligibility.ts`: delete `requires_auth` / `requires_entitlement` logic entirely (returns "eligible" always); delete `hypr-cloud-button.tsx` and its imports.

- [ ] **Step 3: Test.** Add to the existing settings test dir a guard test:

```tsx
// apps/desktop/src/settings/ai/stt/local-only.test.tsx
import { describe, expect, it } from "vitest";
import { _PROVIDERS } from "./shared";

describe("stt providers", () => {
  it("exposes only the on-device provider", () => {
    expect(_PROVIDERS.map((p) => p.id)).toEqual(["hyprnote"]);
  });
});
```

(Export `_PROVIDERS` from `shared.tsx` if not already exported.) Run: `pnpm --dir apps/desktop test -- stt` → PASS.

- [ ] **Step 4: Gate + smoke run** (Settings→AI→Transcription shows only downloadable local models; recording transcribes via local server).

- [ ] **Step 5: Commit** — `git commit -m "feat: STT is local-only; remove cloud STT providers"`

### Task 7: LLM — drop hosted provider only

**Files:**
- Modify: `apps/desktop/src/settings/ai/llm/shared.tsx` (remove the `hyprnote` entry from `_PROVIDERS`; keep all others), `apps/desktop/src/ai/hooks/useLLMConnection.ts` (delete the `hyprnote` switch case that builds the `VITE_API_URL/llm` OpenRouter proxy), `apps/desktop/src/shared/config/configure-paid-settings.ts` (default `current_llm_provider: "openrouter"`, model unset → user configures; imported legacy settings will carry the user's existing choice anyway), `apps/desktop/src/settings/ai/llm/select.tsx` + `context.tsx` (drop any auth/entitlement branches remaining)

- [ ] **Step 1: Remove the `hyprnote` provider entry + switch case + gating.** All BYO-key providers and ollama/lmstudio/custom stay byte-identical.

- [ ] **Step 2: Guard test** (same pattern as Task 6): assert `_PROVIDERS` ids equal the kept list exactly.

- [ ] **Step 3: Gate + smoke** (configure OpenRouter key in dev run → generate a summary).

- [ ] **Step 4: Commit** — `git commit -m "feat: remove hosted LLM provider; BYO-key and local providers remain"`

---

## Phase 3 — Rebrand

### Task 8: Tauri configs + identifier constants

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json`, `tauri.conf.stable.json`, `tauri.conf.staging.json` (productName, mainBinaryName, identifier, deep-link schemes, publisher `Bart Smeets`); delete `tauri.conf.flatpak.json`
- Modify (identifier/name constants): `crates/storage/src/global.rs`, `apps/desktop/src-tauri/src/embedded_cli.rs`, `apps/desktop/src-tauri/src/db.rs`, `apps/desktop/src-tauri/src/commands.rs`, `apps/cli/src/db.rs`, `crates/detect/src/list/mod.rs`, `crates/detect/src/meeting_ax.rs`, `crates/tcc/src/lib.rs`, `plugins/detect/src/*`, `plugins/importer/src/*`, `plugins/tray/src/menu_items/tray_version.rs`, `plugins/deeplink2/src/*`, `crates/notification-linux/src/ui.rs`, `crates/gguf/src/lib.rs`

- [ ] **Step 1: Configs.** Set per Global Constraints. Deep-link `schemes: ["freemeetingtranscriber"]` in `tauri.conf.json` (drop `hyprnote`, `char`).

- [ ] **Step 2: Data-dir ladder.** In `crates/storage/src/global.rs`:

```rust
pub const STAGING_BUNDLE_ID: &str = "org.freemeetingtranscriber.staging";
const RELEASE_APP_FOLDER: &str = "free-meeting-transcriber";
const LEGACY_RELEASE_APP_FOLDERS: [&str; 2] = ["anarlog", "hyprnote"];
```

Update `resolve_app_folder()` to walk the ladder: use `free-meeting-transcriber/` if present or if no legacy dir has data; else first legacy dir that exists (preserves your current `hyprnote`-named dir with `app.db`). Extend the existing unit tests in the same file for the three-rung ladder (new dir wins when present; `anarlog` found; `hyprnote` found).

- [ ] **Step 3: CLI naming.** `embedded_cli.rs`: `command_name_from_identifier` maps `org.freemeetingtranscriber.stable → "fmtr"`, `.staging → "fmtr-staging"`, `.dev → "fmtr-dev"`; `MANAGED_CLI_DIR = ".fmtr-cli"`; legacy-symlink cleanup list gains `"Free Meeting Transcriber.app"` variants and keeps the Anarlog ones (so it cleans up old symlinks). Bundled binary basenames follow `apps/cli` rename in Task 9.

- [ ] **Step 4: Sweep the remaining constants** in the listed Rust files: every `com.hyprnote.*` literal → `org.freemeetingtranscriber.*`; `crates/detect/src/list/mod.rs` self-name list gains `"free meeting transcriber"` (keep detecting Zoom/Meet/etc. untouched); `tray_version.rs` product-name→channel map: `"Free Meeting Transcriber" => stable`, `" Dev"`/`" Staging"` variants (keep old names as fallback arms). Verify sweep completeness:

```bash
rg -n "com\.hyprnote" crates plugins apps --glob '!**/migrations/**' ; # expect: no hits
```

- [ ] **Step 5: Gate + dev smoke:** app runs under new identity; **verify your existing data appears** (ladder found the old dir). macOS will re-prompt mic/screen permissions — expected, accept.

- [ ] **Step 6: Commit** — `git commit -m "feat: rebrand identity — bundle ids, data-dir ladder, CLI naming"`

### Task 9: CLI crate + UI strings + assets + docs

**Files:**
- Modify: `apps/cli/Cargo.toml` (`[[bin]] name = "fmtr"`), regenerate `apps/cli/src/snapshots/*` (`cargo insta review` or `INSTA_UPDATE=always cargo test -p anarlog-cli`), `apps/desktop/src-tauri/src/agents-content.md` (rewrite: new name, strip docs.anarlog.so links), root `AGENTS.md`, `README.md` (rewrite header: name, provenance/credit to fastrepl/anarlog, MIT note), `LICENSE` (add your copyright line above Fastrepl's), Swift strings in `crates/notification-macos/swift-lib/src/NotificationInstance.swift` + `NotificationManager+CompactView.swift`, tray links `plugins/tray/src/menu_items/help_report_bug.rs`/`help_suggest_feature.rs` → `https://github.com/bart6114/free-meeting-transcriber/issues`
- Modify (~37 UI source files): every user-visible `Anarlog` string → `Free Meeting Transcriber` (notably `settings/developers/index.tsx` "Anarlog CLI/MCP", `composer/index.tsx` + `shared/chat-cta.tsx` "Ask Anarlog AI" → "Ask AI", onboarding + permissions screens)
- Icons: replace `apps/desktop/src-tauri/icons/stable/*`, `resources/stable*/`, `resources/notification-icons/`, `assets/dmg-background-*.png`

- [ ] **Step 1: UI string sweep.** `rg -l '\bAnarlog\b' apps/desktop/src --glob '!**/i18n/**'` → edit each (mind casing and "Anarlog AI"/"Anarlog Cloud" phrasings; cloud phrasings should already be gone from Phase 1-2 — any hit there means dead code to delete).

- [ ] **Step 2: i18n re-extract.** `pnpm --dir apps/desktop i18n:extract && pnpm --dir apps/desktop i18n:compile`. Renamed msgids lose their translations (fall back to English) — acceptable; commit the regenerated catalogs.

- [ ] **Step 3: Icons.** Generate from a single 1024×1024 source PNG (simple monogram is fine to start): `pnpm --dir apps/desktop tauri icon path/to/icon-1024.png -o src-tauri/icons/stable`. Replace notification icons + DMG background with the same artwork (any PNG of matching dimensions).

- [ ] **Step 4: CLI rename + snapshots;** `cargo test -p anarlog-cli` green after `INSTA_UPDATE=always` re-record. (Crate *package* name `anarlog-cli` may stay — internal.)

- [ ] **Step 5: Phase gate:**

```bash
rg -in "anarlog" apps/desktop/src --glob '!**/i18n/**' ; # expect: no user-visible hits (comments ok)
cargo check -p desktop && pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop test && pnpm --dir apps/desktop build
```

Dev smoke: About/menu-bar/notifications say "Free Meeting Transcriber"; `~/.local/bin/fmtr` installs from Settings→Developers.

- [ ] **Step 6: Commit** — `git commit -m "feat: rebrand strings, CLI binary, icons, docs"`

---

## Phase 4 — Restore the storage-dir setting

### Task 10: Settings → Storage "Change folder" row

**Files:**
- Create: `apps/desktop/src/settings/general/storage/change-location.tsx`
- Create: `apps/desktop/src/settings/general/storage/change-location.test.tsx`
- Modify: `apps/desktop/src/settings/general/storage/index.tsx`

**Interfaces:**
- Consumes: `settingsCommands.vaultBase() / copyVault(newPath) / setVaultBase(newPath) / isEmptyOrMissingDir(path) / obsidianVaults()` from `@hypr/plugin-settings` (all still shipped in `plugins/settings/src/commands.rs`); `scheduleAutomaticRelaunch` from `~/shared/relaunch`; folder picker from `@tauri-apps/plugin-dialog`. Reference implementation: `apps/desktop/src/onboarding/folder-location.tsx` and the pre-removal wizard `git show 4ccafaa0d~1:apps/desktop/src/settings/general/storage/use-storage-wizard.ts`.

- [ ] **Step 1: Recover the old wizard for reference**

```bash
git show 4ccafaa0d~1:apps/desktop/src/settings/general/storage/use-storage-wizard.ts > /tmp/use-storage-wizard.ref.ts
git show 4ccafaa0d~1:apps/desktop/src/settings/general/storage/index.tsx > /tmp/storage-index.ref.tsx
```

- [ ] **Step 2: Write the failing test**

```tsx
// apps/desktop/src/settings/general/storage/change-location.test.tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@hypr/plugin-settings", () => ({
  commands: {
    vaultBase: vi.fn(async () => ({ status: "ok", data: "/Users/x/Drive/vault" })),
    copyVault: vi.fn(async () => ({ status: "ok", data: null })),
    setVaultBase: vi.fn(async () => ({ status: "ok", data: null })),
    isEmptyOrMissingDir: vi.fn(async () => ({ status: "ok", data: true })),
    obsidianVaults: vi.fn(async () => ({ status: "ok", data: [] })),
  },
}));

import { ChangeLocationRow } from "./change-location";

describe("ChangeLocationRow", () => {
  it("shows the current vault path and a change button", async () => {
    render(<ChangeLocationRow />);
    expect(await screen.findByText(/Drive\/vault/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /change/i })).toBeTruthy();
  });
});
```

Run: `pnpm --dir apps/desktop test -- change-location` → FAIL (module not found).

- [ ] **Step 3: Implement** — adapt the onboarding section into a settings row (structure mirrors `folder-location.tsx`: `useQuery` for current base, folder-picker dialog, `copyVault → setVaultBase → scheduleAutomaticRelaunch` mutation, error toast on `validate_vault_base_change` failure, Obsidian vault quick-picks). Wrap queries in the same react-query pattern used by `folder-location.tsx`; wrap picker+mutation in a confirm dialog stating "App restarts to apply. Existing files are copied, not moved."

- [ ] **Step 4: Mount it** — in `storage/index.tsx` render `<ChangeLocationRow />` above `<LegacyMigrationCleanupRow />`.

- [ ] **Step 5: Test passes; gate; manual verify** — dev run: change vault to a temp dir → relaunch → new recording's `audio.mp3` lands under the new dir; change it back to your Drive folder.

- [ ] **Step 6: Commit** — `git commit -m "feat: restore user-settable storage folder in Settings"`

---

## Phase 5 — Vault files become the source of truth

Order matters: rebuild-from-vault first (safety), then the DB→file mirror, then external-edit watching, then de-authority ceremonies. After this phase: deleting `app.db` loses nothing; editing `_memo.md` in Drive shows up in the app; every app edit shows up as a file.

### Task 11: Move the tantivy search index out of the vault

**Files:**
- Modify: `plugins/tantivy/src/ext.rs` (~L80: `vault_base().join(config.path)` → `global_base().join(config.path)` — the app-data dir accessor from `plugins/settings`), `plugins/notify/src/path.rs` (`should_skip_path` may drop its `search_index` rule once the index is out of the vault; keep the rule for one release to skip stale dirs)

- [ ] **Step 1:** Make the edit; on startup the existing rebuild-on-missing logic (schema_version file) rebuilds the index at the new location automatically.
- [ ] **Step 2:** Gate + dev smoke: search works; `vault/search_index/` no longer written (delete the stale dir manually from Drive afterwards).
- [ ] **Step 3: Commit** — `git commit -m "feat: relocate search index from vault to app-data (keep Drive clean)"`

### Task 12: Continuous reconcile-from-vault on startup (rebuildable DB)

**Files:**
- Modify: `plugins/db/src/import/mod.rs` (`import_legacy_data` + `legacy_import_attempt_required`), `plugins/db/src/import/legacy_vault.rs`, `crates/db-app/src/legacy_import.rs`
- Test: `plugins/db/tests/` (new integration test alongside existing import tests)

**Interfaces:**
- Consumes: the existing importer IR (`LegacyImportBatch`, `classify_source`, `parse_source`) and audit tables (`migration_import_items.source_sha256`).
- Produces: `sync_from_vault(pool, vault_base) -> Result<SyncReport>` — idempotent; called on every startup; skips files whose sha256 matches the last-imported hash; imports new/changed files; never deletes DB rows for missing files (deletion propagation is Task 14).

- [ ] **Step 1: Failing test** — in a temp vault write a session dir (`_meta.json` + `_memo.md`), open a fresh DB, call `sync_from_vault` twice; assert: session + note document exist after first call; second call reports 0 imports (hash skip). Then modify `_memo.md`; third call re-imports exactly one document.

- [ ] **Step 2: Implement** — rename/gate change: `import_legacy_data` currently returns early when a completed run exists; replace the boolean gate with per-file sha256 comparison against the latest run's `migration_import_items` (the columns already exist). Content conflict rule: file hash differs AND DB row `updated_at` newer than file mtime → file still wins (log a warning; DB copy is exported to `<name>.conflict-<timestamp>.md` beside it before overwrite). Keep `storage_migration_state` writes for observability, but `parity_verified` no longer gates anything.

- [ ] **Step 3: The delete-app.db drill (manual, gated):** quit dev app; `rm '~/Library/Application Support/free-meeting-transcriber/app.db'*; relaunch; all sessions/notes/transcripts restored from vault files (audio plays; search rebuilt).

- [ ] **Step 4: Remove the "Migration needs attention" flow** — `apps/desktop/src/settings/general/storage/legacy-cleanup.tsx` and the `cleanup_legacy_files` command path (`plugins/db/src/import/cleanup.rs` + its commands + permission files): DELETE — in a file-canonical world, deleting vault text files is data loss. Remove the row from `storage/index.tsx`.

- [ ] **Step 5: Gate + commit** — `git commit -m "feat: app.db is a rebuildable cache — idempotent vault reconcile on startup"`

### Task 13: DB→vault file mirror (write-through export)

**Files:**
- Create: `apps/desktop/src-tauri/src/vault_export.rs` (worker modeled 1:1 on `apps/desktop/src-tauri/src/search_index.rs`)
- Create: `crates/db-app/migrations/<timestamp>_vault_export_dirty.sql`
- Modify: `apps/desktop/src-tauri/src/lib.rs` (spawn the worker next to the search-index worker), `plugins/fs-sync/src/commands.rs` (its `write_document_batch`/`write_json_batch` internals become the render helpers — promote them into `crates/fs-sync-core/src/export.rs` so Rust can call them without the Tauri command layer)

**Interfaces:**
- Consumes: `db.change_notifier()` (same subscription API as `search_index.rs`), `hypr_tiptap::tiptap_json_to_md`, `fs_sync_core::frontmatter::ParsedDocument::render`, `notify.mark_own_writes(paths)` (loop prevention — the watcher must ignore our own writes).
- Produces: on any committed write to `sessions / session_documents / transcripts / session_participants / humans / organizations / calendars / events / daily_notes / action_items / tags / chat_groups / chat_messages / app_settings`, the corresponding vault file(s) re-render within the debounce window (500 ms), in the exact layout from Global Constraints (importer-compatible round-trip).

- [ ] **Step 1: Migration** — `vault_export_dirty (entity_type TEXT, entity_id TEXT, generation INTEGER, queued_at TEXT, PRIMARY KEY(entity_type, entity_id))` + one `AFTER INSERT/UPDATE/DELETE` trigger per table above, copying the existing `search_index_dirty` trigger pattern verbatim (see `crates/db-app/migrations/20260714120100_search_index.sql` etc. for the template).

- [ ] **Step 2: Failing integration test** (in `apps/desktop/src-tauri/tests/` or `crates/fs-sync-core/tests/` for the render half): given a session row + note document (prosemirror JSON) + transcript in a temp DB, run one drain cycle; assert vault contains `sessions/<id>/_meta.json` (matching the importer's parse), `_memo.md` (frontmatter `id`/`session_id` + markdown body equal to `tiptap_json_to_md(body)`), `transcript.json`, `_summary.md` for the summary document. Round-trip guard: feed the rendered files back through `plugins/db` `parse_source` and assert the re-imported batch equals the original rows.

- [ ] **Step 3: Implement the worker** — copy `search_index.rs` structure: subscribe → drain `vault_export_dirty` → for each (entity_type, entity_id) load rows → render → `mark_own_writes` → atomic write (tmp + rename; Drive-friendly). Deletion rows (entity gone) → move the file to `vault/.trash/<date>/` (never hard-delete on Drive).

- [ ] **Step 4: Full-vault export command** — add `export_vault_now` Tauri command (enqueue all entities, like `enqueue_all_entities` in search) + a "Re-export all files" button in Settings→Storage. First run of the app after this task performs one full export automatically when `vault_export_dirty` is empty AND a marker file `vault/.fmt-export-version` is missing.

- [ ] **Step 5: Gate + dev smoke:** edit a note in the app → within ~1 s `_memo.md` updates in the vault (visible in Drive); create a session → folder + `_meta.json` appear.

- [ ] **Step 6: Commit** — `git commit -m "feat: write-through DB-to-vault file mirror"`

### Task 14: External-edit ingestion (watcher → import)

**Files:**
- Modify: `plugins/notify/src/ext.rs` (the `FileChanged` event currently has no listener), `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/src/vault_watch.rs`

**Interfaces:**
- Consumes: `FileChanged` events (debounced, own-writes filtered via `is_external_path`), `sync_from_vault`-style single-path import: add `import_paths(pool, vault_base, paths: &[PathBuf])` to `plugins/db/src/import/mod.rs` reusing `classify_source` + `parse_source`.
- Produces: editing/adding a vault file outside the app (Drive sync, text editor) updates the DB — and thus the UI via `db-reactive` — within the debounce window; deleting a session folder externally soft-hides the session (status flag), never cascading hard deletes.

- [ ] **Step 1: Failing test** — with the app runtime pieces stubbed (pool + temp vault), call `import_paths` for a modified `_memo.md`; assert the `session_documents` row body updated and `body_format` stayed consistent (md → prosemirror via existing `md_to_tiptap_json`).

- [ ] **Step 2: Implement `vault_watch.rs`** — listen to `FileChanged`, filter `is_external_path`, coalesce for 2 s, map paths through `classify_source`, call `import_paths`. Ignore `audio.*` (already file-native), `.trash/`, `.conflict-*`, `attachments/` (file-native).

- [ ] **Step 3: Loop test (manual):** edit `_memo.md` in a text editor while the app shows that note → UI updates; verify no export↔import ping-pong (the `mark_own_writes` set plus content-hash equality check in `import_paths` must short-circuit identical content).

- [ ] **Step 4: Gate + commit** — `git commit -m "feat: vault file watcher imports external edits"`

### Task 15: Authority ceremonies + docs

**Files:**
- Modify: `apps/desktop/src/session/content-mutations.ts` (no code change expected — verify the write queue's txn commit triggers the mirror via the dirty table; if any write path bypasses `plugins/db` `execute*`, route it through), `docs/` new page `docs/storage.md`
- Modify: `apps/desktop/src/settings/general/storage/index.tsx` (add copy: "Your data lives as files in this folder. The internal database is a cache and can be rebuilt at any time.")

- [ ] **Step 1: Audit for bypass writes** — `rg -n "execute_transaction|executeTransaction|\.execute\(" apps/desktop/src | grep -v liveQueryClient` and confirm all canonical writes flow through `plugins/db` (they do, per research — the write queue in `apps/desktop/src/db/write-queue.ts`); any stragglers get routed.
- [ ] **Step 2: Write `docs/storage.md`** — vault layout table, authority rules (files win; conflict copies as `.conflict-*`; `.trash/` semantics), rebuild instructions (`rm app.db` drill), Drive-specific notes (CloudStorage local-first semantics; app must stay usable offline — files write locally and Drive syncs in background).
- [ ] **Step 3: Full-phase gate** — the three standard commands + build + the delete-app.db drill once more, now with mirror+watcher active.
- [ ] **Step 4: Commit** — `git commit -m "feat: vault is canonical — docs and final wiring"`

---

## Phase 6 — Personal migration + release

### Task 16: Migrate Bart's real data

**Files:** no source changes — operational runbook.

- [ ] **Step 1:** Launch the app once (data-dir ladder finds the existing `hyprnote`/`anarlog` app-data dir; vault still points at `…/My Drive/anarlog`).
- [ ] **Step 2:** Settings→Storage→Change folder → create/select `…/My Drive/free-meeting-transcriber` (copyVault copies sessions incl. audio; relaunch).
- [ ] **Step 3:** Recover the two orphaned pre-June-2 memos: copy `~/Library/Application Support/hyprnote/sessions/62a41d38-*` ("sander") and `02be0e69-*` ("Property Purchase") `_memo.md` files into the matching session folders in the new vault; the watcher imports them. Verify both notes show their memos in-app.
- [ ] **Step 4:** Full export (`Re-export all files`) → spot-check in Drive web: every session folder has `_meta.json` + `_memo.md`/`_summary.md`/`transcript.json`.
- [ ] **Step 5:** Retire the old Anarlog install (quit + move `/Applications/Anarlog.app` to Trash when satisfied; old Drive `anarlog` folder kept as a frozen backup for a month, then delete).

### Task 17: Local release build

- [ ] **Step 1:** `pnpm --dir apps/desktop tauri:build` (config chain: verify it uses `tauri.conf.stable.json` — check the build script/`--config` flag in the deleted CI for the exact invocation and replicate: `pnpm --dir apps/desktop tauri build --config src-tauri/tauri.conf.stable.json`). Unsigned local build is fine (right-click→Open on first launch) — or ad-hoc sign: `codesign --force --deep -s - 'target/release/bundle/macos/Free Meeting Transcriber.app'`.
- [ ] **Step 2:** Install to `/Applications`, run the full smoke: record → local transcript → LLM summary via OpenRouter → files in Drive vault → `fmtr` CLI works.
- [ ] **Step 3:** Tag `v0.1.0`, push. `git tag v0.1.0 && git push origin v0.1.0`.

---

## Risk register

| Risk | Mitigation |
|---|---|
| Model weights hosted on `hyprnote.s3.…amazonaws.com` disappear | Keep URLs for now; post-v0.1 task: download once, re-host in a private bucket/Drive and patch `crates/whisper-local-model` + `crates/local-model` URLs |
| Export↔import feedback loop (mirror writes trigger watcher) | `mark_own_writes` + content-hash short-circuit (Tasks 13/14); loop test is mandatory before merging Task 14 |
| Google Drive FS quirks (delayed sync, file-provider locks) | Atomic tmp+rename writes; app.db and tantivy index NEVER in the vault; offline-first is preserved because CloudStorage writes locally |
| Prosemirror↔markdown lossiness (complex nodes) | Round-trip guard test in Task 13 Step 2; unsupported nodes fall back to `body_format: markdown` passthrough |
| i18n regressions after msgid rename | Accepted: English fallback; catalogs regenerate via `i18n:extract` |
| CloudSync/E2EE dead code drifts | It's inert (config `None`) and untouched; upstream merges stay possible via `upstream` remote |

## Self-review notes

- Spec coverage: name change → Tasks 8–9; drop SQLite-as-canonical → Tasks 11–15 (kept as cache, rationale in header); storage dir setting → Task 10; everything on Drive as files → Tasks 13–15 + layout constraint; local STT only → Task 6; LLM minus hosted → Task 7; no cloud/subscription → Tasks 1–5; private repo → done pre-plan.
- Type consistency: `sync_from_vault`/`import_paths` (Tasks 12/14) both live in `plugins/db/src/import/mod.rs`; the render helpers promoted to `fs_sync_core::export` (Task 13) are consumed only by `vault_export.rs`.
- Known soft spots called out inline rather than hidden: exact stub fields (Task 4 Step 1) and the exact on-device default model id (Task 6 Step 2) are verified by rg/source at execution time.
