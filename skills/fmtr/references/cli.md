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

Search across titles, notes, summaries, and transcript words (query and/or `--speaker` required; transcript hits return a `start_ms` matching the transcript's timestamps):

```bash
fmtr --json meetings search "budget forecast" --limit 20
fmtr --json meetings search --speaker "bob" --kind transcript
```

Read the full speaker-labeled transcript (`[HH:MM:SS] Speaker: ...` lines):

```bash
fmtr --json meetings transcript MEETING_ID
```

JSON success responses contain `schema_version`, `command`, `data`, and optional `pagination`. Continue from `pagination.next_offset` only when more context is necessary.

Create a meeting note (prints the new meeting id; `--note` seeds the body from a file, or stdin with `-`):

```bash
fmtr --json meetings new --title "Weekly sync" --note notes.md
echo "Agenda" | fmtr --json meetings new --title "Weekly sync" --note -
```

Edit an existing meeting's note (`--set` replaces, `--append` adds after a separating newline; exactly one of the two, fails if the meeting does not exist):

```bash
fmtr --json meetings note MEETING_ID --set notes.md
echo "Follow-up" | fmtr --json meetings note MEETING_ID --append -
```

Import an audio file as a new meeting (prints the new meeting id; the audio is converted into the vault's format; accepts wav, mp3, ogg, mp4, m4a, flac, webm, or aac; `--title` defaults to the file name; transcription does not run):

```bash
fmtr --json import recording.m4a --title "Weekly sync"
```

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
