#!/usr/bin/env python3
"""Exercise release policy and Store API seams without credentials or network."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.dont_write_bytecode = True
SCRIPT = Path(__file__).with_name("channels.py")
spec = importlib.util.spec_from_file_location("channels", SCRIPT)
channels = importlib.util.module_from_spec(spec)
spec.loader.exec_module(channels)


class ChannelsTest(unittest.TestCase):
    def dry_run(self, channel, version, env, fixture=None):
        command = [sys.executable, "-I", str(SCRIPT), "publish", channel, "--version", version, "--dry-run"]
        with tempfile.TemporaryDirectory() as tmp:
            if fixture:
                path = Path(tmp) / "status.json"
                path.write_text(json.dumps(fixture))
                command += ["--store-status", str(path)]
            result = subprocess.run(command, env={**os.environ, **env}, text=True, capture_output=True, check=True)
        print(result.stdout, end="")
        return result.stdout

    def test_prereleases_and_missing_secrets_skip(self):
        for channel, names in channels.SECRETS.items():
            with self.subTest(channel=channel):
                env = {name: "" for name in names}
                self.assertIn("skipped prerelease", self.dry_run(channel, "1.2.3-alpha.1", env))
                self.assertIn("missing", self.dry_run(channel, "1.2.3", env))
                env = {name: "fixture-not-a-credential" for name in names}
                self.assertIn("would publish", self.dry_run(channel, "1.2.3", env))

    def test_workflow_gate_emits_skip_and_enable_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "actions-output"
            env = {**os.environ, "GITHUB_OUTPUT": str(output), "WINGET_TOKEN": ""}
            command = [sys.executable, "-I", str(SCRIPT), "gate", "winget", "--version", "1.2.3"]
            result = subprocess.run(command, env=env, text=True, capture_output=True, check=True)
            self.assertIn("missing WINGET_TOKEN", result.stdout)
            self.assertEqual(output.read_text(), "enabled=false\n")
            env["WINGET_TOKEN"] = "fixture-not-a-credential"
            subprocess.run(command, env=env, check=True)
            self.assertEqual(output.read_text(), "enabled=false\nenabled=true\n")

    def test_store_certification_and_manual_drafts_are_not_deleted(self):
        env = {name: "fixture-not-a-credential" for name in channels.SECRETS["store"]}
        for status in ("Certification", "CommitStarted", "PreProcessing", "PendingCommit", "CertificationFailed"):
            fixture = {"application": {"pendingApplicationSubmission": {"id": "previous"}}, "status": {"status": status}}
            output = self.dry_run("store", "1.2.3", env, fixture)
            self.assertIn("skipped pending submission", output)
            self.assertIn(status, output)
        fixture = {"application": {}, "status": {}}
        self.assertIn("first submission", self.dry_run("store", "1.2.3", env, fixture))

    def test_metadata_references_both_archive_binaries(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for target in (*channels.MAC_TARGETS, channels.WINDOWS_TARGET):
                (root / channels.archive_name("1.2.3", target)).write_bytes(b"fixture archive")
            for kind in ("homebrew", "winget"):
                subprocess.run([sys.executable, "-I", str(SCRIPT), "generate", kind, "--version", "1.2.3",
                                "--dist", tmp, "--output", str(root / kind)], check=True)
            formula = (root / "homebrew/artifactize.rb").read_text()
            self.assertIn('bin.install "artifactize", "artifactize-tools"', formula)
            self.assertIn("test do", formula)
            for target in channels.MAC_TARGETS:
                self.assertIn(channels.archive_name("1.2.3", target), formula)
            manifest = json.loads((root / f"winget/{channels.PACKAGE}.installer.yaml").read_text())
            nested = manifest["NestedInstallerFiles"]
            self.assertEqual([item["PortableCommandAlias"] for item in nested], ["artifactize", "artifactize-tools"])
            self.assertTrue(all(item["RelativeFilePath"].startswith("artifactize-v1.2.3-") for item in nested))

    def test_store_preserves_listing_and_replaces_packages_before_commit(self):
        calls = []
        submission = {"id": "new", "fileUploadUrl": "https://upload.invalid/fixture",
                      "listings": {"en-us": {"description": "published listing"}},
                      "applicationPackages": [{"fileName": "old.msix", "fileStatus": "Uploaded"}]}

        def api(url, method="GET", body=None, token=None):
            calls.append((url, method, body))
            self.assertEqual(token, "fixture")
            if url.endswith(channels.STORE_ID):
                return {"lastPublishedApplicationSubmission": {"id": "old"}}
            if method == "POST" and url.endswith("/submissions"):
                return submission
            return {}

        def upload(url, archive):
            self.assertEqual(url, submission["fileUploadUrl"])
            with zipfile.ZipFile(archive) as package:
                self.assertEqual(package.namelist(), ["new.msix"])
                self.assertEqual(package.read("new.msix"), b"test msix")
            calls.append(("upload", "PUT", None))

        with tempfile.TemporaryDirectory() as tmp:
            package = Path(tmp) / "new.msix"
            package.write_bytes(b"test msix")
            env = {name: "fixture" for name in channels.SECRETS["store"]}
            with patch("urllib.request.urlopen", return_value=io.BytesIO(b'{"access_token":"fixture"}')):
                channels.submit_store(package, env, api=api, upload=upload)
        self.assertEqual([call[1] for call in calls], ["GET", "POST", "PUT", "PUT", "POST"])
        update = calls[2][2]
        self.assertEqual(update["listings"], {"en-us": {"description": "published listing"}})
        self.assertEqual(update["applicationPackages"], [
            {"fileName": "old.msix", "fileStatus": "PendingDelete"},
            {"fileName": "new.msix", "fileStatus": "PendingUpload",
             "minimumDirectXVersion": "None", "minimumSystemRam": "None"}])
        self.assertTrue(calls[-1][0].endswith("/commit"))

    def test_live_store_path_skips_before_mutating_pending_submission(self):
        calls = []

        def api(url, method="GET", body=None, token=None):
            calls.append(method)
            self.assertEqual(method, "GET")
            if url.endswith("/status"):
                return {"status": "Certification"}
            return {"pendingApplicationSubmission": {"id": "previous"}}

        env = {name: "fixture" for name in channels.SECRETS["store"]}
        with patch("urllib.request.urlopen", return_value=io.BytesIO(b'{"access_token":"fixture"}')):
            with contextlib.redirect_stdout(io.StringIO()) as output:
                channels.submit_store(Path("not-read.msix"), env, api=api)
        self.assertEqual(calls, ["GET", "GET"])
        self.assertIn("Certification", output.getvalue())


if __name__ == "__main__":
    unittest.main()
