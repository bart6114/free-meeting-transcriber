---
name: run-tests
description: Run the repository's pull-request CI checks locally. Use before pushing or merging changes, when diagnosing a GitHub Actions failure, or when asked to run the full test suite; use narrower project commands for quick iteration when full CI parity is unnecessary.
---

# Run Tests

Run the checked-in script from the repository root:

```bash
bash .agents/skills/run-tests/scripts/run.sh all
```

`all` mirrors the validation jobs from `ci.yaml`, `cli_ci.yaml`, `desktop_ci.yaml`, and `zizmor.yaml`: formatting, desktop lint/frontend/Rust checks, CLI checks and smoke tests, docs build, and workflow security analysis. It intentionally excludes deployment, release, signing, and desktop E2E workflows.

For iteration, replace `all` with one or more groups: `fmt`, `lint`, `frontend`, `rust`, `cli`, or `workflows`. Run `all` before reporting that a branch is ready to push or merge.

The Rust CI job is macOS-specific. On a fresh worktree, run `scripts/setup-shared-target.sh` first. The script installs locked pnpm dependencies and fails fast on the first failing CI group. If a CI workflow or its local composite actions changed, compare the script with the changed workflow before trusting parity and update both together when needed.

Do not fix unrelated failures automatically. Report the failing group and decisive output; make changes only when the user requested implementation or the failure is caused by the current task.
