#!/bin/sh
# Set the artifactize crate version in this checkout for a release build:
#
#   .github/scripts/set-version.sh VERSION
#
# Releases take their version from the tag, so main keeps a placeholder version and no
# release needs a version-bump commit. The change stays in the CI checkout.
set -eu

version=$1
crate() {
    cargo metadata --no-deps --format-version 1 |
        jq -r '.packages[] | select(.name == "artifactize") | .version'
}
current=$(crate)
if [ "$version" != "$current" ]; then
    sed -i.orig "s/^version = \"$current\"\$/version = \"$version\"/" Cargo.toml
    rm Cargo.toml.orig
    cargo update --workspace --quiet
fi
[ "$(crate)" = "$version" ] || {
    echo "::error::cannot set the version to $version" >&2
    exit 1
}
