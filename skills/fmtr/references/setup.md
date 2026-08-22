# Setup

## MCP

Run the local stdio server with:

```bash
fmtr mcp
```

A generic client configuration is:

```json
{
  "mcpServers": {
    "fmtr": {
      "command": "fmtr",
      "args": ["mcp"]
    }
  }
}
```

Restart the client after changing its MCP configuration.

## CLI

Install the prebuilt binary on macOS or Linux:

```bash
curl -fsSL https://freemeetingtranscriber.com/install.sh | bash
```

Or install from source:

```bash
git clone https://github.com/bart6114/free-meeting-transcriber.git
cd free-meeting-transcriber
cargo install --locked --path apps/cli
fmtr --version
```

Run the Free Meeting Transcriber desktop app at least once so its local vault exists. Homebrew and Windows binary distribution are planned but not yet available.

Use `--vault-path DIR` or `FMTR_VAULT_PATH` only when the vault is outside Free Meeting Transcriber's default application-data location.
