---
name: desktop-release
description: Use when cutting/shipping a stable desktop release, building the signed DMG, running the desktop-release workflow, or writing/backfilling release notes ("What's new" changelog) for a version.
---

# Desktop Release

Releases are two-stage: every push to `main` auto-creates a tagged GitHub release
(`release.yaml` — version bump from conventional commits, `--generate-notes`, no binary).
The signed + notarized DMG and updater artifacts are built on demand by the
`desktop-release` workflow. This skill runs that second stage and makes sure real
release notes exist first.

## 1. Preflight

```bash
git status --short                 # must be clean
git pull --rebase origin main      # bot bump commits mean local main is always behind
gh auth status
```

Never tag or bump versions by hand — the tag for pushed work already exists.

**All CI on the release commit must be green before triggering the release build.**
Check every workflow run for the commit the target tag points at (bot bump commits
carry `[skip ci]`, so the runs usually live on its parent — walk back to the first
commit that has runs):

```bash
gh run list --commit $(git rev-parse <tag>^{commit}) \
  --json name,status,conclusion \
  --jq '.[] | "\(.name): \(.status) \(.conclusion // "")"'
```

Every run must show `completed success`. If any run failed, is still in progress,
or the commit has no runs (check the parent), stop and resolve that first — do not
`gh workflow run desktop-release` on a commit with red or pending CI.

## 2. Resolve the target tag

User-specified, or the latest release:

```bash
gh release view --json tagName --jq .tagName
```

## 3. Ensure release notes exist

The in-app "What's new in X?" dialog reads `packages/changelog/content/<version>.md`
(no leading `v`): bundled into the build when it's the highest-numbered file at the tag,
otherwise fetched at runtime from
`https://raw.githubusercontent.com/bart6114/free-meeting-transcriber/main/packages/changelog/content/<version>.md`
— i.e. from `main`, so backfilling notes for an already-tagged version works.

If `<version>.md` is missing:

1. Draft it from `git log <prev-tag>..<tag> --format='%s%n%b'`, skipping
   `chore(release)` bot commits. Write for end users, not developers. Format
   (see existing files in `packages/changelog/content/`):

   ```markdown
   ---
   date: "YYYY-MM-DD"
   summary: "Version X.Y.Z one-sentence summary."
   ---

   ## Section

   - User-facing bullet
   ```

2. **Show the draft to the user for approval before committing.**
3. Commit and push with `[skip ci]` in the message — **required**: without it the
   push triggers `release.yaml`, which cuts a new version that again has no
   changelog, forever.

   ```bash
   git commit -m "docs: changelog for vX.Y.Z [skip ci]"
   ```

## 4. Fill the GitHub release body

`--generate-notes` only produces a compare link (no PRs on this repo). Put the real
notes there too: strip the frontmatter from `<version>.md`, keep the existing
`**Full Changelog**` compare link at the bottom, then:

```bash
gh release edit <tag> --notes-file <file>
```

## 5. Build the DMG

```bash
gh workflow run desktop-release -f tag=<tag>
sleep 10 && gh run list --workflow=desktop-release --limit 1
gh run watch <run-id>              # full macOS build + notarization, ~15-30 min
```

## 6. Verify

All 5 assets on the release, and the updater feed advanced:

```bash
gh release view <tag> --json assets --jq '.assets[].name'
# expect: .dmg, .dmg.sha256, .app.tar.gz, .app.tar.gz.sig, latest.json
gh release download updater --pattern latest.json -O - | jq -r .version
curl -fsS https://raw.githubusercontent.com/bart6114/free-meeting-transcriber/main/packages/changelog/content/<version>.md >/dev/null && echo notes-ok
```

## Gotchas

- **Never delete the `updater` prerelease** — it's the endpoint shipped apps poll.
  The feed is version-guarded and only moves forward (re-running an old tag won't
  downgrade it).
- The bundled changelog fast path only covers the highest-numbered content file at
  the tag; every other version depends on the runtime fetch from `main` — so notes
  must be on `main` before users first open the "What's new" dialog (it fires on
  first launch after an update).
- Changelog-only commits without `[skip ci]` spawn a fresh release: the release
  loop never converges.
