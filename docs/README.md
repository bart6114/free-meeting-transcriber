# Free Meeting Transcriber documentation

This Mintlify project holds the Free Meeting Transcriber docs. It is not
currently published anywhere.

## Local preview

Install the Mintlify CLI, then run it from this directory:

```bash
npm install --global mint
cd docs
mint dev
```

Update `docs.json` whenever a page is added, moved, or removed. Keep CLI and MCP reference content aligned with `apps/cli/src/cli.rs` and `apps/cli/src/mcp.rs`.

Before deploying, run:

```bash
mint validate
mint broken-links --check-anchors --check-redirects
```
