#!/usr/bin/env python3
"""Build and exercise native desktop packages; never publish to an install channel.

Release and instrumented test packages use the same bundler recipe. Only the latter
embed WDIO; they stay in runner scratch and are never uploaded. externalBin puts
both commands in Contents/MacOS on macOS and usr/bin on Linux, beside the app,
so core's existing sibling lookup works without PATH or a bundle-specific fallback.
"""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]
DESKTOP = REPO / "crates/artifactize-desktop"
UI = DESKTOP / "ui"
APPLE_KEYS = ("APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY",
              "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID")


def run(*args, cwd=REPO, env=None, capture=False):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), cwd=cwd, env=env, check=True,
                          text=True, stdout=subprocess.PIPE if capture else None).stdout


def pnpm(*args, **kwargs):
    # Windows package-manager shims are command files, not native executables.
    command = ["cmd.exe", "/d", "/c", "pnpm"] if sys.platform == "win32" else ["pnpm"]
    return run(*command, "-C", UI, *args, **kwargs)


def target_directory(target):
    return Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")).resolve() / target / "release"


def build(target, version, output, scratch, e2e=False):
    windows = "windows" in target
    macos = "apple" in target
    suffix = ".exe" if windows else ""
    binaries = target_directory(target)
    env = os.environ.copy()
    signed = macos and not e2e and env.get("DESKTOP_APPLE_SIGNED") == "true"
    if not signed:
        for key in APPLE_KEYS:
            env.pop(key, None)
    if macos:
        print("Developer ID signing and notarization enabled" if signed else
              "Unsigned build: Developer ID, spctl and notarization/staple checks skipped", flush=True)
    pnpm("build", "--mode", "e2e" if e2e else "production", env=env)
    # Only the packaging overlay declares sidecars. Plain cargo/dev/E2E builds do
    # not need staged release binaries, and bundling remains explicitly enabled.
    sidecars = scratch / "sidecars"
    sidecars.mkdir()
    for name in ("artifactize", "artifactize-tools"):
        shutil.copy2(binaries / (name + suffix), sidecars / f"{name}-{target}{suffix}")
    config = {"bundle": {
        "externalBin": [str(sidecars / name) for name in ("artifactize", "artifactize-tools")],
        # A license resource is not an installer EULA: licenseFile would make
        # macOS disk-image mounting require accepting a new license agreement.
        "resources": {str(REPO / "LICENSE"): "LICENSE"},
    }}
    if e2e:
        declared = json.loads((DESKTOP / "tauri.conf.json").read_text())
        config["app"] = {"withGlobalTauri": True, "windows": [
            {**window, "backgroundThrottling": "disabled"} for window in declared["app"]["windows"]]}
    overlay = scratch / "bundle.json"
    overlay.write_text(json.dumps(config))
    args = ["node", UI / "node_modules/@tauri-apps/cli/tauri.js", "build", "--verbose", "--target", target,
            "--config", overlay]
    if windows:
        args += ["--no-bundle"]
    else:
        # Remove stale production packages before creating the diagnostic twins.
        shutil.rmtree(binaries / "bundle", ignore_errors=True)
        args += ["--bundles", "app,dmg" if macos else "appimage,deb"]
    if e2e:
        args += ["--features", "e2e"]
    run(*args, "--", "--locked", cwd=DESKTOP, env=env)
    output.mkdir(parents=True, exist_ok=True)
    if windows:
        # package.sh and build-msix.ps1 consume this executable with both commands.
        return
    if macos:
        app = binaries / "bundle/macos/Artifactize.app"
        if signed:
            run("spctl", "--assess", "--type", "execute", "--verbose=2", app)
            run("xcrun", "stapler", "validate", app)
        archive = output / f"Artifactize-v{version}-{target}.app.zip"
        # Preserve macOS resource forks and metadata, including the notarized app.
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", app, archive)
        packages = list((binaries / "bundle/dmg").glob("*.dmg"))
    else:
        packages = list((binaries / "bundle/appimage").glob("*.AppImage"))
        packages += list((binaries / "bundle/deb").glob("*.deb"))
    if len(packages) != (1 if macos else 2):
        raise RuntimeError(f"missing native packages: {packages}")
    for package in packages:
        # Include the Rust target, including macOS's two architectures, in asset names.
        dest = output / f"Artifactize-v{version}-{target}{package.suffix}"
        shutil.copy2(package, dest)
    for path in output.iterdir():
        if path.is_file() and not path.name.endswith(".sha256"):
            with path.open("rb") as stream:
                digest = hashlib.sha256()
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)
                digest = digest.hexdigest()
            path.with_name(path.name + ".sha256").write_text(f"{digest}  {path.name}\n")


def versions(directory, version):
    for name in ("artifactize", "artifactize-tools"):
        suffix = ".exe" if sys.platform == "win32" else ""
        actual = run(directory / (name + suffix), "--version", capture=True).strip()
        if actual != f"{name} {version}":
            raise RuntimeError(f"bundled {name} version: {actual}")


def launch(app, root, macos=False):
    """The uninstrumented shipped app must stay alive, not fail at dynamic loading.

    The diagnostic twin below checks rendered state and all desktop journeys.
    macOS goes through LaunchServices, as opening an installed .app does.
    """
    args = [str(app), "--state-dir", str(root / "launch-state"), "--reviewer", "packaging-smoke"]
    if macos:
        args = ["open", "-W", "-n", "-a", str(app), "--args", *args[1:]]
    process = subprocess.Popen(args, cwd=root, start_new_session=sys.platform == "linux")
    try:
        try:
            code = process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            return
        raise RuntimeError(f"packaged app exited during launch: {code}")
    finally:
        if macos:
            # open -W is a waiter; terminating it does not terminate its app.
            run("osascript", "-e", 'tell application id "dev.artifactize.desktop" to quit')
        elif sys.platform == "linux":
            # AppImage extract-and-run has a runtime waiter in front of the app.
            # This launch probe owns the whole process group, not only that waiter.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        elif process.poll() is None:
            process.terminate()
        process.wait(timeout=30)


def journeys(app, cli, installation, observer, scratch):
    root = scratch / "candidate"
    root.mkdir()
    (root / "tmp").mkdir()
    shutil.copy2(observer, root / observer.name)
    manifest = root / "candidate.json"
    manifest.write_text(json.dumps({"root": str(root), "manifest": str(manifest),
        "app": str(app), "cli": str(cli), "observer": str(root / observer.name),
        "installation": str(installation)}))
    env = {**os.environ, "DESKTOP_TEST_CANDIDATE": str(manifest)}
    pnpm("e2e", "packaged", env=env)


def test_unix(target, packages, scratch, version, e2e=False):
    observer = target_directory(target) / "artifactize-e2e-observe"
    macos = "apple" in target
    if macos:
        app = Path("/Applications/Artifactize.app")
        for kind in ("dmg", "app.zip"):
            owned = scratch / kind
            owned.mkdir()
            if app.exists():
                raise RuntimeError(f"refuse to replace an existing app: {app}")
            package, = packages.glob(f"*.{kind}")
            if kind == "dmg":
                mount = owned / "volume"
                mount.mkdir()
                run("hdiutil", "attach", package, "-nobrowse", "-readonly", "-mountpoint", mount)
                try:
                    run("ditto", mount / "Artifactize.app", app)
                finally:
                    run("hdiutil", "detach", mount)
            else:
                run("ditto", "-x", "-k", package, owned)
                run("ditto", owned / "Artifactize.app", app)
            try:
                binaries = app / "Contents/MacOS"
                versions(binaries, version)
                if e2e:
                    journeys(binaries / "artifactize-desktop", binaries / "artifactize", binaries, observer, owned)
                else:
                    launch(app, owned, macos=True)
            except BaseException:
                print(f"Retained failed installation for native shutdown diagnostics: {app}", file=sys.stderr)
                raise
            else:
                shutil.rmtree(app)
    else:
        image, = packages.glob("*.AppImage")
        image.chmod(0o755)
        # FUSE is not required with AppImage's own extract-and-run support. Still
        # launch the AppImage, never a loose target/release desktop executable.
        os.environ["APPIMAGE_EXTRACT_AND_RUN"] = "1"
        extracted = scratch / "image"
        extracted.mkdir()
        run(image, "--appimage-extract", cwd=extracted, capture=True)
        binaries = extracted / "squashfs-root/usr/bin"
        versions(binaries, version)
        if e2e:
            # The AppImage runtime's extract-and-run waiter does not exec the app,
            # so WDIO cannot own its native lifetime through that wrapper. Its own
            # extracted AppRun execs the app with the bundled GTK/GStreamer setup,
            # preserving the real package layout and the service's sole ownership.
            appdir = extracted / "squashfs-root"
            journeys(appdir / "AppRun", binaries / "artifactize", appdir, observer, scratch)
        else:
            launch(image, scratch)
        deb, = packages.glob("*.deb")
        run("sudo", "apt-get", "install", "-y", deb)
        try:
            binaries = Path("/usr/bin")
            versions(binaries, version)
            if e2e:
                other = scratch / "deb-test"
                other.mkdir()
                journeys(binaries / "artifactize-desktop", binaries / "artifactize", binaries, observer, other)
            else:
                launch(binaries / "artifactize-desktop", scratch)
        except BaseException:
            print("Retained failed Debian installation for native shutdown diagnostics", file=sys.stderr)
            raise
        else:
            package = run("dpkg-deb", "-f", deb, "Package", capture=True).strip()
            run("sudo", "apt-get", "remove", "-y", package)


@contextmanager
def resources():
    # On failure the harness may not have confirmed native shutdown. Keep every
    # immutable package and fixture input until the disposable runner is retired.
    scratch = Path(tempfile.mkdtemp(prefix="artifactize-packages-", dir=os.environ.get("RUNNER_TEMP"))).resolve()
    try:
        yield scratch
    except BaseException:
        print(f"Retained packaging diagnostics and inputs: {scratch}", file=sys.stderr)
        raise
    else:
        shutil.rmtree(scratch)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["build", "test", "installed", "probe"])
    parser.add_argument("--target", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, default=REPO / "desktop-dist")
    parser.add_argument("--app", type=Path)
    args = parser.parse_args()
    # Each invocation owns one empty directory, including every extraction. Never
    # share test fixture .artf files through the system temporary-directory root.
    with resources() as scratch:
        output = args.output.resolve()
        if args.mode == "probe":
            if args.app is None:
                parser.error("probe requires --app")
            app = args.app.resolve()
            versions(app.parent, args.version)
            launch(app, scratch)
        elif args.mode == "installed":
            if args.app is None:
                parser.error("installed requires --app")
            # Keep the registered AppExecLink spelling: resolving a package path
            # bypasses activation and Windows refuses execution from that shell.
            app = args.app.absolute()
            cli = app.parent / "artifactize.exe"
            if run(cli, "--version", capture=True).strip() != f"artifactize {args.version}":
                raise RuntimeError("installed MSIX CLI version mismatch")
            journeys(app, cli, app.parent,
                     target_directory(args.target) / "artifactize-e2e-observe.exe", scratch)
        elif args.mode == "build":
            build(args.target, args.version, output, scratch)
        else:
            if "windows" in args.target:
                import zipfile
                archive = REPO / f"dist/artifactize-v{args.version}-{args.target}.zip"
                portable = scratch / "shipped-portable"
                with zipfile.ZipFile(archive) as zip_file:
                    zip_file.extractall(portable)
                binaries = portable / archive.stem
                versions(binaries, args.version)
                launch(binaries / "artifactize-desktop.exe", scratch)
            else:
                test_unix(args.target, output, scratch, args.version)
            # Rebuild with test-only instrumentation, then package and install the
            # twin exactly as the shipped app. None of it enters uploaded artifacts.
            twin = scratch / "instrumented"
            twin.mkdir()
            packages = twin / "packages"
            build(args.target, args.version, packages, twin, e2e=True)
            if "windows" in args.target:
                run("sh", ".github/scripts/package.sh", args.target, args.version)
                run("powershell", "-NoProfile", "-File", ".github/scripts/smoke-channels.ps1",
                    "-Version", args.version, "-Dist", "dist", "-Channel", "msix", "-DesktopE2E")
                # Exercise the portable archive too, with the same diagnostic twin.
                archive = REPO / f"dist/artifactize-v{args.version}-{args.target}.zip"
                import zipfile
                with zipfile.ZipFile(archive) as zip_file:
                    zip_file.extractall(twin / "portable")
                directory = twin / "portable" / archive.stem
                journeys(directory / "artifactize-desktop.exe", directory / "artifactize.exe", directory,
                         target_directory(args.target) / "artifactize-e2e-observe.exe", twin)
            else:
                test_unix(args.target, packages, twin, args.version, e2e=True)


if __name__ == "__main__":
    main()
