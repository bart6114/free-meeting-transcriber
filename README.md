# Free Meeting Transcriber

A local-first, privacy-first AI meeting notetaker. It joins your meetings,
transcribes them entirely on-device, and turns the transcript into markdown
notes you own on disk — no cloud backend, no account, and no telemetry. Bring
your own LLM (OpenAI, Anthropic, Gemini, OpenRouter, Ollama, LM Studio, or
anything OpenAI-compatible) to summarize and chat about your meetings.

This is a private hard fork of [fastrepl/anarlog](https://github.com/fastrepl/anarlog)
(MIT-licensed). All hosted/cloud-sync, accounts, and billing functionality
has been removed — everything here runs locally on your own machine.

## How to use it

Build it yourself (see Development below) and run it. Join a meeting:
recording and transcription happen entirely on-device, and notes are saved
as markdown on disk.

## Why

- **Your data, your disk.** Every meeting is a `.md` file you can inspect,
  search, and sync yourself (Dropbox, iCloud, Syncthing, git). No cloud
  backend, no cloud lock-in.
- **Local transcription.** Transcription runs on-device; audio never leaves
  your machine.
- **Bring your own AI.** Any LLM provider, including OpenAI-compatible
  services and local models (Ollama, LM Studio).
- **No accounts, no tracking.** There is no hosted account model and no
  telemetry.
- **CLI + MCP included.** The bundled `fmtr` CLI and MCP server give scripts
  and coding agents read-only access to your local meeting data.

## Development

This is a pnpm-workspace monorepo: a Tauri desktop app (`apps/desktop/`) plus
a Rust CLI (`apps/cli/`), built on SQLite (schema/migrations in
`crates/db-app/`) with Zustand for UI state and TipTap for the editor.

```sh
pnpm install
pnpm -F @hypr/desktop tauri:dev   # run the desktop app
cargo build -p anarlog-cli         # build the fmtr CLI
```

See [AGENTS.md](./AGENTS.md) for the fuller dev-guidance (formatting,
typechecking, code-style conventions).

## Provenance

Forked from [fastrepl/anarlog](https://github.com/fastrepl/anarlog), MIT
licensed. See [LICENSE](./LICENSE) for the full license and copyright
history.

---

**License:** MIT · **Issues:** [github.com/bart6114/free-meeting-transcriber/issues](https://github.com/bart6114/free-meeting-transcriber/issues)
