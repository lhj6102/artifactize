#!/usr/bin/env bash
# Re-create the landing page media from the promo master (1920x1080, 60 fps):
#   website/make-media.sh path/to/artifactize-promo-v10-en.mp4
# The master is not committed. Writes, under website/landing/media/:
#   promo-v10.webm       AV1, 1600x900, 30 fps
#   promo-v10.mp4        H.264 High, 1600x900, 30 fps, +faststart
#   promo-v10-poster.webp  the "Reuse" frame (18.8 s), shown before playback
#                          and instead of it under prefers-reduced-motion
#   og.jpg               1200x630 social card from the end card (24.45 s,
#                        before its last line appears)
# Needs ffmpeg with libx264, libsvtav1 and libwebp.
set -euo pipefail
src=${1:?usage: website/make-media.sh MASTER.mp4}
out="$(dirname "$0")/landing/media"
mkdir -p "$out"
scale="fps=30,scale=1600:-2:flags=lanczos"

ffmpeg -hide_banner -loglevel error -y -i "$src" -an -vf "$scale" \
    -c:v libsvtav1 -preset 4 -crf 30 -g 150 -pix_fmt yuv420p10le "$out/promo-v10.webm"
ffmpeg -hide_banner -loglevel error -y -i "$src" -an -vf "$scale" \
    -c:v libx264 -preset veryslow -tune animation -crf 20 -profile:v high -level 4.1 \
    -pix_fmt yuv420p -movflags +faststart "$out/promo-v10.mp4"
ffmpeg -hide_banner -loglevel error -y -ss 18.8 -i "$src" -frames:v 1 \
    -vf "scale=1600:-2:flags=lanczos" -c:v libwebp -quality 82 "$out/promo-v10-poster.webp"
ffmpeg -hide_banner -loglevel error -y -ss 24.45 -i "$src" -frames:v 1 \
    -vf "crop=1920:1008:0:36,scale=1200:630:flags=lanczos" -q:v 3 "$out/og.jpg"
ls -l "$out"
