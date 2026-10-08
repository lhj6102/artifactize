#!/usr/bin/env bash
# Record the demo tapes with VHS, then encode the landing page's copies:
#   website/demo/record.sh [reuse change human team]
# Writes website/demo/media/NAME.gif (for the README) and
# website/landing/media/demo/NAME.{mp4,webp} (video and its final frame).
#
# Needs vhs with ttyd, ffmpeg (libx264, libwebp), tmux, unshare and
# the binary to record: ARTIFACTIZE_BIN, by default target/release/artifactize
# (cargo build --release). Each tape runs in a fresh mktemp -d; see session.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
export ARTIFACTIZE_BIN=${ARTIFACTIZE_BIN:-target/release/artifactize}
"$ARTIFACTIZE_BIN" --version
landing=website/landing/media/demo
mkdir -p website/demo/media "$landing"
work=
cleanup_recording() {
    if [[ -n $work ]]; then
        rm -rf -- "$work"
    fi
}
trap cleanup_recording EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for name in "${@:-reuse change human team}"; do
    for one in $name; do
        case $one in
            reuse | change | human | team) ;;
            *) printf 'unknown scenario: %s\n' "$one" >&2; exit 2 ;;
        esac
        work=$(mktemp -d "${TMPDIR:-/tmp}/artifactize-demo.XXXXXX")
        DEMO_WORK=$work vhs "website/demo/$one.tape"
        rm -rf -- "$work"
        work=
        raw=website/demo/media/$one.mp4
        ffmpeg -hide_banner -loglevel error -y -i "$raw" -an \
            -c:v libx264 -preset veryslow -tune stillimage -crf 26 -pix_fmt yuv420p \
            -movflags +faststart "$landing/$one.mp4"
        ffmpeg -hide_banner -loglevel error -y -sseof -0.3 -i "$raw" -frames:v 1 \
            -c:v libwebp -quality 85 "$landing/$one.webp"
        rm -f "$raw"
        ls -l "website/demo/media/$one.gif" "$landing/$one".*
    done
done
