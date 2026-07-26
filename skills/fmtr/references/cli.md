# CLI commands

Use `--json` for agent-readable output.

```bash
fmtr --json doctor
fmtr --json meetings list --query "planning" --limit 20 --offset 0
fmtr --json meetings get MEETING_ID
fmtr --json meetings note MEETING_ID --kind note
fmtr --json meetings note MEETING_ID --kind summary
```

`doctor` exits with status 1 when its response contains `ready: false`.

Read transcripts in bounded word pages:

```bash
fmtr --json meetings transcript MEETING_ID --limit 200 --offset 0
```

JSON success responses contain `schema_version`, `command`, `data`, and optional `pagination`. Continue from `pagination.next_offset` only when more context is necessary.

Export is intended for an explicit user request to save or transfer a complete meeting:

```bash
fmtr meetings export MEETING_ID --format markdown --output meeting.md
fmtr meetings export MEETING_ID --format json --output meeting.json
```

Export refuses to replace an existing file. Pass `--force` only after the user explicitly approves overwriting that exact path.

Global vault overrides:

```bash
fmtr --vault-path /path/to/vault --json meetings list
fmtr --base /path/to/vault --json meetings list
```
