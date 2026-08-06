#!/bin/zsh
# Generate the menu-bar tray icon set from the beer-mug brand artwork
# (see apps/desktop/src-tauri/scripts/make-icon.sh for the app-icon variant).
# States: default (white mug, waveform bars knocked out), recording_0..3 (gray
# mug, white bars dancing inside), degraded (amber mug + "!"), update (mug +
# down-arrow badge). Reproducible via ImageMagick.
set -euo pipefail
cd "$(dirname "$0")/.."   # plugins/tray

# bar columns: x1,x2 fixed; y2 (bottom) fixed; full height per bar
BARS=(
  "353,389,660,100"
  "415,451,690,160"
  "477,513,725,230"
  "539,575,690,160"
  "601,637,660,100"
)

mug_shapes() { # stdout: -draw args for the mug silhouette (no bars)
  echo "-draw 'roundrectangle 330,400 660,800 40,40'"
  echo "-draw 'roundrectangle 640,480 800,700 70,70'"
  echo "-draw 'roundrectangle 328,368 662,430 30,30'"
  echo "-draw 'circle 370,368 370,300'"
  echo "-draw 'circle 455,352 455,272'"
  echo "-draw 'circle 545,362 545,290'"
  echo "-draw 'circle 615,355 615,285'"
  echo "-draw 'ellipse 340,462 24,42 0,360'"
  echo "-draw 'circle 332,570 332,552'"
}

knock_shapes() { # handle hole + full-height bars
  echo "-draw 'roundrectangle 696,528 748,652 26,26'"
  for b in $BARS; do
    IFS=, read x1 x2 y2 h <<<"$b"
    echo "-draw 'roundrectangle $x1,$((y2 - h)) $x2,$y2 18,18'"
  done
}

bar_overlay() { # $1..$5 height fractions (percent) -> white bars, bottom-anchored
  local i=1
  for b in $BARS; do
    IFS=, read x1 x2 y2 h <<<"$b"
    local frac=${(P)i}
    local bh=$((h * frac / 100))
    echo "-draw 'roundrectangle $x1,$((y2 - bh)) $x2,$y2 18,18'"
    i=$((i + 1))
  done
}

render_mug() { # $1 fill color, $2 out file, $3 optional extra white-bar draws
  local fill=$1 out=$2 bars=${3:-}
  eval magick -size 1024x1024 xc:none \
    -fill "'$fill'" $(mug_shapes | tr '\n' ' ') \
    \\\( -size 1024x1024 xc:none -fill white $(knock_shapes | tr '\n' ' ') \\\) \
    -compose DstOut -composite -compose Over \
    ${bars:+-fill white $bars} \
    -background none -rotate -8 -gravity center -extent 1024x1024 "$out"
}

T=$(mktemp -d)

# default: white mug, bars as negative space
render_mug white "$T/default.png"

# recording frames: gray mug, white bars dancing inside the knockouts
render_mug '#8e8e8e' "$T/rec0.png" "$(bar_overlay 40 70 100 70 40 | tr '\n' ' ')"
render_mug '#8e8e8e' "$T/rec1.png" "$(bar_overlay 70 100 55 85 50 | tr '\n' ' ')"
render_mug '#8e8e8e' "$T/rec2.png" "$(bar_overlay 100 55 80 45 75 | tr '\n' ' ')"
render_mug '#8e8e8e' "$T/rec3.png" "$(bar_overlay 55 85 40 100 60 | tr '\n' ' ')"

# degraded: amber mug + upright white exclamation on the right
render_mug '#f59e0b' "$T/degraded_base.png"
magick "$T/degraded_base.png" -fill white \
  -draw 'roundrectangle 858,420 922,640 32,32' \
  -draw 'circle 890,724 890,684' "$T/degraded.png"

# update: white mug + down-arrow badge bottom-right (ring gap for contrast)
render_mug white "$T/update_base.png"
magick "$T/update_base.png" \
  \( -size 1024x1024 xc:none -fill white -draw 'circle 780,780 780,628' \) \
  -compose DstOut -composite -compose Over \
  -fill white -draw 'circle 780,780 780,662' \
  \( -size 1024x1024 xc:none -fill white \
     -draw 'roundrectangle 762,682 798,790 18,18' \
     -draw 'polygon 712,780 848,780 780,872' \) \
  -compose DstOut -composite -compose Over "$T/update.png"

# crop away the transparent canvas padding (macOS scales the whole canvas to
# menu-bar height, so padding shrinks the glyph). One union bbox across all
# states keeps the mug the same size/position when the icon changes state.
bbox=$(magick "$T"/{default,rec0,rec1,rec2,rec3,degraded,update}.png \
  -background none -flatten -format '%@' info:)
w=${bbox%%x*} rest=${bbox#*x} h=${rest%%+*}
side=$(( w > h ? w : h ))

for pair in default:tray_default rec0:tray_recording_0 rec1:tray_recording_1 \
  rec2:tray_recording_2 rec3:tray_recording_3 degraded:tray_degraded update:tray_update; do
  src=${pair%%:*} dst=${pair##*:}
  magick "$T/$src.png" -crop "$bbox" +repage \
    -background none -gravity center -extent "${side}x${side}" \
    -resize 160x160 "icons/$dst.png"
done

echo "tray icons regenerated"
