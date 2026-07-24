#!/bin/zsh
# Generate the Free Meeting Transcriber icon artwork and fan it out to every surface.
# Design: five rounded waveform bars (audio), amber center bar (the moment of capture),
# on a deep indigo->slate diagonal gradient. Reproducible via ImageMagick.
set -euo pipefail
cd "$(dirname "$0")/.."   # src-tauri

OUT_FULL=icons/src/fmt-fullbleed-1024.png     # full-bleed square (Icon Composer / actool input)
OUT_SQUIRCLE=icons/src/fmt-squircle-1024.png  # pre-rounded with margins (tauri icon input)

# --- 1. full-bleed artwork -------------------------------------------------
magick -size 1024x1024 -define gradient:angle=135 gradient:'#312e81'-'#0f172a' \
  \( -size 1024x1024 xc:none \
     -fill '#f1f5f9' \
     -draw 'roundrectangle 204,404 276,620 36,36' \
     -draw 'roundrectangle 340,320 412,704 36,36' \
     -fill '#f59e0b' \
     -draw 'roundrectangle 476,224 548,800 36,36' \
     -fill '#f1f5f9' \
     -draw 'roundrectangle 612,320 684,704 36,36' \
     -draw 'roundrectangle 748,404 820,620 36,36' \
  \) -composite "$OUT_FULL"

# --- 2. squircle rendition (margins + shadow) for tauri icon ---------------
magick "$OUT_FULL" -resize 824x824 \
  \( -size 824x824 xc:none -draw 'roundrectangle 0,0 823,823 184,184' \) \
  -alpha set -compose DstIn -composite \
  -compose Over -background none -gravity center -extent 1024x1024 \
  \( +clone -background black -shadow 40x24+0+12 \) \
  +swap -background none -layers merge +repage -extent 1024x1024 "$OUT_SQUIRCLE"

# --- 3. tauri icon set (window/ico/android/etc.) ---------------------------
(cd .. && ./node_modules/.bin/tauri icon src-tauri/"$OUT_SQUIRCLE" -o src-tauri/icons/stable)

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
magick -size 1320x800 -define gradient:angle=135 gradient:'#1e1b4b'-'#0f172a' \
  \( "$OUT_SQUIRCLE" -resize 220x220 \) -gravity northwest -geometry +180+240 -composite \
  -gravity northwest -fill '#f8fafc' -pointsize 44 -font /System/Library/Fonts/HelveticaNeue.ttc \
  -annotate +180+540 'Free Meeting Transcriber' \
  -fill '#94a3b8' -pointsize 24 -font /System/Library/Fonts/HelveticaNeue.ttc \
  -annotate +180+600 'Drag to Applications to install' \
  assets/dmg-background-stable.png

echo "icon fan-out complete"
