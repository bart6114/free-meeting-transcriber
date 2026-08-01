# Free Meeting Transcriber

A fork of [anarlog](https://github.com/fastrepl/anarlog), stripped down to
just the part I actually needed: a local-first AI meeting notetaker that
transcribes on-device and writes markdown notes to disk.

This is software for an audience of one — I built it because I needed it.
It's an actually free-as-in-beer thing, with all the capture-the-customer
cruft removed: no cloud backend, no accounts, no billing, no telemetry, no
upsells. If it's useful to you too, great.

Bring your own LLM (OpenAI, Anthropic, Gemini, OpenRouter, Ollama, LM
Studio, or anything OpenAI-compatible) to summarize and chat about your
meetings.

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
a Rust CLI (`apps/cli/`). There is no database — the markdown files in your
vault directory are the only source of truth (vault format in
`crates/vault-read/`), with Zustand for UI state and TipTap for the editor.

```sh
pnpm install
pnpm -F @hypr/desktop tauri:dev   # run the desktop app
cargo build -p fmtr-cli            # build the fmtr CLI
```

See [AGENTS.md](./AGENTS.md) for the fuller dev-guidance (formatting,
typechecking, code-style conventions).

## License

MIT licensed. See [LICENSE](./LICENSE) for the full license and copyright
history, which includes the upstream
[anarlog](https://github.com/fastrepl/anarlog) copyright.

---

**License:** MIT · **Issues:** [github.com/bart6114/free-meeting-transcriber/issues](https://github.com/bart6114/free-meeting-transcriber/issues)
