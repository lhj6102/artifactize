#!/bin/sh
# Install a desktop bundle the way a user does and check what packaging can break:
#
#   .github/scripts/smoke-desktop.sh FORMAT VERSION FILE
#
# FORMAT is appimage, deb or dmg. Checks that both commands sit beside the app's own
# executable (where artifactize-core looks for the CLI) and report VERSION, and that the
# installed app opens a window titled Artifactize. With DESKTOP_SIGNED=true a dmg must
# also pass Gatekeeper and carry a stapled notarization ticket.
#
# Disposable runners only: deb installs into the system (run it in a container), and
# Linux starts its own X server. The desktop E2E journeys run against the source build in
# CI; this covers only the packaging seam.
set -eu

format=$1
version=$2
file=$(cd "$(dirname "$3")" && pwd)/$(basename "$3")
work=$(mktemp -d "${TMPDIR:-/tmp}/artifactize-desktop-smoke.XXXXXXXX")
app_pid=
cleanup() {
    # Linux starts the app in its own process group: AppRun keeps it as a descendant.
    if [ -n "$app_pid" ]; then kill -- "-$app_pid" 2>/dev/null || kill "$app_pid" 2>/dev/null || true; fi
    if [ -n "${xvfb_pid:-}" ]; then kill "$xvfb_pid" 2>/dev/null || true; fi
    if [ -n "${mount:-}" ]; then hdiutil detach -quiet "$mount" || true; fi
    rm -rf "$work"
}
trap cleanup EXIT

fail() {
    echo "::error::$*" >&2
    exit 1
}

# The commands beside the app's executable, as the installed layout puts them.
check_commands() {
    for command in artifactize-desktop artifactize artifactize-tools; do
        [ -x "$1/$command" ] || fail "$format: $1/$command is missing"
    done
    for command in artifactize artifactize-tools; do
        out=$("$1/$command" --version)
        [ "$out" = "$command $version" ] || fail "$format: $command --version printed '$out'"
    done
    echo "$format: both commands report $version beside the app executable"
}

# Open the app on a fresh state directory and wait for its window. Containers have no
# USER to name the default reviewer.
launch() {
    mkdir -p "$work/home" "$work/state"
    HOME="$work/home" $group "$@" --state-dir "$work/state" --reviewer smoke >"$work/app.log" 2>&1 &
    app_pid=$!
    i=0
    until has_window; do
        kill -0 "$app_pid" 2>/dev/null || { cat "$work/app.log"; fail "$format: the app exited before showing a window"; }
        i=$((i + 1))
        [ "$i" -lt 120 ] || { cat "$work/app.log"; fail "$format: no Artifactize window within 60 seconds"; }
        sleep 0.5
    done
    echo "$format: the installed app opened its Artifactize window"
}

case $format in
    appimage | deb)
        Xvfb :99 -screen 0 1440x900x24 >/dev/null 2>&1 &
        xvfb_pid=$!
        export DISPLAY=:99 GDK_BACKEND=x11
        group=setsid
        unset WAYLAND_DISPLAY
        # AppRun runs the app as a child process; this display holds only that app.
        has_window() { xdotool search --onlyvisible --name '^Artifactize$' >/dev/null 2>&1; }
        ;;
    dmg)
        cat >"$work/window.swift" <<'EOF'
import CoreGraphics
import Foundation
// Window titles need Screen Recording permission; owner and layer do not.
let pid = Int32(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
exit(windows.contains { ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 } ? 0 : 1)
EOF
        xcrun swiftc -O -o "$work/window" "$work/window.swift"
        group=
        has_window() { "$work/window" "$app_pid"; }
        ;;
    *) fail "smoke-desktop.sh: unknown format $format" ;;
esac

case $format in
    appimage)
        cp "$file" "$work/Artifactize.AppImage"
        chmod +x "$work/Artifactize.AppImage"
        (cd "$work" && ./Artifactize.AppImage --appimage-extract >/dev/null)
        check_commands "$work/squashfs-root/usr/bin"
        # Runners lack FUSE; extract-and-run executes the same AppRun entry point.
        APPIMAGE_EXTRACT_AND_RUN=1 launch "$work/Artifactize.AppImage"
        ;;
    deb)
        apt-get install -y -qq "$file" >/dev/null
        check_commands /usr/bin
        launch /usr/bin/artifactize-desktop
        ;;
    dmg)
        mount="$work/mount"
        hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$file"
        mkdir "$work/Applications"
        cp -R "$mount/Artifactize.app" "$work/Applications/"
        hdiutil detach -quiet "$mount"
        mount=
        app="$work/Applications/Artifactize.app"
        check_commands "$app/Contents/MacOS"
        if [ "${DESKTOP_SIGNED:-false}" = true ]; then
            spctl --assess --type execute --verbose "$app"
            xcrun stapler validate "$app"
            echo "dmg: Gatekeeper accepts the notarized app"
        else
            echo "dmg: unsigned build; skipping Gatekeeper and notarization checks"
        fi
        launch "$app/Contents/MacOS/artifactize-desktop"
        ;;
esac
