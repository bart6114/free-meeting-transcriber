# Free Meeting Transcriber

So this is my fork of [anarlog](https://github.com/fastrepl/anarlog). I built
it because I needed it: a meeting notetaker that transcribes on-device and
writes plain markdown files to disk, without all the "let's capture customers"
cruft. No cloud backend, no accounts, no billing, no telemetry, no upsells.
Actually free as in beer.

Bring your own LLM for summaries and chat (OpenAI, Anthropic, Gemini,
OpenRouter, Ollama, LM Studio, or anything OpenAI-compatible).

Fair warning: this scratches my own itch. If it's useful to you too, great.

## How to use it

Build it yourself (see Development below) and run it. Join a meeting, it
records and transcribes locally, and your notes end up as markdown on disk.
That's it, really.

## Why

- **Your data, your disk.** Every meeting is a `.md` file you can inspect,
  search, and sync however you like (Dropbox, iCloud, Syncthing, git).
- **Local transcription.** Audio never leaves your machine.
- **Bring your own AI.** Any LLM provider, including local models via Ollama
  or LM Studio.
- **No accounts, no tracking.** There's nothing to sign up for and nobody to
  phone home to.
- **CLI + MCP included.** The bundled `fmtr` CLI and MCP server give scripts
  and coding agents read-only access to your meeting notes.

## Development

It's a pnpm-workspace monorepo: a Tauri desktop app (`apps/desktop/`) plus a
Rust CLI (`apps/cli/`). There is no database. The markdown files in your vault
directory are the only source of truth (vault format lives in
`crates/vault-read/`), with Zustand for UI state and TipTap for the editor.

Let's get it running:

```sh
pnpm install
pnpm -F @hypr/desktop tauri:dev   # run the desktop app
cargo build -p fmtr-cli            # build the fmtr CLI
```

See [AGENTS.md](./AGENTS.md) for the fuller dev guidance (formatting,
typechecking, code-style conventions).

## License

MIT. See [LICENSE](./LICENSE) for the full license and copyright history,
which includes the upstream
[anarlog](https://github.com/fastrepl/anarlog) copyright.
