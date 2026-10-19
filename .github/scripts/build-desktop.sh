#!/bin/sh
# Bundle the desktop app with both commands beside it, the way binaries.yml publishes it:
#
#   .github/scripts/build-desktop.sh TARGET VERSION [OUTPUT]
#
# Expects crates/artifactize-desktop/ui/dist built and both release commands in
# CARGO_TARGET_DIR (default target)/TARGET/release/. tauri.bundle.json adds the
# release-only bundle settings: the commands become sidecars next to the app's own
# executable, where artifactize-core looks for the CLI. Writes to OUTPUT (default dist/),
# each with a .sha256 line as package.sh writes it:
# - macOS: artifactize-desktop-vVERSION-TARGET.dmg;
# - Linux: artifactize-desktop-vVERSION-TARGET.AppImage and .deb.
#
# macOS signs and notarizes when the APPLE_* secrets Tauri reads are present; without
# them the app is signed ad hoc, which runs locally but not after a download.
set -eu

target=$1
version=$2
output=${3:-dist}
name="artifactize-desktop-v$version-$target"
crate=crates/artifactize-desktop
release=${CARGO_TARGET_DIR:-$PWD/target}/$target/release
case $target in
    *-apple-darwin) formats="dmg:dmg" ;;
    *-linux-*) formats="appimage:AppImage deb:deb" ;;
    *) echo "build-desktop.sh: no bundle for $target" >&2; exit 1 ;;
esac

rm -rf "$crate/binaries"
mkdir -p "$crate/binaries" "$output"
for bin in artifactize artifactize-tools; do
    cp "$release/$bin" "$crate/binaries/$bin-$target"
done

# An empty secret is unset in Actions; Tauri would treat it as a certificate to import.
for var in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
    eval "value=\${$var:-}"
    if [ -z "$value" ]; then unset "$var"; fi
done
case $target in
    *-apple-darwin) if [ -z "${APPLE_CERTIFICATE:-}" ]; then export APPLE_SIGNING_IDENTITY=-; fi ;;
esac

bundles=
for format in $formats; do bundles="$bundles${bundles:+,}${format%%:*}"; done
(cd "$crate" && cargo tauri build --ci --target "$target" --bundles "$bundles" --config tauri.bundle.json)
rm -rf "$crate/binaries"

for format in $formats; do
    bundle=${format%%:*}
    ext=${format#*:}
    set -- "$release/bundle/$bundle"/*."$ext"
    if [ $# -ne 1 ] || [ ! -f "$1" ]; then
        echo "build-desktop.sh: expected one .$ext in $release/bundle/$bundle" >&2
        exit 1
    fi
    cp "$1" "$output/$name.$ext"
    (
        cd "$output"
        if command -v sha256sum >/dev/null 2>&1; then
            sum=$(sha256sum "$name.$ext" | cut -d ' ' -f 1)
        else
            sum=$(shasum -a 256 "$name.$ext" | cut -d ' ' -f 1)
        fi
        printf '%s  %s\n' "$sum" "$name.$ext" >"$name.$ext.sha256"
        cat "$name.$ext.sha256"
    )
done
