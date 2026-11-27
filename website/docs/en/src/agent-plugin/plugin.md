# Working with Coding Agents

The `artifactize` plugin gives **Claude Code** and **Codex** three things in one install:

- **[Hey Arti](#hey-arti-shape-and-audit-your-project)**: Artifactize's Forward Deployed Engineer skill (`/artifactize:hey-arti` in Claude Code, `$artifactize:hey-arti` in Codex), which learns your repository, proposes Artifacts and evals, and diagnoses results.
- **[Focus then sweep](#focus-then-sweep-iterate-on-red-evals)**: the builder iteration skill (`/artifactize:focus-then-sweep` in Claude Code, or `$artifactize:focus-then-sweep` from the `$` skill picker in Codex), which reruns only the failing eval while fixing it and sweeps the full graph at the end.
- **[The MCP server](../reference/cli-and-mcp.md#mcp-stdio-server)**: `artifactize mcp`, which lets the agent inspect the graph, start Runs, and read logs or verdicts.

## Add the plugin {#updates}

[Install `artifactize`](../getting-started/install.md) first so the binary is on `PATH`.

**Claude Code** (inside a session, or run `claude plugin ...` in a shell):

```text
/plugin marketplace add lhj6102/artifactize
/plugin install artifactize@artifactize
```

Run `/reload-plugins` (or start a new session in your project directory) and check `/mcp` to confirm the `artifactize` server is connected. Update later with `/plugin marketplace update artifactize`.

**Codex** (add the marketplace in a shell, then run `codex plugin add` or pick `artifactize` in `/plugins`):

```sh
codex plugin marketplace add lhj6102/artifactize
codex plugin add artifactize@artifactize
```

Start a new Codex session in your project directory. Update later with `codex plugin marketplace upgrade artifactize`.

**Other hosts (manual setup)**: register `artifactize mcp` over stdio ([Register the MCP server](../reference/cli-and-mcp.md#register-the-mcp-server)) and attach the canonical [`hey-arti/SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md) and [`focus-then-sweep/SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/focus-then-sweep/SKILL.md) files to your conversation. Do not combine the plugin and a manual MCP registration in the same host.

## Hey Arti: shape and audit your project

Invoke `/artifactize:hey-arti` in Claude Code or select `$artifactize:hey-arti` from the `$` skill picker in Codex:

```text
/artifactize:hey-arti Explain Artifactize and tell me how we could apply it to this project
/artifactize:hey-arti Help me move our existing tests and reviews into Artifactize
```

How Arti works with you:

1. **Learns and interviews.** Arti inspects your repository's code, docs, specs, and test commands, then asks about what it cannot read: your goals, which pieces change together, external specs or shared policies, and which decisions require a person.
2. **Proposes a composition with reasons.** Arti suggests Artifacts and evals—drawing boundaries around independently edited modules rather than one whole-repository review—and waits for your agreement before writing `.artf` files under `.artifactize/`.
3. **Applies and validates.** Once you agree, Arti writes the definitions, validates them with `config_check`, uses `status` to preview which evals verification would run or reuse, and starts verification via `verify_start`.
4. **Respects human authority.** Arti marks an Artifact `basis = true` only when your team agrees nobody reviews it, never claims or submits a Human sign-off, and hands waiting sign-offs over to you as described in [Working toward YELLOW](#working-toward-yellow).

Arti also helps **adopt Artifactize gradually** in a large repo (starting with the single area that hurts most), **audit an existing setup** (`config_check` and `config_graph`), and **diagnose why checks re-ran** using `status`.

## Focus then sweep: iterate on RED evals

Invoke `/artifactize:focus-then-sweep` in Claude Code, or select `$artifactize:focus-then-sweep` from the `$` skill picker in Codex before asking it to fix a failing Run.

When an edit changes a fingerprint that other evals depend on, their earlier results become stale. Running `verify --all` after every edit wastes time and tokens. The `focus-then-sweep` skill teaches your coding agent three plans:

Start the CLI commands below without `--wait`. Keep the printed Run ID and inspect the open Run with `artifactize run show RUN_ID --json`, or follow `artifactize run watch RUN_ID` and stop watching with Ctrl+C without cancelling. With MCP, keep the ID from `verify_start`, then use `verify_wait` and `run_show`. If the host supplies no project root, pass the absolute project path as `project` on every call.

- **Plan A: focus first, sweep last (default).** Locate one failed eval and its request ID with `verify_result` or `run_show` (CLI: `artifactize run show RUN_ID --json`). Use `request_show` or `artifactize request show REQUEST_ID --json` for the saved result and `request_logs` or `artifactize request logs REQUEST_ID` for runtime output. Fix the evaluated Artifact and rerun only that eval with `artifactize verify --eval app/review --speculate` or MCP `verify_start` with `scope: {"kind":"eval","evalId":"app/review"}` and `speculate: true`. Check the eval's own verdict, even when the Run is not GREEN. Fix one failed eval at a time, then sweep with `artifactize verify --all` or MCP `scope: {"kind":"all"}`. A new failure sends you back to focus.
- **Plan B: focus with expected impact, sweep last.** Query each Artifact the fix actually changed. If `app/review` failed and you changed `spec`, use `artifactize config graph spec --impact` or `config_graph` with `scope: {"kind":"artifact","artifactId":"spec"}` and `impact: true`. Use `status` to see what needs review. Focus on the failed eval and affected evals together (`artifactize verify --evals app/review,docs/matches-app --speculate`, or MCP `scope: {"kind":"evals","evalIds":["app/review","docs/matches-app"]}` and `speculate: true`). Add all evals on any reference cycle the focus touches, then sweep as in Plan A.
- **Plan C: always sweep.** When checks are few or cheap or most read the changed Artifact, run `artifactize verify --all` or MCP `scope: {"kind":"all"}` after every fix.

### Working toward YELLOW {#working-toward-yellow}

Inspect requests and evidence even while a Run is `WAITING_HUMAN`:

1. Handle `RED` or `ERROR` first, without waiting for other work. A sign-off superseded by a newer Run is unanswered rather than failed; use the newer request.
2. If no request failed and machine work can still run, keep observing. Call `verify_wait` and inspect `run_show` again in MCP; `verify_result` can be unready while sign-offs are deferred.
3. Once machine work is done, check the settled Run's `validation.obligations`. If the Run is open, also use `config_graph` or `status` to check for bare Artifacts. Resolve unmet non-sign-off obligations before sweeping again, or stop and report the blocker if you cannot resolve it.
4. When all machine checks pass and only sign-offs remain, hand off their request IDs. A person can open one with `artifactize review REQUEST_ID` if the desktop app is installed, or use the [`artifactize request` commands](../reference/cli-and-mcp.md#command-reference). The builder never submits a sign-off.
