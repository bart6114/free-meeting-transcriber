#!/bin/zsh
# Generate the Free Meeting Transcriber icon artwork and fan it out to every surface.
# Design: five rounded waveform bars (audio), amber center bar (the moment of capture),
# on a deep indigo->slate diagonal gradient. Reproducible via ImageMagick.
set -euo pipefail
cd "$(dirname "$0")/.."   # src-tauri

OUT_FULL=icons/src/fmt-fullbleed-1024.png     # full-bleed square (Icon Composer / actool input)
OUT_SQUIRCLE=icons/src/fmt-squircle-1024.png  # pre-rounded with margins (tauri icon input)

# --- 1. full-bleed artwork: tipsy beer mug ("free as in beer") -------------
MUG=$(mktemp -d)/mug.png
magick -size 1024x1024 xc:none \
  -fill '#f59e0b' \
  -draw 'roundrectangle 330,400 660,800 40,40' \
  -draw 'roundrectangle 640,480 800,700 70,70' \
  \( -size 1024x1024 xc:none -fill white -draw 'roundrectangle 696,528 748,652 26,26' \) -compose DstOut -composite -compose Over \
  -fill '#fffbeb' \
  -draw 'roundrectangle 353,560 389,660 18,18' \
  -draw 'roundrectangle 415,530 451,690 18,18' \
  -draw 'roundrectangle 477,495 513,725 18,18' \
  -draw 'roundrectangle 539,530 575,690 18,18' \
  -draw 'roundrectangle 601,560 637,660 18,18' \
  -fill '#f8fafc' \
  -draw 'roundrectangle 328,368 662,430 30,30' \
  -draw 'circle 370,368 370,300' \
  -draw 'circle 455,352 455,272' \
  -draw 'circle 545,362 545,290' \
  -draw 'circle 615,355 615,285' \
  -draw 'ellipse 340,462 24,42 0,360' \
  -draw 'circle 332,570 332,552' \
  -background none -rotate -8 \
  -gravity center -extent 1024x1024 "$MUG"
magick -size 1024x1024 -define gradient:angle=135 gradient:'#312e81'-'#0f172a' "$MUG" -gravity center -composite "$OUT_FULL"

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
