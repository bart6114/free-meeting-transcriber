#!/bin/zsh
# Generate the Loofah icon artwork and fan it out to every surface.
# Design: the Loofah wheel mark in deep green on a warm cream-to-sage field.
set -euo pipefail
cd "$(dirname "$0")/.."   # src-tauri

OUT_FULL=icons/src/loofah-fullbleed-1024.png     # full-bleed square (Icon Composer / actool input)
OUT_SQUIRCLE=icons/src/loofah-squircle-1024.png  # pre-rounded with margins (tauri icon input)
MARK=icons/src/loofah-mark-1024.png

# --- 1. full-bleed artwork --------------------------------------------------
magick -size 1024x1024 -define gradient:angle=135 gradient:'#f7f2e8'-'#d8e4d6' \
  \( "$MARK" -trim +repage -resize 720x720 \) \
  -gravity center -composite "$OUT_FULL"

# --- 2. squircle rendition (margins + shadow) for tauri icon ---------------
magick "$OUT_FULL" -resize 824x824 \
  \( -size 824x824 xc:none -draw 'roundrectangle 0,0 823,823 184,184' \) \
  -alpha set -compose DstIn -composite \
  -compose Over -background none -gravity center -extent 1024x1024 \
  \( +clone -background black -shadow 40x24+0+12 \) \
  +swap -background none -layers merge +repage -extent 1024x1024 "$OUT_SQUIRCLE"

# --- 3. tauri icon set and in-app surfaces ---------------------------------
(cd .. && ./node_modules/.bin/tauri icon src-tauri/"$OUT_SQUIRCLE" -o src-tauri/icons/stable)
cp icons/stable/icon.png ../public/assets/app-icon.png
zsh ../../../plugins/tray/scripts/make-tray-icons.sh

# --- 4. Icon Composer source + recompile Assets.car ------------------------
magick "$OUT_FULL" -quality 95 icons/src/stable.icon/Assets/icon.jpg
rm -f resources/stable/Assets.car resources/stable/AppIcon.icns
bash scripts/compile-icons.sh

# --- 5. dark-variant icns (used by tauri.conf resources map) ---------------
ICONSET=$(mktemp -d)/AppIcon.iconset
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  magick "$OUT_SQUIRCLE" -resize ${s}x${s} "$ICONSET/icon_${s}x${s}.png"
  magick "$OUT_SQUIRCLE" -resize $((s*2))x$((s*2)) "$ICONSET/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$ICONSET" -o resources/stable-dark/AppIcon.icns

# --- 6. DMG background -----------------------------------------------------
magick -size 1320x800 -define gradient:angle=135 gradient:'#f7f2e8'-'#d8e4d6' \
  \( "$OUT_SQUIRCLE" -resize 220x220 \) -gravity northwest -geometry +180+240 -composite \
  -gravity northwest -fill '#293529' -pointsize 44 -font /System/Library/Fonts/HelveticaNeue.ttc \
  -annotate +180+540 'Loofah' \
  -fill '#5f6f61' -pointsize 24 -font /System/Library/Fonts/HelveticaNeue.ttc \
  -annotate +180+600 'Drag to Applications to install' \
  assets/dmg-background-stable.png

magick -size 1320x800 -define gradient:angle=135 gradient:'#eee8f5'-'#c9d9c7' \
  \( "$OUT_SQUIRCLE" -resize 220x220 \) -gravity northwest -geometry +180+240 -composite \
  -gravity northwest -fill '#293529' -pointsize 44 -font /System/Library/Fonts/HelveticaNeue.ttc \
  -annotate +180+540 'Loofah Staging' \
  -fill '#655d72' -pointsize 24 -font /System/Library/Fonts/HelveticaNeue.ttc \
  -annotate +180+600 'Drag to Applications to install' \
  assets/dmg-background-staging.png

echo "icon fan-out complete"
