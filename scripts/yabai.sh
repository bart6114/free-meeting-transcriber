#!/bin/bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

app_fmtr=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --app-fmtr)
      app_fmtr="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [[ -n "$app_fmtr" ]]; then
  "$SCRIPT_DIR/yabai_impl.sh" --bundle-id "$app_fmtr" --position left
fi
