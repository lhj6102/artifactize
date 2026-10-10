#!/bin/sh
# Package both release binaries the way binaries.yml publishes them:
#
#   .github/scripts/package.sh TARGET VERSION
#
# Reads artifactize and artifactize-tools from target/TARGET/release/ (.exe on
# Windows) and writes to dist/:
# - artifactize-vVERSION-TARGET.tar.gz (.zip for Windows targets), holding
#   artifactize-vVERSION-TARGET/ with both binaries, LICENSE and README.md;
# - <archive>.sha256, one line "<hex digest>  <archive>", as sha256sum writes it.
#
# install.sh, install.ps1, cargo-binstall and every later channel point at these
# names, so keep them stable. A new target needs a case here only for a new format.
set -eu

target=$1
version=$2
name="artifactize-v$version-$target"
case $target in
    *-windows-*) ext=.exe ;;
    *) ext= ;;
esac

mkdir -p dist
rm -rf "dist/$name"
mkdir "dist/$name"
for bin in artifactize artifactize-tools; do
    cp "target/$target/release/$bin$ext" "dist/$name/"
done
cp LICENSE README.md "dist/$name/"
cd dist
case $target in
    *-windows-*)
        archive="$name.zip"
        rm -f "$archive"
        7z a -tzip -bso0 -bsp0 "$archive" "$name"
        ;;
    *)
        archive="$name.tar.gz"
        if tar --version 2>/dev/null | grep -q 'GNU tar'; then
            tar -czf "$archive" --owner=0 --group=0 "$name"
        else
            tar -czf "$archive" "$name"
        fi
        ;;
esac
rm -r "$name"

if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum "$archive" | cut -d ' ' -f 1)
else
    sum=$(shasum -a 256 "$archive" | cut -d ' ' -f 1)
fi
printf '%s  %s\n' "$sum" "$archive" >"$archive.sha256"
ls -l
cat "$archive.sha256"
