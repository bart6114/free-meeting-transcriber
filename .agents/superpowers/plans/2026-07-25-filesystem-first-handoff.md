# Filesystem-First Sessions — Handoff & Remaining Work

**Date:** 2026-07-25
**Branch:** `refactor/filesystem-first-sessions` (from `bf2b7521d` on main; NOT pushed)
**Status:** Tasks 1–12 of 14 complete and reviewed. Task 13 blocked on a design gap, now resolved by owner decisions (below) but not implemented. Task 14 not started.
**Spec:** `docs/superpowers/specs/2026-07-24-filesystem-first-sessions-design.md`
**Original plan:** `docs/superpowers/plans/2026-07-24-filesystem-first-sessions.md`
**Execution ledger (detailed per-task record):** `.superpowers/sdd/progress.md` (git-ignored, on this machine)
**Task 13 dependency analysis:** `.superpowers/sdd/task-13-analysis.md`

## Why this project exists

On 2026-07-23 a recording's transcription was permanently lost: the old vault
watcher misread the app's own export/trash renames as "session folder removed
externally" and soft-hid the live session; the transcript INSERT was gated on
`deleted_at IS NULL` and silently wrote 0 rows while the UI showed the words
from memory. Full root-cause chain is in the spec's Motivation section.

## What is done (Tasks 1–12, all reviewed, all gates green)

**Phase 1 — removals** (commits `bf2b7521d..6b96b5650` + i18n batch `d259cd615`):
contacts/humans/orgs gone (speaker labels are plain strings); calendar +
meeting notifications gone; calendar/humans dropped from the Rust data layer
(migration `20260724100000`); cloudsync/e2ee/workspaces/sharing/attachments
gone (migration `20260724110000`); all `session_attachments` FE consumers
stopgapped then properly rewired in Task 9.

**Phase 2 — the session store** (`apps/desktop/src-tauri/src/session_store/`):

- **Task 5** (`..58e0892e6`): scaffold — atomic writes (tmp + fsync + rename,
  NO trash-before-overwrite), write journal (path → sha256), store-wide write
  lock pairing rename+journal.
- **Task 6** (`..096ad2ac2`): meta/note/document write-through, file-first
  then index; transactional session delete to `.trash/`.
- **Task 7** (`..1c4d11d2a`): live transcript buffer — ZERO preconditions
  (regression test `recording_into_unknown_session_still_persists` encodes the
  incident), debounced flush with re-dirty-on-failure + retry loop,
  transcript_id-scoped buffers.
- **Task 8** (`..fb9c9f073`): one-way index rebuild + per-session refresh —
  never writes the vault; NotFound-vs-IO-error discrimination (transient EACCES
  cannot cascade into index deletion); ghost sessions reported.
- **Task 9** (`..1c2341ea7`): 13 Tauri commands; every FE session-content
  write flipped from SQL to store commands; note is file-canonical; audio in
  `sessions/<id>/audio/`; trash-stack restore for undo; loud failures
  everywhere (persistent toast on transcript-write failure); exit flush via
  `complete_app_exit`.
- **Task 10** (`..e50c2bde9`): startup = `migrate::run_once` +
  `rebuild_index` (replacing `sync_from_vault`); throttled focus rescan. The
  boot smoke caught a live interim corruption loop (old exporter frontmatter
  vs store raw markdown) — fixed with WHERE-guarded upserts + recursive
  key-targeted frontmatter strip; owner's corrupted files restored.
- **Task 11** (`..deffb31c3`): watcher rewritten — pure
  `classify_event → Ignore | Refresh(id)`, journal-hash own-write filter (no
  TTL), index-only. The mandated incident replay (`rm _meta.json` mid-run)
  found the legacy exporter still trashing folders on row-absence — fixed
  (unconditional no-op). The incident failure mode is now structurally gone.
- **Task 12** (`..4bad5b5d9`): one-time migration sweep — words_json repair
  (missing/null/string-typed numeric fields; the int-coercion premise was
  disproven) runs BEFORE the final legacy-export drain; marker
  `.store-migrated-v1`. **Already executed on this machine's real vault**
  (marker written, repaired=0).

**Verification discipline that paid off:** three live bugs were caught only by
mandated real-app verification, not tests — the interim frontmatter corruption
loop (Task 10), the exporter row-absent folder-trash (Task 11), and Task 13's
schema blocker below.

## Task 13 — blocked, decisions made, NOT implemented

The plan's "recreate index tables minimal and clean" DDL is wrong: live
subsystems still read columns it drops, and some content classes exist ONLY in
the DB with no file home — a clean drop would destroy them:

- `search_index.rs` reads `sessions.event_json`, filters `deleted_at`.
- `session_ops.rs` (feeding the MCP `list_meetings`/`get_meeting` commands)
  selects many wider columns.
- ~10 FE files use raw SQL on `owner_user_id`, `folder_path`, `event_json`,
  `template_id`, `source_hash`, `generation_metadata_json`, `sort_order`,
  `created_by/updated_by`, and `deleted_at` (tags, meeting-chat capture,
  key-facts, duplicate-summary hiding, transcript batch-supersede).

**Owner decisions (2026-07-25):**
1. Superset schema now: recreate tables keeping every column live code reads;
   the migration COPIES data (CREATE new + INSERT..SELECT + DROP old +
   recreate search triggers) — zero data loss. Clean-minimal schema deferred.
2. **Drop the key-facts feature entirely** (like calendar/contacts).
3. **Drop the meeting-chat capture feature entirely.**
4. `folder_path` / `generation_metadata_json` (and the other wide columns):
   usage verdicts and a concrete proposed superset DDL are in
   `.superpowers/sdd/task-13-analysis.md` — written by the agent that did the
   dependency investigation. Read it before implementing.
5. Content classes that survive but stay DB-only for now (no file home)
   should eventually get one (e.g. folder assignment → `_meta.json`) — that is
   a follow-up design task, out of scope for this branch.

## Remaining work

**Revised Task 13** (was: delete old machinery):
1. Feature removals first, mechanically (pattern of Tasks 1–2): key-facts
   (UI, flows, `session_documents` kind rows, search touchpoints) and
   meeting-chat capture (FE capture flow, its document rows). Inventory is in
   the analysis file.
2. Then the deletions as originally planned: `vault_export.rs` (whole worker),
   `plugins/db/src/import/` (whole module), `crates/db-app/src/legacy_import.rs`,
   `plugins/db/tests/vault_export_round_trip.rs` + `vault_sync.rs` (the 7
   ignored legacy tests die with their files), orphaned `crates/e2ee`.
3. Migration with the SUPERSET DDL + data copy from the analysis file
   (NOT the original plan's minimal DDL). `deleted_at` kept only where the
   analysis says a surviving feature needs it; FE `deleted_at` filter removal
   only where the column actually goes.
4. Simplify `migrate.rs`: marker + words_json repair only; the legacy-drain
   integration goes (it already ran on the only real installation; post-13,
   index tables are rebuilt from files so DB-only legacy content cannot
   exist). Add the single-writer-context comment on the repair UPDATE.
5. Settings → Storage "Re-export all files" button → `sessionRebuildIndex`
   ("Rebuild index from files").
6. End-state greps (empty, excluding docs/.superpowers):
   `sync_from_vault|vault_export_dirty|external_soft_hide|import_paths|reconcile_vault_conflicts`.
7. Mandatory boot verification + external-edit watcher check (evidence in
   report). This step has caught a real bug in 3 of the last 4 tasks — do not
   skip it.

**Task 14** (hardening + QA) — accumulated list (also in the ledger):
- fs-sync-core: kill remaining `unwrap_or_default()` on content
  (render_transcripts) — errors, never `[]`; then dead-code sweep (orphaned
  renderer inventory is in the Task 13 analysis/report).
- Re-derive the unparseable-words inventory from the DB (strict-parse check
  on `transcripts.words_json`) — do NOT rely on rotated logs.
- Task 7 fast-follows: direct test for the `needs_flush_before_switch` branch;
  consider retry backoff/cap; collapse the check-then-act lock scopes.
- Exit paths bypassing `flush_all` (`should_force_quit`, emit-failure
  fallthrough in lib.rs) — pre-existing, close them.
- Conflict-backup `*.md` ignore in `scan_document_files` (or confirm the
  producer died with the import module).
- Markdown round-trip fidelity spot-check on real notes; `.tmp-` basename
  anchor; N+1 prune deletes (cosmetic).
- Full QA pass per the plan's Task 14 (qa-critical-ux minus calendar), incl.
  the original incident scenario end-to-end: record → quit → relaunch →
  transcript present.
- Batched i18n regeneration (Node ≥ 24 via nvm) — new strings were added in
  Tasks 9–13 after the Phase-1 batch regen.

**Then:** final whole-branch review (superpowers:requesting-code-review — the
per-task Minor findings to triage are itemized in the ledger), and
superpowers:finishing-a-development-branch to merge into `main`.

## How to resume

Start a session in this repo on this branch and say e.g. "resume the
filesystem-first sessions plan — read
`docs/superpowers/plans/2026-07-25-filesystem-first-handoff.md`". The ledger
(`.superpowers/sdd/progress.md`) is the durable per-task record (base/head
commits, review verdicts, accumulated follow-ups); task briefs and reports for
all 13 dispatched tasks are beside it. Execution used
superpowers:subagent-driven-development (fresh implementer per task, task
review, fix rounds, controller verification) — recommended for the remainder.

## Safety of the current state

The branch is boot-verified and self-consistent as of `4bad5b5d9`: the store
owns all writes, the watcher is index-only, the incident scenario is dead, and
the old machinery still present (export worker, import module) coexists
safely (verified by review traces in Tasks 10–11). Nothing in the remaining
work is required for day-to-day safety; it is cleanup, feature removal, and
hardening. The dev vault on this machine is healthy (sessions intact,
`.store-migrated-v1` present).
