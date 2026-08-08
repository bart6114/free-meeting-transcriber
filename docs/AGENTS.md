# Free Meeting Transcriber documentation instructions

## Scope

- This is the Mintlify project source. It is not currently published for
  this fork. There is no hosted docs site, but the content is kept as the
  source of truth for CLI and MCP behavior.
- Write for Free Meeting Transcriber users, developers, and agents using the
  CLI or MCP server.
- Configuration lives in `docs.json`; content pages are MDX.
- The public agent skill is maintained in `../skills/fmtr/`.

## Sources of truth

- Treat `apps/cli/src/cli.rs` as the CLI command contract.
- Treat `apps/cli/src/mcp.rs` as the MCP tool and resource contract.
- Treat current release automation as the source of truth for installation channels.
- Do not infer product behavior from raw vault files; the write path is `apps/desktop/src-tauri/src/session_store/`.

## Writing

- Use active voice and second person.
- Keep headings and sentences concise.
- Put the result before implementation detail.
- Use `Free Meeting Transcriber` for the product and `fmtr` for the executable.
- Use root-relative links between Mintlify pages. Do not reference a public
  docs URL in external instructions or agent metadata. None is published for
  this fork, so point to the GitHub repository instead.

## Accuracy boundaries

- Document only commands, options, tools, resources, and output behavior present in the source.
- Mark planned features and distribution channels as forthcoming.
- Never describe Homebrew or Windows binaries as available until release automation publishes them.
- Never tell agents to crawl or modify the vault files directly; agents go through the CLI or MCP server.
- Make it clear that CLI and MCP transcript commands return the complete transcript and can produce large responses.

## Verification

- Check `docs.json` after adding or moving a page.
- Run `pnpm exec dprint fmt docs skills` from the repository root.
- Run `pnpm exec dprint check docs skills` before submitting.
- Run `mint validate` and `mint broken-links --check-anchors --check-redirects` from `docs/` before deploying.
