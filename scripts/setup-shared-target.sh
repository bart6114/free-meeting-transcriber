#!/usr/bin/env bash
# Symlink this checkout's cargo target dirs to a machine-shared cache so fresh
# worktrees reuse compiled dependencies instead of paying the cold build.
# Safe to run repeatedly; seeds the cache from an existing local build.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
legacy_cache="$HOME/.cache/fmtr"
cache="${LOOFAH_TARGET_CACHE:-${FMTR_TARGET_CACHE:-$HOME/.cache/loofah}}"

if [ "$cache" = "$HOME/.cache/loofah" ] && [ ! -e "$cache" ] && [ -d "$legacy_cache" ]; then
  mv "$legacy_cache" "$cache"
fi

link_target() {
  local dir="$1" shared="$2"
  if [ -L "$dir" ]; then
    return
  fi
  mkdir -p "$shared"
  if [ -d "$dir" ]; then
    if [ -z "$(ls -A "$shared")" ]; then
      rmdir "$shared"
      mv "$dir" "$shared"
    else
      rm -rf "$dir"
    fi
  fi
  ln -s "$shared" "$dir"
  echo "linked $dir -> $shared"
}

link_target "$root/target" "$cache/target-root"
link_target "$root/apps/desktop/src-tauri/target" "$cache/target-tauri"
