# Baseline (fork-base, upstream v1.3.7, 2026-07-23)

- `pnpm install`: clean (pnpm 11.1.1 via corepack; needed one-time `COREPACK_INTEGRITY_KEYS=0 corepack install` due to stale corepack keys in Node 22.13).
- `pnpm --dir apps/desktop typecheck`: PASS.
- `pnpm --dir apps/desktop test`: PASS — 254 files, 1956 tests.
- `cargo check -p desktop`: recorded below after first full compile.

No pre-existing failures; all later gates must stay fully green.

- `cargo check -p desktop`: PASS (`Finished dev profile in 4m 13s`). Required one-time host fix: `xcodebuild -downloadComponent MetalToolchain` (Metal Toolchain 17A324) for the soniqo MLX build script.
