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
2. **Proposes a composition with reasons.** Arti suggests Artifacts and evals—drawing boundaries around independently edited modules rather than one whole-repository review—and waits for your agreement before writing `.artifactize/<name>.artf` files.
3. **Applies and validates.** Once you agree, Arti writes the definitions, validates them with `config_check`, uses `status` to preview which evals verification would run or reuse, and starts verification via `verify_start`.
4. **Respects human authority.** Arti marks an Artifact `basis = true` only when your team agrees nobody reviews it, never claims or submits a Human sign-off, and hands waiting sign-offs over to you as described in [Working toward YELLOW](#working-toward-yellow).

Arti also helps **adopt Artifactize gradually** in a large repo (starting with the single area that hurts most), **audit an existing setup** (`config_check` and `config_graph`), and **diagnose why checks re-ran** using `status`.

## Focus then sweep: iterate on RED evals

Invoke `/artifactize:focus-then-sweep` in Claude Code, or select `$artifactize:focus-then-sweep` from the `$` skill picker in Codex before asking it to fix a failing Run.

When an edit changes a fingerprint that other evals depend on, their earlier results become stale. Running `verify --all` after every edit wastes time and tokens. The `focus-then-sweep` skill teaches your coding agent three plans:

- **Plan A: focus first, sweep last (default).**
  1. **Focus**: take one `RED` eval, inspect its finding first (`verify_result`, `request_show`, or `request_logs`; CLI: `artifactize run show RUN_ID --json`), fix the Artifact it evaluates, and rerun only that eval with [`--speculate`](../concepts/runs.md#speculation) (`artifactize verify --eval app/review --speculate --wait`, or MCP `verify_start` with `scope: {"kind": "eval", "evalId": "app/review"}` and `speculate: true` followed by repeating `verify_wait` on the same Run ID while machine work is active; when the host supplies no project root—such as the installed Codex plugin—also pass `"project": "/absolute/path/to/project"` on every MCP call). Repeat until the focused eval's own verdict passes (`GREEN` or `PROVISIONAL`, checked via `run_show` or `request_show`) rather than judging by the Run's overall outcome ([Selecting what to verify](../concepts/runs.md#verify-and-runs) and [Submit, observe, and cancel](../concepts/runs.md#submit-observe-and-cancel)); if the Run reveals another required eval with a `RED` or `ERROR` result, focus on that eval next, leaving merely stale or unreviewed obligations for the sweep. If an eval is `BLOCKED`, focus on the upstream `RED` or `ERROR` eval first. To redo a `RED` human sign-off after making requested changes, select its Artifact (`artifactize verify app`, or `artifactize verify app --recursive` when stale upstream prerequisites also need to run) so its prerequisites pass and the replacement sign-off is requested, then move on without waiting for the person.
  2. **Sweep**: once all focused evals pass (or wait on a replacement sign-off), run the full project once (`artifactize verify --all --wait`, or MCP `verify_start` with `scope: {"kind": "all"}`) and follow [Working toward YELLOW](#working-toward-yellow). Unchanged reusable verdicts are reused, and stale downstream evals run once their upstream gates pass. If the sweep reveals any `RED` or `ERROR`, return to focus; if a non-sign-off obligation is unmet (`INCOMPLETE`), resolve or report it before sweeping again.
- **Plan B: focus with expected impact, sweep last.** When a fix changes a shared spec, rule document, or library, use `artifactize config graph app --impact` (MCP `config_graph` with `scope: {"kind": "artifact", "artifactId": "app"}` and `impact: true`) to find downstream evals and `status` to see which ones need review. Run the original `RED` eval together with the affected downstream evals in the focus Run (`--evals app/review,docs/matches-app --speculate --wait`, or MCP `verify_start` with `scope: {"kind": "evals", "evalIds": ["app/review", "docs/matches-app"]}` and `speculate: true`); when the focus touches an Artifact in a reference cycle, keep the full set of focused and affected evals and add every eval on every Artifact in that reference cycle (including the remaining evals on the Artifact already in focus) to the same focus Run, then sweep as in Plan A.
- **Plan C: always sweep.** Run the full sweep (`artifactize verify --all --wait`, or MCP `verify_start` with `scope: {"kind": "all"}`) after each fix when the project has few or cheap evals, or when one change affects nearly every eval, following [Working toward YELLOW](#working-toward-yellow) except that any `RED`, `ERROR`, or non-sign-off `INCOMPLETE` obligation is fixed immediately and verified with another full sweep rather than switching to targeted focus.

### Working toward YELLOW {#working-toward-yellow}

In MCP, repeat `verify_wait` on the same Run ID while the Run is `"preparing"`, `"waiting"`, or `"running"`, and read `verify_result` (`ready: true`) and `validation.obligations` in `run_show` once settled. As soon as `verify_wait` reports `"waiting_human"` (or in the CLI once no runnable machine checks remain—see [Submit, observe, and cancel](../concepts/runs.md#submit-observe-and-cancel) for stopping `--wait` with `Ctrl+C`), inspect `requests` and `evidence` in `run_show` / `artifactize run show RUN_ID` (and check `config_graph` or `status` for bare unreviewed Artifacts while the Run is still open, since `validation.obligations` is populated at settlement): if any machine or Human request is `RED` or `ERROR`, handle it immediately without waiting for sign-offs (returning to focus in Plans A and B, or fixing and sweeping again in Plan C); if any non-sign-off obligation is unmet (`INCOMPLETE`), resolve or report it before sweeping again; if machine work is still running or runnable, repeat `verify_wait`. Once all machine checks pass (`GREEN` or `PROVISIONAL`), no other obligation is unmet, and only unsubmitted human sign-offs remain ([Sign-offs and the end of a Run](../concepts/runs.md#sign-offs-and-the-end-of-a-run)), the builder continues independent work rather than polling or keeping turns alive, lists the waiting requests with `request_list` and `request_show`, hands them to the human reviewer (`artifactize review REQUEST_ID`), and never claims or submits a sign-off itself.
