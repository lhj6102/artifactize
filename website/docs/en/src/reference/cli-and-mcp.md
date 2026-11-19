# CLI and MCP Reference

Most `artifactize` subcommands accept `--repo PATH`, `--state-dir PATH`, and `--json` before or after the subcommand:
- `monitor` and `review` are desktop launch shortcuts and reject `--json`.
- `mcp` uses `--repo` and `--state-dir`, and always speaks JSON-RPC on `stdout` with diagnostics on `stderr`.
- Current-directory built-in tools (`artifactize tools <read|list|glob|grep|section|help|open>` and standalone `artifactize-tools`) always run in the current directory with their own text or JSON output and do not use `--repo`, `--state-dir`, or `--json`.

`SELECTOR` is one of: positional `ARTIFACT` or `ARTIFACT/EVAL`, `--eval ID`, `--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`, or `--all` (`--evals-file` and `--artifacts-file` accept a JSON array of strings or one qualified eval ID / Artifact name per line).

## CLI command reference {#command-reference}

| Command | Flags & arguments | Output & behavior | Exit codes |
|---|---|---|---|
| `verify SELECTOR` | `[--profile NAME]` (see [`profile_variants`](declarations.md#eval-fields-evalsid)), `--recursive`, `--force`, `--jobs N`, `--fingerprint-jobs N` (default: available CPUs), `--max-executions N`, `--wait` \| `--watch`, `--request-key KEY`, `--comment TEXT`, `--reuse-only KINDS`, `--speculate` | Submits or follows a Run; see [Runs and Workflows](../concepts/runs.md). | `0` accepted, or [Run outcome](../concepts/runs.md#sign-offs-and-the-end-of-a-run) with `--wait`/`--watch` |
| `status [SELECTOR]` (default `--all`) | `[--profile NAME]`, `--recursive`, `--force`, `--fingerprint-jobs N` (default: available CPUs) | Reports eval and Artifact states, predicted actions (`execute`, `reuse`, `wait`, `blocked`; see [Runs and Workflows](../concepts/runs.md)), and changed inputs when no reusable result matches (see [Fingerprints and Reuse](../concepts/reuse.md#completed-result-reuse)). | `0` satisfied, `1` obligations remain, `2` command/state error |
| `config check` | — | Validates `.artifactize/` definitions statically without running project code. | `0` valid, `2` invalid |
| `config graph [ARTIFACT]` | `--impact` | Prints Artifacts, evals, cycle components, and input-to-consumer relations (`--impact` adds downstream consumers). | `0` |
| `config migrate` | `--repo PATH` | Migrates in-tree `.artf` files into `.artifactize/<name>.artf`. | `0` |
| `config rename OLD NEW` | — | Renames Artifact `OLD` to `NEW` across definitions, `parts`, and `{OLD}` references. | `0` |
| `run list` | `--repo-only` \| `--all`, `--limit N` (50), `--offset N` (0) | Lists saved Runs newest first for the current/`--repo` repository (`--repo-only`, the default) or across all repositories in the state (`--all`), with status, counts, and comment. | `0` |
| `run show RUN_ID` | `--wait [--timeout-ms MS]` | Prints full saved Run JSON (`evidence`, `requests`, `usage`, `validation`). `--wait` waits for an open Run (default 10-minute observation timeout; `--timeout-ms` requires `--wait`). | `0` (or Run outcome with `--wait`; `3` when `--wait` times out, leaving the Run running) |
| `run watch RUN_ID` | — | Streams JSON-line progress (`update`, `eval_result`, `outcome`) until settlement. | Run outcome |
| `run cancel RUN_ID` | — | Records and acknowledges a cancellation request for an open Run (`RUNNING` or `WAITING_HUMAN`), keeping results already saved. | `0` |
| `request list` | `--run RUN_ID` | Lists saved requests with status and current claimant (in text output; JSON includes `claim` and `summary`). | `0` |
| `request show ID` | — | Prints the saved request JSON (top-level request fields such as `evalId`, `status`, and `result`, plus `claim`, `execution`, `definition`, `provenance`, and `summary`). | `0` |
| `request logs ID` | `--stream stdout\|stderr\|both` (default `both`), `[--tail N \| --start N [--end N] \| --end N]` (default `--tail 50`), `--search TEXT` | Reads bounded runtime `stdout`/`stderr` capture (`complete`, `partial`, `saved_audit`, `missing`, or `not_runtime` for Agent/Human requests; `--tail` and `--start`/`--end` range queries are mutually exclusive). | `0` |
| `request claim ID` | `--reviewer NAME` | Claims a waiting Human request (default reviewer: `$USER`, then `$LOGNAME` on Unix or `%USERNAME%` on Windows). | `0` |
| `request unclaim ID` | `--reviewer NAME` | Releases a Human claim without submitting a verdict. | `0` |
| `request tool ID TOOL` | `--reviewer NAME` | Runs a declared Human tool on a claimed request. | `0` (`2` tool error) |
| `request submit ID` | `--verdict GREEN\|RED`, `[--fields JSON \| --fields-file PATH]`, `--reviewer NAME` | Validates owner fields (mutually exclusive `--fields` or `--fields-file`; omitting both submits `{}`) against the verdict's schema and settles the Human request. | `0` (also for `RED`) |
| `session show REF` | `--summary` | Prints saved Agent review transcript (or token/turn/tool-call summary). | `0` |
| `session send REF MSG` | — | Continues a saved Agent conversation with a follow-up question (verdict unchanged). | `0` |
| `tools check [EVAL]` | static: `[EVAL \| --eval ID] [--tool NAME]` or `[--artifact ID] [--audience agent\|human] [--tool NAME]`; execute: `--execute --tool NAME (EVAL \| --eval ID \| --artifact ID --audience agent\|human) [--args JSON]` (`--args` is for Agent tools only) | Validates tool declarations and executables statically (or runs one tool with `--execute`). | `0` ready/ok, `1` check/tool failed |
| `tools <subcommand>` | `read`, `list`, `glob`, `grep`, `section`, `help`, `open` | Runs a current-directory built-in tool (also available as `artifactize-tools`). | `0` ok, `1` tool error, `2` usage |
| `cache list` | `--history` | Lists latest (or all) cached reuse-key records. | `0` |
| `cache show KEY` | `--history` | Prints cached record JSON for 64-hex `KEY`. | `0` (`4` missing) |
| `cache rm KEY` | — | Deletes local cache records for `KEY`. | `0` |
| `login` / `logout` | `openai\|anthropic\|gemini\|codex\|claude-agent` | Saves/removes API keys (stdin) or signs in/out of Codex or Claude Code. | `0` |
| `provider` | `list`, `add NAME (--alias P \| --api openai-responses\|openai-chat\|anthropic\|gemini --url URL [--key])`, `remove NAME` | Manages custom Agent providers in `$STATE/auth/providers.json`. | `0` |
| `models PROVIDER` | built-in or custom provider name | Lists available model IDs and display names from the provider. | `0` |
| `remote` | `login URL [--share summary\|full]`, `status`, `push [--dry-run]`, `logout` | Configures and syncs with a team review store (`summary` shares top-level `executionPaths` and omits the full `execution` object including `execution.provenance.repoPath` and `execution.provenance.executionPaths`; `full` includes `execution`). | `0` (`1` not signed in on `status`) |
| `server` | `run [--listen IP:PORT]`, `token add NAME --scopes SCOPES`, `token list`, `token revoke NAME [--purge]`, `rm KEY` | Runs and administers the team review store. | `0` |
| `doctor` | `--repo PATH` | Checks state directory, schema, limits, credentials, remote store, desktop app, and latest release. | `0` ready, `1` hard error |
| `limits` | `--signoff-idle-grace SECONDS` | Shows or sets limits in `$STATE/limits.json` (shared by Runs using that state directory). | `0` |
| `prune` | `[--older-than DURATION]`, `[--dry-run]` | Removes scratch output folders of finished `GREEN`/`RED`/`ERROR`/`INCOMPLETE` Runs that have no unsettled requests (omitting `--older-than` prunes all eligible finished Runs regardless of age; `YELLOW` and open Runs are skipped) and trims Agent sessions. | `0` |
| `monitor [PATH \| --repo PATH \| --all]` | mutually exclusive initial scope selector | Opens the desktop app on all projects or preselects `PATH`'s branch. | `0` |
| `review [REQUEST_ID]` | `--repo PATH` \| `--all`, `--reviewer NAME` | Opens the desktop app on waiting Human reviews (or `REQUEST_ID` directly). | `0` |

## State, cache, and `limits.json` {#state-cache-and-limits}

All local Runs, cache entries, saved Agent conversations, and Artifactize-saved credentials (`codex`, API keys, custom providers, and remote store tokens; `claude-agent` keeps its login in Claude Code itself) live in one state directory outside any repository: `--state-dir PATH`, else `$ARTIFACTIZE_STATE_HOME`, else `$XDG_STATE_HOME/artifactize`, else `%LOCALAPPDATA%\artifactize` on Windows or `~/.local/state/artifactize` on Linux and macOS (the Microsoft Store package keeps an isolated package-private state). Auth files under `$STATE/auth/` are refused if the state directory lies inside a Git worktree or Artifactize workspace.

Optional `$STATE/limits.json` configures bounds across all Runs sharing that state directory:

```json
{
  "backends": { "codex": 4, "openai": 16 },
  "signoffIdleGraceSeconds": 120,
  "agentSessions": { "enabled": true }
}
```

| Setting | Default | Purpose |
|---|---|---|
| `backends.<name>` | unlimited | Max concurrent in-flight Agent reviews per backend across Runs sharing that state directory. |
| `signoffIdleGraceSeconds` | `120` | Idle grace in seconds before a Run with only unclaimed sign-offs ends `YELLOW` (see [Sign-offs and the end of a Run](../concepts/runs.md#sign-offs-and-the-end-of-a-run)). |
| `agentSessions.enabled` | `true` | Saves Agent review transcripts under `$STATE/agent-sessions/`. |
| `agentSessions.maxBytes` / `targetBytes` | `1073741824` / `805306368` | Size ceiling and trim target (in bytes) for automatic cleanup of `$STATE/agent-sessions/`. |

## MCP stdio server (`artifactize mcp`) {#mcp-stdio-server}

`artifactize mcp` exposes verification and non-interactive inspection tools over stdio. Each tool call resolves its target project from the first available source: (1) the call's optional `project` argument (an absolute project folder), (2) `--repo PATH` on the server command (use `--repo PATH` when registering a project-specific server manually in a host that does not launch in the project folder), (3) the host's first local MCP root, or (4) for a host without MCP roots support, the directory the server started in, unless that is the plugin folder that launched it. When the server has no project from `--repo`, host MCP roots, or an allowed startup directory (for example, Codex starts the installed plugin's server in the plugin folder without `--repo`), pass `project` on every call.

### Register the MCP server {#register-the-mcp-server}

The [plugin](../agent-plugin/plugin.md) registers the server automatically in Claude Code and Codex. To register it manually from your project directory:

```sh
claude mcp add --transport stdio --scope project artifactize -- artifactize mcp   # Claude Code
codex mcp add artifactize -- artifactize mcp                                      # Codex CLI
```

### Verification and inspection tools

Every MCP tool accepts an optional `project` argument (absolute project path); inspection tools also accept an optional `cursor` for pagination:

| Tool | Arguments | Purpose |
|---|---|---|
| `verify_start` | **`scope`** (`{"kind":"all"}`, `{"kind":"artifact","artifactId":"..."}`, `{"kind":"eval","evalId":"..."}`, `{"kind":"artifacts","artifactIds":[...]}`, `{"kind":"evals","evalIds":[...]}`), optional `request_key`, `comment`, `jobs`, `speculate` | Submits a Run and returns its Run ID and initial state. |
| `verify_status` | **`run`** | Returns saved Run state, counts, elapsed seconds, and `next_check`. |
| `verify_wait` | **`run`**, optional `seconds` (0–40, default 30) | Waits for Run settlement or the timeout bound. |
| `verify_watch` | **`run`** | Follows Run changes for up to 30s; sends `notifications/progress` (`update`, `eval_result`, `outcome`) when the host supplies `_meta.progressToken` in request metadata (use `verify_wait` when the host does not display progress). |
| `verify_result` | **`run`**, optional `cursor` | Returns final verdict and paged `findings` (`ready: false` while running). |
| `verify_cancel` | **`run`** | Requests cancellation of an open Run. |
| `status` | optional `scope` (Selection object, default `{"kind":"all"}`) | Predicts current states and `execute`/`reuse`/`wait`/`blocked` actions. |
| `config_check` | none | Static validation of `.artifactize/` definitions. |
| `config_graph` | optional `scope` (Selection object), `impact` (boolean, default `false`) | Static graph of Artifacts, evals, cycles, and relations (`impact: true` includes downstream consumers). |
| `request_logs` | **`id`**, optional `stream` (`"stdout"\|"stderr"\|"both"`), `tail`, `start`, `end`, `search` | Reads bounded runtime output lines for request `id`. |
| `run_list` | optional `limit` (default 20), `offset` (default 0) | Lists saved Runs for this project, newest first. |
| `run_show` | **`run`** | Full saved Run JSON. |
| `request_list` | optional `run` | Lists saved requests for this project (or for `run`). |
| `request_show` | **`id`** | Full saved request JSON. |
| `session_show` | **`reference`**, optional `summary` (boolean, default `false`) | Reads a saved Agent review conversation (or summary). |
| `tools_check` | optional `eval`, or `artifact`, `audience` (`"agent"\|"human"`), `tool` | Static validation of declared review tools. |
| `doctor` | none | Checks local state, credentials, remote store, desktop app, and latest release. |
| `models` | **`provider`** | Lists model IDs for a built-in or custom provider. |
| `remote_status` | none | Checks remote review store configuration and reachability. |
| `cache_show` | **`key`**, optional `history` (boolean, default `false`) | Reads cached record(s) for a 64-hex reuse key. |

Inspection tools return CLI JSON in paged envelopes (`{"encoding":"json","entries":[{"offset":0,"text":"..."}],"next_cursor":...}`); concatenate `entries[].text` and pass `next_cursor` as `cursor` on the same call until `next_cursor` is `null`. See [Hey Arti](../agent-plugin/plugin.md#hey-arti-shape-and-audit-your-project) for declaration validation with `config_check` and [Focus then sweep](../agent-plugin/plugin.md#focus-then-sweep-iterate-on-red-evals) for how an agent combines `verify_start`, `verify_wait`, `verify_result`, `request_show`, and `request_logs` across a verification loop.

MCP deliberately omits credential login/logout, Human claim/submit/unclaim, `prune`, `cache rm`, `remote push`, and `session send` so human authority and destructive maintenance stay with the person.
