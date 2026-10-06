#!/bin/sh
# Test website/landing/install.sh against archives built by binaries.yml. A local
# http server serves them in the GitHub release layout (v<version>/<archive>), and
# the script's internal ARTIFACTIZE_DOWNLOAD_URL points the installer at it.
#
#   .github/scripts/test-install.sh DIST [IMAGE]
#
# DIST holds artifactize-v<version>-<target>.tar.gz and its .sha256 for this
# machine's CPU. With IMAGE (for example debian:stable-slim or alpine), the checks
# run as root in a fresh container of that image; without it, on this machine with
# a throwaway HOME. PORT sets the server's port (default 8737).
set -eu

dist=$1
image=${2:-}
port=${PORT:-8737}
root=$(cd "$(dirname "$0")/../.." && pwd)

set -- "$dist"/artifactize-v*.tar.gz
if [ $# -ne 1 ] || [ ! -f "$1" ]; then
    echo "expected one artifactize-v*.tar.gz in $dist" >&2
    exit 1
fi
name=${1##*/}
rest=${name#artifactize-v}
rest=${rest%.tar.gz}
target=$(printf '%s\n' "$rest" | sed 's/^.*-\([^-]*-[^-]*-[^-]*-[^-]*\)$/\1/')
version=${rest%-"$target"}
echo "testing install.sh with artifactize $version for $target"

work=$(mktemp -d)
server=
trap '[ -z "$server" ] || kill "$server" 2>/dev/null || true; rm -rf "$work"' EXIT
serve="$work/serve"
mkdir -p "$serve/v$version" "$serve/v0.0.1" "$work/home"
cp "$root/website/landing/install.sh" "$serve/"
cp "$dist/$name" "$dist/$name.sha256" "$serve/v$version/"
# Release 0.0.1 has a tampered archive: the real one plus a byte, under the real checksum.
bad="artifactize-v0.0.1-$target.tar.gz"
cp "$dist/$name" "$serve/v0.0.1/$bad"
printf 'x' >>"$serve/v0.0.1/$bad"
sed "s/$name/$bad/" "$dist/$name.sha256" >"$serve/v0.0.1/$bad.sha256"

python3 -m http.server "$port" --bind 127.0.0.1 --directory "$serve" >"$work/server.log" 2>&1 &
server=$!
tries=0
until python3 -c 'import sys, urllib.request; urllib.request.urlopen(sys.argv[1])' \
    "http://127.0.0.1:$port/install.sh" 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -ge 50 ]; then
        cat "$work/server.log" >&2
        exit 1
    fi
    sleep 0.2
done

# The checks, run by sh in the container or on this machine.
# shellcheck disable=SC2016 # expanded by the inner shell
checks='
set -eu
unset ARTIFACTIZE_STATE_HOME XDG_STATE_HOME ARTIFACTIZE_INSTALL_DIR ARTIFACTIZE_VERSION
export ARTIFACTIZE_DOWNLOAD_URL="$BASE"
fail() { echo "FAIL: $*" >&2; exit 1; }
if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    # debian:stable-slim has neither; Alpine has BusyBox wget.
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends curl >/dev/null
fi
if command -v curl >/dev/null 2>&1; then echo "downloader: curl"; else echo "downloader: wget"; fi
get() { if command -v curl >/dev/null 2>&1; then curl -fsSL "$1"; else wget -q -O - "$1"; fi; }
script=$(get "$BASE/install.sh")
logs=${LOGS:-/tmp}
# run_install NAME [VAR=VALUE...]: pipe install.sh into sh as documented; output in $logs/NAME.log.
run_install() {
    log=$logs/$1.log; shift
    printf "%s\n" "$script" | env "$@" sh >"$log" 2>&1
}

echo "== fresh install of $VERSION into ~/.local/bin"
run_install fresh ARTIFACTIZE_VERSION="$VERSION" || { cat "$logs/fresh.log"; fail "install failed"; }
cat "$logs/fresh.log"
out=$("$HOME/.local/bin/artifactize" --version)
[ "$out" = "artifactize $VERSION" ] || fail "--version printed: $out"
grep -q "is not on PATH" "$logs/fresh.log" || fail "no PATH hint"
"$HOME/.local/bin/artifactize" doctor || fail "artifactize doctor failed"

echo "== install and update into a directory on PATH, with a v-prefixed version"
dir="$HOME/custom bin"
for run in install update; do
    run_install "$run" PATH="$dir:$PATH" ARTIFACTIZE_INSTALL_DIR="$dir" ARTIFACTIZE_VERSION="v$VERSION" ||
        { cat "$logs/$run.log"; fail "$run failed"; }
    cat "$logs/$run.log"
    ! grep -q "is not on PATH" "$logs/$run.log" || fail "PATH hint for a directory on PATH"
    out=$(PATH="$dir:$PATH" artifactize --version)
    [ "$out" = "artifactize $VERSION" ] || fail "--version printed: $out"
done
[ "$(ls -A "$dir")" = artifactize ] || fail "unexpected files in $dir: $(ls -A "$dir")"

echo "== a checksum mismatch installs nothing"
if run_install mismatch ARTIFACTIZE_VERSION=0.0.1 ARTIFACTIZE_INSTALL_DIR="$HOME/mismatch"; then
    cat "$logs/mismatch.log"; fail "a tampered archive was installed"
fi
cat "$logs/mismatch.log"
grep -q "checksum mismatch" "$logs/mismatch.log" || fail "no checksum mismatch error"
[ ! -e "$HOME/mismatch/artifactize" ] || fail "the tampered binary was installed"

echo "== a missing release fails"
if run_install missing ARTIFACTIZE_VERSION=9.9.9 ARTIFACTIZE_INSTALL_DIR="$HOME/missing"; then
    cat "$logs/missing.log"; fail "a missing release installed"
fi
cat "$logs/missing.log"
grep -q "cannot download" "$logs/missing.log" || fail "no download error"

echo "all install.sh checks passed"
'

base="http://127.0.0.1:$port"
if [ -n "$image" ]; then
    docker run --rm --network host -e BASE="$base" -e VERSION="$version" "$image" sh -c "$checks"
else
    mkdir "$work/logs"
    HOME="$work/home" LOGS="$work/logs" BASE="$base" VERSION="$version" sh -c "$checks"
fi
