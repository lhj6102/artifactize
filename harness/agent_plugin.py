"""Install the agent plugin from a repository marketplace into a disposable host
configuration and compare what the host loaded with the plugin folder.

    python3 -I agent_plugin.py claude PLUGIN_DIR .claude-plugin/marketplace.json
    python3 -I agent_plugin.py codex PLUGIN_DIR .agents/plugins/marketplace.json

The host is the installed `claude` or `codex` program found on PATH. Only the
host's own commands and answers are observed: its validator, install, plugin and
MCP listings, and Codex's skill inventory. No MCP server is started.
"""
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import sys
import tempfile
import threading

sys.dont_write_bytecode = True

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
# A parent Claude Code session exports these; a host started under them acts as its child.
SESSION_VARIABLES = ("CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID")


def started(argv, env, cwd, **pipes):
    """Start a host command as the leader of its own process group, so that `ended` can end
    every process it starts."""
    program = shutil.which(argv[0], path=env.get("PATH"))
    if program is None:
        sys.exit(f"{argv[0]} is not on PATH; install the host to check its plugin")
    group = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
    return subprocess.Popen([program, *argv[1:]], cwd=cwd, env=env, text=True, **group, **pipes)


def ended(process):
    """End every process still in the host command's group, then reap its leader."""
    if os.name == "nt":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(process.pid)], capture_output=True)
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait()


def run(argv, env, cwd):
    process = started(argv, env, cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        stdout, stderr = process.communicate(timeout=COMMAND_SECONDS)
    finally:
        ended(process)
    if process.returncode != 0:
        sys.exit(f"{' '.join(argv)} exited {process.returncode}:\n{stdout}{stderr}")
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


def codex_skills(project, env):
    """Codex's skill inventory for the project, from its app-server; it starts no MCP server."""
    process = started(["codex", "app-server", "--stdio"], env, project, stdin=subprocess.PIPE,
                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    lines = queue.Queue()

    def read():
        for line in process.stdout:
            lines.put(line)
        lines.put(None)

    reader = threading.Thread(target=read, daemon=True)
    reader.start()

    def ask(identifier, method, params):
        process.stdin.write(json.dumps({"id": identifier, "method": method, "params": params}) + "\n")
        process.stdin.flush()
        while True:
            line = lines.get(timeout=ANSWER_SECONDS)
            if line is None:
                sys.exit("codex app-server closed before answering")
            message = json.loads(line)
            if message.get("id") == identifier:
                if "error" in message:
                    sys.exit(f"codex app-server {method}: {message['error']}")
                return message["result"]

    try:
        ask(1, "initialize", {"clientInfo": {"name": "artifactize-plugin-check", "version": "1"}})
        process.stdin.write(json.dumps({"method": "initialized"}) + "\n")
        process.stdin.flush()
        return ask(2, "skills/list", {"cwds": [str(project)]})["data"]
    finally:
        try:
            # The app-server leaves when its input closes; whatever it started is ended with it.
            process.stdin.close()
            try:
                process.wait(timeout=ANSWER_SECONDS)
            except subprocess.TimeoutExpired:
                pass
        finally:
            ended(process)
            reader.join()


def main(host, plugin, catalog_path):
    plugin = plugin.resolve(strict=True)
    catalog_path = catalog_path.resolve(strict=True)
    root = catalog_path.parents[len(CATALOGS[host].parts) - 1]
    plugin_id = listed_plugin(host, json.loads(catalog_path.read_text(encoding="utf-8")), root, plugin)
    # Agent Plugins finds skills only in skills/<name>/SKILL.md, one level down.
    expected = {path.parent.name for path in plugin.glob("skills/*/SKILL.md")}
    with tempfile.TemporaryDirectory(prefix="agent-plugin-") as scratch:
        scratch = Path(scratch)
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
    if len(sys.argv) != 4 or sys.argv[1] not in CATALOGS:
        sys.exit(__doc__)
    main(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]))
