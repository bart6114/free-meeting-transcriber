# Loofah documentation

This Astro Starlight project holds the Loofah docs, published
at https://loofah.io via a Cloudflare Worker serving static
assets.

## Local preview

```bash
pnpm install
pnpm -F @hypr/docs dev
```

Content pages live in `src/content/docs/`. Update the `sidebar` in
`astro.config.mjs` whenever a page is added, moved, or removed. Keep CLI and
MCP reference content aligned with `apps/cli/src/cli.rs` and
`apps/cli/src/mcp.rs`.

## Build and deploy

```bash
pnpm -F @hypr/docs build     # static output in docs/dist/
pnpm -F @hypr/docs deploy    # build + wrangler deploy (needs Cloudflare auth)
```

Pushes to `main` that touch `docs/` deploy automatically through
`.github/workflows/docs_deploy.yaml` (requires the `CLOUDFLARE_API_TOKEN` and
`CLOUDFLARE_ACCOUNT_ID` repository secrets). The canonical Worker
(`loofah-docs`) is configured in `wrangler.jsonc`.
`wrangler.redirect.jsonc` keeps the old domains and `www.loofah.io`
redirecting to `https://loofah.io` while preserving paths and query strings.
