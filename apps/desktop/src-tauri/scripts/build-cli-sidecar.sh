#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src_tauri_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$src_tauri_dir/../../.." && pwd)"

host_triple="$(rustc -vV | awk '/^host: /{print $2}')"
triple="${1:-$host_triple}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"

cd "$repo_root"
if [ "$triple" = "$host_triple" ]; then
  cargo build --locked --release -p fmtr-cli
  built="$target_dir/release/fmtr"
else
  cargo build --locked --release -p fmtr-cli --target "$triple"
  built="$target_dir/$triple/release/fmtr"
fi

install -m 755 "$built" "$src_tauri_dir/binaries/fmtr-$triple"
# Extra copy so the dev app (no externalBin in the dev config) finds the CLI
# via embedded_cli.rs's resources/cli fallback.
install -m 755 "$built" "$src_tauri_dir/resources/cli/fmtr-$triple"
echo "[build-cli-sidecar] fmtr -> binaries/fmtr-$triple (+ resources/cli copy)"
