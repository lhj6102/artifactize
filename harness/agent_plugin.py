"""Install the agent plugin from a repository marketplace into a disposable host
configuration and compare what the host loaded with the plugin folder.

    python3 -I agent_plugin.py claude PLUGIN_DIR .claude-plugin/marketplace.json
    python3 -I agent_plugin.py codex PLUGIN_DIR .agents/plugins/marketplace.json

The host is the installed `claude` or `codex` program found on PATH. Only the
host's own commands and answers are observed: its validator, install, plugin and
MCP listings, and Codex's skill inventory. No MCP server is started. It runs on
Linux uses a subreaper to own the command's descendants. macOS keeps the
command's process group anchored until teardown.
"""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
OWNERS = Path(__file__).resolve().parent / "hey-arti"
sys.path.insert(0, str(OWNERS))
import driver as ownership

# Where each host documents its marketplace file, relative to the marketplace root.
CATALOGS = {"claude": Path(".claude-plugin", "marketplace.json"), "codex": Path(".agents", "plugins", "marketplace.json")}
# The MCP server must run `artifactize mcp` for the session's project: Claude Code
# substitutes its project root into the arguments; Codex starts the server in the
# installed plugin's folder, where each call names the project or the client's roots do.
SERVER = "artifactize"
LAUNCH = {
    "claude": {"command": "artifactize", "args": ["--repo", "${CLAUDE_PROJECT_DIR}", "mcp"]},
    "codex": {"command": "artifactize", "args": ["mcp"]},
}
# Host commands only read and copy local files; the bound stops a hung host, it does not order anything.
COMMAND_SECONDS = 120
# Bound for one Codex app-server answer, for the same reason.
ANSWER_SECONDS = 60
# Bound for a killed host command's processes to exit; it stops a hang, it orders nothing.
EXIT_SECONDS = 30
# A parent Claude Code session exports these; a host started under them acts as its child.
SESSION_VARIABLES = ("CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID")


class MacHost(subprocess.Popen):
    """The broker remains the unreaped group leader, even after its command exits."""

    def __init__(self, argv, **options):
        self.command_result = None
        read, write = os.pipe()
        self.status = os.fdopen(read, "rb")
        try:
            try:
                super().__init__([sys.executable, "-I", str(Path(__file__).resolve()),
                                  "--keep-group", str(write), *argv],
                                 pass_fds=(write,), start_new_session=True, **options)
            finally:
                os.close(write)
        except BaseException:
            self.status.close()
            raise

    def _wait(self, timeout):
        # Popen.communicate observes the command status without reaping the group anchor.
        if self.command_result is None:
            with selectors.DefaultSelector() as ready:
                ready.register(self.status, selectors.EVENT_READ)
                if not ready.select(timeout):
                    raise subprocess.TimeoutExpired(self.args, timeout)
            self.command_result = int(os.read(self.status.fileno(), 64))
        return self.command_result

    def close(self):
        try:
            # Never signal or probe this numerical group after reaping its leader.
            try:
                os.killpg(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            super()._wait(EXIT_SECONDS)
            # Failure to close the pipes can mean another process is using the fixture.
            with selectors.DefaultSelector() as ready:
                for stream in (self.stdout, self.stderr):
                    if stream is not None and not stream.closed:
                        ready.register(stream, selectors.EVENT_READ)
                deadline = time.monotonic() + EXIT_SECONDS
                while ready.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not (events := ready.select(remaining)):
                        raise RuntimeError("Host output is still open after stopping the owned group")
                    for key, _ in events:
                        if not os.read(key.fd, 32768):
                            ready.unregister(key.fileobj)
        except BaseException as error:
            raise ownership.ProcessExitError("Mac host cleanup was not confirmed", process=self) from error
        finally:
            self.status.close()
            for stream in (self.stdin, self.stdout, self.stderr):
                if stream is not None:
                    stream.close()


def keep_group(status, command):
    """Report the actual command's status and retain the group's numeric identity."""
    # The command must not inherit the control pipe or keep the group's leader alive itself.
    os.set_inheritable(status, False)
    try:
        process = subprocess.Popen(command)
    except OSError as error:
        print(error, file=sys.stderr)
        code = 127
    else:
        for descriptor in (0, 1, 2):
            os.close(descriptor)
        code = process.wait()
    os.write(status, str(code).encode("ascii"))
    os.close(status)
    while True:
        signal.pause()


@contextmanager
def host_command(argv, env, cwd, **pipes):
    program = shutil.which(argv[0], path=env.get("PATH"))
    if program is None:
        sys.exit(f"{argv[0]} is not on PATH; install the host to check its plugin")
    argv = [program, *argv[1:]]
    options = {"cwd": cwd, "env": env, "text": True, **pipes}
    if sys.platform == "linux":
        with ownership.owned_child(argv, **options) as process:
            yield process
    else:
        process = None
        try:
            with ownership.defer_interrupts():
                process = MacHost(argv, **options)
            yield process
        finally:
            if process is not None:
                with ownership.defer_interrupts():
                    process.close()


def run(argv, env, cwd):
    with host_command(argv, env, cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
        stdout, stderr = process.communicate(timeout=COMMAND_SECONDS)
        code = process.wait(timeout=0)
    if code:
        sys.exit(f"{' '.join(argv)} exited {code}:\n{stdout}{stderr}")
    return stdout


def listed_plugin(host, catalog, root, plugin):
    """The marketplace entry whose source is the plugin folder under check."""
    for entry in catalog["plugins"]:
        source = entry["source"] if host == "claude" else entry["source"].get("path")
        if isinstance(source, str) and (root / source).resolve() == plugin:
            return f"{entry['name']}@{catalog['name']}"
    sys.exit(f"{CATALOGS[host].as_posix()} lists no plugin with source {plugin.relative_to(root).as_posix()}")


def claude(market, project, env, plugin_id):
    # Warnings pass: the manifest leaves out `version` so installs follow the repository's commits.
    print(run(["claude", "plugin", "validate", str(market)], env, project), end="")
    run(["claude", "plugin", "marketplace", "add", str(market)], env, project)
    run(["claude", "plugin", "install", plugin_id], env, project)
    details = run(["claude", "plugin", "details", plugin_id], env, project)
    line = next((line for line in details.splitlines() if line.strip().startswith("Skills (")), "")
    skills = set(line.split(")", 1)[1].replace(",", " ").split()) if ")" in line else set()
    installed = json.loads(run(["claude", "plugin", "list", "--json"], env, project))
    record = next((item for item in installed if item["id"] == plugin_id), None)
    if record is None:
        sys.exit(f"claude plugin list does not report {plugin_id} after installing it")
    return skills, record["enabled"], record.get("mcpServers", {}).get(SERVER)


def codex(market, project, env, plugin_id):
    run(["codex", "plugin", "marketplace", "add", str(market)], env, project)
    run(["codex", "plugin", "add", plugin_id], env, project)
    servers = json.loads(run(["codex", "mcp", "list", "--json"], env, project))
    server = next((item for item in servers if item["name"] == SERVER), None)
    prefix = plugin_id.split("@")[0] + ":"
    skills = {skill["name"].removeprefix(prefix) for group in codex_skills(project, env)
              for skill in group["skills"] if skill.get("pluginId") == plugin_id and skill["enabled"]}
    return skills, server is not None and server["enabled"], server and server["transport"]


class AppServer:
    def __init__(self, process):
        self.process = process
        self.buffer = bytearray()

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def ask(self, identifier, method, params):
        self.send({"id": identifier, "method": method, "params": params})
        deadline = time.monotonic() + ANSWER_SECONDS
        with selectors.DefaultSelector() as ready:
            ready.register(self.process.stdout, selectors.EVENT_READ)
            while True:
                while (newline := self.buffer.find(b"\n")) >= 0:
                    message = json.loads(self.buffer[:newline])
                    del self.buffer[:newline + 1]
                    if message.get("id") == identifier:
                        if "error" in message:
                            sys.exit(f"codex app-server {method}: {message['error']}")
                        return message["result"]
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not ready.select(remaining):
                    sys.exit(f"codex app-server did not answer {method}")
                if not (data := os.read(self.process.stdout.fileno(), 32768)):
                    sys.exit("codex app-server closed before answering")
                self.buffer.extend(data)


def codex_skills(project, env):
    """Codex's skill inventory for the project, from its app-server; it starts no MCP server."""
    with host_command(["codex", "app-server", "--stdio"], env, project, stdin=subprocess.PIPE,
                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as process:
        server = AppServer(process)
        server.ask(1, "initialize", {"clientInfo": {"name": "artifactize-plugin-check", "version": "1"}})
        server.send({"method": "initialized"})
        return server.ask(2, "skills/list", {"cwds": [str(project)]})["data"]


def main(host, plugin, catalog_path):
    plugin = plugin.resolve(strict=True)
    catalog_path = catalog_path.resolve(strict=True)
    root = catalog_path.parents[len(CATALOGS[host].parts) - 1]
    plugin_id = listed_plugin(host, json.loads(catalog_path.read_text(encoding="utf-8")), root, plugin)
    # Agent Plugins finds skills only in skills/<name>/SKILL.md, one level down.
    expected = {path.parent.name for path in plugin.glob("skills/*/SKILL.md")}
    scratch = Path(tempfile.mkdtemp(prefix="agent-plugin-"))
    cleanup = True
    try:
        # Copy only the catalog and the plugin folder: like a host's plugin cache,
        # the installed plugin must need nothing else from the repository.
        market = scratch / "marketplace"
        (market / CATALOGS[host]).parent.mkdir(parents=True)
        shutil.copy2(catalog_path, market / CATALOGS[host])
        shutil.copytree(plugin, market / plugin.relative_to(root), symlinks=True)
        project = scratch / "project"
        project.mkdir()
        config = scratch / "config"
        config.mkdir()
        env = {key: value for key, value in os.environ.items() if key not in SESSION_VARIABLES}
        env["CLAUDE_CONFIG_DIR" if host == "claude" else "CODEX_HOME"] = str(config)
        skills, enabled, server = (claude if host == "claude" else codex)(market, project, env, plugin_id)
    except ownership.ProcessExitError as error:
        cleanup = False
        raise RuntimeError(f"Host could still be using its scratch configuration at {scratch}") from error
    finally:
        if cleanup:
            shutil.rmtree(scratch)
    launch = {key: (server or {}).get(key) for key in LAUNCH[host]}
    problems = [message for failed, message in (
        (skills != expected, f"skills: loaded {sorted(skills)}, the plugin folder has {sorted(expected)}"),
        (not enabled, f"{plugin_id} or its {SERVER} MCP server is not enabled"),
        (launch != LAUNCH[host], f"MCP server {SERVER}: launched as {launch}, expected {LAUNCH[host]}"),
    ) if failed]
    if problems:
        sys.exit(f"{host}: " + "; ".join(problems))
    print(f"{host}: {plugin_id} loads skills {sorted(skills)} and MCP server {SERVER} as {launch}")


if __name__ == "__main__":
    if len(sys.argv) > 3 and sys.argv[1] == "--keep-group":
        keep_group(int(sys.argv[2]), sys.argv[3:])
    if len(sys.argv) != 4 or sys.argv[1] not in CATALOGS:
        sys.exit(__doc__)
    # A host command's descendants are owned through its process group, which Windows lacks:
    # there they could outlive the check and keep its scratch configuration in use.
    if os.name != "posix":
        sys.exit("The agent plugin check runs on Linux and macOS only.")
    try:
        with ownership.term_as_interrupt():
            main(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]))
    except ownership.Terminated as error:
        sys.exit(error.exit_code)
