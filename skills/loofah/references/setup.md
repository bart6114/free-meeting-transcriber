# Setup

## MCP

Run the local stdio server with:

```bash
loofah mcp
```

A generic client configuration is:

```json
{
  "mcpServers": {
    "loofah": {
      "command": "loofah",
      "args": ["mcp"]
    }
  }
}
```

Restart the client after changing its MCP configuration.

## CLI

Install the prebuilt binary on macOS or Linux:

```bash
curl -fsSL https://loofah.io/install.sh | bash
```

Or install from source:

```bash
git clone https://github.com/bart6114/loofah.git
cd loofah
cargo install --locked --path apps/cli
loofah --version
```

Run the Loofah desktop app at least once so its local vault exists. Homebrew and Windows binary distribution are planned but not yet available.

Use `--vault-path DIR` or `LOOFAH_VAULT_PATH` only when the vault is outside Loofah's default application-data location.
