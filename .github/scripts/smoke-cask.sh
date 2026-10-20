#!/bin/sh
# Install the desktop app's cask from this build's dmg through a real local tap, never a
# public release: the app opens and its bundled commands are on Homebrew's PATH.
set -eu
version=$1
dist=$(cd "$2" && pwd)
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
apps=$(mktemp -d "${TMPDIR:-/tmp}/artifactize-cask.XXXXXXXX")
tap="artifactize/packaging-cask-test"
cleanup() {
    brew uninstall --cask --force "$tap/artifactize" >/dev/null 2>&1 || true
    brew untap "$tap" >/dev/null 2>&1 || true
    rm -rf "$apps"
}
trap cleanup EXIT HUP INT TERM
brew tap-new "$tap"
casks="$(brew --repository "$tap")/Casks"
mkdir -p "$casks"
python3 -I "$script_dir/channels.py" generate cask --version "$version" \
    --dist "$dist" --output "$casks" --base-url "file://$dist"
brew style "$tap/artifactize"
# Homebrew quarantines what it installs; only a notarized app may keep that and still open.
set -- --appdir="$apps"
[ "${DESKTOP_SIGNED:-false}" = true ] || set -- "$@" --no-quarantine
brew install --cask "$@" "$tap/artifactize"
for command in artifactize artifactize-tools; do
    actual=$("$(brew --prefix)/bin/$command" --version)
    [ "$actual" = "$command $version" ] || { echo "the cask's $command reports '$actual'" >&2; exit 1; }
done
sh "$script_dir/smoke-desktop.sh" installed-mac "$version" "$apps/Artifactize.app"
