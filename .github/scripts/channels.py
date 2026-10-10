#!/usr/bin/env python3
"""Generate release channel metadata and publish only explicitly enabled channels.

Run with python -I. No third-party dependencies; --dry-run never contacts services.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile

PACKAGE = "lhj6102.Artifactize"
STORE_ID = "9PB6W4LL165D"
SCHEMA = "1.9.0"
MAC_TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin")
WINDOWS_TARGET = "x86_64-pc-windows-msvc"
SECRETS = {
    "homebrew": ("HOMEBREW_TAP_TOKEN",),
    "winget": ("WINGET_TOKEN",),
    "store": ("STORE_TENANT_ID", "STORE_CLIENT_ID", "STORE_CLIENT_SECRET"),
}


def archive_name(version, target):
    suffix = "zip" if target == WINDOWS_TARGET else "tar.gz"
    return f"artifactize-v{version}-{target}.{suffix}"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as archive:
        for chunk in iter(lambda: archive.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest().upper()


def generate(args):
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    base = args.base_url or f"https://github.com/lhj6102/artifactize/releases/download/v{args.version}"
    if args.kind == "homebrew":
        lines = ["class Artifactize < Formula", '  desc "Declare and verify artifacts with reusable evidence"',
                 '  homepage "https://artifactize.dev"', f'  version "{args.version}"',
                 '  license "Apache-2.0"', "", "  on_macos do"]
        for target in args.targets:
            archive = archive_name(args.version, target)
            arch = "arm" if target.startswith("aarch64") else "intel"
            lines += [f"    on_{arch} do", f'      url "{base}/{archive}"',
                      f'      sha256 "{sha256(Path(args.dist) / archive).lower()}"', "    end"]
        lines += ["  end", "", "  def install", '    bin.install "artifactize", "artifactize-tools"',
                  "  end", "", "  test do", '    assert_equal "artifactize #{version}", shell_output("#{bin}/artifactize --version").strip',
                  '    assert_equal "artifactize-tools #{version}", shell_output("#{bin}/artifactize-tools --version").strip',
                  '    (testpath/"sample.txt").write("artifactize packaging test\\n")',
                  '    assert_match "artifactize packaging test", shell_output("#{bin}/artifactize-tools read sample.txt")',
                  "  end", "end", ""]
        (output / "artifactize.rb").write_text("\n".join(lines), encoding="utf-8")
    else:
        common = {"PackageIdentifier": PACKAGE, "PackageVersion": args.version}
        archive = archive_name(args.version, WINDOWS_TARGET)
        # JSON is a YAML 1.2 subset accepted by winget and avoids YAML quoting bugs.
        manifests = {
            "": {**common, "DefaultLocale": "en-US", "ManifestType": "version"},
            ".locale.en-US": {**common, "PackageLocale": "en-US", "Publisher": "lhj6102",
                              "PackageName": "Artifactize", "License": "Apache-2.0",
                              "LicenseUrl": "https://github.com/lhj6102/artifactize/blob/main/LICENSE",
                              "ShortDescription": "Declare and verify artifacts with reusable evidence",
                              "PackageUrl": "https://artifactize.dev", "ManifestType": "defaultLocale"},
            ".installer": {**common, "InstallerType": "zip", "NestedInstallerType": "portable",
                           "NestedInstallerFiles": [
                               {"RelativeFilePath": f"artifactize-v{args.version}-{WINDOWS_TARGET}/{command}.exe",
                                "PortableCommandAlias": command}
                               for command in ("artifactize", "artifactize-tools")],
                           "Commands": ["artifactize", "artifactize-tools"],
                           "Installers": [{"Architecture": "x64", "InstallerUrl": f"{base}/{archive}",
                                           "InstallerSha256": sha256(Path(args.dist) / archive)}],
                           "ManifestType": "installer"},
        }
        for suffix, manifest in manifests.items():
            manifest["ManifestVersion"] = SCHEMA
            (output / f"{PACKAGE}{suffix}.yaml").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def channel_enabled(channel, version, env):
    # A normal Homebrew formula follows stable releases, not --HEAD/prereleases.
    if "-" in version:
        print(f"{channel}: skipped prerelease {version}")
        return False
    missing = [name for name in SECRETS[channel] if not env.get(name)]
    if missing:
        print(f"{channel}: skipped; missing {', '.join(missing)}")
        return False
    return True


def store_pending(application, status):
    pending = application.get("pendingApplicationSubmission")
    if pending:
        # Never delete someone else's draft, failed submission or certification.
        print(f"store: skipped pending submission {pending['id']} ({status.get('status', 'unknown')}); certification/draft must finish first")
        return True
    if not application.get("lastPublishedApplicationSubmission"):
        print("store: skipped; first submission must be published manually in Partner Center")
        return True
    return False


def request_json(url, method="GET", body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=120) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        # Do not echo URLs/bodies: the upload URL contains a SAS credential.
        raise RuntimeError(f"Store API {method} failed: HTTP {error.code}") from None


def submit_store(package, env, api=request_json, upload=None):
    tenant = urllib.parse.quote(env["STORE_TENANT_ID"], safe="")
    data = urllib.parse.urlencode({"grant_type": "client_credentials", "client_id": env["STORE_CLIENT_ID"],
                                  "client_secret": env["STORE_CLIENT_SECRET"],
                                  "resource": "https://manage.devcenter.microsoft.com"}).encode()
    req = urllib.request.Request(f"https://login.microsoftonline.com/{tenant}/oauth2/token", data=data)
    with urllib.request.urlopen(req, timeout=120) as response:
        token = json.load(response)["access_token"]
    base = f"https://manage.devcenter.microsoft.com/v1.0/my/applications/{STORE_ID}"
    application = api(base, token=token)
    pending = application.get("pendingApplicationSubmission")
    status = api(f"{base}/submissions/{pending['id']}/status", token=token) if pending else {}
    if store_pending(application, status):
        return
    submission = api(f"{base}/submissions", method="POST", token=token)
    submission_id = submission["id"]
    # Keep the published listing, ratings, pricing and capabilities. Replace only packages.
    packages = submission.get("applicationPackages", [])
    for old in packages:
        old["fileStatus"] = "PendingDelete"
    packages.append({"fileName": package.name, "fileStatus": "PendingUpload",
                     "minimumDirectXVersion": "None", "minimumSystemRam": "None"})
    submission["applicationPackages"] = packages
    api(f"{base}/submissions/{submission_id}", method="PUT", body=submission, token=token)
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / "submission.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
            bundle.write(package, package.name)
        if upload:
            upload(submission["fileUploadUrl"], archive)
        else:
            req = urllib.request.Request(submission["fileUploadUrl"], data=archive.read_bytes(), method="PUT",
                                         headers={"x-ms-blob-type": "BlockBlob"})
            try:
                with urllib.request.urlopen(req, timeout=300):
                    pass
            except urllib.error.URLError:
                # HTTPError's usual traceback prints the SAS-bearing upload URL.
                raise RuntimeError("Store package upload failed; check the pending submission") from None
    api(f"{base}/submissions/{submission_id}/commit", method="POST", token=token)
    print(f"store: committed submission {submission_id}; certification is asynchronous")


def publish(args):
    if not channel_enabled(args.channel, args.version, os.environ):
        return
    if args.dry_run:
        if args.channel == "store":
            fixture = json.loads(Path(args.store_status).read_text()) if args.store_status else {
                "application": {"lastPublishedApplicationSubmission": {"id": "published"}}, "status": {}}
            if store_pending(fixture["application"], fixture["status"]):
                return
        print(f"{args.channel}: dry-run would publish {args.version}; no network or writes")
        return
    if args.channel == "store":
        submit_store(Path(args.package), os.environ)
    elif args.channel == "homebrew":
        with tempfile.TemporaryDirectory() as tmp:
            env = {**os.environ, "GH_TOKEN": os.environ["HOMEBREW_TAP_TOKEN"]}
            subprocess.run(["gh", "repo", "clone", "lhj6102/homebrew-tap", tmp], env=env, check=True)
            tap = Path(tmp)
            formula = tap / "Formula" / "artifactize.rb"
            formula.parent.mkdir(exist_ok=True)
            formula.write_bytes(Path(args.formula).read_bytes())
            for command in (["config", "user.name", "artifactize release"],
                            ["config", "user.email", "67728205+lhj6102@users.noreply.github.com"],
                            ["add", "Formula/artifactize.rb"]):
                subprocess.run(["git", "-C", tmp, *command], check=True)
            if subprocess.run(["git", "-C", tmp, "diff", "--cached", "--quiet"]).returncode == 0:
                print("homebrew: formula already current")
                return
            subprocess.run(["git", "-C", tmp, "commit", "-m", f"artifactize {args.version}"], check=True)
            # gh supplies credentials without putting the PAT in git URLs/logs/config.
            subprocess.run(["gh", "auth", "setup-git"], env=env, check=True)
            subprocess.run(["git", "-C", tmp, "push"], env=env, check=True)
    else:
        # Submit the generated set, not `update`: archive-root names change per version.
        result = subprocess.run(["wingetcreate", "submit", "--no-open", "--token", os.environ["WINGET_TOKEN"],
                                 str(Path(args.manifests).resolve())])
        if result.returncode:
            # CalledProcessError would include argv, including the credential.
            raise RuntimeError(f"wingetcreate submission failed: exit {result.returncode}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("generate")
    gen.add_argument("kind", choices=("homebrew", "winget"))
    gen.add_argument("--dist", required=True)
    gen.add_argument("--output", required=True)
    gen.add_argument("--base-url")
    gen.add_argument("--targets", nargs="+", choices=MAC_TARGETS, default=MAC_TARGETS)
    gate = sub.add_parser("gate", help="Evaluate release policy and emit an Actions enabled output")
    gate.add_argument("channel", choices=SECRETS)
    pub = sub.add_parser("publish")
    pub.add_argument("channel", choices=SECRETS)
    pub.add_argument("--dry-run", action="store_true")
    pub.add_argument("--store-status", help="JSON fixture used only with --dry-run")
    pub.add_argument("--package")
    pub.add_argument("--formula")
    pub.add_argument("--manifests")
    for cmd in (gen, gate, pub):
        cmd.add_argument("--version", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", args.version):
        parser.error("version must be major.minor.patch[-prerelease]")
    if args.command == "gate":
        enabled = channel_enabled(args.channel, args.version, os.environ)
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                output.write(f"enabled={str(enabled).lower()}\n")
    else:
        (generate if args.command == "generate" else publish)(args)


if __name__ == "__main__":
    main()
