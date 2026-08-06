---
name: fmtr
description: Query local Free Meeting Transcriber meetings, notes, summaries, transcripts, and action items. Use when a user asks about their Free Meeting Transcriber meeting data or wants meeting context for another task.
---

# Free Meeting Transcriber

Use Free Meeting Transcriber's read-only data surfaces. Prefer the MCP server when its tools are connected. Otherwise use the `fmtr` CLI with `--json`.

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
- Do not claim to update meetings. The current CLI and MCP server cannot mutate Free Meeting Transcriber data.
- CLI export may create a separate file. Never pass `--force` unless the user explicitly approves overwriting that exact path.
- Preserve uncertainty when search results are ambiguous. Ask the user to choose between likely meetings.

For setup and failures, see [setup](references/setup.md) and [errors](references/errors.md).
