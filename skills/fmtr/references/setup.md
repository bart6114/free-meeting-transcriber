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

The CLI currently installs from source:

```bash
git clone https://github.com/bart6114/free-meeting-transcriber.git
cd free-meeting-transcriber
cargo install --locked --path apps/cli
fmtr --version
```

Run the Free Meeting Transcriber desktop app at least once so its local database exists. Homebrew, desktop-bundled, and Windows binary distribution are planned but not yet available.

Use `--db-path FILE` or `FMTR_DB_PATH` only when the database is outside Free Meeting Transcriber's default application-data location.
