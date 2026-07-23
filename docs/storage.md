# Storage: the vault is canonical

Free Meeting Transcriber keeps your data as plain files in a folder you
choose (the **vault**). The app's local SQLite database (`app.db`) and its
search index are both disposable caches, rebuilt from the vault on demand.
If you've ever wondered "what actually happens if I edit a file by hand,
delete `app.db`, or point this folder at Google Drive" — this page is the
answer.

## Two locations, two different guarantees

| | Vault (`vault_base`) | App data (`global_base`) |
|---|---|---|
| **What lives here** | `sessions/`, `humans/`, `organizations/`, `chats/`, `calendars.json`, `events.json`, `daily_notes.json`, `tasks.json`, `settings.json`, `.trash/`, audio recordings, attachments | `app.db` (SQLite), `search_index/` (Tantivy) |
| **Is it your data?** | Yes — this is the thing you back up, sync, and can read without the app | No — a rebuildable cache; deleting it costs you a few seconds on next launch, nothing more |
| **Where it is** | Wherever you point it in **Settings → Storage** (defaults to the same folder as App data until you change it) | Your OS's per-app data directory (e.g. `~/Library/Application Support/<bundle id>` on macOS), fixed — never follows the vault |
| **Safe to sync to Drive/iCloud/Dropbox?** | Yes, that's the point | No — and it never ends up there, even if you point your vault at a synced folder |

The vault and the app-data directory are the *same folder* on a fresh
install (before you've ever changed anything in Settings). The moment you
choose a different storage location — typically to point the vault at a
Drive/iCloud/Dropbox-synced folder — only the vault content moves.
`app.db` and `search_index/` always stay behind in the fixed OS app-data
location, so your sync backend only ever sees your notes and files, never
the disposable cache. This is deliberate (see "Google Drive and other
synced folders" below).

## Vault layout

| Path | Contents | Written by |
|---|---|---|
| `sessions/<folder>/<id>/_meta.json` | Session title, timestamps, calendar event fields, participants, tags, key facts | DB → vault export |
| `sessions/<folder>/<id>/_memo.md` | The first note document for the session | DB → vault export |
| `sessions/<folder>/<id>/_summary.md` | The first AI summary for the session | DB → vault export |
| `sessions/<folder>/<id>/<document-id>.md` | Any additional note/summary/template-output document | DB → vault export |
| `sessions/<folder>/<id>/transcript.json` | All transcripts for the session | DB → vault export |
| `sessions/<folder>/<id>/attachments/` | File attachments | Written directly by the app when you attach a file — **file-native**, never rendered from the DB and never overwritten by the export worker |
| `sessions/<folder>/<id>/audio.{mp3,wav,ogg}` | The recording itself | Written directly by the recorder/importer — **file-native**, same as attachments |
| `humans/<id>.md` | A contact | DB → vault export |
| `organizations/<id>.md` | An organization | DB → vault export |
| `chats/<chat-group-id>/messages.json` | An AI chat thread | DB → vault export |
| `calendars.json` / `events.json` / `daily_notes.json` / `tasks.json` | Whole-table snapshots, re-rendered on every change to the underlying table | DB → vault export |
| `settings.json` | A single legacy settings blob (see note below) | DB → vault export |
| `templates.json` | Custom templates | **Import-only** — read on startup if present (e.g. carried over from an older install), never written back out. There's no live round-trip for templates yet. |
| `.conflict-<timestamp>.<ext>` | A point-in-time backup of DB content that lost a files-win conflict | Legacy-vault reconciler (see "Files win" below) |
| `.trash/<YYYY-MM-DD>/<original relative path>` | Anything the app removed or overwrote, preserved instead of deleted | Vault export worker, on every deletion and every content-changing overwrite |
| `.fmt-export-version` | A marker file; its mere presence (not its contents) records "this vault has already had its one-time full export" | Vault export worker, first run only |

`settings.json` only mirrors one legacy row (`app_settings.legacy_settings_document`) — the rest of the app's
settings table (provider configs, sync bookkeeping) intentionally has no vault file.

## Authority rules

### 1. New content is always additive

Any file the app has never seen before — a new `_meta.json`, a new contact,
a hand-written `.md` dropped into `sessions/some-folder/`, a whole folder
copied in from another vault — gets imported as a new row. This applies to
every entity kind.

### 2. "Files win" — but only for session content

If you hand-edit a file that already has a matching database row with
**different** content, one of two things happens, depending on what kind of
file it is:

- **Sessions, session documents (`_memo.md`, `_summary.md`, etc.), and
  transcripts**: the file wins. Your edit is imported into the database,
  and whatever the database used to hold is written out as a sibling
  `<name>.conflict-<RFC3339 timestamp>.<ext>` backup next to the live file —
  nothing is silently discarded. This is the "legacy SQLite is
  authoritative" rule turned inside-out for the common, expected case: you
  edited a note in your editor or synced client, and you want that edit to
  stick.
- **Everything else** (contacts, organizations, calendars, events, daily
  notes, tasks, chats, settings): the conflict is recorded internally but
  **not** resolved in favor of the file. The database's content keeps
  winning here — an external edit to `humans/<id>.md` sits unintegrated
  until the next time you edit that contact *in the app*, at which point
  the export worker overwrites your external edit with the database's
  version (trashing your edit first, per rule 3 below, so it isn't lost —
  just superseded). There's currently no UI that surfaces or resolves these
  conflicts; they're a quiet, additive-only design constraint rather than a
  first-class feature.

### 3. Trash-before-overwrite: nothing is ever silently destroyed

Every time the vault export worker is about to overwrite a file with
different content, or remove one because its database row is gone, it
first moves the existing file to `.trash/<today's date>/<its original
relative path>`. This is true for a single file overwrite (e.g. re-rendering
a note after you edited its title) and for a whole session folder being
removed. If two things would land at the same trash path on the same day,
the second one gets a `-1`, `-2`, ... suffix — nothing in `.trash/` ever
clobbers anything else in `.trash/`.

Byte-identical writes are skipped entirely (no trash, no write, nothing
happens) — this is what keeps the export worker from generating endless
no-op churn every time it re-renders unchanged content.

### 4. In-app delete vs. external delete: different meanings, both safe

- **Deleting a note in the app** marks it deleted in the database. The
  export worker notices and moves the whole session folder to
  `.trash/<date>/sessions/<id>/` — recoverable, and treated as final once
  the undo window passes.
- **Deleting `_meta.json` outside the app** (a sync client blip, an
  accidental `rm`, a half-finished manual edit) is read by the live watcher
  as "this session might be gone" and **soft-hides** it — same
  `deleted_at` column, but flagged internally as an *external* hide. The
  export worker sees that flag and deliberately does **not** touch the
  remaining files: an external actor owns them, and a transient
  delete-then-recreate (some sync clients do this) shouldn't cause the app
  to go dismantle a folder it didn't decide to remove. If `_meta.json`
  reappears — same bytes or different — the session is revived exactly as
  it was.
- The practical difference: an in-app delete's files end up in `.trash/`;
  an externally-triggered hide leaves its files exactly where they were,
  inert, until you either restore them yourself or the session's
  `_meta.json` comes back.

### 5. An imported external edit gets reformatted — expected, not a bug

Once an external edit to a note is imported (rule 2), the export worker
re-renders it from the database's own field set. That render is not
guaranteed to be byte-identical to what you (or your editor, or your sync
client) actually wrote — it's a clean projection of `kind`/`title`/`body`/
etc., not a copy of your file's exact key order or whitespace. So the very
next export pass commonly rewrites the file again, in the app's canonical
shape (your content survives; the formatting doesn't). This is a one-time
settling step, not a live back-and-forth: the app marks every write it
makes as its own before making it, so the live file watcher never reacts
to it, and the vault reaches a fixed point — one import, at most one
reformat, then quiet.

## Rebuilding: delete `app.db`, nothing is lost

`app.db` and `search_index/` are caches. If either is deleted, corrupted,
or simply missing (e.g. you're moving to a new machine and only copied the
vault folder), the next launch:

1. Creates a fresh, empty `app.db`.
2. Walks the entire vault and re-imports everything into it (sessions,
   documents, transcripts, contacts, organizations, calendars, chats,
   tasks, daily notes) — this is the same reconcile pass that runs on
   every startup, just with nothing to compare against yet.
3. Rebuilds the search index from the freshly-populated database.

Your notes, transcripts, recordings, and attachments are exactly as they
were, because they were never anywhere else.

## Revival: transient blips heal themselves

A session that gets externally soft-hidden (rule 4) and then has its
`_meta.json` reappear — with the same content or different — comes back
automatically, whether that happens while the app is running (the live
watcher notices) or the next time it starts (the regular startup
reconcile). You don't need to do anything for a sync client's momentary
delete-then-recreate to resolve itself.

## Google Drive and other synced folders

Pointing your vault at a folder that Google Drive (or iCloud Drive,
Dropbox, etc.) syncs is supported and is exactly what the file-based
design is for. A few things worth knowing:

- **The app is offline-first.** Every write lands on your local disk
  immediately; Drive/iCloud sync happens in the background, on its own
  schedule, entirely outside the app. You are never blocked waiting for a
  cloud round-trip.
- **`app.db` and `search_index/` never enter the synced folder** — see
  "Two locations" above. Your Drive folder only ever contains the vault:
  notes, transcripts, recordings, attachments, and `.trash/`.
- **The first time you point an existing, "legacy" vault at this app**
  (one that has files but has never been exported by this write-through
  mirror), expect a one-time burst of re-writes: the export worker
  re-renders every session, contact, and calendar file once so that its
  format matches what the importer expects to read back. After that single
  pass, only your actual edits cause writes. This is the same mechanism as
  rule 5 above, just applied to everything at once instead of one file.

## Known residuals (honest gaps, not hidden)

- **`.trash/` has no retention policy yet.** Nothing currently prunes old
  dated folders under `.trash/`. Over a long enough time this can
  accumulate; cleaning it up today is a manual, "look inside `.trash/` and
  delete what you don't need" operation.
- **A permanent external deletion leaves its files inert, not cleaned up.**
  If a session folder's `_meta.json` is deleted outside the app and it
  really was intentional (not a sync blip), the session disappears from
  the app's UI, but any remaining files (attachments, audio) are left
  exactly where they were — nothing trashes them, since the app
  deliberately never acts on files it didn't decide to touch (rule 4). This
  is a trade-off, not an oversight: the alternative (auto-trashing) would
  risk destroying a folder mid-sync during a transient blip.
- **The startup vault scan still walks into `.trash/`.** It reads and
  hashes those files like anything else, then discards them, since nothing
  under `.trash/...` matches a recognized vault path shape. Harmless today
  (a little wasted I/O on large trash folders), not a correctness issue.
- **A legacy direct-delete fallback still exists for in-app note
  deletion**, alongside the trash-based deletion described in rule 4. A few
  seconds after you delete a note (once the undo toast expires), the app
  also asks the filesystem layer to remove anything still sitting at the
  note's original path — a plain, non-trash removal. In practice this
  almost always finds nothing there, because the database-triggered export
  worker's trash step runs first (within roughly half a second of the
  delete, well inside the several-second undo window). It is not a
  structural guarantee, though: if the export worker were ever meaningfully
  delayed, this fallback would permanently remove files that would
  otherwise have landed safely in `.trash/`. Flagged for follow-up rather
  than changed here, since removing this fallback is a product decision
  about whether "permanently delete" should ever mean *actually* permanent.
