#!/usr/bin/env bash
# Render the README's promo GIF from the landing page's promo video:
# 640 px wide, 25 fps, 64 colours, with a 3 px progress bar along the bottom edge.
# The video's film grain is smoothed first; without that every frame differs and
# the GIF grows about four times larger.
set -euo pipefail
cd "$(dirname "$0")"
src=../landing/media/promo-v10.mp4
out=media/promo.gif
duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$src")
width=640 height=360 fps=25 colours=64 bar=3
ffmpeg -hide_banner -loglevel error -y -i "$src" \
  -f lavfi -i "color=c=0x262E3B:s=${width}x${bar}" \
  -f lavfi -i "color=c=0x3EE08F:s=${width}x${bar}" \
  -filter_complex "[0:v]hqdn3d=8:6:12:9,fps=${fps},scale=${width}:${height}:flags=lanczos[v];\
[v][1:v]overlay=x=0:y=H-${bar}:shortest=1[track];\
[track][2:v]overlay=x='-W+W*t/${duration}':y=H-${bar}:shortest=1,split[a][b];\
[a]palettegen=max_colors=${colours}:stats_mode=diff[palette];\
[b][palette]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
  -t "$duration" "$out"
echo "wrote website/demo/$out ($(stat -c %s "$out") bytes)"
