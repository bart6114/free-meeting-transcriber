# `plugins/db`

## Use This Plugin For

- Desktop Tauri transport over the app database: bootstrap, one-shot execution, live-query channels, and startup vault reconcile (`sync_from_vault`).

## Put Changes Elsewhere When

- Schema, migration contents, and table helpers belong in `db-app`.
- Open policy, hooks, or CloudSync internals belong in `db-core` / `db-change`.
- Live-query semantics belong in `db-reactive`.
- App-facing hooks, caches, and domain query helpers belong in `apps/desktop`.

## Hard Rules

- Rust owns database opening, migration, and initialization. TypeScript stays a thin command wrapper.
- Keep `execute` and `executeProxy` separate on purpose: named object rows for app SQL, positional rows for Drizzle proxy consumers.
- `QueryEvent` shape and `js/bindings.gen.ts` are ABI. If Rust types change, regenerate bindings; do not hand-edit generated TS.
- `subscribe()` may legitimately return `NonReactive`. The current contract is "warn and keep going," not "throw."
- `app.db` is a rebuildable cache: `sync_from_vault` runs on every startup, comparing each vault file's sha256 against the last-imported hash and importing new/changed files. This is idempotent and non-destructive (never deletes rows for missing files) — but for `sessions` / `session_documents` / `transcripts` the vault file wins content conflicts (unlike direct `import_legacy_vault` callers such as `run_legacy_import`, which keep the historical "existing SQLite row wins" behavior for the one-time legacy migration).
- This plugin is transport-only. Do not add app-specific state, caching, or domain workflows here.
- Every successful subscription needs a matching `unsubscribe`, and the JS wrapper should detach the channel handler before sending it.
