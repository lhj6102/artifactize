#!/bin/sh
# Install a desktop bundle the way a user does and check what packaging can break:
#
#   .github/scripts/smoke-desktop.sh FORMAT VERSION FILE
#
# FORMAT is appimage, deb or dmg, or installed-linux or installed-mac with FILE the app
# install.sh unpacked (~/.local/share/artifactize/desktop, ~/Applications/Artifactize.app).
# Checks that both commands sit beside the app's own executable (every install of the app
# can start the Runs it shows) and report VERSION, and that the installed app opens a window titled
# Artifactize; installed-linux starts it through install.sh's launcher and menu entry. With DESKTOP_SIGNED=true a dmg must
# also pass Gatekeeper and carry a stapled notarization ticket. An app started through an
# AppImage's AppRun (appimage, installed-linux) must start its programs with the variables
# the smoke gave it, not the bundle's library and program paths.
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
registered_mac=false
cleanup() {
    # Linux starts the app in its own process group: AppRun keeps it as a descendant.
    if [ -n "$app_pid" ]; then kill -- "-$app_pid" 2>/dev/null || kill "$app_pid" 2>/dev/null || true; fi
    if [ -n "${xvfb_pid:-}" ]; then kill "$xvfb_pid" 2>/dev/null || true; fi
    if [ -n "${mount:-}" ]; then hdiutil detach -quiet "$mount" || true; fi
    if [ "$registered_mac" = true ]; then sudo -n rm -f /etc/paths.d/artifactize; fi
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
    # Installed channels already provide the CLI. Keep that command reachable while
    # the app's own HOME and state are isolated from the runner's personal data.
    terminal_path=$PATH
    if [ "$format" = installed-mac ]; then terminal_path="$HOME/.local/bin:$PATH"; fi
    if [ -d "$work/probe" ]; then terminal_path="$work/probe:$terminal_path"; fi
    HOME="$work/home" XDG_DATA_HOME="$work/home/.local/share" PATH="$terminal_path" ZDOTDIR="$work/home" \
        $group "$@" --state-dir "$work/state" --reviewer smoke >"$work/app.log" 2>&1 &
    app_pid=$!
    i=0
    until has_window; do
        kill -0 "$app_pid" 2>/dev/null || { cat "$work/app.log"; fail "$format: the app exited before showing a window"; }
        i=$((i + 1))
        [ "$i" -lt 120 ] || { cat "$work/app.log"; fail "$format: no Artifactize window within 60 seconds"; }
        sleep 0.5
    done
    echo "$format: the installed app opened its Artifactize window"
    i=0
    until grep -qF 'desktop command setup complete' "$work/app.log"; do
        kill -0 "$app_pid" 2>/dev/null || { cat "$work/app.log"; fail "$format: the app exited during command setup"; }
        i=$((i + 1))
        [ "$i" -lt 120 ] || { cat "$work/app.log"; fail "$format: command setup did not finish within 60 seconds"; }
        sleep 0.5
    done
    if grep -qF 'cannot make the artifactize command available:' "$work/app.log"; then
        cat "$work/app.log"
        fail "$format: command setup failed"
    fi
}

# The variables an AppImage's runtime, AppRun and GTK hook set or extend.
SMOKE_VARIABLES='PATH LD_LIBRARY_PATH APPDIR APPIMAGE ARGV0 OWD PYTHONHOME PYTHONPATH
PYTHONDONTWRITEBYTECODE PERLLIB QT_PLUGIN_PATH GST_PLUGIN_SYSTEM_PATH GST_PLUGIN_SYSTEM_PATH_1_0
XDG_DATA_DIRS GSETTINGS_SCHEMA_DIR GI_TYPELIB_PATH GIO_MODULE_DIR GTK_THEME GTK_DATA_PREFIX
GTK_EXE_PREFIX GTK_PATH GTK_IM_MODULE_FILE GDK_PIXBUF_MODULE_FILE'
export SMOKE_VARIABLES

# A git stand-in first on the PATH the app is given: the app runs git for the repository
# it opens on, and the stand-in records those variables as a program the app starts sees
# them, with shell builtins only, before it runs the real git.
probe_children() {
    SMOKE_GIT=$(command -v git) || fail "$format: the runner has no git"
    export SMOKE_GIT
    mkdir "$work/probe" "$work/repo"
    "$SMOKE_GIT" init -q "$work/repo"
    cat >"$work/probe/git" <<'EOF'
#!/bin/sh
for name in $SMOKE_VARIABLES; do
    eval "value=\${$name-(unset)}"
    printf '%s=%s\n' "$name" "$value"
done >"$SMOKE_RECORD.$$"
: >"$SMOKE_RECORD.$$.done"
exec "$SMOKE_GIT" "$@"
EOF
    chmod +x "$work/probe/git"
}

# Compare what the app's git saw with what the same stand-in sees when the smoke runs it
# with the PATH the app was given.
check_children() {
    i=0
    until set -- "$work"/child.*.done && [ -e "$1" ]; do
        kill -0 "$app_pid" 2>/dev/null || { cat "$work/app.log"; fail "$format: the app exited before running git"; }
        i=$((i + 1))
        [ "$i" -lt 120 ] || { cat "$work/app.log"; fail "$format: the app did not run git for --repo within 60 seconds"; }
        sleep 0.5
    done
    PATH="$work/probe:$PATH" SMOKE_RECORD="$work/expected" git --version >/dev/null
    set -- "$work"/expected.*.done
    expected=${1%.done}
    for done_marker in "$work"/child.*.done; do
        record=${done_marker%.done}
        if ! cmp -s "$expected" "$record"; then
            diff "$expected" "$record" || true
            fail "$format: a program the app started got the bundle's environment"
        fi
    done
    echo "$format: programs the app starts get the environment the app was given"
}

case $format in
    appimage | deb | installed-linux)
        Xvfb :99 -screen 0 1440x900x24 >/dev/null 2>&1 &
        xvfb_pid=$!
        export DISPLAY=:99 GDK_BACKEND=x11
        group=setsid
        unset WAYLAND_DISPLAY
        # AppRun runs the app as a child process; this display holds only that app.
        has_window() { xdotool search --onlyvisible --name '^Artifactize$' >/dev/null 2>&1; }
        ;;
    dmg | installed-mac)
        cat >"$work/window.swift" <<'EOF'
import CoreGraphics
import Foundation
// Window titles need Screen Recording permission; owner and layer do not.
let pid = Int32(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
exit(windows.contains { ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 } ? 0 : 1)
EOF
        xcrun swiftc -O -o "$work/window" "$work/window.swift" || fail "dmg: cannot build the window probe"
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
        probe_children
        # Runners lack FUSE; extract-and-run executes the same AppRun entry point.
        APPIMAGE_EXTRACT_AND_RUN=1 SMOKE_RECORD="$work/child" launch "$work/Artifactize.AppImage" --repo "$work/repo"
        check_children
        for command in artifactize artifactize-tools; do
            out=$("$work/home/.local/bin/$command" --version)
            [ "$out" = "$command $version" ] || fail "appimage: copied $command reports '$out'"
            target=$(readlink "$work/home/.local/bin/$command")
            [ "$target" = "$work/home/.local/share/artifactize/bin/$command" ] || fail "appimage: link still depends on a temporary mount"
        done
        echo "appimage: first launch installed stable per-user commands"
        ;;
    deb)
        apt-get install -y -qq "$file" >/dev/null
        check_commands /usr/bin
        launch /usr/bin/artifactize-desktop
        ;;
    dmg)
        mount="$work/mount"
        mkdir "$mount"
        hdiutil verify "$file" >"$work/hdiutil.log" 2>&1 || { cat "$work/hdiutil.log"; fail "dmg: $file is not a valid disk image"; }
        # Hosted macOS runners sometimes report the disk-image service busy; that is the
        # runner, not the image, so retry the mount only.
        i=0
        until hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$file" >"$work/hdiutil.log" 2>&1; do
            i=$((i + 1))
            if [ "$i" -ge 3 ]; then cat "$work/hdiutil.log"; hdiutil info; fail "dmg: hdiutil cannot mount $file"; fi
            sleep 5
        done
        mkdir "$work/Applications"
        cp -R "$mount/Artifactize.app" "$work/Applications/"
        hdiutil detach -quiet "$mount"
        mount=
        app="$work/Applications/Artifactize.app"
        check_commands "$app/Contents/MacOS"
        if [ "${DESKTOP_SIGNED:-false}" = true ]; then
            spctl --assess --type execute --verbose "$app"
            # Hosted macOS runners lose stapler's HTTP/3 (UDP 443) connections to Apple's ticket
            # service, which fails with Error 68 (actions/runner-images#14924); over TCP it answers.
            echo "block return out quick proto udp from any to any port 443" | sudo -n pfctl -f - 2>/dev/null
            sudo -n pfctl -E 2>/dev/null || true
            xcrun stapler validate "$app"
            echo "dmg: Gatekeeper accepts the notarized app"
        else
            echo "dmg: unsigned build; skipping Gatekeeper and notarization checks"
        fi
        [ ! -e /etc/paths.d/artifactize ] || fail "dmg: runner already has /etc/paths.d/artifactize"
        registered_mac=true
        launch "$app/Contents/MacOS/artifactize-desktop"
        [ "$(cat /etc/paths.d/artifactize)" = "$app/Contents/MacOS" ] || fail "dmg: paths.d does not name the installed bundle"
        out=$(HOME="$work/home" ZDOTDIR="$work/home" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
            /bin/zsh -l -c 'artifactize --version')
        [ "$out" = "artifactize $version" ] || fail "dmg: a fresh login terminal reports '$out'"
        echo "dmg: a fresh login terminal finds the bundled artifactize command"
        ;;
    installed-linux)
        check_commands "$file/usr/bin"
        entry=${XDG_DATA_HOME:-$HOME/.local/share}/applications/artifactize.desktop
        grep -qxF "Exec=\"$file/AppRun\"" "$entry" || fail "installed-linux: $entry does not start $file/AppRun"
        probe_children
        SMOKE_RECORD="$work/child" launch "$HOME/.local/bin/artifactize-desktop" --repo "$work/repo"
        check_children
        ;;
    installed-mac)
        check_commands "$file/Contents/MacOS"
        if [ -e /etc/paths.d/artifactize ]; then cp /etc/paths.d/artifactize "$work/paths.before"; fi
        launch "$file/Contents/MacOS/artifactize-desktop"
        if [ -f "$work/paths.before" ]; then
            cmp "$work/paths.before" /etc/paths.d/artifactize || fail "installed-mac: changed paths.d despite an existing CLI"
        else
            [ ! -e /etc/paths.d/artifactize ] || fail "installed-mac: created paths.d despite an existing CLI"
        fi
        ;;
esac
