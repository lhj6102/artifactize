#!/bin/sh
# Bundle the desktop app with both commands beside it, the way binaries.yml publishes it:
#
#   .github/scripts/build-desktop.sh TARGET VERSION [OUTPUT]
#
# Expects crates/artifactize-desktop/ui installed and built (dist) and both release commands in
# CARGO_TARGET_DIR (default target)/TARGET/release/; for universal-apple-darwin, in both the
# aarch64-apple-darwin and x86_64-apple-darwin directories. tauri.bundle.json adds the
# release-only bundle settings: the commands become sidecars next to the app's own
# executable, so every install of the app can also start the Runs it shows. Writes to OUTPUT (default dist/),
# each with a .sha256 line as package.sh writes it:
# - macOS: artifactize-desktop-vVERSION-universal-apple-darwin.dmg, one app for both CPUs;
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
    universal-apple-darwin) formats="dmg:dmg" ;;
    *-linux-*) formats="appimage:AppImage deb:deb" ;;
    *) echo "build-desktop.sh: no bundle for $target" >&2; exit 1 ;;
esac

rm -rf "$crate/binaries"
mkdir -p "$crate/binaries" "$output"
for bin in artifactize artifactize-tools; do
    case $target in
        universal-apple-darwin)
            # Tauri builds and checks each CPU's app on its own, then joins them.
            for cpu in aarch64 x86_64; do
                cp "${release%/universal-apple-darwin/release}/$cpu-apple-darwin/release/$bin" \
                    "$crate/binaries/$bin-$cpu-apple-darwin"
            done
            lipo -create -output "$crate/binaries/$bin-$target" \
                "$crate/binaries/$bin-aarch64-apple-darwin" "$crate/binaries/$bin-x86_64-apple-darwin"
            ;;
        *) cp "$release/$bin" "$crate/binaries/$bin-$target" ;;
    esac
done

# An empty secret is unset in Actions; Tauri would treat it as a certificate to import.
for var in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
    eval "value=\${$var:-}"
    if [ -z "$value" ]; then unset "$var"; fi
done
case $target in
    *-apple-darwin) if [ -z "${APPLE_CERTIFICATE:-}" ]; then export APPLE_SIGNING_IDENTITY=-; fi ;;
esac

# Hosted macOS runners lose stapler's HTTP/3 (UDP 443) connections to Apple's ticket
# service, which fails with Error 68 (actions/runner-images#14924); over TCP it answers. Only a
# disposable Actions runner gets its outbound UDP 443 blocked, never a developer's machine.
stapler_over_tcp() {
    if [ "${GITHUB_ACTIONS:-}" = true ]; then
        echo "block return out quick proto udp from any to any port 443" | sudo -n pfctl -f - 2>/dev/null
        sudo -n pfctl -E 2>/dev/null || true
    fi
}

bundles=
for format in $formats; do bundles="$bundles${bundles:+,}${format%%:*}"; done
case $target in *-apple-darwin) stapler_over_tcp ;; esac
# The prebuilt Tauri CLI pinned in the desktop UI's package.json (pnpm install puts it there).
(cd "$crate" && ui/node_modules/.bin/tauri build --ci --target "$target" --bundles "$bundles" --config tauri.bundle.json)
rm -rf "$crate/binaries"

# Hosted macOS runners sometimes report the disk-image service busy (hdiutil exits 16); that
# is the runner, not the image, so retry the mount only.
attach() {
    i=0
    until hdiutil attach -nobrowse "$@" >/dev/null; do
        i=$((i + 1))
        [ "$i" -lt 5 ] || { echo "build-desktop.sh: hdiutil cannot mount the image" >&2; exit 1; }
        sleep 5
    done
}

# Tauri staples the notarized app once and carries on when Apple has not published the ticket
# yet, so the dmg can hold an unstapled app. Staple it inside the dmg until Apple answers,
# then rebuild and re-sign the image.
staple_dmg() {
    work=$(mktemp -d "${TMPDIR:-/tmp}/artifactize-staple.XXXXXXXX")
    attach -readonly -mountpoint "$work/mount" "$1"
    stapled=true
    xcrun stapler validate -q "$work/mount/Artifactize.app" >/dev/null 2>&1 || stapled=false
    hdiutil detach -quiet "$work/mount"
    if [ "$stapled" = true ]; then rm -rf "$work"; return; fi
    hdiutil convert -quiet "$1" -format UDRW -o "$work/rw.dmg"
    attach -mountpoint "$work/mount" "$work/rw.dmg"
    attempt=1
    until xcrun stapler staple -q "$work/mount/Artifactize.app"; do
        [ "$attempt" -lt 20 ] || { hdiutil detach -quiet "$work/mount"; echo "build-desktop.sh: Apple has no ticket to staple" >&2; exit 1; }
        attempt=$((attempt + 1))
        sleep 30
    done
    hdiutil detach -quiet "$work/mount"
    rm -f "$1"
    hdiutil convert -quiet "$work/rw.dmg" -format UDZO -o "$1"
    # Tauri's signing keychain is gone by now; import the certificate once more for the image.
    printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$work/certificate.p12"
    security create-keychain -p "" "$work/signing.keychain-db"
    security unlock-keychain -p "" "$work/signing.keychain-db"
    security import "$work/certificate.p12" -k "$work/signing.keychain-db" \
        -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
    rm -f "$work/certificate.p12"
    security set-key-partition-list -S apple-tool:,apple: -s -k "" "$work/signing.keychain-db" >/dev/null
    # codesign finds an identity only in a keychain on the user's search list.
    keychains=$(security list-keychains -d user | tr -d '"')
    # shellcheck disable=SC2086 # one keychain path per word
    security list-keychains -d user -s "$work/signing.keychain-db" $keychains
    codesign --force --timestamp --keychain "$work/signing.keychain-db" \
        --sign "$APPLE_SIGNING_IDENTITY" "$1"
    # shellcheck disable=SC2086
    security list-keychains -d user -s $keychains
    security delete-keychain "$work/signing.keychain-db"
    rm -rf "$work"
    echo "build-desktop.sh: stapled the app after $attempt attempts and re-signed the dmg"
}
if [ "$target" = universal-apple-darwin ] && [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_CERTIFICATE:-}" ]; then
    set -- "$release/bundle/dmg"/*.dmg
    staple_dmg "$1"
fi

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
