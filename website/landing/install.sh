#!/bin/sh
# Install artifactize on Linux or macOS from its GitHub release.
#
#   curl -fsSL https://artifactize.dev/install.sh | sh
#
# Other available channels (Homebrew and Cargo), plus winget and the Microsoft
# Store (in preparation, not available yet), are listed at
# https://artifactize.dev/docs/getting-started/install.html
#
# Downloads the release archive for this OS and CPU over https, checks its SHA-256
# against the release's .sha256 file, and installs artifactize and artifactize-tools to
# ~/.local/bin. It never uses sudo and never edits shell startup files; if the
# directory is not on PATH, it prints the line to add. Run it again to update.
#
# It also installs the desktop app, checked the same way, where the release has one
# for this system: on macOS Artifactize.app in ~/Applications; on x86_64 Linux with
# glibc the AppImage, unpacked once (no FUSE needed) into ~/.local/share/artifactize/
# desktop, with an artifactize-desktop launcher beside the CLI and a menu entry.
# Elsewhere only the CLI is installed.
#
# Environment:
#   ARTIFACTIZE_VERSION      install this release (0.5.2 or v0.5.2) instead of the latest
#                            stable one; only this way installs a prerelease (0.6.0-alpha.1)
#   ARTIFACTIZE_INSTALL_DIR  install into this directory instead of ~/.local/bin
#
# Internal, for this repository's CI only:
#   ARTIFACTIZE_DOWNLOAD_URL replaces https://github.com/lhj6102/artifactize/releases/download;
#                            archives are fetched from $ARTIFACTIZE_DOWNLOAD_URL/v<version>/,
#                            and http:// and file:// are then allowed too.
#
# The whole script is one function, called on the last line, so a partial
# download runs nothing.

set -eu

REPO=lhj6102/artifactize

say() {
    printf 'artifactize-install: %s\n' "$*"
}

err() {
    printf 'artifactize-install: error: %s\n' "$*" >&2
    exit 1
}

has() {
    command -v "$1" >/dev/null 2>&1
}

# The Rust target of the release archive for this machine.
detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case $arch in
        x86_64 | amd64) arch=x86_64 ;;
        aarch64 | arm64) arch=aarch64 ;;
        *) err "no prebuilt binary for the $arch CPU; build it with: cargo install artifactize artifactize-tools --locked" ;;
    esac
    # One line per OS that has release binaries.
    case $os in
        Linux) printf '%s\n' "$arch-unknown-linux-musl" ;;
        Darwin) printf '%s\n' "$arch-apple-darwin" ;;
        *) err "no prebuilt binary for $os; build it with: cargo install artifactize artifactize-tools --locked" ;;
    esac
}

# fetch URL FILE
fetch() {
    case $downloader in
        curl) curl --proto "$protocols" --tlsv1.2 --fail --silent --show-error --location \
            --retry 3 --output "$2" "$1" ;;
        # BusyBox wget (Alpine) takes only the short options.
        wget) wget -q -O "$2" "$1" ;;
    esac
}

# GitHub's latest release is the newest stable one; it skips prereleases.
latest_version() {
    api="https://api.github.com/repos/$REPO/releases/latest"
    fetch "$api" "$tmp/latest.json" ||
        err "cannot look up the latest release at $api; set ARTIFACTIZE_VERSION to choose one"
    tag=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1)
    [ -n "$tag" ] || err "no tag_name in $api"
    printf '%s\n' "$tag"
}

main() {
    if has curl; then
        downloader=curl
    elif has wget; then
        downloader=wget
    else
        err "needs curl or wget"
    fi
    if has sha256sum; then
        sha256='sha256sum'
    elif has shasum; then
        sha256='shasum -a 256'
    elif has openssl; then
        sha256='openssl dgst -sha256 -r'
    else
        err "needs sha256sum, shasum or openssl to check the download"
    fi
    has tar || err "needs tar"

    target=$(detect_target)

    base=${ARTIFACTIZE_DOWNLOAD_URL:-}
    if [ -n "$base" ]; then
        protocols='=https,http,file'
        say "using the test download URL $base"
    else
        base="https://github.com/$REPO/releases/download"
        protocols='=https'
    fi
    base=${base%/}

    install_dir=${ARTIFACTIZE_INSTALL_DIR:-}
    if [ -z "$install_dir" ]; then
        [ -n "${HOME:-}" ] || err "HOME is not set; set ARTIFACTIZE_INSTALL_DIR"
        install_dir="$HOME/.local/bin"
    fi

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t artifactize-install)
    staged=
    mounted=
    trap '[ -z "$mounted" ] || hdiutil detach -quiet "$mounted"; rm -rf "$tmp"; [ -z "$staged" ] || rm -rf "$staged"' EXIT
    trap 'exit 1' HUP INT TERM

    version=${ARTIFACTIZE_VERSION:-}
    [ -n "$version" ] || version=$(latest_version)
    version=${version#v}
    case $version in
        '' | *[!0-9A-Za-z.+-]*) err "not a version: '$version'" ;;
    esac

    name="artifactize-v$version-$target"
    archive="$name.tar.gz"
    url="$base/v$version/$archive"
    say "downloading artifactize $version for $target"
    fetch "$url" "$tmp/$archive" || err "cannot download $url"
    fetch "$url.sha256" "$tmp/$archive.sha256" || err "cannot download $url.sha256"
    check_sum "$archive" "$url.sha256" "nothing was installed"

    tar -xzf "$tmp/$archive" -C "$tmp"
    for bin in artifactize artifactize-tools; do
        [ -f "$tmp/$name/$bin" ] || err "$archive does not contain $name/$bin; nothing was installed"
    done

    # Stage both binaries next to their destinations and check them before replacing
    # either installed command. Each rename keeps the installed file complete.
    mkdir -p "$install_dir"
    staged=$(mktemp -d "$install_dir/.artifactize-install.XXXXXXXX")
    for bin in artifactize artifactize-tools; do
        cp "$tmp/$name/$bin" "$staged/$bin"
        chmod 755 "$staged/$bin"
        installed=$("$staged/$bin" --version) || err "the new $bin does not run; existing binaries were kept"
        [ "$installed" = "$bin $version" ] || err "the new $bin reports '$installed', not '$bin $version'; existing binaries were kept"
    done
    for bin in artifactize artifactize-tools; do
        mv -f "$staged/$bin" "$install_dir/$bin"
        say "installed $bin $version to $install_dir/$bin"
    done
    rmdir "$staged"
    staged=

    install_desktop

    case ":${PATH:-}:" in
        *":$install_dir:"* | *":$install_dir/:"*) ;;
        *)
            say "$install_dir is not on PATH. To use artifactize and artifactize-tools in this shell, run:"
            # shellcheck disable=SC2016 # $PATH is meant literally
            printf '\n    export PATH="%s:$PATH"\n\n' "$install_dir"
            say "and add that line to your shell's startup file (such as ~/.profile, ~/.bashrc or ~/.zshrc)."
            ;;
    esac
    for bin in artifactize artifactize-tools; do
        warn_other_copies "$install_dir/$bin" "$bin"
    done
}

# check_sum FILE SHA256-URL CONSEQUENCE: $tmp/FILE must match its downloaded .sha256 file,
# which reads "<hex digest>  <file name>".
check_sum() {
    read -r expected _ <"$tmp/$1.sha256" || true
    expected=$(printf '%s' "${expected:-}" | tr '[:upper:]' '[:lower:]')
    case $expected in
        *[!0-9a-f]* | '') err "malformed checksum file $2; $3" ;;
    esac
    [ ${#expected} -eq 64 ] || err "malformed checksum file $2; $3"
    # shellcheck disable=SC2086 # $sha256 is a command with its arguments
    actual=$($sha256 "$tmp/$1" | { read -r sum _ && printf '%s' "$sum"; } | tr '[:upper:]' '[:lower:]')
    if [ "$actual" != "$expected" ]; then
        err "checksum mismatch for $1: expected $expected, got ${actual:-nothing}; $3"
    fi
    say "checked SHA-256 $actual"
}

# The desktop app starts Runs through the artifactize beside its own executable, so each
# bundle carries both commands. A release or system without a bundle keeps the CLI only.
install_desktop() {
    case $target in
        x86_64-unknown-linux-musl)
            if ldd --version 2>&1 | grep -qi musl; then
                say "the desktop app needs glibc; installed the CLI only"
                return 0
            fi
            bundle="artifactize-desktop-v$version-x86_64-unknown-linux-gnu.AppImage"
            ;;
        # One app for both Mac CPUs.
        *-apple-darwin) bundle="artifactize-desktop-v$version-universal-apple-darwin.dmg" ;;
        *)
            say "no desktop app for $target; installed the CLI only"
            return 0
            ;;
    esac
    url="$base/v$version/$bundle"
    if ! fetch "$url.sha256" "$tmp/$bundle.sha256" 2>/dev/null; then
        say "release $version has no desktop app for this system; installed the CLI only"
        return 0
    fi
    say "downloading the desktop app"
    fetch "$url" "$tmp/$bundle" || err "cannot download $url; the CLI is installed, the desktop app is not"
    check_sum "$bundle" "$url.sha256" "the CLI is installed, the desktop app is not"
    case $bundle in
        *.AppImage) install_appimage ;;
        *.dmg) install_mac_app ;;
    esac
}

# The bundle's own artifactize must run before it replaces an installed app.
check_bundle() {
    installed=$("$1/artifactize" --version) ||
        err "the desktop app's artifactize does not run; the CLI is installed, the previous app was kept"
    [ "$installed" = "artifactize $version" ] ||
        err "the desktop app reports '$installed', not 'artifactize $version'; the previous app was kept"
}

install_appimage() {
    share=${XDG_DATA_HOME:-$HOME/.local/share}
    app="$share/artifactize/desktop"
    mkdir -p "$share/artifactize" "$share/applications"
    staged=$(mktemp -d "$share/artifactize/.desktop-install.XXXXXXXX")
    chmod 755 "$tmp/$bundle"
    (cd "$staged" && "$tmp/$bundle" --appimage-extract >/dev/null) ||
        err "cannot unpack $bundle; the CLI is installed, the desktop app is not"
    check_bundle "$staged/squashfs-root/usr/bin"
    rm -rf "$app"
    mv "$staged/squashfs-root" "$app"
    rmdir "$staged"
    staged=
    # AppRun finds the app beside itself, so the launcher runs it by its real path.
    # shellcheck disable=SC2016 # "$@" belongs to the launcher
    printf '#!/bin/sh\nexec "%s/AppRun" "$@"\n' "$app" >"$install_dir/artifactize-desktop"
    chmod 755 "$install_dir/artifactize-desktop"
    {
        printf '[Desktop Entry]\nType=Application\nName=Artifactize\n'
        printf 'Comment=Declare and verify artifacts with reusable evidence\n'
        printf 'Exec="%s/AppRun"\nIcon=%s/Artifactize.png\n' "$app" "$app"
        printf 'Categories=Development;\nTerminal=false\nStartupWMClass=artifactize-desktop\n'
    } >"$share/applications/artifactize.desktop"
    say "installed the desktop app $version to $app (run artifactize-desktop, or Artifactize from your menu)"
}

install_mac_app() {
    apps="$HOME/Applications"
    mkdir -p "$apps" "$tmp/mount"
    hdiutil attach -nobrowse -readonly -quiet -mountpoint "$tmp/mount" "$tmp/$bundle" ||
        err "cannot open $bundle; the CLI is installed, the desktop app is not"
    mounted="$tmp/mount"
    staged=$(mktemp -d "$apps/.artifactize-install.XXXXXXXX")
    cp -R "$mounted/Artifactize.app" "$staged/"
    hdiutil detach -quiet "$mounted"
    mounted=
    check_bundle "$staged/Artifactize.app/Contents/MacOS"
    rm -rf "$apps/Artifactize.app"
    mv "$staged/Artifactize.app" "$apps/Artifactize.app"
    rmdir "$staged"
    staged=
    say "installed the desktop app $version to $apps/Artifactize.app"
}

# Every other copy on PATH: one that comes first runs instead, and a shell that ran one
# before may keep running it from its command cache. A warning only; the install succeeded.
warn_other_copies() {
    others=$(
        IFS=:
        set -f
        for dir in ${PATH:-}; do
            other=${dir%/}/$2
            # -ef is supported by the Linux/macOS shells we install on; it also
            # avoids warnings for symlinks or alternate paths to this same file.
            # shellcheck disable=SC3013
            if [ -n "$dir" ] && [ -f "$other" ] && [ -x "$other" ] && ! [ "$other" -ef "$1" ]; then
                printf '%s\n' "$other"
            fi
        done | sort -u
    )
    [ -n "$others" ] || return 0
    say "another $2 on your PATH may run instead of this one:"
    printf '%s\n' "$others" | while IFS= read -r other; do
        printf '    %s (%s)\n' "$other" "$("$other" --version 2>/dev/null || echo 'version unknown')"
    done
    say "remove that install (cargo: 'cargo uninstall $2'; Homebrew: 'brew uninstall artifactize') or put $(dirname "$1") first on PATH."
    say "a shell that already ran $2 may keep the old path: run 'hash -r' (zsh: 'rehash') or open a new terminal."
}

main "$@"
