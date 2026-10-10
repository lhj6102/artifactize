#!/bin/sh
# Install the native PR archive through a real local tap, never a public release.
set -eu
version=$1
target=$2
dist=$(cd "$3" && pwd)
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
root=$(mktemp -d)
tap="artifactize/packaging-test"
cleanup() {
    brew uninstall --force "$tap/artifactize" >/dev/null 2>&1 || true
    brew untap "$tap" >/dev/null 2>&1 || true
    rm -rf "$root"
}
trap cleanup EXIT HUP INT TERM
brew tap-new "$tap"
formula="$(brew --repository "$tap")/Formula"
python3 -I "$script_dir/channels.py" generate homebrew --version "$version" \
    --targets "$target" --dist "$dist" --output "$formula" --base-url "file://$dist"
brew install --formula "$tap/artifactize"
brew test "$tap/artifactize"
for command in artifactize artifactize-tools; do
    actual=$("$command" --version)
    [ "$actual" = "$command $version" ] || { echo "Unexpected $command version: $actual" >&2; exit 1; }
done
