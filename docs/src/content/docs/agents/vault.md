---
title: "Vault guide for agents"
description: "The standing AGENTS.md every vault carries: what Free Meeting Transcriber is, how the vault is structured, and how to work with it."
---

Free Meeting Transcriber (fmtr) is a local-first desktop app for meeting notes and transcription. Everything lives in a **vault**: a plain folder of Markdown and JSON files that is the only source of truth — there is no database or cloud copy. A copy of this page is kept at the vault root as `AGENTS.md`. Full, current documentation lives at https://freemeetingtranscriber.com/ — machine-readable indexes at https://freemeetingtranscriber.com/llms.txt and https://freemeetingtranscriber.com/llms-full.txt.

## Vault structure

```text
<vault>/
  AGENTS.md              this file (auto-regenerated)
  config.json            app configuration
  settings.json          app settings
  tags.json  tasks.json  people.json  events.json  calendars.json
  templates/  humans/  organizations/
  .trash/                soft-deleted files, kept by date; recoverable
  sessions/<id>/         one meeting per directory
    _meta.json           identity + metadata; its presence marks a session
    notes.md             the user's note (legacy vaults: _memo.md)
    transcript.json      speaker-labeled transcript
    tasks.json           session tasks
    audio.mp3|wav|ogg    the recording, with audio.peaks.json waveform cache
    enhanced/<uuid>.md   AI-generated documents (summaries)
    attachments/         files embedded in the note
```

Ownership rules:

- Inside a session directory the app owns exactly the names above. **Any other file is a user attachment: leave it alone**, and never claim unknown files as app content.
- Dot-prefixed files (`.tmp-*`, `.DS_Store`, `.trash/`) are never content.
- Do not create or rename files under the app-owned names; use the CLI to write.

## Authorship

`_meta.json` may carry an optional `author` field. When it is absent the note
was written by the vault owner; when set (a free-form name such as
`claude-code`) the note was written by someone else, and the app marks it as
not written by the owner.

Rules for agents:

- **Always pass `--author <your-agent-name>` when creating a meeting** with
  `fmtr meetings new` or `fmtr import`. Pick one stable name (for example
  `claude-code`) and keep using it.
- Write your own notes as **new** meetings with `--author` set. When asked to
  edit an existing note, never add, change, or remove its `author` — editing
  the owner's note does not make it yours.

## Reading meeting data

Use Free Meeting Transcriber's typed, read-only interfaces for meeting data.
Do not use `find`, `grep`, `rg`, filesystem crawling, or direct SQLite queries
to find or read meetings.

Prefer the fmtr MCP tools when they are available:

- `list_meetings` to resolve a meeting ID
- `get_meeting` for notes, summaries, and action items
- `get_meeting_transcript` for the full speaker-labeled transcript

If MCP is unavailable, use the fmtr CLI with `--json`:

```sh
fmtr --json meetings list --query "planning"
fmtr --json meetings get MEETING_ID
fmtr --json meetings transcript MEETING_ID
```

The CLI discovers Free Meeting Transcriber's vault from the platform
application-data directory, following the `vault_path` redirect in its
`global.json` when the vault has been relocated. Use
`--vault-path ABSOLUTE_VAULT_DIR` only when the user explicitly provides a
non-default vault path; do not crawl the filesystem to find one. Never guess a
meeting ID. Fetch a transcript only when notes and summaries do not contain
the needed context.

## The fmtr CLI

Run `fmtr doctor` first to verify the CLI can reach the vault (it also repairs
a missing or stale `AGENTS.md`). Always pass `--json` for machine-readable
output.

| Command | Purpose |
| --- | --- |
| `doctor` | Check CLI and vault access without changing data. |
| `meetings list` | List meetings, optionally filtered with `--query`. |
| `meetings search` | Full-text search across titles, notes, summaries, and transcripts. |
| `meetings get` | Metadata, note, summaries, and action items for one meeting. |
| `meetings new` | Create a meeting note and print its id; pass `--author` when writing as an agent. |
| `meetings note` | Show a meeting's note, or edit it with `--set` / `--append`. |
| `meetings transcript` | The full speaker-labeled transcript. |
| `meetings tag add` | Add tags to a meeting, registering new ones in the vault. |
| `meetings tag remove` | Remove tags from a meeting. |
| `meetings path` | Print the absolute path of a meeting's session directory. |
| `meetings attach` | Store a file as a note attachment and print its id. |
| `meetings export` | Export a meeting to Markdown or JSON. |
| `import` | Import an audio file as a new (or into an existing) meeting. |
| `transcribe` | Transcribe a meeting's audio with the configured on-device model. |
| `mcp` | Run the read-only MCP server over stdio. |
| `tags list` | List every tag registered in the vault. |

Per-command flags are documented at
https://freemeetingtranscriber.com/reference/cli/.
