#!/bin/zsh
# Task 16 — one-time migration of Bart's real data to the new vault.
# Safe by design: COPIES only; the old Drive/anarlog vault and the old
# hyprnote app-data dir are never modified except global.json's vault_path
# (single line, backed up first). Run with the desktop app QUIT.
set -euo pipefail

OLD_VAULT="$HOME/Library/CloudStorage/GoogleDrive-bartsmeets86@gmail.com/My Drive/anarlog"
NEW_VAULT="$HOME/Library/CloudStorage/GoogleDrive-bartsmeets86@gmail.com/My Drive/free-meeting-transcriber"
APPDATA="$HOME/Library/Application Support/hyprnote"   # ladder's live legacy dir (holds app.db + global.json)
LOCAL_LEGACY="$HOME/Library/Application Support/hyprnote/sessions"  # pre-June-2 local vault (orphaned memos)

[[ -d "$OLD_VAULT" ]] || { echo "old vault missing"; exit 1; }
pgrep -f "Free Meeting Transcriber|Anarlog" >/dev/null && { echo "Quit the app(s) first"; exit 1; }

echo "== 1/5 create new vault + copy content (rsync, additive)"
mkdir -p "$NEW_VAULT"
rsync -a --exclude 'search_index/' --exclude '.DS_Store' "$OLD_VAULT/" "$NEW_VAULT/"

echo "== 2/5 recover orphaned pre-June-2 memos (sander, Property Purchase)"
for id in 62a41d38-8af6-480f-9ff2-f79765e4c0ed 02be0e69-c313-43be-9c47-dac8e35d8669; do
  src="$LOCAL_LEGACY/$id/_memo.md"
  dst="$NEW_VAULT/sessions/$id"
  if [[ -f "$src" ]]; then
    mkdir -p "$dst"
    if [[ -f "$dst/_memo.md" ]]; then echo "  $id: _memo.md already present, skipping"; else cp "$src" "$dst/_memo.md" && echo "  $id: memo recovered"; fi
  else echo "  $id: source memo not found (already migrated?)"; fi
done

echo "== 3/5 repoint vault_path (backup first)"
cp "$APPDATA/global.json" "$APPDATA/global.json.pre-migration.bak"
python3 - "$APPDATA/global.json" "$NEW_VAULT" <<'EOF'
import json,sys
p,new=sys.argv[1],sys.argv[2]
d=json.load(open(p)); d['vault_path']=new
json.dump(d,open(p,'w'),indent=2)
print(f"  vault_path -> {new}")
EOF

echo "== 4/5 sanity counts"
echo "  sessions in new vault: $(ls "$NEW_VAULT/sessions" | wc -l | tr -d ' ')"
echo "  sessions in old vault: $(ls "$OLD_VAULT/sessions" | wc -l | tr -d ' ')"

echo "== 5/5 done — launch 'Free Meeting Transcriber.app'; first run will reconcile + full-export (one-time reformat churn is expected; originals go to .trash/)"
