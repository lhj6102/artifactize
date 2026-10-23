#!/bin/sh
# Publish the workspace crates to crates.io for a release:
#
#   .github/scripts/publish-crates.sh VERSION
#
# One crate at a time, each after the crates it depends on. A version crates.io already
# has is skipped, so a rerun finishes what an earlier attempt started. cargo waits only a
# minute for an upload to reach the index and then stops (v0.16.0 lost four crates that
# way), so each crate waits here until the index serves it before the next one packages.
set -eu

version=$1
# The sparse index path of a crate name of four or more characters: ab/cd/name.
indexed() {
    curl -fsS "https://index.crates.io/$(echo "$1" | cut -c1-2)/$(echo "$1" | cut -c3-4)/$1" 2>/dev/null |
        jq -e --arg version "$version" 'select(.vers == $version)' >/dev/null
}
for crate in artifactize-tools artifactize-core artifactize-tui artifactize-mcp artifactize; do
    if indexed "$crate"; then
        echo "$crate $version is on crates.io already"
        continue
    fi
    # A timed-out wait still uploaded the crate; the index check below decides.
    cargo publish --locked --allow-dirty --no-verify -p "$crate" || echo "cargo publish $crate exited $?"
    attempt=1
    until indexed "$crate"; do
        [ "$attempt" -lt 60 ] || {
            echo "::error::$crate $version did not reach the crates.io index" >&2
            exit 1
        }
        attempt=$((attempt + 1))
        sleep 10
    done
    echo "$crate $version is on crates.io"
done
