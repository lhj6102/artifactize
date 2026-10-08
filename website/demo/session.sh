#!/usr/bin/env bash
# Start the throwaway shell that a demo tape records:
#   website/demo/session.sh reuse|change|human|team
# Run from the repository root (the tapes do this in a hidden step).
#
# The shell runs in private user, mount, UTS, network and PID namespaces, so the
# recording shows a clean machine: user alice on host "laptop", home
# /home/alice, its own loopback network and an empty /tmp. Nothing outside
# DEMO_WORK is written. Only the artifactize binary under test, python3, grep
# and tmux run; no model or provider is called and no GUI program starts.
#
#   ARTIFACTIZE_BIN  the binary to record (default: target/release/artifactize)
#   DEMO_WORK        an empty scratch directory (default: a new mktemp -d)
set -euo pipefail
scenario=${1:?usage: website/demo/session.sh reuse|change|human|team}
demo=$(cd "$(dirname "$0")" && pwd)
bin=$(realpath "${ARTIFACTIZE_BIN:-target/release/artifactize}")
work=${DEMO_WORK:-$(mktemp -d)}
work=$(realpath "$work")

if [[ -n $(find "$work" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
    echo "DEMO_WORK must be an empty scratch directory: $work" >&2
    exit 2
fi
mkdir -p "$work/alice/bin" "$work/bob"
cp "$bin" "$work/alice/bin/artifactize"
case $scenario in
reuse | change) cp -R "$demo/shop" "$work/alice/shop" ;;
human) cp -R "$demo/brand" "$work/alice/brand" ;;
team)
    cp -R "$demo/shop" "$work/alice/shop"
    cp -R "$demo/shop" "$work/bob/shop"
    ;;
*) echo "unknown scenario: $scenario" >&2; exit 2 ;;
esac
cp "$demo/tmux.conf" "$work/alice/.tmux.conf"
sed "s|@WORK@|$work|g; s|@SCENARIO@|$scenario|g" "$demo/rc.sh" >"$work/rc.sh"
# Exiting the namespace's first process also reaps tmux and the fixture server,
# including when VHS fails or closes its terminal before the tape finishes.
exec unshare --user --map-root-user --mount --uts --net \
    --pid --fork --kill-child --mount-proc \
    env -i TERM="${TERM:-xterm-256color}" bash --noprofile --rcfile "$work/rc.sh" -i
