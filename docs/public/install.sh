#!/usr/bin/env bash
# Installer for the fmtr CLI (https://freemeetingtranscriber.com).
#
#   curl -fsSL https://freemeetingtranscriber.com/install.sh | bash
#
# Downloads the latest release binary for this platform, verifies its
# checksum, and installs it to ~/.local/bin/fmtr (override with
# FMTR_INSTALL_DIR). Re-run to upgrade. Uninstall with: rm ~/.local/bin/fmtr

set -euo pipefail

main() {
  local repo="bart6114/free-meeting-transcriber"
  local base="https://github.com/${repo}/releases/download/updater"
  local install_dir="${FMTR_INSTALL_DIR:-$HOME/.local/bin}"

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
      echo "See https://freemeetingtranscriber.com/installation for supported platforms." >&2
      exit 1
      ;;
  esac

  # not `local`: the EXIT trap fires after main returns, when locals are gone
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  local asset="fmtr-latest-${triple}.tar.gz"
  echo "Downloading fmtr for ${triple}..."
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
  install -m 755 "$tmp/fmtr" "$install_dir/fmtr"

  echo "Installed $("$install_dir/fmtr" --version) to $install_dir/fmtr"

  case ":$PATH:" in
    *":$install_dir:"*) ;;
    *)
      echo
      echo "warning: $install_dir is not on your PATH." >&2
      local shell_name rc_file
      shell_name="$(basename "${SHELL:-}")"
      case "$shell_name" in
        zsh) rc_file="~/.zshrc" ;;
        bash) rc_file="~/.bashrc" ;;
        *) rc_file="your shell profile" ;;
      esac
      echo "Add it by appending this line to ${rc_file}:" >&2
      echo >&2
      echo "  export PATH=\"$install_dir:\$PATH\"" >&2
      ;;
  esac

  echo
  echo "Run 'fmtr --help' to get started."
}

main "$@"
