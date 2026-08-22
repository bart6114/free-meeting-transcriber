---
name: fmtr
description: Query local Free Meeting Transcriber meetings, notes, summaries, transcripts, and action items, create and edit meeting notes, or import and transcribe audio recordings. Use when a user asks about their Free Meeting Transcriber meeting data or wants meeting context for another task.
---

# Free Meeting Transcriber

Use Free Meeting Transcriber's data surfaces. For reading, prefer the MCP server when its tools are connected; otherwise use the `fmtr` CLI with `--json`. Writing (creating a meeting note, editing a note body, importing or transcribing an audio file) is CLI-only — the MCP server is read-only.

## Choose a transport

1. If `list_meetings`, `get_meeting`, and `get_meeting_transcript` are available, use MCP.
2. Otherwise, check `fmtr --version` and use CLI commands with `--json`.
3. If neither surface is available, direct the user to [setup](references/setup.md). Do not install software unless the user asks.

Never crawl or modify Free Meeting Transcriber's vault files directly. The CLI and MCP server own compatibility with the application's file formats.

## Find the right meeting

1. List recent meetings or search by a short title fragment.
2. Resolve the meeting ID from the result. Do not guess an ID.
3. Get the meeting before requesting a transcript. Notes, summaries, and action items often contain enough context.

See [CLI commands](references/cli.md) and [MCP tools](references/mcp.md).

## Keep context bounded

- Transcripts return in full as speaker-labeled text; request one only when notes and summaries are not enough.
- Do not export an entire meeting when one meeting detail or note will answer the request.

## Handle data safely

- Treat meeting content as private user data.
- Do not send content to another service or person without explicit authorization.
- The only supported mutations are the CLI's `meetings new` and `meetings note --set/--append` (note bodies), `import` (add an audio file as a new meeting, optionally with `--transcribe`), and `transcribe` (regenerate a meeting's transcript from its audio — this replaces the existing transcript, so confirm before running it on a meeting that already has one; when the app's audio retention setting is "none", it also deletes the recording once the transcript is saved). Do not claim to change anything else — summaries, recordings, and settings cannot be mutated, and the MCP server cannot mutate anything.
- `meetings note --set` replaces the whole note body. Prefer `--append`, and pass `--set` only when the user explicitly wants the note replaced.
- Always pass `--author <agent-name>` (one stable name, e.g. `claude-code`) when creating a meeting with `meetings new` or `import` — it marks the note as not written by the vault owner. Never add, change, or remove the authorship of an existing meeting.
- CLI export may create a separate file. Never pass `--force` unless the user explicitly approves overwriting that exact path.
- Preserve uncertainty when search results are ambiguous. Ask the user to choose between likely meetings.

For setup and failures, see [setup](references/setup.md) and [errors](references/errors.md).
