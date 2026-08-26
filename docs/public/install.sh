#!/usr/bin/env bash
# Installer for the loofah CLI (https://loofah.io).
#
#   curl -fsSL https://loofah.io/install.sh | bash
#
# Downloads the latest release binary for this platform, verifies its
# checksum, and installs it to ~/.local/bin/loofah (override with
# LOOFAH_INSTALL_DIR; FMTR_INSTALL_DIR remains supported). Re-run to upgrade.
# Uninstall with: rm ~/.local/bin/loofah ~/.local/bin/fmtr

set -euo pipefail

# Persist $1 on PATH for the user's login shell. The script runs under
# `curl | bash`, so $SHELL (not the running interpreter) picks the rc file.
add_to_path() {
  local dir="$1" line shell_name rc_file
  case "$dir" in
    "$HOME"/*) line="export PATH=\"\$HOME/${dir#"$HOME"/}:\$PATH\"" ;;
    *) line="export PATH=\"$dir:\$PATH\"" ;;
  esac

  shell_name="$(basename "${SHELL:-}")"
  case "$shell_name" in
    zsh) rc_file="${ZDOTDIR:-$HOME}/.zshrc" ;;
    bash)
      # macOS terminals start login shells, which read .bash_profile, not .bashrc
      if [ "$(uname -s)" = "Darwin" ]; then
        rc_file="$HOME/.bash_profile"
      else
        rc_file="$HOME/.bashrc"
      fi
      ;;
    fish)
      rc_file="${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish"
      line="fish_add_path \"$dir\""
      ;;
    *)
      echo
      echo "warning: $dir is not on your PATH." >&2
      echo "Add it by appending this line to your shell profile:" >&2
      echo >&2
      echo "  export PATH=\"$dir:\$PATH\"" >&2
      return
      ;;
  esac

  echo
  if [ -f "$rc_file" ] && grep -Fqx "$line" "$rc_file"; then
    echo "$dir is already added to PATH in $rc_file — restart your shell to pick it up."
    return
  fi
  mkdir -p "$(dirname "$rc_file")"
  printf '\n%s\n' "$line" >>"$rc_file"
  echo "Added $dir to PATH in $rc_file."
  echo "Restart your shell or run:  source $rc_file"
}

main() {
  local repo="bart6114/loofah"
  local base="https://github.com/${repo}/releases/download/updater"
  local install_dir="${LOOFAH_INSTALL_DIR:-${FMTR_INSTALL_DIR:-$HOME/.local/bin}}"

  local os arch triple
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os/$arch" in
    Darwin/arm64) triple="aarch64-apple-darwin" ;;
    Darwin/x86_64) triple="x86_64-apple-darwin" ;;
    Linux/x86_64) triple="x86_64-unknown-linux-gnu" ;;
    Linux/aarch64 | Linux/arm64) triple="aarch64-unknown-linux-gnu" ;;
    *)
      echo "error: unsupported platform: $os/$arch" >&2
      echo "See https://loofah.io/installation for supported platforms." >&2
      exit 1
      ;;
  esac

  # not `local`: the EXIT trap fires after main returns, when locals are gone
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  local asset="loofah-latest-${triple}.tar.gz"
  echo "Downloading loofah for ${triple}..."
  local file
  for file in "$asset" "$asset.sha256"; do
    if ! curl -fsSL --proto '=https' --tlsv1.2 -o "$tmp/$file" "$base/$file"; then
      echo "error: failed to download $base/$file" >&2
      echo "Check https://github.com/${repo}/releases for available builds." >&2
      exit 1
    fi
  done

  (
    cd "$tmp"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum -c "$asset.sha256" >/dev/null
    else
      shasum -a 256 -c "$asset.sha256" >/dev/null
    fi
  )

  tar -xzf "$tmp/$asset" -C "$tmp"
  mkdir -p "$install_dir"
  install -m 755 "$tmp/loofah" "$install_dir/loofah"
  install -m 755 "$tmp/loofah" "$install_dir/fmtr"

  echo "Installed $("$install_dir/loofah" --version) to $install_dir/loofah"
  echo "Compatibility alias installed at $install_dir/fmtr"

  case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) add_to_path "$install_dir" ;;
  esac

  echo
  echo "Run 'loofah --help' to get started."
}

main "$@"
