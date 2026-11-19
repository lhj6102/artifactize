"""Deterministic harness seams. Fake declarations are tests, not skill-quality evidence."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
import errno
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
from threading import Event
import traceback
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent
# Loading the driver must not leave bytecode among the reviewed harness inputs.
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT))
import driver

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

    def node(name, target, body):
        if target.endswith("/"):
            (repo / target).mkdir(parents=True, exist_ok=True)
        write(f".artifactize/{name}.artf", f"path = {json.dumps(target)}\n{body}")

    def runtime(name, instruction="Check the local fixture behavior."):
        return f'''[evals.check]
title = "{name} behavior"
profile = {{ kind = "runtime", command = {json.dumps(sys.executable)}, args = ["-I", "{{checks}}", "{name}"] }}
payload.instruction = {json.dumps(instruction)}
'''

    node("checks", "check.py", "basis = true\n")
    if case_id == "library-consumer":
        node("pricing", "library/", runtime("pricing"))
        node("receipt", "app/", runtime("receipt", "Check the receipt using {pricing}."))
        node("label", "independent/", runtime("label"))
    elif case_id == "docs-assets":
        node("content", "site/content/", runtime("content"))
        node("theme", "site/theme/", runtime("theme"))
        node("license", "assets/LICENSE.txt", "basis = true\n")
        node("banner", "assets/banner.svg", runtime("banner", "Check {banner} against {license}."))
        node("site", "site/", runtime("site", "Check {content}, {theme} and {banner}.").replace('"site"]', '"all"]'))
    else:
        node("api", "api/", runtime("api", "Check API against {contract}."))
        node("contract", "contract/", runtime("contract"))
        node("preview-driver", "preview/probe.py", "basis = true\n")
        ui = runtime("ui", "Check UI consumes {api} and {contract}.")
        node("ui", "ui/", ui + f'''
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
        node("integration", "integration/", '''[evals.signoff]
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
    for match in re.finditer(r"```toml\n(.*?)```", body, re.DOTALL):
        text = match.group(1)
        declaration = driver.tomllib.loads(text)
        labels = re.findall(r"`\.artifactize/([^`]+)\.artf`", body[:match.start()])
        if "path" in declaration:
            declaration["name"] = Path(labels[-1]).stem if labels else Path(declaration["path"].rstrip("/")).stem
        kinds = {evaluation["profile"]["kind"] for evaluation in declaration.get("evals", {}).values()}
        if declaration.get("basis"):
            role = "basis"
        elif {"runtime", "agent"} <= kinds:
            role = "application"
        elif "human" in kinds:
            role = "human"
        elif declaration.get("views", {}).get("agent_sessions"):
            role = "preview"
        else:
            continue  # Additional valid explanatory fragments do not reorder roles.
        if role in roles:
            raise ValueError(f"ambiguous {role} example")
        roles[role] = (text, declaration)
    return roles


# A watchdog bounds broken fixtures; readiness comes only from the pipe marker.
READY_BOUND = 10


def read_marker(stream, expected):
    deadline = time.monotonic() + READY_BOUND
    descriptor = stream.fileno()
    received = bytearray()
    while len(received) < len(expected):
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([descriptor], [], [], remaining)[0]:
            raise TimeoutError(f"Fixture did not announce {expected!r}")
        fragment = os.read(descriptor, len(expected) - len(received))
        if not fragment:
            break
        received.extend(fragment)
    return bytes(received)


def owned_descriptor(acquire, register):
    """Publish a new descriptor to its fixture before replaying an interrupt."""
    with driver.defer_interrupts():
        descriptor = acquire()
        if descriptor is not None:
            register(os.close, descriptor)
        return descriptor


def pidfd_pid(descriptor):
    """Read the process pinned by a pidfd; Linux reports -1 once it is reaped."""
    info = Path(f"/proc/self/fdinfo/{descriptor}").read_text().splitlines()
    return next(int(line.split()[1]) for line in info if line.startswith("Pid:"))


class OwnedTestCase(unittest.TestCase):
    def setUp(self):
        with driver.defer_interrupts():
            self.root = Path(tempfile.mkdtemp(prefix="hey-arti-test-")).resolve()
            self.children = []
            self.fixture_cleanups = []
            driver.child_cleanup.fixtures.add(self.release_files)
            self.addCleanup(self.release_files)

    def spawn(self, command, **options):
        with driver.defer_interrupts():
            try:
                process = driver.spawn(command, **options)
            except driver.ProcessExitError as error:
                if error.process is not None:
                    self.register_child(error.process)
                raise
            self.register_child(process)
        return process

    def register_child(self, process):
        self.children.append(process)
        driver.child_cleanup.add(process)

    def command(self, *args, on_spawn=None, **options):
        def register(process):
            self.register_child(process)
            if on_spawn is not None:
                on_spawn(process)

        try:
            return driver.run_command(*args, on_spawn=register, **options)
        except driver.ProcessExitError as error:
            if error.process is not None and error.process not in self.children:
                self.register_child(error.process)
            raise

    def doCleanups(self):
        # unittest consumes callbacks as it starts them. Don't replay a handled
        # signal between taking ownership of a callback and running it.
        with driver.defer_interrupts():
            return super().doCleanups()

    def release_files(self):
        with driver.defer_interrupts():
            failures = []
            for process in self.children:
                try:
                    driver.child_cleanup.close(process)
                except BaseException as error:
                    failures.append(error)
            # Finalizers can still stop deferred descendants and release other
            # resources after a join fails. Retry only unfinished finalizers.
            pending = []
            for release in reversed(self.fixture_cleanups):
                try:
                    release()
                except BaseException as error:
                    pending.append(release)
                    failures.append(error)
            self.fixture_cleanups[:] = reversed(pending)
            if failures:
                raise BaseExceptionGroup(
                    f"Fixture cleanup incomplete; files retained at {self.root.as_posix()}", failures)
            if self.root.exists():
                shutil.rmtree(self.root)
            driver.child_cleanup.fixtures.discard(self.release_files)


class HarnessTests(OwnedTestCase):

    def test_canonical_skill_examples_use_valid_current_declarations(self):
        skill_path = ROOT.parents[1] / "agent-plugin/skills/hey-arti/SKILL.md"
        body = skill_path.read_text(encoding="utf-8")
        frontmatter = dict(line.split(":", 1) for line in body.split("---", 2)[1].strip().splitlines())
        self.assertEqual(frontmatter["name"].strip(), "hey-arti")
        self.assertTrue(frontmatter["description"].strip())
        roles = skill_examples(body)
        self.assertTrue({"basis", "application", "human", "preview"} <= roles.keys())
        basis = roles["basis"][1]["name"]
        application = roles["application"][1]["name"]
        human = roles["human"][1]["name"]
        self.assertIn("{" + application + "}", roles["preview"][0])
        if CANDIDATE is None:
            self.skipTest("explicit candidate validates example references")
        repo = self.root / "examples"
        products = {f".artifactize/{basis}.artf": roles["basis"][0],
                    f".artifactize/{application}.artf": roles["application"][0] + roles["preview"][0],
                    f".artifactize/{human}.artf": roles["human"][0],
                    f"{basis}/requirements.md": "Fixture requirements.\n", f"{human}.svg": "<svg/>\n",
                    f"{application}/src/lib.rs": "// Source fixture.\n",
                    f"{application}/tools/preview.py": "# Static example only.\n"}
        for relative, text in products.items():
            path = repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        result = self.command([str(CANDIDATE), "--repo", str(repo), "config", "check"],
                                text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_skill_examples_are_semantic_when_sections_reorder_or_new_fragments_appear(self):
        body = (ROOT.parents[1] / "agent-plugin/skills/hey-arti/SKILL.md").read_text()
        baseline = skill_examples(body)
        snippets = re.findall(r"```toml\n(.*?)```", body, re.DOTALL)
        changed = "\n".join("```toml\n" + snippet + "```" for snippet in reversed(snippets))
        changed += '\n```toml\nfingerprint = false\n```\n'
        self.assertEqual({role: text for role, (text, _) in skill_examples(changed).items()},
                         {role: text for role, (text, _) in baseline.items()})

    def test_check_keeps_suite_in_process_and_cleans_interrupted_fixtures(self):
        suite_root = self.root / "suite"
        suite_root.mkdir()
        report = self.root / "interrupted.json"
        (suite_root / "test_driver.py").write_text(f'''import importlib.util
import json
import os
from pathlib import Path
import signal
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
        assert module.read_marker(child.stdout, b"ready\\n") == b"ready\\n"
        Path({str(report)!r}).write_text(json.dumps({{"parent": os.getpid(), "child": child.pid, "root": self.root.as_posix()}}))
        os.kill(os.getpid(), signal.SIGTERM)

    def test_z_cannot_start_after_termination(self):
        Path({str(suite_root / 'continued')!r}).touch()
try:
    module.run_suite(sys.modules[__name__])
except module.driver.Terminated as error:
    raise SystemExit(error.exit_code)
''')
        original_arguments = list(sys.argv)
        start_child = driver.spawn
        children = []
        def capture_child(command, **options):
            process = start_child(command, **options)
            children.append(process)
            self.register_child(process)
            return process
        with mock.patch.object(driver, "ROOT", suite_root), \
                mock.patch.object(driver, "spawn", capture_child), \
                mock.patch.object(tempfile, "tempdir", str(self.root)):
            self.assertEqual(driver.run_tests(None), 128 + signal.SIGTERM)
        self.assertFalse((suite_root / "continued").exists())
        observed = json.loads(report.read_text())
        self.assertEqual(observed["parent"], os.getpid())
        self.assertFalse(Path(observed["root"]).exists())
        self.assertEqual(sum(child.pid == observed["child"] for child in children), 1)
        for child in children:
            self.assertIsNotNone(child.returncode, f"interrupted suite must join owned child {child.pid}")
        self.assertEqual(sys.argv, original_arguments)

    def test_command_refuses_to_launch_without_pidfd_support(self):
        command = [sys.executable, "-I", "-c", "pass"]
        for module, operation in ((os, "pidfd_open"), (signal, "pidfd_send_signal"),
                                  (driver.process_owner.selectors.DefaultSelector, "register")):
            with self.subTest(operation=operation), \
                    mock.patch.object(module, operation, create=True, side_effect=OSError(errno.ENOSYS, operation)), \
                    mock.patch.object(subprocess.Popen, "__init__", side_effect=AssertionError("launched a child")) as launch:
                with self.assertRaisesRegex(RuntimeError, "requires Linux pidfd"):
                    self.command(command, timeout=READY_BOUND)
                launch.assert_not_called()

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux descendant exit observation uses pidfd")
    def test_command_timeout_joins_owned_descendants(self):
        for detached in (False, True):
            with self.subTest(detached=detached):
                self.check_command_timeout(detached)

    def check_command_timeout(self, detached):
        report = self.root / "descendant.json"
        program = self.root / "declared-tree.py"
        program.write_text(f'''import json
from pathlib import Path
import subprocess
import sys
release = int(sys.argv[1])
child = subprocess.Popen([sys.executable, "-I", "-c", "import os, sys; os.read(int(sys.argv[1]), 1)", str(release)],
                         pass_fds=(release,), start_new_session={detached!r})
Path({str(report)!r}).write_text(json.dumps({{"child": child.pid}}))
print("tree-ready", flush=True)
child.wait()
''')
        spawn = driver.spawn
        communicate = subprocess.Popen.communicate

        def close_fixture_child(process, handles):
            # The pipe has already released well-behaved children. Force-stop
            # and join the owned tree too, in case launch never reached readiness.
            driver.child_cleanup.close(process)
            for descriptor in handles:
                self.assert_exit_ready(descriptor)

        with ExitStack() as fixture, ExitStack() as cleanup, ExitStack() as release:
            with driver.defer_interrupts():
                read_end, write_end = os.pipe()
                fixture.callback(os.close, read_end)
                release.callback(os.close, write_end)
            handles = []
            expired = False

            def launch(command, **options):
                process = spawn(command, pass_fds=(read_end,), **options)
                # The independent release pipe stops descendants even if the
                # driver never signals their group, including on test failure.
                cleanup.callback(close_fixture_child, process, handles)
                self.register_child(process)
                descriptor = owned_descriptor(lambda: os.pidfd_open(process.pid), fixture.callback)
                handles.append(descriptor)
                self.assertEqual(read_marker(process.stdout, b"tree-ready\n"), b"tree-ready\n")
                child = json.loads(report.read_text())["child"]
                descriptor = owned_descriptor(lambda: os.pidfd_open(child), fixture.callback)
                handles.append(descriptor)
                return process

            def expiry(process, *args, **options):
                nonlocal expired
                if not expired:
                    expired = True
                    raise subprocess.TimeoutExpired(process.args, options["timeout"])
                return communicate(process, *args, **options)

            with mock.patch.object(driver, "spawn", side_effect=launch), \
                    mock.patch.object(subprocess.Popen, "communicate", expiry):
                with self.assertRaises(subprocess.TimeoutExpired):
                    self.command([sys.executable, "-I", str(program), str(read_end)],
                                       capture_output=True, text=True, timeout=READY_BOUND)
            self.assertTrue(expired, "the controlled expiry must be reached")
            self.assertEqual(len(handles), 2, "the fixture must capture both process identities")
            self.assertEqual(set(select.select(handles, [], [], 0)[0]), set(handles))
            for descriptor in handles:
                self.assertEqual(pidfd_pid(descriptor), -1, "the exited child was not reaped")

    def test_reaps_a_detached_descendant_that_exited_before_teardown(self):
        program = """import os, sys
if os.fork() == 0:
    os.setsid()
    print(os.getpid(), flush=True)
    os._exit(0)
# Keep the parent alive without reaping its child. Only teardown can
# make the zombie an orphan for the command's reaper to adopt.
sys.stdin.read()
"""
        for defer_detached in (False, True):
            with self.subTest(deferred=defer_detached), ExitStack() as observation:
                detached = driver.Descendants() if defer_detached else None
                if detached is not None:
                    self.fixture_cleanups.append(detached.finish)
                process = self.spawn([sys.executable, "-I", "-c", program], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, detached=detached)
                self.assertTrue(select.select([process.stdout], [], [], READY_BOUND)[0])
                pid = int(process.stdout.readline())

                def reap_if_adopted_by_test(pid=pid):
                    # Also own cleanup if a regression has the test process,
                    # rather than the command's reaper, adopt the exited child.
                    try:
                        os.waitpid(pid, os.WNOHANG)
                    except ChildProcessError:
                        pass

                self.fixture_cleanups.append(reap_if_adopted_by_test)
                descriptor = owned_descriptor(lambda: os.pidfd_open(pid), observation.callback)
                self.assert_exit_ready(descriptor)
                self.assertEqual(pidfd_pid(descriptor), pid, "the fixture parent must hold its unreaped child")
                driver.child_cleanup.close(process)
                if detached is not None:
                    detached.finish()
                self.assertEqual(pidfd_pid(descriptor), -1, "the early-exiting detached child must be reaped")

    def assert_exit_ready(self, descriptor):
        self.assertTrue(select.select([descriptor], [], [], READY_BOUND)[0],
                        "fixture-owned process did not reach exit readiness")

    def test_worker_exit_requires_all_threads_not_just_main_thread(self):
        program = """import ctypes, sys, threading
ready = threading.Event()
def worker():
    ready.set()
    sys.stdin.read()
threading.Thread(target=worker).start()
ready.wait()
ctypes.CDLL(None).pthread_exit(None)
"""
        process = self.spawn([sys.executable, "-I", "-c", program], stdin=subprocess.PIPE)
        stat = Path(f"/proc/{process.pid}/stat")
        deadline = time.monotonic() + READY_BOUND
        while True:
            fields = stat.read_bytes().rsplit(b")", 1)[1].split()
            if fields[0] == b"Z":
                break
            self.assertLess(time.monotonic(), deadline, "the fixture leader did not exit its main thread")
        owner = {"pid": process.pid, "start_time": int(fields[19])}
        try:
            with self.assertRaisesRegex(RuntimeError, "did not exit"):
                driver.wait_for_exit(owner, timeout=0)
        finally:
            process.stdin.close()
        driver.wait_for_exit(owner)

    def test_draining_a_fixture_does_not_reap_an_unrelated_child(self):
        unrelated = [sys.executable, "-I", "-c", "raise SystemExit(23)"]
        with subprocess.Popen(unrelated) as process, ExitStack() as observation:
            descriptor = owned_descriptor(lambda: os.pidfd_open(process.pid), observation.callback)
            self.assert_exit_ready(descriptor)
            self.command([sys.executable, "-I", "-c", "pass"], timeout=READY_BOUND, check=True)
            status = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            self.assertIsNotNone(status)
            self.assertEqual((status.si_code, status.si_status), (os.CLD_EXITED, 23))

    def test_subreaper_drains_descendants_when_leader_stop_fails(self):
        reaper = """import socket, subprocess, sys
sys.dont_write_bytecode = True
sys.path.insert(0, sys.argv[1])
import process_owner

def fail_kill(self):
    raise PermissionError("controlled leader stop failure")

subprocess.Popen.kill = fail_kill
with socket.socket(fileno=int(sys.argv[2])) as channel:
    try:
        process_owner.serve(channel, [sys.executable, "-I", "-c", sys.argv[3]], (), False)
    except PermissionError as error:
        if str(error) != "controlled leader stop failure":
            raise
        raise SystemExit(23)
"""
        tree = """import os, sys
if os.fork() == 0:
    os.setsid()
    print(os.getpid(), flush=True)
sys.stdin.read()
"""
        for operation in (None, "stop", "finish"):
            with self.subTest(operation=operation), ExitStack() as resources:
                with driver.defer_interrupts():
                    channel, endpoint = socket.socketpair(type=socket.SOCK_SEQPACKET)
                    resources.enter_context(channel)
                    resources.enter_context(endpoint)
                    process = self.spawn([sys.executable, "-I", "-c", reaper, str(ROOT),
                                          str(endpoint.fileno()), tree], pass_fds=(endpoint.fileno(),),
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE)
                    endpoint.close()
                channel.settimeout(READY_BOUND)
                leader = driver.process_owner.receive(channel)["pid"]
                self.assertTrue(select.select([process.stdout], [], [], READY_BOUND)[0])
                descendant = int(process.stdout.readline())
                descriptors = [owned_descriptor(lambda: os.pidfd_open(pid), resources.callback)
                               for pid in (leader, descendant)]

                if operation is None:
                    channel.shutdown(socket.SHUT_WR)
                    self.assertEqual(channel.recv(1), b"")
                else:
                    driver.process_owner.send(channel, [operation])
                    self.assertEqual(driver.process_owner.receive(channel), {"error": "controlled leader stop failure"})
                # The outer fixture is still active, so it can't mask a leak in
                # the inner subreaper by adopting and cleaning up its children.
                for descriptor in descriptors:
                    self.assert_exit_ready(descriptor)
                    self.assertEqual(pidfd_pid(descriptor), -1)
                if operation is None:
                    self.assertEqual(process.wait(timeout=READY_BOUND), 23)
                else:
                    driver.process_owner.send(channel, [operation])
                    self.assertEqual(driver.process_owner.receive(channel), {"result": -signal.SIGKILL})
                    self.assertEqual(process.wait(timeout=READY_BOUND), 0)

    def test_child_cleanup_reports_interrupted_join_and_never_resignals_a_reaped_group(self):
        process = self.spawn([sys.executable, "-I", "-c", "import sys; sys.stdin.read()"], stdin=subprocess.PIPE)
        with mock.patch.object(driver.process_owner, "send", side_effect=KeyboardInterrupt):
            with self.assertRaises(driver.ProcessExitError):
                driver.close_child(process)
        self.assertIsNone(process.returncode)
        driver.close_child(process)
        self.assertIsNotNone(process.returncode)
        with mock.patch.object(driver.process_owner, "send", side_effect=AssertionError("group no longer owned")):
            driver.close_child(process)

    def test_managed_child_with_unconfirmed_exit_stays_owned_until_retry(self):
        owner = driver.ChildCleanup()
        close = driver.close_child

        def independent_cleanup(process):
            # Not registration with the retry owner whose behavior is under test.
            self.addCleanup(close, process)

        with mock.patch.object(driver, "child_cleanup", owner):
            with mock.patch.object(driver, "close_child", side_effect=driver.ProcessExitError("join unconfirmed")):
                with self.assertRaises(driver.ProcessExitError):
                    with driver.owned_child([sys.executable, "-I", "-c", "import sys; sys.stdin.read()"],
                                            stdin=subprocess.PIPE, on_spawn=independent_cleanup) as process:
                        self.assertIsNone(process.returncode)
            self.assertIsNone(process.returncode)
            self.assertFalse(process.stdin.closed)
            owner.retry()
            self.assertIsNotNone(process.returncode)
            self.assertTrue(process.stdin.closed)

    def test_shutdown_retry_removes_disposable_root_after_a_failed_join(self):
        cleanup = driver.ChildCleanup()
        fixture = OwnedTestCase()
        with mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(tempfile, "tempdir", str(self.root)):
            fixture.setUp()
            try:
                child = fixture.spawn([sys.executable, "-I", "-c", "import sys; sys.stdin.read()"],
                                      stdin=subprocess.PIPE)
                self.children.append(child)  # Independently release it if the tested retry regresses.
                marker = fixture.root / "owned.txt"
                marker.write_text("fixture input")
                independent = fixture.root / "independent.txt"
                independent.touch()
                released = self.root / "released"

                def release_after_join():
                    if child.returncode is None:
                        raise OSError("controlled finalizer failure")
                    released.touch(exist_ok=False)

                # Run the failing finalizer first. The successful unlink must
                # still run, and it must not run again on shutdown retry.
                fixture.fixture_cleanups.extend((independent.unlink, release_after_join))
                with mock.patch.object(driver.process_owner, "send", side_effect=PermissionError("controlled stop failure")):
                    with self.assertRaises(ExceptionGroup) as raised:
                        fixture.release_files()
                self.assertCountEqual([type(error) for error in raised.exception.exceptions],
                                      [driver.ProcessExitError, OSError])
                self.assertTrue(marker.exists())
                self.assertIsNone(child.returncode)
                self.assertFalse(independent.exists())
            finally:
                cleanup.retry()
            self.assertIsNotNone(child.returncode)
            self.assertTrue(released.exists())
            self.assertFalse(fixture.root.exists())

    def test_three_source_cases_are_private_independent_and_have_real_runtime_behavior(self):
        self.assertEqual(set(driver.cases()), {"library-consumer", "docs-assets", "monorepo"})
        area = driver.materialize(self.root / "cases")
        for case_id, case in driver.cases().items():
            repo = area / case_id / "repo"
            self.assertFalse(list(repo.rglob("*.artf")))
            self.command([sys.executable, "-I", str(repo / "check.py")], check=True, timeout=10)
            independent = (repo / case["independent_path"]).read_bytes()
            edited = repo / case["edit_path"]
            original = edited.read_text(encoding="utf-8")
            self.assertEqual(original.count(case["edit_before"]), 1)
            edited.write_text(original.replace(case["edit_before"], case["edit_after"]), encoding="utf-8")
            self.assertEqual((repo / case["independent_path"]).read_bytes(), independent)
            if case_id != "docs-assets":
                result = self.command([sys.executable, "-I", str(repo / "check.py")],
                                        capture_output=True, timeout=10)
                self.assertNotEqual(result.returncode, 0, "consumer regression must be observable")
            if case_id == "library-consumer":
                observed = self.command([sys.executable, "-I", "-c", """
import runpy, sys
sys.dont_write_bytecode = True
sys.path.insert(0, 'library')
print(runpy.run_path('app/receipt.py')['receipt'](3))
"""], cwd=repo, check=True, capture_output=True, text=True, timeout=READY_BOUND)
                self.assertEqual(observed.stdout.strip(), "Total: 24")
            elif case_id == "monorepo":
                observed = self.command([sys.executable, "-I", "-c", """
import runpy
api = runpy.run_path('api/handler.py')
ui = runpy.run_path('ui/view.py')
print(ui['render'](api['response']()))
"""], cwd=repo, check=True, capture_output=True, text=True, timeout=READY_BOUND)
                self.assertEqual(observed.stdout.strip(), "Hello apples")
            source = driver.inside(ROOT, case["source"]) / case["edit_path"]
            self.assertEqual(source.read_text(encoding="utf-8"), original)
        with self.assertRaises(FileExistsError):
            driver.materialize(area)
        missing_parent = self.root / "missing-parent"
        with self.assertRaises(FileNotFoundError):
            driver.materialize(missing_parent / "cases")
        self.assertFalse(missing_parent.exists())
        project = self.root / "disposable-project"
        harness = project / "harness/hey-arti"
        shutil.copytree(ROOT, harness)
        with mock.patch.object(driver, "ROOT", harness), self.assertRaises(ValueError):
            driver.materialize(project / "outputs-must-not-be-created")

    def test_monorepo_preview_uses_actual_inputs_and_artifactize_session_envelope(self):
        area = driver.materialize(self.root / "cases")
        repo = area / "monorepo/repo"
        session = self.root / "preview-session"
        driver.private_directory(session)
        environment = dict(os.environ, ARTIFACTIZE_SESSION_DIR=str(session))
        script = repo / "preview/probe.py"
        process = self.spawn([sys.executable, "-I", str(script), "start"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment)
        process.stdin.write(tool_request({}))
        process.stdin.close()
        process.stdin = None
        self.assertEqual(read_marker(process.stdout, b"ready\n"), b"ready\n")
        result = self.command([sys.executable, "-I", str(script), "inspect"], input=tool_request({}),
                                capture_output=True, text=True, check=True, timeout=10, env=environment)
        self.assertEqual(json.loads(result.stdout)["content"][0]["data"]["rendered"], "Hello orchard")
        source = repo / "api/handler.py"
        source.write_text(source.read_text(encoding="utf-8").replace("Hello orchard", "Hello apples"), encoding="utf-8")
        result = self.command([sys.executable, "-I", str(script), "inspect"], input=tool_request({}),
                                capture_output=True, text=True, check=True, timeout=10, env=environment)
        self.assertEqual(json.loads(result.stdout)["content"][0]["data"]["rendered"], "Hello apples")
        self.command([sys.executable, "-I", str(script), "stop"], input=tool_request({}), text=True,
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
            if case_id == "monorepo":
                preview = self.track_preview(root, repo)
            fake_builder(repo, case_id)
            command = [str(CANDIDATE), "--repo", str(repo), "--state-dir", str(root / "state")]
            environment = dict(os.environ, ARTIFACTIZE_REMOTE="off")
            for args in (["config", "check"], ["config", "graph", "--json"]):
                result = self.command(command + args, capture_output=True, text=True, timeout=30, env=environment)
                self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
                if "--json" in args:
                    json.loads(result.stdout)

            def status_keys():
                result = self.command(command + ["status", "--all", "--json"], capture_output=True,
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
                        "docs-assets": {"content/check", "site/check"},
                        "monorepo": {"api/check", "ui/check", "ui/review", "integration/signoff"}}
            self.assertEqual(changed, expected[case_id], "real consumers, not independent inputs, must invalidate")
            declaration = {"library-consumer": ".artifactize/label.artf", "docs-assets": ".artifactize/theme.artf",
                           "monorepo": ".artifactize/contract.artf"}[case_id]
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
                # Eval-scoped selection uses the declared operation's published name.
                self.check_preview(command, environment, *preview, graceful=True)
                self.check_preview(command, environment, *preview, graceful=False)
            # Fake CI text is an input, not evidence that a real builder or CI passed.
            self.assertFalse(list((root / "evidence").iterdir()), "fake results are never live evidence")

    def track_preview(self, root, repo):
        # Gate the private copy from launch. A process that gets past the gate
        # must first publish its own identity, even if the candidate restarts it.
        owners = root / "preview-owners"
        owners.mkdir()
        permit = root / "preview-permit"
        permit.touch()
        with driver.defer_interrupts():
            listener = socket.socket()
            self.addCleanup(listener.close)
        listener.settimeout(READY_BOUND)
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        port = listener.getsockname()[1]
        script = repo / "preview/probe.py"
        probe = script.read_text()
        script.write_text(f'''import json, os, socket, sys
from pathlib import Path

def await_shutdown_gate(stage):
    with socket.create_connection(("127.0.0.1", {port}), timeout={READY_BOUND}) as gate:
        gate.sendall(stage + owner.name.encode("ascii") + b"\\n")
        if gate.recv(1) != b"x":
            sys.exit("Preview fixture did not release its gate")

class ReadyOutput:
    def __init__(self, output):
        self.output = output
        self.ready = False

    def write(self, text):
        return self.output.write(text)

    def flush(self):
        port = Path(os.environ["ARTIFACTIZE_SESSION_DIR"]) / "preview-port"
        if not self.ready and port.is_file():
            self.ready = True
            await_shutdown_gate(b"r")
        self.output.flush()

role = sys.argv[1]
if role in ("start", "stop"):
    fields = Path("/proc/self/stat").read_bytes().rsplit(b")", 1)[1].split()
    owner = Path({str(owners)!r}) / f"{{role}}-{{os.getpid()}}-{{fields[19].decode()}}.json"
    staging = owner.with_suffix(".pending")
    staging.write_text(json.dumps({{"pid": os.getpid(), "start_time": int(fields[19])}}))
    staging.replace(owner)
    if not Path({str(permit)!r}).exists():
        sys.exit("Preview fixture already closed")
    # Pin every process, including replacements, before it can start or stop a server.
    await_shutdown_gate(b"a")
if role == "start":
    sys.stdout = ReadyOutput(sys.stdout)
elif role == "stop":
    await_shutdown_gate(b"s")
    if os.environ.get("HEY_ARTI_TEST_SKIP_GRACEFUL_STOP"):
        # Report success without sending the real stop message; only the
        # candidate's process teardown can end this preview.
        sys.exit(0)
''' + probe + '''
# Stop has notified the server, but the two sides of shutdown are still
# independently gated. The controller observes real server exit before
# permitting the stop command to complete.
if operation == "start":
    await_shutdown_gate(b"p")
elif operation == "stop":
    await_shutdown_gate(b"d")
''')
        detached = driver.Descendants()

        def cleanup():
            try:
                # A late attempt can no longer establish a listener after revocation.
                permit.unlink(missing_ok=True)
                for owner in owners.glob("*.json"):
                    with ExitStack() as observation:
                        identity = json.loads(owner.read_text())
                        descriptor = owned_descriptor(lambda: driver.process_descriptor(identity), observation.callback)
                        if descriptor is not None:
                            try:
                                signal.pidfd_send_signal(descriptor, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                            self.assert_exit_ready(descriptor)
            finally:
                detached.finish()

        self.fixture_cleanups.append(cleanup)
        return listener, owners, detached

    def check_preview(self, command, environment, listener, owners, detached, *, graceful):
        environment = dict(environment)
        if graceful:
            environment.pop("HEY_ARTI_TEST_SKIP_GRACEFUL_STOP", None)
        else:
            environment["HEY_ARTI_TEST_SKIP_GRACEFUL_STOP"] = "1"
        previous = {receipt.name for receipt in owners.glob("*.json")}
        with ExitStack() as observation:
            # Files let the candidate finish without a pipe reader that might
            # also reap it and run the harness's fallback group cleanup.
            stdout = observation.enter_context(tempfile.TemporaryFile(dir=self.root, mode="w+t"))
            stderr = observation.enter_context(tempfile.TemporaryFile(dir=self.root, mode="w+t"))
            process = self.spawn(command + ["tools", "check", "--execute", "--eval", "ui/review", "--tool", "inspect_ui"],
                                 stdout=stdout, stderr=stderr, env=environment, detached=detached)
            cli = owned_descriptor(lambda: os.pidfd_open(process.pid), observation.callback)
            exits = observation.enter_context(select.epoll())
            exits.register(cli, select.EPOLLIN | select.EPOLLONESHOT)
            exits.register(listener.fileno(), select.EPOLLIN)
            admitted = {}
            pending = set()
            ready = set()
            gates = {}
            unblocked = {b"r"}
            complete = False

            def allow(gate):
                with gate:
                    gate.sendall(b"x")

            def accept_gate():
                gate = observation.enter_context(listener.accept()[0])
                gate.settimeout(READY_BOUND)
                with gate.makefile("rb") as stream:
                    message = stream.readline()
                stage, receipt = message[:1], message[1:-1].decode("ascii")
                if stage == b"a":
                    self.assertNotIn(receipt, previous | admitted.keys())
                    owner = json.loads((owners / receipt).read_text())
                    descriptor = owned_descriptor(lambda: driver.process_descriptor(owner), observation.callback)
                    self.assertIsNotNone(descriptor, f"lost the gated process {receipt} before observing it")
                    exits.register(descriptor, select.EPOLLIN | select.EPOLLONESHOT)
                    admitted[receipt] = descriptor
                    pending.add(descriptor)
                    allow(gate)
                else:
                    self.assertIn(stage, {b"r", b"s", b"p", b"d"})
                    descriptor = admitted[receipt]
                    if stage == b"r":
                        ready.add(descriptor)
                    if stage in unblocked:
                        allow(gate)
                    else:
                        gates.setdefault(stage, []).append((descriptor, gate))

            def release(stage):
                unblocked.add(stage)
                for _, gate in gates.pop(stage, []):
                    allow(gate)

            def observe_until(done):
                nonlocal complete
                deadline = time.monotonic() + READY_BOUND
                while not done():
                    self.assertFalse(complete, "CLI exited before the expected fixture state")
                    remaining = deadline - time.monotonic()
                    self.assertGreater(remaining, 0, "candidate and fixture events did not arrive")
                    # Consume in kernel readiness order. Every fixture pidfd is
                    # registered before its admission gate is released.
                    for descriptor, _ in exits.poll(remaining, 1):
                        if descriptor == listener.fileno():
                            accept_gate()
                        elif descriptor == cli:
                            self.assertFalse(pending, f"preview or stop outlived the CLI (graceful={graceful})")
                            complete = True
                        else:
                            pending.remove(descriptor)

            observe_until(lambda: bool(ready & pending) and bool(gates.get(b"s")))
            self.assertFalse(select.select([cli], [], [], 0)[0],
                             "CLI exited before the stop command was released")
            release(b"s")

            if graceful:
                observe_until(lambda: bool(gates.get(b"p")) and bool(gates.get(b"d")))
                previews = {descriptor for descriptor, _ in gates[b"p"]}
                stops = {descriptor for descriptor, _ in gates[b"d"]}
                self.assertFalse(select.select([cli, *previews, *stops], [], [], 0)[0],
                                 "CLI, preview and stop must still be alive at the shutdown gates")
                release(b"p")
                observe_until(lambda: previews.isdisjoint(pending))
                self.assertFalse(select.select([cli], [], [], 0)[0],
                                 "the CLI must still be alive after the preview has exited")
                release(b"d")

            observe_until(lambda: complete)
            # A receipt without a pidfd admission cannot prove exit order, even
            # if the process is already gone when checked after the CLI exits.
            self.assertEqual(admitted.keys(), {receipt.name for receipt in owners.glob("*.json")} - previous)

            result = process.wait(timeout=READY_BOUND)
            stdout.seek(0)
            stderr.seek(0)
            output = stdout.read()
            self.assertEqual(result, 0, stderr.read() + output)
            self.assertEqual(json.loads(output)["result"]["content"][0]["data"]["rendered"], "Hello orchard")


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
        skill = self.root / "project/agent-plugin/skills/hey-arti/SKILL.md"
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
        launcher, self.launcher = self.controlled_worker()
        driver.close_child(launcher)

    def response(self, command, code=0, data=None, text="", error=""):
        return subprocess.CompletedProcess(command, code, json.dumps(data) if data is not None else text, error)

    def host_cli(self, failure=None, cancel_code=0, terminal="ERROR", show_code=2,
                 owner=None, saved_status="RUNNING", control=None):
        if control is None:
            worker = ({"state": "admitted", "owner": owner} if owner else
                      {"state": "starting", "launcher": self.launcher})
            control = {"worker": worker}

        def call(command, **options):
            self.calls.append(command)
            if command[-1] == "--version":
                return self.response(command, text="declared version\n")
            if command[0] == "claude":
                if "mcp" in command:
                    executable = Path(command[command.index("--") + 1])
                    self.assertTrue(executable.is_absolute())
                    self.assertEqual(executable.read_bytes(), self.candidate.read_bytes())
                    if failure == "registration":
                        return self.response(command, 1, text="registration evidence", error="rejected")
                    if failure == "registration-timeout":
                        raise subprocess.TimeoutExpired(command, 60, output=b"partial registration", stderr=b"timeout")
                    (options["cwd"] / ".mcp.json").write_text('{"servers": {"fixture": {}}}')
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
                if failure == "show-timeout" and "--wait" in command:
                    raise subprocess.TimeoutExpired(command, 45)
                return self.response(command, show_code, {"id": "run-owned", "status": terminal, "control": control})
            self.fail(f"unexpected CLI operation: {command}")
        return call

    def run_live(self, **options):
        with mock.patch.object(driver, "run_command", side_effect=self.host_cli(**options)):
            return live.run(self.candidate, self.output, "test-model", self.evidence)

    def controlled_worker(self):
        process = self.spawn([sys.executable, "-I", "-c",
                              "import sys; print('worker-ready', flush=True); sys.stdin.read()"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.assertEqual(read_marker(process.stdout, b"worker-ready\n"), b"worker-ready\n")
        fields = Path(f"/proc/{process.pid}/stat").read_bytes().rsplit(b")", 1)[1].split()
        return process, {"pid": process.pid, "start_time": int(fields[19])}

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux worker-exit observation uses pidfd")
    def test_live_waits_for_running_and_already_terminal_workers_before_archive_or_delete(self):
        copy_tree, remove_tree = shutil.copytree, shutil.rmtree
        exit_ready = driver.exit_ready
        for status, pending in (("RUNNING", "worker"), ("ERROR", "worker"), ("ERROR", "supervisor")):
            with self.subTest(saved_status=status, pending=pending):
                self.output = self.root / f"working-{status}-{pending}"
                self.evidence = self.root / f"saved-{status}-{pending}"
                process, owner = self.controlled_worker()
                descriptor = owned_descriptor(lambda: os.pidfd_open(process.pid), self.addCleanup)
                control = {"worker": {"state": "admitted", "owner": owner}}
                if pending == "supervisor":
                    control = {"worker": {"state": "admitted", "owner": self.launcher}, "supervisor": owner}
                cli = self.host_cli(control=control, saved_status=status)

                def respond(command, **options):
                    result = cli(command, **options)
                    if command[0] == str(self.candidate) and "show" in command and "--wait" in command:
                        self.assertFalse(select.select([descriptor], [], [], 0)[0],
                                         "the process must still be alive when the CLI reports terminal")
                    return result

                observing = Event()

                def observe_exit(waited, timeout):
                    if pidfd_pid(waited) == process.pid:
                        observing.set()
                    return exit_ready(waited, timeout)

                def release_worker():
                    self.assertTrue(observing.wait(READY_BOUND), "the harness never observed the worker")
                    self.assertFalse(select.select([descriptor], [], [], 0)[0], "worker exited before release")
                    process.stdin.close()

                def require_worker_exit(action):
                    self.assertTrue(select.select([descriptor], [], [], 0)[0],
                                    f"{action} must follow process exit")

                def copy(source, destination, *args, **kwargs):
                    if Path(destination).is_relative_to(self.evidence):
                        require_worker_exit("archive")
                    return copy_tree(source, destination, *args, **kwargs)

                def remove(path, *args, **kwargs):
                    if Path(path).is_relative_to(self.output):
                        require_worker_exit("deletion")
                    return remove_tree(path, *args, **kwargs)

                # A separate fixture action releases the waiting worker. The
                # harness observer only reports the wait and reads its pidfd.
                with ThreadPoolExecutor(max_workers=1) as actions:
                    release = actions.submit(release_worker)
                    with mock.patch.object(driver, "run_command", side_effect=respond), \
                            mock.patch.object(driver, "exit_ready", side_effect=observe_exit), \
                            mock.patch.object(shutil, "copytree", side_effect=copy), \
                            mock.patch.object(shutil, "rmtree", side_effect=remove):
                        live.run(self.candidate, self.output, "test-model", self.evidence)
                    release.result()
                self.assertFalse(self.output.exists())
                cleanup = json.loads((self.evidence / "library-consumer/evidence/cleanup.json").read_text())
                self.assertEqual(cleanup[0]["worker_exit"], "observed")
                if pending == "supervisor":
                    self.assertEqual(cleanup[0]["supervisor_exit"], "observed")
                if status == "ERROR":
                    self.assertNotIn("cancel_exit", cleanup[0], "already terminal Runs are observed, not recancelled")

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux worker-exit observation uses pidfd")
    def test_worker_observation_expiry_retains_inputs_and_cleanup_error(self):
        process, owner = self.controlled_worker()
        descriptor = owned_descriptor(lambda: os.pidfd_open(process.pid), self.addCleanup)
        wait_for_exit = driver.wait_for_exit
        with mock.patch.object(driver, "wait_for_exit", side_effect=lambda identity: wait_for_exit(identity, timeout=0)):
            with self.assertRaises(ExceptionGroup):
                self.run_live(owner=owner)
        self.assertFalse(select.select([descriptor], [], [], 0)[0], "fixture worker must still be running")
        cleanup = json.loads((self.assert_unconfirmed() / "cleanup.json").read_text())
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
        self.assertEqual(cleanup[0]["worker_exit"], "not_admitted")
        self.assert_cancel_precedes_wait(self.calls)

    def assert_cancel_precedes_wait(self, calls):
        owned = [command[command.index("run") + 1:] for command in calls if "run" in command]
        waited = next((index for index, args in enumerate(owned)
                       if args[:2] == ["show", "run-owned"] and "--wait" in args), None)
        self.assertIsNotNone(waited, "the owned Run must be observed to terminal state")
        self.assertTrue(any(args[:2] == ["cancel", "run-owned"] for args in owned[:waited]),
                        "cancellation must precede terminal observation for the owned Run")

    def assert_unconfirmed(self):
        retained = self.output / "library-consumer"
        self.assertTrue((retained / "repo/builder-evidence.json").is_file())
        self.assertTrue((retained / "state/owned.txt").is_file())
        self.assertTrue((retained / "host").is_dir())
        self.assertTrue((retained / "bin/artifactize").is_file())
        self.assertTrue((retained / "repo/.mcp.json").is_file())
        archive = self.evidence / "library-consumer"
        self.assertFalse((archive / "repo").exists())
        self.assertFalse((archive / "state").exists())
        self.assertFalse((self.evidence / "index.json").exists())
        diagnostics = archive / "evidence"
        self.assertIn(str(retained), (diagnostics / "settlement-unconfirmed.txt").read_text())
        return diagnostics

    def test_unrecognized_worker_state_retains_inputs_and_only_publishes_diagnostics(self):
        for worker in ({"state": "legacy"}, {"state": "unexpected"}, {}):
            with self.subTest(worker=worker):
                self.output = self.root / f"working-{worker.get('state')}"
                self.evidence = self.root / f"saved-{worker.get('state')}"
                with self.assertRaises(ExceptionGroup):
                    self.run_live(control={"worker": worker})
                cleanup = json.loads((self.assert_unconfirmed() / "cleanup.json").read_text())
                self.assertIn("Unconfirmed Run worker state", cleanup[0]["settled_error"])
                self.assertNotIn("worker_exit", cleanup[0])

    def test_registration_errors_and_timeouts_archive_diagnostics_and_remove_temporary_files(self):
        for failure, expected in (("registration", ("registration evidence", "rejected")),
                                  ("registration-timeout", ("partial registration", "timeout"))):
            with self.subTest(failure=failure):
                self.output = self.root / failure
                self.evidence = self.root / f"{failure}-saved"
                with self.assertRaises(ExceptionGroup):
                    self.run_live(failure=failure)
                self.assertFalse(self.output.exists())
                diagnostic = (self.evidence / "library-consumer/evidence/registration.txt").read_text()
                for stream in expected:
                    self.assertIn(stream, diagnostic)
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
        self.assertIn("host descendants did not settle", (self.assert_unconfirmed() / "failure.txt").read_text())
        self.assertFalse(any("run" in command for command in self.calls),
                         "listing may miss new Runs while the builder is still unconfirmed")
        smoke_output = self.root / "unsafe-smoke"
        with mock.patch.object(driver, "run_command", side_effect=driver.ProcessExitError("host still owns inputs")):
            with self.assertRaises(driver.ProcessExitError):
                host_smoke.smoke("claude", self.candidate, smoke_output, "test-model")
        self.assertTrue((smoke_output / "repo/.artifactize/smoke.artf").exists())
        self.assertTrue((smoke_output / "bin/artifactize").exists())

    def test_shutdown_retry_releases_smoke_inputs_after_teardown_interrupt(self):
        cleanup = driver.ChildCleanup()
        defer = driver.defer_interrupts
        ready = False

        def inventory(host, candidate, output, model):
            nonlocal ready
            for name in ("repo", "config", "bin", "state"):
                (output / name).mkdir()
            (output / "result.json").write_text("recorded output\n")
            ready = True

        def interrupt_before_release():
            nonlocal ready
            if ready:
                ready = False
                os.kill(os.getpid(), signal.SIGTERM)
            return defer()

        with driver.term_as_interrupt(), mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(driver, "defer_interrupts", side_effect=interrupt_before_release), \
                mock.patch.object(host_smoke, "inventory", side_effect=inventory):
            with self.assertRaises(driver.Terminated):
                host_smoke.smoke("claude", self.candidate, self.output, "test-model")
            cleanup.retry()
        self.assertEqual({path.name for path in self.output.iterdir()}, {"result.json"})

    def test_shutdown_retry_joins_host_before_releasing_smoke_inputs(self):
        process, _ = self.controlled_worker()
        cleanup = driver.ChildCleanup()

        def inventory(host, candidate, output, model):
            for name in ("repo", "config", "bin", "state"):
                (output / name).mkdir()
            (output / "transcript.jsonl").write_text("recorded output\n")
            raise driver.ProcessExitError("join not confirmed", process=process)

        with mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(host_smoke, "inventory", side_effect=inventory):
            with self.assertRaises(driver.ProcessExitError):
                host_smoke.smoke("claude", self.candidate, self.output, "test-model")
            self.assertIsNone(process.returncode)
            self.assertTrue((self.output / "bin").is_dir())
            cleanup.retry()
        self.assertIsNotNone(process.returncode)
        self.assertEqual({path.name for path in self.output.iterdir()}, {"transcript.jsonl"})
        self.assertEqual((self.output / "transcript.jsonl").read_text(), "recorded output\n")

    def test_shutdown_retry_joins_host_then_settles_and_archives_owned_runs(self):
        process, _ = self.controlled_worker()
        descriptor = owned_descriptor(lambda: os.pidfd_open(process.pid), self.addCleanup)
        cleanup = driver.ChildCleanup()
        cli = self.host_cli()

        def host_exit_unconfirmed(command, **options):
            response = cli(command, **options)
            if command[0] == "claude" and "-p" in command:
                raise driver.ProcessExitError("join not confirmed", process=process)
            if command[0] == str(self.candidate) and "run" in command:
                self.assertTrue(select.select([descriptor], [], [], 0)[0], "Run cleanup must follow host exit")
            return response

        with mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(driver, "run_command", side_effect=host_exit_unconfirmed):
            with self.assertRaises(ExceptionGroup):
                live.run(self.candidate, self.output, "test-model", self.evidence)
            self.assert_unconfirmed()
            self.assertIsNone(process.returncode)
            cleanup.retry()
        self.assertIsNotNone(process.returncode)
        self.assertFalse(self.output.exists())
        self.assert_cancel_precedes_wait(self.calls)
        saved = self.evidence / "library-consumer/evidence"
        self.assertFalse((saved / "settlement-unconfirmed.txt").exists())
        self.assertEqual(json.loads((saved / "cleanup.json").read_text())[0]["worker_exit"], "not_admitted")

    def test_final_retry_stops_detached_workers_even_when_run_settlement_stays_unconfirmed(self):
        cleanup = driver.ChildCleanup()
        cli = self.host_cli(failure="listing")
        workers = []

        def launch_detached_worker(command, **options):
            if command[0] == "claude" and "-p" in command:
                with driver.defer_interrupts():
                    detached = options["detached"]
                    self.fixture_cleanups.append(detached.finish)
                    child = self.spawn([sys.executable, "-I", "-c",
                                        "import sys; print('worker-ready', flush=True); sys.stdin.read()"],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, detached=detached)
                    # Only the case's retry should stop this tree during the test.
                    cleanup.pending.remove(child)
                    descriptor = owned_descriptor(lambda: os.pidfd_open(child.pid), self.addCleanup)
                    workers.append(descriptor)
                self.assertEqual(read_marker(child.stdout, b"worker-ready\n"), b"worker-ready\n")
            return cli(command, **options)

        with mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(driver, "run_command", side_effect=launch_detached_worker):
            with self.assertRaises(ExceptionGroup):
                live.run(self.candidate, self.output, "test-model", self.evidence)
            self.assertEqual(len(workers), 1)
            descriptor = workers[0]
            self.assertFalse(select.select([descriptor], [], [], 0)[0])
            cleanup.retry()
        self.assertTrue(select.select([descriptor], [], [], 0)[0], "the final retry must join the detached worker")
        self.assertIn("listing_error", json.loads((self.assert_unconfirmed() / "cleanup.json").read_text())[0])

    def test_final_retry_stops_detached_workers_even_when_host_join_fails(self):
        cleanup = driver.ChildCleanup()
        host, _ = self.controlled_worker()
        worker = None
        cli = self.host_cli()
        close_child = driver.close_child

        def call(command, **options):
            nonlocal worker
            response = cli(command, **options)
            if command[0] == "claude" and "-p" in command:
                with driver.defer_interrupts():
                    detached = options["detached"]
                    self.fixture_cleanups.append(detached.finish)
                    worker = self.spawn([sys.executable, "-I", "-c", "import sys; sys.stdin.read()"],
                                        stdin=subprocess.PIPE, detached=detached)
                    # Only the case's retry should stop this tree during the test.
                    cleanup.pending.remove(worker)
                raise driver.ProcessExitError("host exit unconfirmed", process=host)
            return response

        def close(process):
            if process is host:
                raise driver.ProcessExitError("host exit still unconfirmed", process=host)
            close_child(process)

        with mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(driver, "run_command", side_effect=call), \
                mock.patch.object(driver, "close_child", side_effect=close):
            with self.assertRaises(ExceptionGroup):
                live.run(self.candidate, self.output, "test-model", self.evidence)
            self.assertIsNotNone(worker)
            descriptor = owned_descriptor(lambda: os.pidfd_open(worker.pid), self.addCleanup)
            self.assertFalse(select.select([descriptor], [], [], 0)[0])
            cleanup.retry()
            self.assertTrue(select.select([descriptor], [], [], 0)[0], "the detached worker must be joined")
            self.assertIsNone(host.returncode)
            self.assertFalse(any("run" in command for command in self.calls))
            self.assert_unconfirmed()

    def test_retry_requires_previous_run_cleanup_command_exit_before_archival(self):
        cleanup = driver.ChildCleanup()
        command_process, _ = self.controlled_worker()
        descriptor = owned_descriptor(lambda: os.pidfd_open(command_process.pid), self.addCleanup)
        cli = self.host_cli()
        close_child = driver.close_child
        blocked = True
        failed_command = False

        def call(command, **options):
            nonlocal failed_command
            response = cli(command, **options)
            if "cancel" in command and not failed_command:
                failed_command = True
                options["on_spawn"](command_process)
                cleanup.add(command_process)
                # The spawn receipt must be sufficient even if the failure
                # does not repeat the command handle.
                raise driver.ProcessExitError("cancel exit unconfirmed")
            return response

        def close(process):
            if process is command_process and blocked:
                raise driver.ProcessExitError("cancel exit still unconfirmed", process=command_process)
            close_child(process)

        with mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(driver, "run_command", side_effect=call), \
                mock.patch.object(driver, "close_child", side_effect=close):
            with self.assertRaises(ExceptionGroup):
                live.run(self.candidate, self.output, "test-model", self.evidence)
            self.assert_unconfirmed()
            # The simulated CLI will now report successful settlement, but the
            # previous cancellation command is still using the case.
            cleanup.retry()
            self.assert_unconfirmed()
            self.assertFalse(select.select([descriptor], [], [], 0)[0])
            blocked = False
            cleanup.retry()
            self.assertTrue(select.select([descriptor], [], [], 0)[0])
            self.assertFalse(self.output.exists())
            self.assertTrue((self.evidence / "index.json").is_file())

    def test_failed_cancellation_or_nonterminal_observation_never_deletes_run_inputs(self):
        scenarios = ({"cancel_code": 2}, {"failure": "cancel-timeout"},
                     {"terminal": "RUNNING", "show_code": 3}, {"failure": "show-timeout"},
                     {"failure": "listing"})
        for index, options in enumerate(scenarios):
            with self.subTest(options=options):
                self.output = self.root / f"working-{index}"
                self.evidence = self.root / f"saved-{index}"
                self.calls.clear()
                with self.assertRaises(ExceptionGroup):
                    self.run_live(**options)
                self.assertTrue((self.assert_unconfirmed() / "cleanup.json").is_file())
                if options.get("failure") != "listing":
                    self.assert_cancel_precedes_wait(self.calls)

    def test_shutdown_retry_removes_working_files_without_replacing_published_archive(self):
        cleanup = driver.ChildCleanup()
        remove = shutil.rmtree
        working = self.output / "library-consumer"

        def fail_removal(path, *args, **kwargs):
            if Path(path) == working:
                raise PermissionError("working files are temporarily unavailable")
            return remove(path, *args, **kwargs)

        with mock.patch.object(driver, "child_cleanup", cleanup):
            with mock.patch.object(shutil, "rmtree", side_effect=fail_removal):
                with self.assertRaises(ExceptionGroup) as failure:
                    self.run_live()
            self.assertIsNotNone(failure.exception.subgroup(PermissionError))
            self.assertTrue((working / "bin/artifactize").is_file())
            evidence = self.evidence / "library-consumer/evidence"
            self.assertTrue((evidence / "live.json").is_file())
            marker = evidence / "archive-observed"
            marker.touch()
            cleanup.retry()
            self.assertFalse(self.output.exists())
            self.assertTrue(marker.is_file(), "deletion retry must not replace the published archive")

    def test_archive_failure_releases_host_inputs_and_keeps_recovery_data_after_join(self):
        copytree = shutil.copytree
        def fail_archive(source, destination, *args, **kwargs):
            if Path(destination).is_relative_to(self.evidence):
                raise OSError("archive storage unavailable")
            return copytree(source, destination, *args, **kwargs)
        with mock.patch.object(live.shutil, "copytree", side_effect=fail_archive):
            with self.assertRaises(ExceptionGroup):
                self.run_live()
        retained = self.output / "library-consumer"
        self.assertEqual((retained / "repo/builder-evidence.json").read_text(), '{"handoff":"pending Human"}')
        self.assertEqual((retained / "state/owned.txt").read_text(), "saved Run evidence")
        self.assertEqual((retained / "evidence/transcript.jsonl").read_text(), "host transcript\n")
        cleanup = json.loads((retained / "evidence/cleanup.json").read_text())
        self.assertEqual(cleanup[0]["worker_exit"], "not_admitted")
        for disposable in ("host", "bin", "repo/.mcp.json"):
            self.assertFalse((retained / disposable).exists(), disposable)
        self.assertFalse((self.evidence / "index.json").exists())

    def test_shutdown_retry_owns_claimed_and_unclaimed_cases_at_teardown(self):
        cleanup = driver.ChildCleanup()
        defer = driver.defer_interrupts
        cli = self.host_cli()
        remaining_interrupts = 0

        def respond(command, **options):
            nonlocal remaining_interrupts
            result = cli(command, **options)
            if command[0] == "claude" and "-p" in command:
                remaining_interrupts = 2  # Interrupt both case and unclaimed cleanup entry.
            return result

        def interrupt_before_release():
            nonlocal remaining_interrupts
            if remaining_interrupts:
                remaining_interrupts -= 1
                os.kill(os.getpid(), signal.SIGTERM)
            return defer()

        with driver.term_as_interrupt(), mock.patch.object(driver, "child_cleanup", cleanup), \
                mock.patch.object(driver, "defer_interrupts", side_effect=interrupt_before_release), \
                mock.patch.object(driver, "run_command", side_effect=respond):
            with self.assertRaises(driver.Terminated):
                live.run(self.candidate, self.output, "test-model", self.evidence)
            cleanup.retry()
        self.assertFalse(self.output.exists())
        index = json.loads((self.evidence / "index.json").read_text())
        self.assertEqual(set(index["cases"]), {"library-consumer"})
        self.assert_cancel_precedes_wait(self.calls)

    def test_termination_during_materialization_releases_unclaimed_cases(self):
        materialize = driver.materialize

        def interrupt_after_copy(output):
            work = materialize(output)
            os.kill(os.getpid(), signal.SIGTERM)
            return work

        with driver.term_as_interrupt(), mock.patch.object(driver, "materialize", side_effect=interrupt_after_copy):
            with self.assertRaises(driver.Terminated):
                self.run_live()
        self.assertFalse(self.output.exists())
        self.assertFalse(self.calls, "no host should start after interrupted materialization")

    def test_version_failure_removes_cases_that_never_started_a_host(self):
        with mock.patch.object(driver, "run_command", side_effect=OSError("missing host")):
            with self.assertRaises(OSError):
                live.run(self.candidate, self.output, "test-model", self.evidence)
        self.assertFalse(self.output.exists())

    def test_cleanup_reads_all_run_pages_and_joins_other_runs_after_one_cancel_fails(self):
        area = driver.materialize(self.output) / "library-consumer"
        observed = []
        terminal = [{"id": f"run-saved-{index}", "status": "GREEN"} for index in range(257)]
        active = [{"id": identifier, "status": "RUNNING"} for identifier in ("run-first", "run-second")]
        rows = [*terminal, *active]

        def cli(command, **options):
            operation = command[command.index("run") + 1]
            if operation == "list":
                limit = int(command[command.index("--limit") + 1])
                offset = int(command[command.index("--offset") + 1])
                return self.response(command, data=rows[offset:offset + limit])
            identifier = command[command.index("run") + 2]
            observed.append((operation, identifier))
            if operation == "cancel":
                return self.response(command, 1 if identifier == "run-first" else 0,
                                     {"run": identifier, "state": "cancelling", "cancellation_requested": True})
            return self.response(command, 0 if identifier.startswith("run-saved-") else 2,
                                 {"id": identifier, "status": "GREEN" if identifier.startswith("run-saved-") else "ERROR",
                                  "control": {"worker": {"state": "starting", "launcher": self.launcher}}})
        with mock.patch.object(driver, "run_command", side_effect=cli):
            with self.assertRaises(ExceptionGroup):
                live.settle_runs(self.candidate, area)
        self.assertEqual({identifier for operation, identifier in observed if operation == "cancel"},
                         {"run-first", "run-second"})
        self.assertEqual({identifier for operation, identifier in observed if operation == "show"},
                         {row["id"] for row in rows})
        self.assertTrue((area / "repo").exists())
        cleanup = {entry["run"]: entry for entry in json.loads((area / "evidence/cleanup.json").read_text())}
        self.assertIn("cancel_error", cleanup["run-first"])
        self.assertTrue(all(entry["worker_exit"] == "not_admitted" for entry in cleanup.values()))

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux worker-exit observation uses pidfd")
    def test_real_candidate_cancel_wait_wire_and_worker_exit_precede_fixture_removal(self):
        if CANDIDATE is None:
            self.skipTest("explicit candidate is required for the real Run seam")
        candidate = CANDIDATE.resolve(strict=True)
        # Separate from setUp's disposable root: failed teardown must retain the
        # actual Run's files, rather than an unrelated tempdir cleanup deleting them.
        handles = {}
        run_commands = set()
        cleanup_commands = live.RunCommands()
        detached = driver.Descendants()
        submission_attempted = False
        identifier = None
        # A watchdog bounds broken fixture observation, not execution or an ordering assertion.
        observation_bound = 30

        def capture_worker(view):
            worker = (view.get("control") or {}).get("worker", {})
            if worker.get("state") != "admitted":
                return None
            owner = worker["owner"]
            identity = (owner["pid"], owner["start_time"])
            if identity not in handles:
                with driver.defer_interrupts():
                    descriptor = driver.process_descriptor(owner)
                    if descriptor is None:
                        return None
                    handles[identity] = descriptor
            return handles[identity]

        def command(arguments, check=True, **ownership):
            try:
                return self.command([str(candidate), "--repo", str(repo), "--state-dir", str(state)] + arguments,
                                    env=dict(os.environ, ARTIFACTIZE_REMOTE="off"), capture_output=True,
                                    text=True, timeout=observation_bound, check=check,
                                    on_spawn=run_commands.add, **ownership)
            except driver.ProcessExitError as error:
                if error.process is not None:
                    # A failed launch can own a process before on_spawn runs.
                    run_commands.add(error.process)
                raise

        def release():
            with driver.defer_interrupts():
                if not area.exists():
                    return
                if not submission_attempted:
                    shutil.rmtree(area)
                    return
                try:
                    try:
                        failures = []
                        for process in run_commands:
                            try:
                                driver.child_cleanup.close(process)
                            except Exception as error:
                                failures.append(error)
                        if failures:
                            raise ExceptionGroup(f"Run commands still own inputs at {area.as_posix()}", failures)
                        live.settle_runs(candidate, area, cleanup_commands)
                        for descriptor in handles.values():
                            self.assertTrue(select.select([descriptor], [], [], 0)[0],
                                            f"Run settlement did not join the worker; fixture retained at {area.as_posix()}")
                    finally:
                        detached.finish()
                    cleanup = json.loads((area / "evidence/cleanup.json").read_text())
                    if identifier is not None:
                        self.assertTrue(any(entry.get("result", {}).get("id") == identifier
                                            and entry["result"]["status"] in {"GREEN", "RED", "INCOMPLETE", "YELLOW", "ERROR"}
                                            for entry in cleanup), f"No terminal result for owned Run {identifier}")
                finally:
                    for descriptor in handles.values():
                        os.close(descriptor)
                    handles.clear()
                # Never delete the private repository/state until BOTH the terminal
                # receipt and actual admitted worker exit have been observed.
                shutil.rmtree(area)
                return cleanup

        with driver.defer_interrupts():
            area = Path(tempfile.mkdtemp(prefix="hey-arti-real-run-")).resolve()
            self.fixture_cleanups.append(release)
        repo, state = area / "repo", area / "state"
        repo.mkdir()
        state.mkdir()
        (area / "evidence").mkdir()
        (repo / ".artifactize").mkdir()
        (repo / ".artifactize/owned-harness-run.artf").write_text('''path = "./"
fingerprint = false
[evals.handoff]
title = "Owned fixture waits at its Human boundary"
profile = { kind = "human" }
payload.instruction = "Keep this request pending; the fixture cancels without approving it."
''')
        submission_attempted = True
        submitted = json.loads(command(["verify", "--all", "--json"], detached=detached).stdout)
        identifier = submitted["run"]
        deadline = time.monotonic() + observation_bound
        while True:
            view = json.loads(command(["run", "show", identifier]).stdout)
            self.assertIn(view["status"], {"RUNNING", "WAITING_HUMAN"}, view)
            descriptor = capture_worker(view)
            if (view["status"] == "WAITING_HUMAN" and descriptor is not None
                    and any(request["status"] == "DEFERRED" for request in view["requests"])):
                self.assertFalse(select.select([descriptor], [], [], 0)[0],
                                 "owned worker must still be running before cancellation")
                break
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
        self.assertEqual(cleanup[0]["worker_exit"], "observed")
        print(f"{identifier}: real cancel/terminal wire and worker exit verified; fixture removed at {area.as_posix()}", flush=True)

    def test_smoke_registers_renamed_candidate_absolutely_and_cleans_private_configuration(self):
        for host in ("claude", "codex"):
            with self.subTest(host=host):
                output = self.root / f"smoke-{host}"
                def registration(command, **options):
                    executable = Path(command[command.index("--") + 1])
                    self.assertTrue(executable.is_absolute())
                    self.assertEqual(executable.read_bytes(), self.candidate.read_bytes())
                    return self.response(command, 1, text="registration failed evidence")
                with mock.patch.object(driver, "run_command", side_effect=registration):
                    with self.assertRaises(subprocess.CalledProcessError):
                        host_smoke.smoke(host, self.candidate, output, "test-model")
                for name in ("repo", "config", "bin", "state"):
                    self.assertFalse((output / name).exists(), f"private working directory {name} survived registration failure")
                self.assertEqual((output / "registration.txt").read_text(), "registration failed evidence")

    # This native inventory seam observes POSIX selectable subprocess pipes and
    # executes shebang fixtures; portable host lifecycle tests above run on every OS.
    @unittest.skipUnless(os.name == "posix", "Native Codex inventory uses POSIX pipe observation and shebang fixtures")
    def test_codex_host_is_joined_and_transcript_preserved_on_success_rpc_error_and_broken_input(self):
        tool_names = ["verify_start", "verify_status", "verify_wait", "verify_watch", "verify_result", "verify_cancel",
                      "status", "config_check", "config_graph", "request_logs", "run_list", "run_show", "request_list", "request_show",
                      "session_show", "tools_check", "doctor", "models", "remote_status", "cache_show"]
        # Execute a genuinely renamed candidate through the registered command,
        # with an executable artifactize on PATH that must never be selected.
        self.candidate.write_text(f'#!{sys.executable} -I\nprint("declared-candidate-entrypoint")\n')
        self.candidate.chmod(0o700)
        stale = self.root / "installed-bin"
        stale.mkdir()
        (stale / "artifactize").write_text(f'#!{sys.executable} -I\nprint("wrong-installed-binary")\n')
        (stale / "artifactize").chmod(0o700)
        start_child = driver.OwnedChild
        for scenario in ("success", "rpc-error", "broken-input", "unconfirmed-exit"):
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
        print(json.dumps({{"id": request["id"], "result": {{"userAgent": "declared-host"}}}}), flush=True)
    elif request["method"] == "mcpServerStatus/list":
        payload = ({{"error": {{"message": "inventory unavailable"}}}} if {scenario != "success"!r} else
                   {{"result": {{"data": [{{"name": "artifactize", "tools": dict.fromkeys({tool_names!r}, {{}})}}]}}}})
        print(json.dumps(dict(id=request["id"], **payload)), flush=True)
''')
                host.chmod(0o700)
                owned = []
                def launch(command, **options):
                    process = start_child(command, **options)
                    if command[0] == str(host):
                        self.register_child(process)
                    if "app-server" in command:
                        owned.append(process)
                        if scenario == "broken-input":
                            stream = process.stdin
                            def close_input():
                                stream.close()
                                process.stdin.closed = True
                                raise BrokenPipeError("host input closed")
                            process.stdin = mock.Mock(wraps=stream)
                            process.stdin.closed = False
                            process.stdin.close.side_effect = close_input
                    return process
                close_child, write_text, remove = driver.close_child, Path.write_text, shutil.rmtree

                def close(process):
                    if process in owned and scenario == "unconfirmed-exit":
                        raise driver.ProcessExitError("app-server exit unconfirmed")
                    return close_child(process)

                def write(path, *args, **kwargs):
                    if path == output / "transcript.jsonl" and scenario == "unconfirmed-exit":
                        raise OSError("diagnostic storage unavailable")
                    return write_text(path, *args, **kwargs)

                def remove_fixture(path, *args, **kwargs):
                    if Path(path).parent == output:
                        self.assertTrue(owned, "fixture removal must follow app-server launch")
                        for process in owned:
                            self.assertIsNotNone(process.returncode, "fixture removal must follow app-server join")
                    return remove(path, *args, **kwargs)

                with mock.patch.object(driver, "OwnedChild", side_effect=launch), \
                        mock.patch.object(driver, "close_child", side_effect=close), \
                        mock.patch.object(shutil, "rmtree", side_effect=remove_fixture), \
                        mock.patch.object(Path, "write_text", write), \
                        mock.patch.dict(os.environ, PATH=str(stale) + os.pathsep + os.environ.get("PATH", "")):
                    if scenario == "unconfirmed-exit":
                        with self.assertRaises(driver.ProcessExitError) as failure:
                            host_smoke.smoke(str(host), self.candidate, output, "test-model")
                        report = "".join(traceback.format_exception(failure.exception))
                        self.assertIn("app-server exit unconfirmed", str(failure.exception))
                        self.assertIn("diagnostic storage unavailable", report)
                    elif scenario == "success":
                        host_smoke.smoke(str(host), self.candidate, output, "test-model")
                        result = json.loads((output / "result.json").read_text())
                        self.assertTrue(result["passed"])
                        self.assertEqual(set(result["tools"]), set(tool_names))
                    elif scenario == "broken-input":
                        with self.assertRaises(ExceptionGroup) as failure:
                            host_smoke.smoke(str(host), self.candidate, output, "test-model")
                        self.assertIsNotNone(failure.exception.subgroup(BrokenPipeError))
                    else:
                        with self.assertRaisesRegex(RuntimeError, "inventory unavailable"):
                            host_smoke.smoke(str(host), self.candidate, output, "test-model")
                self.assertEqual((output / "registration.txt").read_text(), "declared-candidate-entrypoint\n")
                self.assertTrue(owned, "the inventory fixture must launch an app-server")
                if scenario == "unconfirmed-exit":
                    self.assertTrue((output / "repo/.artifactize/smoke.artf").is_file())
                    self.assertTrue((output / "config").is_dir())
                    continue
                for process in owned:
                    self.assertIsNotNone(process.returncode, "owned app-server must be joined before fixture deletion")
                messages = [json.loads(line) for line in (output / "transcript.jsonl").read_text().splitlines()]
                self.assertTrue(any(message.get("result") == {"userAgent": "declared-host"} for message in messages))
                if scenario == "success":
                    self.assertTrue(any(message.get("result") == {"data": [
                        {"name": "artifactize", "tools": dict.fromkeys(tool_names, {})}]}
                                        for message in messages))
                else:
                    self.assertTrue(any(message.get("error") == {"message": "inventory unavailable"}
                                        for message in messages))
                self.assertFalse((output / "config").exists())
                self.assertFalse((output / "bin").exists())

    def test_rpc_deadline_wins_even_when_an_unrelated_reply_and_the_target_are_buffered(self):
        notice = {"method": "unrelated", "params": {}}
        response = {"id": 41, "result": {"data": []}}
        with ExitStack() as pipe:
            with driver.defer_interrupts():
                read_end, write_end = os.pipe()
                pipe.callback(os.close, read_end)
                pipe.callback(os.close, write_end)
            stream = pipe.enter_context(os.fdopen(read_end, "rb", closefd=False))
            line = json.dumps(notice).encode() + b"\n"
            os.write(write_end, line + json.dumps(response).encode() + b"\n")
            real_read = os.read
            elapsed = 0
            delivered_notice = False

            def read(descriptor, size):
                nonlocal elapsed, delivered_notice
                if descriptor != read_end:
                    return real_read(descriptor, size)
                if not delivered_notice:
                    delivered_notice = True
                    return real_read(descriptor, len(line))
                data = real_read(descriptor, size)
                if data:
                    elapsed = 2  # The response arrives at the deadline, with no idle pipe.
                return data

            with host_smoke.selectors.DefaultSelector() as reader:
                reader.register(stream, host_smoke.selectors.EVENT_READ)
                messages = []
                with mock.patch.object(os, "read", side_effect=read), \
                        mock.patch.object(host_smoke, "monotonic", side_effect=lambda: elapsed):
                    with self.assertRaises(TimeoutError):
                        host_smoke.rpc_response(reader, stream, bytearray(), messages, response["id"], deadline=2)
                self.assertIn(notice, messages)
                self.assertNotIn(response, messages)

    def test_claude_inventory_keeps_discovery_but_denies_tool_execution(self):
        run = driver.run_command

        def cli(command, **options):
            if "mcp" in command:
                return self.response(command)
            settings = json.loads(command[command.index("--settings") + 1])
            attempted = "mcp__artifactize__verify_start"
            matching = [hook for rule in settings["hooks"]["PreToolUse"]
                        if re.fullmatch(rule["matcher"], attempted) for hook in rule["hooks"]]
            self.assertTrue(matching, "mutating MCP call has no denial hook")
            responses = [run(shlex.split(hook["command"]), cwd=options["cwd"],
                             input=json.dumps({"tool_name": attempted}), timeout=READY_BOUND,
                             check=True, capture_output=True, text=True,
                             on_spawn=self.register_child) for hook in matching]
            self.assertTrue(any(json.loads(result.stdout)["hookSpecificOutput"]["permissionDecision"] == "deny"
                                for result in responses))
            tools = [f"mcp__artifactize__{name}" for name in host_smoke.TOOLS]
            return self.response(command, data={"subtype": "init", "tools": tools})

        with mock.patch.object(driver, "run_command", side_effect=cli):
            host_smoke.smoke("claude", self.candidate, self.output, "test-model")
        self.assertTrue(json.loads((self.output / "result.json").read_text())["passed"])

    def test_smoke_attempts_other_directories_after_a_removal_failure(self):
        remove = shutil.rmtree

        def remove_fixture(path, *args, **kwargs):
            if path == self.output / "repo":
                raise OSError("repo is not removable")
            return remove(path, *args, **kwargs)

        with mock.patch.object(driver, "run_command", return_value=self.response([], code=1, text="registration failed")), \
                mock.patch.object(shutil, "rmtree", side_effect=remove_fixture):
            with self.assertRaises(ExceptionGroup) as failure:
                host_smoke.smoke("claude", self.candidate, self.output, "test-model")
            self.assertIsNotNone(failure.exception.subgroup(OSError))
        self.assertTrue((self.output / "repo").exists())
        self.assertFalse((self.output / "config").exists())
        self.assertFalse((self.output / "bin").exists())
        self.assertEqual((self.output / "registration.txt").read_text(), "registration failed")

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
    with driver.term_as_interrupt():
        try:
            result = unittest.TextTestRunner(verbosity=2).run(suite)
        finally:
            # unittest normally runs these itself; KeyboardInterrupt can skip
            # that path. Public doCleanups is safe again after completion.
            with driver.defer_interrupts():
                failures = [case for case in owned if not case.doCleanups()]
                if failures:
                    raise RuntimeError("Interrupted suite could not release all owned fixtures")
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifactize", type=Path)
    options = parser.parse_args()
    CANDIDATE = options.artifactize.resolve(strict=True) if options.artifactize else None
    try:
        raise SystemExit(run_suite(sys.modules[__name__]))
    except driver.Terminated as error:
        raise SystemExit(error.exit_code) from None
