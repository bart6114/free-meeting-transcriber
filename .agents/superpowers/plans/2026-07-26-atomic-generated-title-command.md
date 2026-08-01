# Plan — `session_apply_generated_title`: one atomic command for title stamping

**Status:** proposed, not implemented
**Branch to build on:** `refactor/obsidian-model-no-db` (@ `ed286d3`)
**Origin:** whole-branch review finding — `applyGeneratedSessionTitle` lost the
atomicity the SQL transaction used to give it. Deliberately left unfixed during the
review fix pass because the correct fix is a design decision, not a patch.

## 1. The defect

`apps/desktop/src/session/content-mutations.ts:78-137` applies a generated title in
four or more independent steps:

1. `sessionGet` → compare `meta.title` against `currentTitle` (a check-then-act that
   holds **no store lock at all**).
2. For each summary/template_output doc: `sessionUpdateEnhancedDoc` with an
   `expected_markdown` CAS. Each call takes and releases the store write lock.
3. `sessionUpdateMeta` with the new title. A third lock acquisition.
4. Separately, `title-success.ts:90-116` stamps the raw note: `sessionReadNote` →
   compare → `sessionWriteNote`. A fourth.

The old implementation was a single `executeTransaction` where every statement carried
`expectedRowsAffected: 1`, so any guard miss rolled the whole batch back.

**What goes wrong now.** With two summary tabs open, if doc #2's CAS rejects because the
user regenerated it mid-flight, the loop throws — but doc #1 has already been written and
the session title never is. The vault is left holding a document whose body is stamped
with a title the session does not have. No compensating write exists, and nothing retries:
`title-success.ts:47` bails when `snapshot.title.trim()` is non-empty, so a *successfully*
titled session never re-runs. The inconsistent state is permanent.

The `enqueueDatabaseWrite("session:<id>")` wrapper serializes this against other frontend
writers **in the same webview only**. Standalone note windows (`routes/app/note.$sessionId.tsx`)
have their own module-scoped queue, and an external editor is not serialized at all — so
step 1's check-then-act is not sound even before the multi-step problem.

## 2. What "atomic" can actually mean here

POSIX gives atomic single-file `rename(2)`; it gives nothing atomic across multiple files.
A literal all-or-nothing multi-file commit would need a write-ahead log and crash recovery
on boot — disproportionate for this, and a new durability surface to get wrong.

The design below is **validate-all → stage-all → commit-all**, which eliminates the defect
that actually occurs and narrows the residual to a genuinely different class of event:

| Failure | Today | After |
|---|---|---|
| CAS rejection (the real bug) | partial write, permanent | nothing written |
| Disk full / permissions / serialize error | partial write | nothing written |
| Crash or I/O error *between two renames* | partial write | partial write |

Every failure mode that occurs in practice moves to "nothing written". The surviving
window requires a failure between two `rename` calls on already-`fsync`ed temp files, on
the same filesystem, with parent directories already created — microseconds, and the only
remaining path to it is process death.

**This must be stated plainly in the doc comment.** Do not describe the command as
transactional.

## 3. Command surface

New file: `apps/desktop/src-tauri/src/session_store/title_stamp.rs`
(sibling of `content.rs`/`enhanced.rs`; keeps the batch logic out of both).

```rust
/// One enhanced doc participating in the stamp. `expected_markdown` is the CAS guard --
/// the body the caller read -- and `next_markdown` is that body with the title stamped in.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct TitleStampDoc {
    pub doc_id: String,
    pub expected_markdown: String,
    pub next_markdown: String,
}

/// The raw note (`_memo.md`). Optional, and its CAS miss is NOT fatal -- see §5.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct TitleStampNote {
    pub expected_markdown: String,
    pub next_markdown: String,
}

#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct SessionTitleStamp {
    /// CAS on `_meta.json`'s title. Typically "" -- the caller only generates a title for
    /// an untitled session.
    pub expected_title: String,
    pub next_title: String,
    pub documents: Vec<TitleStampDoc>,
    pub note: Option<TitleStampNote>,
}

/// Reports which optional participants were dropped, so the caller can log without
/// having to re-read the vault to find out.
#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]
pub struct TitleStampOutcome {
    pub note_skipped: bool,
    pub documents_written: u32,
}
```

Command in `session_store/commands.rs`, registered in `lib.rs`'s `collect_commands!`
alongside the other `session_*` entries (specta collects the types transitively from the
signature — no separate `collect_types` needed):

```rust
#[tauri::command]
#[specta::specta]
pub async fn session_apply_generated_title<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    stamp: SessionTitleStamp,
) -> Result<TitleStampOutcome, String>
```

## 4. Splitting the write primitive — do not fork it

`write_file_locked` (`mod.rs:116-185`) currently does, inside one `spawn_blocking`:
`create_dir_all` → `trash_foreign_bytes` → write tmp → `sync_all` → `rename` → journal record.

The batch needs the first four for every file *before* any rename. The refactor must make
the existing single-file write a **degenerate case of the batch**, not a parallel
implementation — otherwise the `trash_foreign_bytes` data-loss guard (added in `f90d448`)
can silently drift between two code paths.

```rust
/// A file written to its temp sibling and fsynced, awaiting rename.
pub(crate) struct StagedWrite {
    relative: String,
    abs: PathBuf,
    tmp: PathBuf,
    hash: String,
}

impl SessionStore {
    /// create_dir_all + trash_foreign_bytes + write tmp + sync_all. No visible change yet.
    pub(crate) async fn stage_file_locked(
        &self, guard: &WriteGuard<'_>, relative: PathBuf, bytes: Vec<u8>,
    ) -> Result<StagedWrite, StoreError>;

    /// rename each tmp into place, in order, recording journal hashes as it goes.
    pub(crate) async fn commit_staged_locked(
        &self, guard: &WriteGuard<'_>, staged: Vec<StagedWrite>,
    ) -> Result<(), StoreError>;

    /// Best-effort tmp cleanup for the abort path. Never returns an error; logs at debug.
    pub(crate) fn discard_staged(staged: Vec<StagedWrite>);
}

// becomes:
pub(crate) async fn write_file_locked(&self, guard, relative, bytes) -> Result<(), StoreError> {
    let staged = self.stage_file_locked(guard, relative, bytes).await?;
    self.commit_staged_locked(guard, vec![staged]).await
}
```

Note `trash_foreign_bytes` runs at **stage** time, which is correct: it is a rename of the
*existing* file into `.trash`, so it must happen before the tmp replaces it. A batch that
later aborts will have trashed foreign bytes without replacing them — the file is missing
from its normal location. **This is the one behaviour change to think hard about.**

Mitigation: in the batch path, the CAS validation in §5 phase 1 runs *before* any staging,
and a CAS match means the on-disk bytes are exactly what the caller read — so
`trash_foreign_bytes` will find either a journal match or byte-identical content and trash
nothing. Foreign bytes and a passing CAS are mutually exclusive for the doc and note
participants. `_meta.json` has no byte-level CAS (only the title field), so it remains
possible there; accept it, and cover it with a test asserting the meta file still exists
after an aborted batch.

## 5. Execution phases

All four phases run under **one** `self.lock_writes().await` guard.

**Phase 1 — validate, no writes.**
- `validate_session_id(session_id)`; `validate_doc_id` for each doc id. Reject before touching anything.
- `read_meta` → absent ⇒ `StoreError::Io`. `meta.title != stamp.expected_title` ⇒
  `StoreError::Conflict` (the `conflict:` prefix the FE already understands).
- For each doc: `read_enhanced_doc` → absent ⇒ `Io`; `doc.markdown != expected_markdown` ⇒ `Conflict`.
- If `note` is present: `read_note` → compare against `expected_markdown`, treating `None`
  as `""` exactly as `applyGeneratedNoteTitle` does today. **A mismatch drops the note from
  the write set and sets `note_skipped`; it does not fail the batch.** This preserves the
  current, deliberate semantics documented at `title-success.ts:73-78` — someone editing the
  note since the snapshot should lose the stamp, not block the title.
- Reject a doc id appearing twice (two `StagedWrite`s for one path would rename twice).

**Phase 2 — stage.** Render each participant to bytes and `stage_file_locked`:
- docs via `render_enhanced_file(&doc)` after `doc.markdown = next_markdown`
- meta via the same serializer `write_meta_locked` uses, after `meta.title = next_title`
- note as `next_markdown.into_bytes()` — and stamp the index with
  `strip_leading_frontmatter` of it, matching the invariant fixed in `0aaedff`
Any error ⇒ `discard_staged(all)` and return. Nothing renamed, nothing visible.

**Phase 3 — commit.** `commit_staged_locked` renames in this order: **documents, then the
note, then `_meta.json` last.**

Rationale — this is the recoverable direction, and it is worth being explicit. If the
process dies mid-rename with docs committed but meta not, the session title stays empty, so
`title-success.ts:47`'s `snapshot.title.trim()` guard still lets a later generation re-run;
that retry re-reads the (now stamped) doc bodies, so its CAS matches and it converges.
Reversing the order would commit the title first, which permanently suppresses any retry
and freezes the docs unstamped. Ordering meta last also preserves the existing code's
"documents first, title last" intent.

On a mid-loop rename failure: `discard_staged` the remainder, `tracing::error!` naming the
session and the file that failed, refresh the index for that session from disk so the index
never disagrees with the vault, and return `StoreError::Io` whose message states that a
partial application may have occurred.

**Phase 4 — index and notify,** only after every rename succeeded:
- `index_upsert_doc` per doc, `index_set_meta`, `index_set_note`
- **one** `notify_index_changed(Docs, vec![session_id])` and **one**
  `notify_index_changed(Sessions, vec![session_id])` — not one per file. The 10 ms
  coalescer would merge them anyway, but emitting once keeps the bus honest about what a
  single logical change is.

## 6. Frontend changes

`content-mutations.ts::applyGeneratedSessionTitle` collapses to one command call. Delete the
`sessionGet` pre-read (Rust now does that comparison under the lock, which is the point).
Keep the `enqueueDatabaseWrite("session:<id>")` wrapper — it still serializes against other
writers in this webview, and it is now belt-and-braces rather than the sole guard.

`title-success.ts` stops calling `applyGeneratedNoteTitle` and instead passes
`note: { expected_markdown: snapshot.rawMarkdown, next_markdown: ensureMarkdownFirstLineTitle(...) }`
when `snapshot.rawMarkdown.trim()` is non-empty. Delete `applyGeneratedNoteTitle`. Markdown
shaping (`ensureMarkdownFirstLineTitle`) stays on the frontend — it is presentation logic,
not storage, and moving it would drag a markdown-heading parser into Rust for no gain.

Error handling: keep throwing on `Conflict` so the caller's behaviour is unchanged, but the
throw now carries a guarantee it did not before — nothing was written. Use
`isStoreConflictError` to log a benign skip rather than an error.

## 7. Tests

Rust (`title_stamp.rs`), all against a tempdir vault:
1. **Happy path** — docs, note and meta all land; files and index agree; exactly one `Docs`
   and one `Sessions` notification.
2. **Doc CAS miss writes nothing** — two docs, second one stale. Assert `Conflict`, *and*
   that doc #1's bytes and `_meta.json`'s title are untouched. **This is the regression test
   for the reported bug; verify it fails against `main`'s per-call implementation.**
3. **Session title CAS miss writes nothing** — same assertions.
4. **Note CAS miss is not fatal** — docs and meta commit, `note_skipped == true`, note file unchanged.
5. **Staging failure writes nothing** — force a stage error (e.g. a doc path made
   un-writable) and assert no target changed and no `.tmp-` sibling survives.
6. **No temp files survive** either outcome — glob the session dir for `.tmp-` after success and after abort.
7. **Aborted batch does not lose `_meta.json`** — the §4 trash caveat: externally modify
   `_meta.json`, run a batch that aborts on a doc CAS, assert `_meta.json` still exists
   (in place or recoverable from `.trash`).
8. **Duplicate doc id rejected** before any write.
9. **Path traversal rejected** — a doc id of `../../escaped` fails validation, no file outside the vault.
10. **Index matches disk after commit**, including the stripped-note invariant from `0aaedff`.

Frontend: rewrite the `applyGeneratedSessionTitle` tests against the single command mock;
add one asserting a `conflict:` rejection leaves the caller's state untouched and logs
rather than throws an unhandled error. Existing `title-success` tests must keep passing with
the note folded in.

## 8. Gates

- `cargo check -p desktop`, `cargo test -p desktop --lib` — baseline **194 passed, 0 failed**
- `pnpm -F desktop typecheck`, `pnpm -F desktop test` — baseline **1159 tests / 168 files**
- Bindings regenerate diff-free via the in-suite `export_types` test
- `cargo fmt -p desktop`; dprint on changed files only (repo-wide times out on this box)

## 9. Risks

- **Longer lock hold.** The batch holds the write lock across N reads plus N staged writes.
  Docs are small and this runs once per generated title, but it is strictly longer than
  today's three short holds. Acceptable; note it in the doc comment.
- **The §4 trash-at-stage interaction** is the subtlest part of this plan and deserves the
  most scrutiny in review. Test 7 exists specifically to pin it.
- **Scope creep.** `session_replace_transcripts` and the tasks writers have the same
  shape and could be re-expressed on `stage`/`commit`. Do not do it here; land this, then
  consider it separately.

## 10. Deliberately not doing

- A write-ahead log with boot-time recovery. Correct in theory, disproportionate here, and
  it would add a durability surface more dangerous than the one it closes.
- Moving `ensureMarkdownFirstLineTitle` into Rust.
- Making the note participant fatal on CAS miss. That would be a behaviour change, not a
  fix — today's skip-the-note semantics are deliberate and documented.
