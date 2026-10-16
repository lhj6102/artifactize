"""Deterministic harness seams. Fake declarations are tests, not skill-quality evidence."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent
# Loading the driver must not leave bytecode among the reviewed harness inputs.
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("hey_arti_driver", ROOT / "driver.py")
driver = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = driver
sys.modules["driver"] = driver
spec.loader.exec_module(driver)
CANDIDATE = None


def load_harness(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


live = load_harness("live")
host_smoke = load_harness("host_smoke")


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


def skill_examples(body):
    """Select declarations by their documented obligations, not Markdown order."""
    roles = {}
    for text in re.findall(r"```toml\n(.*?)```", body, re.DOTALL):
        declaration = driver.tomllib.loads(text)
        kinds = {evaluation["profile"]["kind"] for evaluation in declaration.get("evals", {}).values()}
        if declaration.get("basis"):
            role = "basis"
        elif {"runtime", "agent"} <= kinds:
            role = "application"
        elif "human" in kinds:
            role = "human"
        elif "dependency" in kinds:
            role = "dependency"
        elif declaration.get("views", {}).get("agent_sessions"):
            role = "preview"
        else:
            continue  # Additional valid explanatory fragments do not reorder roles.
        if role in roles:
            raise ValueError(f"ambiguous {role} example")
        roles[role] = (text, declaration)
    return roles


class OwnedTestCase(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="hey-arti-test-")).resolve()
        self.children = []
        self.addCleanup(self.release_files)

    def spawn(self, command, **options):
        process = driver.spawn(command, **options)
        self.children.append(process)
        return process

    def release_files(self):
        failures = []
        for process in self.children:
            try:
                driver.close_child(process)
            except Exception as error:
                failures.append(error)
        if failures:
            raise ExceptionGroup(f"Owned children remain; fixture retained at {self.root.as_posix()}", failures)
        if self.root.exists():
            shutil.rmtree(self.root)


class HarnessTests(OwnedTestCase):

    def session(self):
        directory = self.root / "session"
        driver.private_directory(directory)
        environment = dict(os.environ, ARTIFACTIZE_SESSION_DIR=str(directory),
                           HEY_ARTI_EVIDENCE_INDEX=str(self.root / "no-live-index.json"))
        process = self.spawn([sys.executable, "-I", str(ROOT / "driver.py"), "start"],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, env=environment)
        # A readiness barrier, not a sleep-length or timestamp assertion.
        def cleanup():
            driver.close_child(process)
        self.addCleanup(cleanup)
        process.stdin.write(tool_request({}))
        process.stdin.close()
        process.stdin = None
        with ThreadPoolExecutor() as executor:
            try:
                ready = executor.submit(process.stdout.readline).result(timeout=10)
            except TimeoutError:
                driver.stop_child(process)
                raise
        self.assertEqual(ready, "ready\n")
        self.assertNotIn("\\", json.loads((directory / "session.json").read_text(encoding="utf-8"))["area"])
        return directory, environment, process

    def call(self, environment, operation, args):
        result = driver.run_command([sys.executable, "-I", str(ROOT / "driver.py"), operation],
                                input=tool_request(args), text=True, capture_output=True,
                                timeout=15, env=environment, check=True)
        return json.loads(result.stdout)

    def test_canonical_skill_examples_use_valid_current_declarations(self):
        skill_path = ROOT.parents[1] / "crates/artifactize-core/integrations/hey-arti/SKILL.md"
        body = skill_path.read_text(encoding="utf-8")
        frontmatter = dict(line.split(":", 1) for line in body.split("---", 2)[1].strip().splitlines())
        self.assertEqual(frontmatter["name"].strip(), "hey-arti")
        self.assertTrue(frontmatter["description"].strip())
        roles = skill_examples(body)
        self.assertTrue({"basis", "application", "human", "dependency", "preview"} <= roles.keys())
        basis = roles["basis"][1]["name"]
        application = roles["application"][1]["name"]
        human = roles["human"][1]["name"]
        dependency = roles["dependency"][1]["name"]
        readiness = roles["dependency"][1]["evals"]
        self.assertTrue(any(set(evaluation["profile"]["depends_on"]) == {application, human}
                            for evaluation in readiness.values()))
        self.assertIn("{" + application + "}", roles["preview"][0])
        if CANDIDATE is None:
            self.skipTest("explicit candidate validates example references")
        repo = self.root / "examples"
        products = {f"{basis}/index.artf": roles["basis"][0],
                    f"{application}/index.artf": roles["application"][0] + roles["preview"][0],
                    f"{human}.svg.artf": roles["human"][0],
                    f"{dependency}/index.artf": roles["dependency"][0],
                    f"{basis}/requirements.md": "Fixture requirements.\n", f"{human}.svg": "<svg/>\n",
                    f"{application}/src/lib.rs": "// Source fixture.\n",
                    f"{application}/tools/preview.py": "# Static example only.\n"}
        for relative, text in products.items():
            path = repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        result = driver.run_command([str(CANDIDATE), "--repo", str(repo), "config", "check"],
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_skill_examples_are_semantic_when_sections_reorder_or_new_fragments_appear(self):
        body = (ROOT.parents[1] / "crates/artifactize-core/integrations/hey-arti/SKILL.md").read_text()
        snippets = re.findall(r"```toml\n(.*?)```", body, re.DOTALL)
        changed = "\n".join("```toml\n" + snippet + "```" for snippet in reversed(snippets))
        changed += '\n```toml\nfingerprint = false\n```\n'
        roles = skill_examples(changed)
        self.assertEqual({role: roles[role][1]["name"] for role in ("basis", "application", "human", "dependency")},
                         {"basis": "spec", "application": "app", "human": "logo", "dependency": "delivery"})
        self.assertIn("preview", roles["preview"][1]["views"]["agent_sessions"])

    def test_check_keeps_suite_in_process_and_cleans_interrupted_fixtures(self):
        suite_root = self.root / "suite"
        suite_root.mkdir()
        report = self.root / "interrupted.json"
        (suite_root / "test_driver.py").write_text(f'''import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
spec = importlib.util.spec_from_file_location("declared_test_runner", {str(ROOT / 'test_driver.py')!r})
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
class Interrupted(module.OwnedTestCase):
    def test_interrupt(self):
        child = self.spawn([sys.executable, "-I", "-c", "import sys; print('ready', flush=True); sys.stdin.read()"],
                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        assert child.stdout.readline() == "ready\\n"
        Path({str(report)!r}).write_text(json.dumps({{"parent": os.getpid(), "child": child.pid, "root": self.root.as_posix()}}))
        raise KeyboardInterrupt
try:
    module.run_suite(sys.modules[__name__])
except KeyboardInterrupt:
    raise SystemExit(130)
''')
        original_arguments = sys.argv
        native_popen = subprocess.Popen
        children = []
        def capture_child(command, **options):
            process = native_popen(command, **options)
            children.append(process)
            self.children.append(process)
            return process
        with mock.patch.object(driver, "ROOT", suite_root), \
                mock.patch.object(subprocess, "Popen", side_effect=capture_child):
            self.assertEqual(driver.run_tests(None), 130)
        observed = json.loads(report.read_text())
        self.assertEqual(observed["parent"], os.getpid())
        self.assertFalse(Path(observed["root"]).exists())
        self.assertEqual(sum(child.pid == observed["child"] for child in children), 1)
        # Native tree cleanup may launch its own child; every captured process must exit.
        for child in children:
            self.assertIsNotNone(child.poll(), f"interrupted suite must join owned child {child.pid}")
        self.assertIs(sys.argv, original_arguments)

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux descendant exit observation uses pidfd")
    def test_command_timeout_joins_the_owned_descendant_group(self):
        report = self.root / "descendant.json"
        program = self.root / "declared-tree.py"
        program.write_text(f'''import json
from pathlib import Path
import subprocess
import sys
child = subprocess.Popen([sys.executable, "-I", "-c", "import sys; print('ready', flush=True); sys.stdin.read()"],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
assert child.stdout.readline() == "ready\\n"
Path({str(report)!r}).write_text(json.dumps({{"child": child.pid}}))
print("tree-ready", flush=True)
sys.stdin.read()
''')
        communicate = subprocess.Popen.communicate
        handles = []
        expired = False
        def expiry(process, *args, **options):
            nonlocal expired
            if not expired and str(program) in process.args:
                expired = True
                self.assertEqual(process.stdout.readline(), "tree-ready\n")
                child = json.loads(report.read_text())["child"]
                handles.extend([os.pidfd_open(process.pid), os.pidfd_open(child)])
                # Controlled expiry at a readiness barrier, not a wall-clock race.
                raise subprocess.TimeoutExpired(process.args, options["timeout"])
            return communicate(process, *args, **options)
        try:
            with mock.patch.object(subprocess.Popen, "communicate", expiry):
                with self.assertRaises(subprocess.TimeoutExpired):
                    driver.run_command([sys.executable, "-I", str(program)], stdin=subprocess.PIPE,
                                       capture_output=True, text=True, timeout=30)
            self.assertEqual(set(select.select(handles, [], [], 0)[0]), set(handles))
        finally:
            for descriptor in handles:
                os.close(descriptor)

    def test_three_source_cases_are_private_independent_and_have_real_runtime_behavior(self):
        self.assertEqual(set(driver.cases()), {"library-consumer", "docs-assets", "monorepo"})
        area = driver.materialize(self.root / "cases")
        for case_id, case in driver.cases().items():
            repo = area / case_id / "repo"
            self.assertFalse(list(repo.rglob("*.artf")))
            driver.run_command([sys.executable, "-I", str(repo / "check.py")], check=True, timeout=10)
            independent = (repo / case["independent_path"]).read_bytes()
            edited = repo / case["edit_path"]
            original = edited.read_text(encoding="utf-8")
            self.assertEqual(original.count(case["edit_before"]), 1)
            edited.write_text(original.replace(case["edit_before"], case["edit_after"]), encoding="utf-8")
            self.assertEqual((repo / case["independent_path"]).read_bytes(), independent)
            if case_id != "docs-assets":
                result = driver.run_command([sys.executable, "-I", str(repo / "check.py")],
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
        self.assertNotIn("\\", result["repository"])
        repo = Path(result["repository"])
        self.assertTrue(repo.is_dir())
        text = "café\n" * 10000
        # Paging returns literal file bytes; keep this UTF-8 fixture identical on every OS.
        (repo / "large.txt").write_text(text, encoding="utf-8", newline="\n")
        offset, pages = 0, []
        while True:
            page = self.call(environment, "read", {"case": "monorepo", "path": "large.txt", "offset": offset})
            data = page["content"][0]["data"]
            # Independent reviewer-envelope expectation, not the driver's cap variable.
            self.assertLess(len(json.dumps(page).encode()), 48 * 1024)
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
        driver.run_command([sys.executable, "-I", str(ROOT / "driver.py"), "stop"], input=tool_request({}),
                       text=True, check=True, timeout=10, env=environment)
        stdout, stderr = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, stderr)
        self.assertEqual(stdout, "")
        self.assertFalse(repo.exists())
        self.assertFalse((directory / "session.json").exists())
        # Safe repeated stop; artifactize owns the final session directory removal.
        driver.run_command([sys.executable, "-I", str(ROOT / "driver.py"), "stop"], input=tool_request({}),
                       text=True, check=True, timeout=10, env=environment)

    def test_monorepo_preview_uses_actual_inputs_and_artifactize_session_envelope(self):
        area = driver.materialize(self.root / "cases")
        repo = area / "monorepo/repo"
        session = self.root / "preview-session"
        driver.private_directory(session)
        environment = dict(os.environ, ARTIFACTIZE_SESSION_DIR=str(session))
        script = repo / "preview/probe.py"
        process = self.spawn([sys.executable, "-I", str(script), "start"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment)
        def cleanup():
            driver.close_child(process)
        self.addCleanup(cleanup)
        process.stdin.write(tool_request({}))
        process.stdin.close()
        process.stdin = None
        with ThreadPoolExecutor() as executor:
            try:
                ready = executor.submit(process.stdout.readline).result(timeout=10)
            except TimeoutError:
                driver.stop_child(process)
                raise
            self.assertEqual(ready, "ready\n")
        result = driver.run_command([sys.executable, "-I", str(script), "inspect"], input=tool_request({}),
                                capture_output=True, text=True, check=True, timeout=10, env=environment)
        self.assertEqual(json.loads(result.stdout)["content"][0]["data"]["rendered"], "Hello orchard")
        source = repo / "api/handler.py"
        source.write_text(source.read_text(encoding="utf-8").replace("Hello orchard", "Hello apples"), encoding="utf-8")
        result = driver.run_command([sys.executable, "-I", str(script), "inspect"], input=tool_request({}),
                                capture_output=True, text=True, check=True, timeout=10, env=environment)
        self.assertEqual(json.loads(result.stdout)["content"][0]["data"]["rendered"], "Hello apples")
        driver.run_command([sys.executable, "-I", str(script), "stop"], input=tool_request({}), text=True,
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
                result = driver.run_command(command + args, capture_output=True, text=True, timeout=30, env=environment)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                if "--json" in args:
                    json.loads(result.stdout)

            def status_keys():
                result = driver.run_command(command + ["status", "--all", "--json"], capture_output=True,
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
                result = driver.run_command(command + ["tools", "check", "--execute", "--artifact", "ui",
                                                   "--audience", "agent", "--tool", "inspect"],
                                        capture_output=True, text=True, timeout=30, env=environment)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                self.assertIn("Hello orchard", result.stdout)
            # Fake CI text is an input, not evidence that a real builder or CI passed.
            self.assertFalse(list((root / "evidence").iterdir()), "fake results are never live evidence")


class HostLifecycleTests(OwnedTestCase):
    """Exercise ownership through host/CLI responses, without live inference."""

    def setUp(self):
        super().setUp()
        self.candidate = self.root / "candidate-with-another-name"
        self.candidate.write_bytes(b"explicit candidate, not an installed PATH binary")
        self.output = self.root / "working"
        self.evidence = self.root / "saved"
        harness = self.root / "project/harness/hey-arti"
        harness.mkdir(parents=True)
        skill = self.root / "project/crates/artifactize-core/integrations/hey-arti/SKILL.md"
        skill.parent.mkdir(parents=True)
        skill.write_text("Declared fixture skill input.\n")
        patch = mock.patch.object(live, "ROOT", harness)
        patch.start()
        self.addCleanup(patch.stop)
        case = driver.cases()["library-consumer"]
        patch = mock.patch.object(live.driver, "cases", return_value={"library-consumer": case})
        patch.start()
        self.addCleanup(patch.stop)
        self.calls = []

    def response(self, command, code=0, data=None, text="", error=""):
        return subprocess.CompletedProcess(command, code, json.dumps(data) if data is not None else text, error)

    def host_cli(self, failure=None, cancel_code=0, terminal="ERROR", show_code=2, owner=None, saved_status="RUNNING"):
        def call(command, **options):
            self.calls.append(command)
            if command[-1] == "--version":
                return self.response(command, text="declared version\n")
            if command[0] == "claude":
                if "mcp" in command:
                    executable = Path(command[command.index("--") + 1])
                    self.assertTrue(executable.is_absolute())
                    self.assertEqual(executable.name, "artifactize")
                    self.assertEqual(executable.read_bytes(), self.candidate.read_bytes())
                    if failure == "registration":
                        return self.response(command, 1, text="registration evidence", error="rejected")
                    if failure == "registration-timeout":
                        raise subprocess.TimeoutExpired(command, 60, output=b"partial registration", stderr=b"timeout")
                    return self.response(command, text="registered explicit candidate")
                options["stdout"].write("host transcript\n")
                options["stderr"].write("host diagnostics\n")
                repo = options["cwd"]
                (repo / "builder-evidence.json").write_text('{"handoff":"pending Human"}')
                (repo.parent / "state/owned.txt").write_text("saved Run evidence")
                if failure == "host-timeout":
                    raise subprocess.TimeoutExpired(command, 3600)
                if failure == "host-tree":
                    raise driver.ProcessExitError("host descendants did not settle")
                return self.response(command, 1 if failure == "host-exit" else 0)
            self.assertEqual(command[0], str(self.candidate))
            operation = command[command.index("run") + 1]
            if operation == "list":
                if failure == "listing":
                    raise subprocess.CalledProcessError(2, command, stderr="state unavailable")
                return self.response(command, data=[{"id": "run-owned", "status": saved_status}])
            if operation == "cancel":
                if failure == "cancel-timeout":
                    raise subprocess.TimeoutExpired(command, 60)
                return self.response(command, cancel_code, {"run": "run-owned", "state": "cancelling",
                                                           "cancellation_requested": True})
            if operation == "show":
                self.assertIn("--wait", command)
                if failure == "show-timeout":
                    raise subprocess.TimeoutExpired(command, 45)
                view = {"id": "run-owned", "status": terminal}
                if owner is not None:
                    view["control"] = {"worker": {"state": "admitted", "owner": owner}}
                return self.response(command, show_code, view)
            self.fail(f"unexpected CLI operation: {command}")
        return call

    def run_live(self, **options):
        with mock.patch.object(driver, "run_command", side_effect=self.host_cli(**options)):
            return live.run(self.candidate, self.output, "test-model", self.evidence)

    def controlled_worker(self):
        process = self.spawn([sys.executable, "-I", "-c",
                              "import sys; print('worker-ready', flush=True); sys.stdin.read()"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.assertEqual(process.stdout.readline(), "worker-ready\n")
        fields = Path(f"/proc/{process.pid}/stat").read_text().rsplit(")", 1)[1].split()
        return process, {"pid": process.pid, "start_time": int(fields[19])}

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux worker-exit observation uses pidfd")
    def test_live_waits_for_running_and_already_terminal_workers_before_archive_or_delete(self):
        native_select = driver.selectors.DefaultSelector.select
        for status in ("RUNNING", "ERROR"):
            with self.subTest(saved_status=status):
                self.output = self.root / f"working-{status}"
                self.evidence = self.root / f"saved-{status}"
                process, owner = self.controlled_worker()
                observing = threading.Event()
                def select_worker(selector, timeout=None):
                    observing.set()
                    return native_select(selector, timeout)
                with ThreadPoolExecutor() as executor:
                    try:
                        with mock.patch.object(driver.selectors.DefaultSelector, "select", select_worker):
                            completion = executor.submit(self.run_live, owner=owner, saved_status=status)
                            self.assertTrue(observing.wait(30), "cleanup did not observe the owned worker")
                            self.assertFalse(completion.done())
                            self.assertTrue((self.output / "library-consumer/repo").exists())
                            self.assertFalse((self.evidence / "library-consumer").exists())
                            driver.close_child(process)
                            completion.result(timeout=30)
                    finally:
                        driver.close_child(process)
                self.assertFalse(self.output.exists())
                cleanup = json.loads((self.evidence / "library-consumer/evidence/cleanup.json").read_text())
                self.assertEqual(cleanup[0]["worker_exit"], "observed")
                if status == "ERROR":
                    self.assertNotIn("cancel_exit", cleanup[0], "already terminal Runs are observed, not recancelled")

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux worker-exit observation uses pidfd")
    def test_worker_observation_expiry_retains_inputs_and_cleanup_error(self):
        process, owner = self.controlled_worker()
        with mock.patch.object(driver.selectors.DefaultSelector, "select", return_value=[]):
            with self.assertRaises(ExceptionGroup):
                self.run_live(owner=owner)
        self.assertIsNone(process.poll())
        self.assertTrue((self.output / "library-consumer/repo/builder-evidence.json").exists())
        cleanup = json.loads((self.evidence / "library-consumer/evidence/cleanup.json").read_text())
        self.assertIn("did not exit", cleanup[0]["settled_error"])
        self.assertNotIn("worker_exit", cleanup[0])

    def test_live_archives_handoff_and_terminal_outcomes_before_removing_working_files(self):
        self.run_live()
        self.assertFalse(self.output.exists())
        archive = self.evidence / "library-consumer"
        self.assertEqual((archive / "repo/builder-evidence.json").read_text(), '{"handoff":"pending Human"}')
        self.assertEqual((archive / "state/owned.txt").read_text(), "saved Run evidence")
        cleanup = json.loads((archive / "evidence/cleanup.json").read_text())
        self.assertEqual(cleanup[0]["result"]["status"], "ERROR")
        self.assertEqual(cleanup[0]["settled_exit"], 2)
        self.assertFalse((archive / "repo/.mcp.json").exists())
        self.assertEqual(json.loads((self.evidence / "index.json").read_text())["cases"],
                         {"library-consumer": archive.as_posix()})
        operations = [command[command.index("run") + 1] for command in self.calls if "run" in command]
        self.assertEqual(operations, ["list", "cancel", "show"])

    def test_registration_errors_and_timeouts_archive_diagnostics_and_remove_temporary_files(self):
        for failure, expected in (("registration", "registration evidencerejected"),
                                  ("registration-timeout", "partial registrationtimeout")):
            with self.subTest(failure=failure):
                self.output = self.root / failure
                self.evidence = self.root / f"{failure}-saved"
                with self.assertRaises(ExceptionGroup):
                    self.run_live(failure=failure)
                self.assertFalse(self.output.exists())
                self.assertEqual((self.evidence / "library-consumer/evidence/registration.txt").read_text(), expected)
                self.assertTrue((self.evidence / "library-consumer/evidence/failure.txt").is_file())
        self.assertFalse(any("run" in command for command in self.calls))

    def test_host_timeout_still_cancels_joins_and_preserves_partial_evidence(self):
        with self.assertRaises(ExceptionGroup):
            self.run_live(failure="host-timeout")
        self.assertFalse(self.output.exists())
        archive = self.evidence / "library-consumer/evidence"
        self.assertEqual((archive / "transcript.jsonl").read_text(), "host transcript\n")
        self.assertEqual(json.loads((archive / "cleanup.json").read_text())[0]["result"]["status"], "ERROR")

    def test_live_and_smoke_retain_inputs_when_host_group_exit_cannot_be_observed(self):
        with self.assertRaises(ExceptionGroup):
            self.run_live(failure="host-tree")
        self.assertTrue((self.output / "library-consumer/repo/builder-evidence.json").exists())
        self.assertTrue((self.evidence / "library-consumer/evidence/failure.txt").exists())
        smoke_output = self.root / "unsafe-smoke"
        with mock.patch.object(driver, "run_command", side_effect=driver.ProcessExitError("host still owns inputs")):
            with self.assertRaises(driver.ProcessExitError):
                host_smoke.smoke("claude", self.candidate, smoke_output, "test-model")
        self.assertTrue((smoke_output / "repo/index.artf").exists())
        self.assertTrue((smoke_output / "bin/artifactize").exists())

    def test_failed_cancellation_or_nonterminal_observation_never_deletes_run_inputs(self):
        scenarios = ({"cancel_code": 2}, {"failure": "cancel-timeout"},
                     {"terminal": "RUNNING", "show_code": 3}, {"failure": "show-timeout"},
                     {"failure": "listing"})
        for index, options in enumerate(scenarios):
            with self.subTest(options=options):
                self.output = self.root / f"working-{index}"
                self.evidence = self.root / f"saved-{index}"
                with self.assertRaises(ExceptionGroup):
                    self.run_live(**options)
                self.assertTrue((self.output / "library-consumer/repo/builder-evidence.json").is_file())
                self.assertTrue((self.evidence / "library-consumer/evidence/cleanup.json").is_file())
        # Even failed/expired cancellation is followed by the authoritative wait.
        self.assertEqual(sum("cancel" in command for command in self.calls), 4)
        self.assertEqual(sum("show" in command for command in self.calls), 4)

    def test_archive_failure_does_not_delete_working_evidence_after_join(self):
        copytree = shutil.copytree
        def fail_archive(source, destination, *args, **kwargs):
            if Path(destination).is_relative_to(self.evidence):
                raise OSError("archive storage unavailable")
            return copytree(source, destination, *args, **kwargs)
        with mock.patch.object(live.shutil, "copytree", side_effect=fail_archive):
            with self.assertRaises(ExceptionGroup):
                self.run_live()
        self.assertEqual((self.output / "library-consumer/state/owned.txt").read_text(), "saved Run evidence")
        self.assertTrue((self.output / "library-consumer/evidence/cleanup.json").is_file())
        self.assertFalse((self.evidence / "index.json").exists())

    def test_version_failure_removes_cases_that_never_started_a_host(self):
        with mock.patch.object(driver, "run_command", side_effect=OSError("missing host")):
            with self.assertRaises(OSError):
                live.run(self.candidate, self.output, "test-model", self.evidence)
        self.assertFalse(self.output.exists())

    def test_cleanup_reads_all_run_pages_and_joins_other_runs_after_one_cancel_fails(self):
        area = driver.materialize(self.output) / "library-consumer"
        observed = []
        def cli(command, **options):
            operation = command[command.index("run") + 1]
            if operation == "list":
                offset = int(command[command.index("--offset") + 1])
                # CLI seam fixture: 100 saved outcomes, then two active Runs.
                rows = ([{"id": f"run-saved-{i}", "status": "GREEN"} for i in range(100)] if offset == 0 else
                        [{"id": "run-first", "status": "RUNNING"}, {"id": "run-second", "status": "RUNNING"}])
                return self.response(command, data=rows)
            identifier = command[command.index("run") + 2]
            observed.append((operation, identifier))
            if operation == "cancel":
                return self.response(command, 1 if identifier == "run-first" else 0,
                                     {"run": identifier, "state": "cancelling", "cancellation_requested": True})
            return self.response(command, 0 if identifier.startswith("run-saved-") else 2,
                                 {"id": identifier, "status": "GREEN" if identifier.startswith("run-saved-") else "ERROR"})
        with mock.patch.object(driver, "run_command", side_effect=cli):
            with self.assertRaises(ExceptionGroup):
                live.settle_runs(self.candidate, area)
        self.assertEqual([identifier for operation, identifier in observed if operation == "cancel"],
                         ["run-first", "run-second"])
        self.assertEqual({identifier for operation, identifier in observed if operation == "show"},
                         {f"run-saved-{i}" for i in range(100)} | {"run-first", "run-second"})
        self.assertTrue((area / "repo").exists())

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux worker-exit observation uses pidfd")
    def test_real_candidate_cancel_wait_wire_and_worker_exit_precede_fixture_removal(self):
        if CANDIDATE is None:
            self.skipTest("explicit candidate is required for the real Run seam")
        candidate = CANDIDATE.resolve(strict=True)
        # Separate from setUp's disposable root: failed teardown must retain the
        # actual Run's files, rather than an unrelated tempdir cleanup deleting them.
        area = Path(tempfile.mkdtemp(prefix="hey-arti-real-run-")).resolve()
        repo, state = area / "repo", area / "state"
        handles = {}
        # A watchdog bounds broken fixture observation, not execution or an ordering assertion.
        observation_bound = 30

        def capture_worker(view):
            worker = (view.get("control") or {}).get("worker", {})
            if worker.get("state") != "admitted":
                return
            owner = worker["owner"]
            identity = (owner["pid"], owner["start_time"])
            if identity in handles:
                return
            try:
                # Pin an independent OS exit observation while the real Human
                # fixture is still RUNNING; cleanup must make this handle ready.
                handles[identity] = os.pidfd_open(owner["pid"])
            except ProcessLookupError:
                return

        def command(arguments, check=True):
            return driver.run_command([str(candidate), "--repo", str(repo), "--state-dir", str(state)] + arguments,
                                  env=dict(os.environ, ARTIFACTIZE_REMOTE="off"), capture_output=True,
                                  text=True, timeout=observation_bound, check=check)

        def release():
            if not area.exists():
                return
            try:
                live.settle_runs(candidate, area)
                cleanup = json.loads((area / "evidence/cleanup.json").read_text())
                # Only this private repository/state is queried. Even a failed
                # submit can have saved a Run before returning its announcement.
                for saved in json.loads(command(["run", "list", "--json"]).stdout):
                    view = json.loads(command(["run", "show", saved["id"]]).stdout)
                    self.assertIn(view["status"], {"GREEN", "RED", "INCOMPLETE", "SIGNOFFS_PENDING", "ERROR"})
                for descriptor in handles.values():
                    self.assertTrue(select.select([descriptor], [], [], observation_bound)[0],
                                    f"owned worker did not exit; fixture retained at {area.as_posix()}")
            finally:
                for descriptor in handles.values():
                    os.close(descriptor)
                handles.clear()
            # Never delete the private repository/state until BOTH the terminal
            # receipt and actual admitted worker exit have been observed.
            shutil.rmtree(area)
            return cleanup

        self.addCleanup(release)
        repo.mkdir()
        state.mkdir()
        (area / "evidence").mkdir()
        (repo / "index.artf").write_text('''name = "owned-harness-run"
fingerprint = false
[evals.handoff]
title = "Owned fixture waits at its Human boundary"
profile = { kind = "human" }
payload.instruction = "Keep this request pending; the fixture cancels without approving it."
''')
        submitted = json.loads(command(["verify", "--all", "--json"]).stdout)
        identifier = submitted["run"]
        deadline = time.monotonic() + observation_bound
        while True:
            view = json.loads(command(["run", "show", identifier]).stdout)
            capture_worker(view)
            if handles and any(request["status"] == "WAITING_HUMAN" for request in view["requests"]):
                break
            self.assertEqual(view["status"], "RUNNING", view)
            self.assertLess(time.monotonic(), deadline, "owned fixture did not reach its Human boundary")
        cleanup = release()
        self.assertFalse(area.exists())
        self.assertEqual(len(cleanup), 1)
        acknowledgement = json.loads(cleanup[0]["cancel_stdout"])
        self.assertEqual(acknowledgement["run"], identifier)
        self.assertTrue(acknowledgement["cancellation_requested"])
        self.assertEqual(cleanup[0]["cancel_exit"], 0)
        self.assertEqual(cleanup[0]["settled_exit"], 2)
        self.assertEqual(cleanup[0]["result"]["id"], identifier)
        self.assertEqual(cleanup[0]["result"]["status"], "ERROR")
        self.assertNotIn("run", cleanup[0]["result"])
        self.assertEqual(cleanup[0]["worker_exit"], "observed")
        print(f"{identifier}: real cancel/terminal wire and worker exit verified; fixture removed at {area.as_posix()}", flush=True)

    def test_smoke_registers_renamed_candidate_absolutely_and_cleans_private_configuration(self):
        for host in ("claude", "codex"):
            with self.subTest(host=host):
                output = self.root / f"smoke-{host}"
                def registration(command, **options):
                    executable = Path(command[command.index("--") + 1])
                    self.assertTrue(executable.is_absolute())
                    self.assertEqual(executable.name, "artifactize")
                    self.assertEqual(executable.read_bytes(), self.candidate.read_bytes())
                    return self.response(command, 1, text="registration failed evidence")
                with mock.patch.object(driver, "run_command", side_effect=registration):
                    with self.assertRaises(subprocess.CalledProcessError):
                        host_smoke.smoke(host, self.candidate, output, "test-model")
                self.assertEqual(sorted(path.name for path in output.iterdir()), ["registration.txt"])
                self.assertEqual((output / "registration.txt").read_text(), "registration failed evidence")

    # This native inventory seam observes POSIX selectable subprocess pipes and
    # executes shebang fixtures; portable host lifecycle tests above run on every OS.
    @unittest.skipUnless(os.name == "posix", "Native Codex inventory uses POSIX pipe observation and shebang fixtures")
    def test_codex_host_is_joined_and_transcript_preserved_on_success_rpc_error_and_broken_input(self):
        tool_names = ["verify_start", "verify_status", "verify_wait", "verify_result", "verify_cancel",
                      "status", "config_check", "run_list", "run_show", "request_list", "request_show",
                      "session_show", "tools_check", "doctor", "models", "remote_status", "cache_show"]
        # Execute a genuinely renamed candidate through the registered command,
        # with an executable artifactize on PATH that must never be selected.
        self.candidate.write_text(f'#!{sys.executable} -I\nprint("declared-candidate-entrypoint")\n')
        self.candidate.chmod(0o700)
        stale = self.root / "installed-bin"
        stale.mkdir()
        (stale / "artifactize").write_text(f'#!{sys.executable} -I\nprint("wrong-installed-binary")\n')
        (stale / "artifactize").chmod(0o700)
        popen = subprocess.Popen
        for scenario in ("success", "rpc-error", "broken-input"):
            with self.subTest(scenario=scenario):
                output = self.root / f"codex-{scenario}"
                host = self.root / f"declared-host-{scenario}"
                host.write_text(f'''#!{sys.executable} -I
import json
import subprocess
import sys
if "app-server" not in sys.argv:
    if "add" in sys.argv:
        executable = sys.argv[sys.argv.index("--") + 1]
        result = subprocess.run([executable, "--version"], capture_output=True, text=True, check=True, timeout=10)
        print(result.stdout, end="")
    sys.exit(0)
for line in sys.stdin:
    request = json.loads(line)
    if request["method"] == "initialize":
        print(json.dumps({{"id": request["id"], "result": {{}}}}), flush=True)
    elif request["method"] == "mcpServerStatus/list":
        payload = ({{"error": {{"message": "inventory unavailable"}}}} if {scenario != "success"!r} else
                   {{"result": {{"data": [{{"name": "artifactize", "tools": dict.fromkeys({tool_names!r}, {{}})}}]}}}})
        print(json.dumps(dict(id=request["id"], **payload)), flush=True)
''')
                host.chmod(0o700)
                owned = []
                def launch(command, **options):
                    process = popen(command, **options)
                    if "app-server" in command:
                        owned.append(process)
                        self.children.append(process)
                        self.addCleanup(driver.close_child, process)
                        if scenario == "broken-input":
                            stream = process.stdin
                            def close():
                                stream.close()
                                raise BrokenPipeError("host input closed")
                            process.stdin = mock.Mock(wraps=stream)
                            process.stdin.close.side_effect = close
                    return process
                with mock.patch.object(host_smoke.subprocess, "Popen", side_effect=launch), \
                        mock.patch.dict(os.environ, PATH=str(stale) + os.pathsep + os.environ.get("PATH", "")):
                    if scenario == "success":
                        host_smoke.smoke(str(host), self.candidate, output, "test-model")
                        result = json.loads((output / "result.json").read_text())
                        self.assertTrue(result["passed"])
                        self.assertEqual(set(result["tools"]), set(tool_names))
                    else:
                        with self.assertRaises(BrokenPipeError if scenario == "broken-input" else RuntimeError):
                            host_smoke.smoke(str(host), self.candidate, output, "test-model")
                self.assertEqual((output / "registration.txt").read_text(), "declared-candidate-entrypoint\n")
                self.assertEqual(len(owned), 1)
                self.assertIsNotNone(owned[0].poll(), "owned app-server must be joined before fixture deletion")
                messages = [json.loads(line) for line in (output / "transcript.jsonl").read_text().splitlines()]
                self.assertEqual([message["id"] for message in messages], [1, 2])
                self.assertFalse((output / "config").exists())
                self.assertFalse((output / "bin").exists())

    def test_smoke_timeout_retains_partial_host_output_without_temporary_configuration(self):
        def cli(command, **options):
            if "mcp" in command:
                return self.response(command)
            raise subprocess.TimeoutExpired(command, 180, output=b"partial transcript", stderr=b"host timeout")
        with mock.patch.object(driver, "run_command", side_effect=cli):
            with self.assertRaises(subprocess.TimeoutExpired):
                host_smoke.smoke("claude", self.candidate, self.output, "test-model")
        self.assertEqual((self.output / "transcript.jsonl").read_text(), "partial transcript")
        self.assertEqual((self.output / "stderr.txt").read_text(), "host timeout")
        self.assertFalse((self.output / "repo").exists())
        self.assertFalse((self.output / "config").exists())


def run_suite(module):
    suite = unittest.defaultTestLoader.loadTestsFromModule(module)
    def cases(suite):
        for item in suite:
            if isinstance(item, unittest.TestSuite):
                yield from cases(item)
            else:
                yield item
    owned = list(cases(suite))
    previous = signal.getsignal(signal.SIGTERM)
    def interrupted(number, frame):
        raise SystemExit(128 + number)
    signal.signal(signal.SIGTERM, interrupted)
    try:
        try:
            result = unittest.TextTestRunner(verbosity=2).run(suite)
        finally:
            # unittest normally runs these itself; KeyboardInterrupt/SystemExit
            # can skip that path. Public doCleanups is safe again after completion.
            failures = [case for case in owned if not case.doCleanups()]
            if failures:
                raise RuntimeError("Interrupted suite could not release all owned fixtures")
    finally:
        signal.signal(signal.SIGTERM, previous)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifactize", type=Path)
    options = parser.parse_args()
    CANDIDATE = options.artifactize.resolve(strict=True) if options.artifactize else None
    raise SystemExit(run_suite(sys.modules[__name__]))
