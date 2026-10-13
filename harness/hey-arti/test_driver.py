"""Deterministic harness seams. Fake declarations are tests, not skill-quality evidence."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
# Loading the driver must not leave bytecode among the reviewed harness inputs.
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("hey_arti_driver", ROOT / "driver.py")
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)
CANDIDATE = None


def fake_builder(repo, case_id):
    """A predictable test double, intentionally not the golden Artifact partition."""
    def write(relative, text):
        path = repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def runtime(name, instruction="Check the local fixture behavior."):
        return f'''name = "{name}"
[evals.check]
title = "{name} behavior"
profile = {{ kind = "runtime", command = {json.dumps(sys.executable)}, args = ["-I", "{{checks}}", "{name}"] }}
payload.instruction = {json.dumps(instruction)}
'''

    write("check.py.artf", 'name = "checks"\nbasis = true\n')
    if case_id == "library-consumer":
        write("library/index.artf", runtime("pricing"))
        write("app/index.artf", runtime("receipt", "Check the receipt using {pricing}."))
        write("independent/index.artf", runtime("label"))
    elif case_id == "docs-assets":
        write("site/content/index.artf", runtime("content"))
        write("site/theme/index.artf", runtime("theme"))
        write("assets/LICENSE.txt.artf", 'name = "license"\nbasis = true\n')
        write("assets/banner.svg.artf", runtime("banner", "Check {banner} against {license}."))
        write("site/index.artf", '''name = "site"
[evals.ready]
title = "Site inputs are ready"
profile = { kind = "dependency", depends_on = ["content", "theme", "banner"] }
''')
    else:
        write("api/index.artf", runtime("api", "Check API against {contract}."))
        write("contract/index.artf", runtime("contract"))
        write("preview/probe.py.artf", 'name = "preview-driver"\nbasis = true\n')
        ui = runtime("ui", "Check UI consumes {api} and {contract}.").replace(
            'name = "ui"\n', 'name = "ui"\nmounts = { preview = "preview-driver", backend = "api", schema = "contract" }\n')
        write("ui/index.artf", ui + f'''
[views.agent_sessions.preview]
start = {{ command = {json.dumps(sys.executable)}, args = ["-I", "{{preview-driver}}", "start"] }}
stop = {{ command = {json.dumps(sys.executable)}, args = ["-I", "{{preview-driver}}", "stop"] }}
[views.agent_tools.inspect]
session = "preview"
description = "Read the private preview."
command = {json.dumps(sys.executable)}
args = ["-I", "{{preview-driver}}", "inspect"]
protocol = "json"
[evals.review]
title = "UI and API contract review"
profile = {{ kind = "agent", backend = "codex", model = "fake-host-model" }}
payload.instruction = "Inspect {{ui}} against {{api}}, {{contract}} and {{preview-driver}}."
''')
        write("integration/index.artf", '''name = "integration"
[evals.signoff]
title = "The person approves integrated publication"
profile = { kind = "human" }
payload.instruction = "A person judges {ui} with {api}; the builder must yield."
''')
    write(".github/workflows/review.yml", '''jobs:
  review:
    steps:
      - run: artifactize verify --all --wait --reuse-only agent,human
        env:
          ARTIFACTIZE_REMOTE: https://reviews.example/
          ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
''')


def tool_request(args):
    return json.dumps({"version": 1, "context": {}, "args": args})


class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="hey-arti-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()

    def session(self):
        directory = self.root / "session"
        driver.private_directory(directory)
        environment = dict(os.environ, ARTIFACTIZE_SESSION_DIR=str(directory),
                           HEY_ARTI_EVIDENCE_INDEX=str(self.root / "no-live-index.json"))
        process = subprocess.Popen([sys.executable, "-I", str(ROOT / "driver.py"), "start"],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, env=environment)
        # A readiness barrier, not a sleep-length or timestamp assertion.
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=10)
        self.addCleanup(cleanup)
        process.stdin.write(tool_request({}))
        process.stdin.close()
        process.stdin = None
        with ThreadPoolExecutor() as executor:
            try:
                ready = executor.submit(process.stdout.readline).result(timeout=10)
            except TimeoutError:
                process.kill()
                raise
        self.assertEqual(ready, "ready\n")
        return directory, environment, process

    def call(self, environment, operation, args):
        result = subprocess.run([sys.executable, "-I", str(ROOT / "driver.py"), operation],
                                input=tool_request(args), text=True, capture_output=True,
                                timeout=15, env=environment, check=True)
        return json.loads(result.stdout)

    def test_canonical_skill_examples_use_valid_current_declarations(self):
        skill_path = ROOT.parents[1] / "crates/artifactize-core/integrations/hey-arti/SKILL.md"
        body = skill_path.read_text(encoding="utf-8")
        frontmatter = body.split("---", 2)[1]
        self.assertEqual({line.split(":", 1)[0] for line in frontmatter.strip().splitlines()}, {"name", "description"})
        snippets = re.findall(r"```toml\n(.*?)```", body, re.DOTALL)
        self.assertEqual(len(snippets), 5)
        for snippet in snippets:
            driver.tomllib.loads(snippet)
        if CANDIDATE is None:
            self.skipTest("candidate validates cross-Artifact references in cargo test")
        repo = self.root / "examples"
        (repo / "app/src").mkdir(parents=True)
        (repo / "app/tools").mkdir()
        (repo / "spec").mkdir()
        (repo / "delivery").mkdir()
        for relative, text in (("spec/index.artf", snippets[0]), ("app/index.artf", snippets[1] + snippets[4]),
                               ("logo.svg.artf", snippets[2]), ("delivery/index.artf", snippets[3]),
                               ("spec/requirements.md", "Fixture requirements.\n"), ("logo.svg", "<svg/>\n"),
                               ("app/src/lib.rs", "// Source fixture.\n"), ("app/tools/preview.py", "# Static example only.\n")):
            (repo / relative).write_text(text, encoding="utf-8")
        result = subprocess.run([str(CANDIDATE), "--repo", str(repo), "config", "check"],
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_three_source_cases_are_private_independent_and_have_real_runtime_behavior(self):
        self.assertEqual(set(driver.cases()), {"library-consumer", "docs-assets", "monorepo"})
        area = driver.materialize(self.root / "cases")
        for case_id, case in driver.cases().items():
            repo = area / case_id / "repo"
            self.assertFalse(list(repo.rglob("*.artf")))
            subprocess.run([sys.executable, "-I", str(repo / "check.py")], check=True, timeout=10)
            independent = (repo / case["independent_path"]).read_bytes()
            edited = repo / case["edit_path"]
            original = edited.read_text(encoding="utf-8")
            self.assertEqual(original.count(case["edit_before"]), 1)
            edited.write_text(original.replace(case["edit_before"], case["edit_after"]), encoding="utf-8")
            self.assertEqual((repo / case["independent_path"]).read_bytes(), independent)
            if case_id != "docs-assets":
                result = subprocess.run([sys.executable, "-I", str(repo / "check.py")],
                                        capture_output=True, timeout=10)
                self.assertNotEqual(result.returncode, 0, "consumer regression must be observable")
            source = driver.inside(ROOT, case["source"]) / case["edit_path"]
            self.assertEqual(source.read_text(encoding="utf-8"), original)
        with self.assertRaises(FileExistsError):
            driver.materialize(area)
        with self.assertRaises(ValueError):
            driver.materialize(ROOT / "outputs-must-not-be-created")

    def test_json_session_case_read_paging_rejection_and_stop(self):
        directory, environment, process = self.session()
        result = self.call(environment, "case", {"case": "monorepo"})["content"][0]["data"]
        self.assertFalse(result["live_evidence"]["available"])
        self.assertIn("Human", result["goal"])
        self.assertIn("preview/probe.py", result["source_files"])
        repo = Path(result["repository"])
        self.assertTrue(repo.is_dir())
        text = "café\n" * 10000
        (repo / "large.txt").write_text(text, encoding="utf-8")
        offset, pages = 0, []
        while True:
            page = self.call(environment, "read", {"case": "monorepo", "path": "large.txt", "offset": offset})
            data = page["content"][0]["data"]
            self.assertLess(len(json.dumps(page).encode()), driver.RESPONSE_BYTES)
            pages.append(data["text"])
            if data["next_offset"] is None:
                break
            offset = data["next_offset"]
        self.assertEqual("".join(pages), text)
        for path in ("../driver.py", str(ROOT / "driver.py"), ".env", "a/../ui/view.py", "ui\\view.py"):
            self.assertTrue(self.call(environment, "read", {"case": "monorepo", "path": path})["isError"])
        self.assertTrue(self.call(environment, "case", {"case": "unknown"})["isError"])
        self.assertTrue(self.call(environment, "read", {"case": "monorepo", "path": "ui/view.py", "limit": 999999})["isError"])
        evidence = driver.area(directory) / "monorepo/evidence/live.json"
        evidence.write_text('{"mode":"deterministic","verdict":"GREEN"}', encoding="utf-8")
        self.assertTrue(self.call(environment, "case", {"case": "monorepo"})["isError"])
        subprocess.run([sys.executable, "-I", str(ROOT / "driver.py"), "stop"], input=tool_request({}),
                       text=True, check=True, timeout=10, env=environment)
        stdout, stderr = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, stderr)
        self.assertEqual(stdout, "")
        self.assertFalse(repo.exists())
        self.assertFalse((directory / "session.json").exists())
        # Safe repeated stop; artifactize owns the final session directory removal.
        subprocess.run([sys.executable, "-I", str(ROOT / "driver.py"), "stop"], input=tool_request({}),
                       text=True, check=True, timeout=10, env=environment)

    def test_monorepo_preview_uses_actual_inputs_and_artifactize_session_envelope(self):
        area = driver.materialize(self.root / "cases")
        repo = area / "monorepo/repo"
        session = self.root / "preview-session"
        driver.private_directory(session)
        environment = dict(os.environ, ARTIFACTIZE_SESSION_DIR=str(session))
        script = repo / "preview/probe.py"
        process = subprocess.Popen([sys.executable, "-I", str(script), "start"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment)
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=10)
        self.addCleanup(cleanup)
        process.stdin.write(tool_request({}))
        process.stdin.close()
        process.stdin = None
        with ThreadPoolExecutor() as executor:
            try:
                ready = executor.submit(process.stdout.readline).result(timeout=10)
            except TimeoutError:
                process.kill()
                raise
            self.assertEqual(ready, "ready\n")
        result = subprocess.run([sys.executable, "-I", str(script), "inspect"], input=tool_request({}),
                                capture_output=True, text=True, check=True, timeout=10, env=environment)
        self.assertEqual(json.loads(result.stdout)["content"][0]["data"]["rendered"], "Hello orchard")
        source = repo / "api/handler.py"
        source.write_text(source.read_text(encoding="utf-8").replace("Hello orchard", "Hello apples"), encoding="utf-8")
        result = subprocess.run([sys.executable, "-I", str(script), "inspect"], input=tool_request({}),
                                capture_output=True, text=True, check=True, timeout=10, env=environment)
        self.assertEqual(json.loads(result.stdout)["content"][0]["data"]["rendered"], "Hello apples")
        subprocess.run([sys.executable, "-I", str(script), "stop"], input=tool_request({}), text=True,
                       check=True, timeout=10, env=environment)
        process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0)

    def test_fake_builder_declarations_validate_and_status_cover_real_consumers(self):
        if CANDIDATE is None:
            self.skipTest("candidate CLI supplied by cargo test, not installed PATH")
        area = driver.materialize(self.root / "cases")
        for case_id in driver.cases():
            root = area / case_id
            repo = root / "repo"
            fake_builder(repo, case_id)
            command = [str(CANDIDATE), "--repo", str(repo), "--state-dir", str(root / "state")]
            environment = dict(os.environ, ARTIFACTIZE_REMOTE="off")
            for args in (["config", "check"], ["config", "graph", "--json"]):
                result = subprocess.run(command + args, capture_output=True, text=True, timeout=30, env=environment)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                if "--json" in args:
                    json.loads(result.stdout)

            def status_keys():
                result = subprocess.run(command + ["status", "--all", "--json"], capture_output=True,
                                        text=True, timeout=30, env=environment)
                self.assertEqual(result.returncode, 1, result.stderr + result.stdout)  # valid but unreviewed
                return {row["id"]: row["key"] for row in json.loads(result.stdout)["evals"] if "key" in row}

            baseline = status_keys()
            self.assertEqual(status_keys(), baseline, "unchanged inputs must keep identical reuse keys")
            case = driver.cases()[case_id]
            edited = repo / case["edit_path"]
            source = edited.read_text(encoding="utf-8")
            edited.write_text(source.replace(case["edit_before"], case["edit_after"]), encoding="utf-8")
            after_edit = status_keys()
            changed = {key for key in baseline if baseline[key] != after_edit[key]}
            expected = {"library-consumer": {"pricing/check", "receipt/check"},
                        "docs-assets": {"content/check"},
                        "monorepo": {"api/check", "ui/check", "ui/review", "integration/signoff"}}
            self.assertEqual(changed, expected[case_id], "real consumers, not independent inputs, must invalidate")
            declaration = {"library-consumer": "independent/index.artf", "docs-assets": "site/theme/index.artf",
                           "monorepo": "contract/index.artf"}[case_id]
            path = repo / declaration
            definition = path.read_text(encoding="utf-8")
            path.write_text(definition.replace('payload.instruction = "', 'payload.instruction = "Extra judgment. ', 1),
                            encoding="utf-8")
            after_judgment = status_keys()
            expected_eval = {"library-consumer": "label/check", "docs-assets": "theme/check", "monorepo": "contract/check"}[case_id]
            self.assertEqual({key for key in after_edit if after_edit[key] != after_judgment[key]}, {expected_eval})
            # Restore application inputs before executing the actual read-only session tool.
            edited.write_text(source, encoding="utf-8")
            # Exercise actual session admission/cleanup through the candidate, not just Popen.
            if case_id == "monorepo":
                result = subprocess.run(command + ["tools", "check", "--execute", "--artifact", "ui",
                                                   "--audience", "agent", "--tool", "inspect"],
                                        capture_output=True, text=True, timeout=30, env=environment)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                self.assertIn("Hello orchard", result.stdout)
            workflow = (repo / ".github/workflows/review.yml").read_text(encoding="utf-8")
            self.assertIn("--wait --reuse-only agent,human", workflow)
            self.assertIn("secrets.ARTIFACTIZE_READ_TOKEN", workflow)
            self.assertFalse(list((root / "evidence").iterdir()), "fake results are never live evidence")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifactize", type=Path)
    options = parser.parse_args()
    CANDIDATE = options.artifactize
    unittest.main(argv=[sys.argv[0]])
