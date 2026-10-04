# Artifactize

A lean Rust port and rebrand of [CCDD](https://github.com/lhj6102/ccdd), from CCDD 7.0.0 (`cbf28b4`).

Review cost follows the size of a change. Folders declare Artifacts and their evals
in static `artifactize.json` files. artifactize builds the dependency graph and runs
runtime, Agent (OpenAI or Anthropic API key, ChatGPT sign-in, or the Claude CLI) and
Human evals. While an Artifact's fingerprint (what its review depends on) is
unchanged it reuses the earlier GREEN/RED result, and `verify` shows what it executed,
what it reused and the tokens reuse saved.
`status` predicts what a change will re-review. A team review store
(`artifactize server`) shares verdicts across machines and CI. The CLI drives reviews,
`artifactize monitor` shows their progress and `artifactize review` works
through waiting Human sign-offs. It runs on Linux and WSL.

Every non-DROP item in the [CCDD 7.0 capability inventory](docs/ccdd-7-inventory.md)
is checked off; the [plan](docs/PLAN.md) records the lean scope and what was dropped.
Start with [Install](docs/INSTALL.md): prerequisites, backend setup, a 5-minute
quick start, the monitor and cleanup.

## Get started

On Linux or WSL 2, with a Rust toolchain and a C compiler, install the binary and
review the runtime-only example. No model or API key is needed:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # keep the tour's state apart
cd examples/runtime-relations
artifactize verify --all    # three GREEN results, exit 0
artifactize verify --all    # exit 0 and nothing executes: all three results are reused
```

[Install](docs/INSTALL.md) has the prerequisites, backend setup and the full
quick start.

## Examples

Install the binary as described in [Install](docs/INSTALL.md), then try the
example projects. Each README lists the exact commands:

- [Runtime relations](examples/runtime-relations/README.md): runtime evals over
  parent/child folders, a mount alias, `{artifact}` references in instructions
  and argv, a basis Artifact, a RED-able check, and reuse through the built-in
  content fingerprint, with `status` explaining what changed.
- [Agent tools](examples/agent-tools/README.md): an Agent eval using the built-in
  `read`, `grep` and `view_image` tools, a declared `plain` tool and a declared `json` tool,
  pass/fail schemas, backend and model selection, and a Human sign-off with
  `launch` and `output` tools.
- [Family](examples/family/README.md): one family declaration with an instance
  list, parameters and variants, shared and per-instance material, family
  selectors, and the fingerprint script form.
- [Team walkthrough](docs/team-walkthrough.md): two machines and CI reuse each
  other's verdicts through one `artifactize server`.

## Folder configuration

A folder with an `artifactize.json` is an Artifact. It has a `name` and usually
`evals`; `mounts`, `basis`, `views`, `fingerprint` and, at the root, `reviewPolicy`
are optional. Each eval has an `id`, a `title`, a `profile` whose `kind` is
`runtime`, `agent` or `human`, and a `payload` with an `instruction`;
`passSchema`, `failSchema` and `profileVariants` are optional. A runtime eval's
exit code is its verdict (0 is GREEN); an Agent or a Human returns GREEN or RED
with owner fields. This is the `guide` Artifact of the
[runtime-relations example](examples/runtime-relations/README.md), whose README
explains children, mounts, `{artifact}` references and basis Artifacts:

```json
{
  "name": "guide",
  "mounts": {
    "terms": "glossary"
  },
  "fingerprint": {
    "files": ["."],
    "dependencies": "direct"
  },
  "evals": [
    {
      "id": "terms",
      "title": "Every bold term is defined in the glossary",
      "profile": {
        "kind": "runtime",
        "command": "python3",
        "args": [
          "check_terms.py",
          "{terms}/terms.txt",
          "{guide}/intro/page.md",
          "{usage}/page.md"
        ],
        "timeoutMs": 10000
      },
      "payload": {
        "instruction": "Check that every bold term in {intro} and {usage} is defined in {terms}."
      }
    }
  ]
}
```

Declarations use `evals`, with qualified eval IDs such as `green/check`.
`fingerprint` declares what a review depends on. artifactize hashes it: while
the fingerprint is unchanged, the prior verdict is reused; when it changes, the
Artifact is reviewed again. It takes one of two forms:

- `{"files":["."],"dependencies":"direct","ignore":[]}`, the built-in
  [content fingerprint](#content-fingerprint). Every field is optional and these
  are the defaults, so `"fingerprint": {}` covers the whole owner folder.
- `{"script":{"command":"/bin/sh","args":["fingerprint.sh"]}}`, an owner-written
  [fingerprint script](docs/reference.md#fingerprint-scripts), optionally with
  `files` (paths that must exist, never hashed) and `timeoutMs`; `weight` is rejected.

One folder can also declare a [family](docs/reference.md#artifact-families) of
instances, as in the [family example](examples/family/README.md). Discovery rules
and rejected legacy fields are in
[Declaration validation](docs/reference.md#declaration-validation).

### Content fingerprint

`"fingerprint": {}` makes review cost follow the size of a change: an Artifact is
reviewed again only when its own files or its dependencies change, and every other
result is reused. The fingerprint is `content:` plus a SHA-256 over the Artifact name,
each input file's owner-relative path and bytes, and one entry per dependency in
the chosen scope.

- `files`: 1–64 unique owner-relative paths, default `["."]` (the whole owner
  folder). Each must exist and stay inside the Artifact: a path into a child
  Artifact or a mount is rejected, because dependencies come from `dependencies`.
  Directory walks skip child Artifact folders, the owner's `artifactize.json` and
  a family's instance list. Their effect already reaches the fingerprint through the
  Eval definition hash and the dependency list.
- `dependencies`: `none`, `direct` (default) or `transitive`. Dependencies are the
  graph's own: children, mounts and `{artifact}` references in instructions and
  runtime argv. With `direct`, merging a change into `core` re-reviews `core` and
  the Artifacts that use it directly, and only those: the new pairing. Artifacts
  further downstream keep their results, because `direct` never looks past one
  hop. `transitive` covers the whole dependency closure and re-reviews everything
  downstream. Choose it when a review really reads indirect dependencies.
- `ignore`: up to 64 `.gitignore`-style globs relative to the owner, without
  negation. They always exclude, like the built-in ignores `.git`,
  `__pycache__/`, `*.pyc`, `target/` and `node_modules/`. `.gitignore` files
  exclude more, with git semantics: every one from the repository root (`--repo`)
  down through the Artifact and the walked directories applies, each pattern is
  relative to its own file's folder, negation works, and a deeper file overrides a
  shallower one. Explicitly named `files` are never ignored.

How dependency entries are hashed, the walk limits and the recorded manifest are
in the [reference](docs/reference.md#content-fingerprint); the script form is under
[fingerprint scripts](docs/reference.md#fingerprint-scripts).

### Agent reviews

Agent evals use one explicit model and backend, with no bundled catalog, aliases,
credential search or fallback:

```json
{
  "kind": "agent",
  "backend": "openai",
  "model": "YOUR_EXACT_MODEL_ID",
  "reasoning": "high",
  "timeoutMs": 240000
}
```

`backend` accepts `openai`, `anthropic`, `chatgpt` or `claude`. The first two use
only `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`, respectively. ChatGPT uses the stored
Sign in with ChatGPT credentials, never an API-key fallback. `claude` runs the
unmodified official `claude` CLI from `PATH`, already signed in, with the eval's
tools served over MCP by the internal `artifactize mcp` command (see
[Tool diagnostics and MCP](docs/reference.md#tool-diagnostics-and-mcp)); artifactize never reads
Claude credentials (launch controls are in `docs/PLAN.md`).
`provider` and `effort` are not config aliases.

For an eligible ChatGPT subscription:

```sh
artifactize login chatgpt
artifactize models chatgpt           # account-visible slug and display name, in server order
artifactize models chatgpt --json
# Set backend: "chatgpt" and model to one returned slug, then:
artifactize verify --all
```

Use the same `--state-dir` for login, models and verify if overriding the default.
`artifactize logout chatgpt` revokes the refresh token and deletes the stored tokens.

`reasoning` is optional. When present, OpenAI and ChatGPT receive exactly `reasoning.effort`
(`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`). Anthropic receives
adaptive thinking and exactly `output_config.effort` (`low`, `medium`, `high`,
`max`). Other values are rejected, never remapped. A model that does not support
the requested setting fails at the provider; artifactize does not substitute a
model or lower effort. If the response reports a model ID, it must match exactly.

Agent evals share runtime evals' dependency gates, fingerprint claims, reuse and final
fingerprint recheck. Final output must be one strict JSON object containing
`"verdict":"GREEN"` or `"verdict":"RED"` and only the permitted owner-schema fields.
One tools-disabled repair is allowed for invalid final output, within the original
deadline. `maxTokens` and `maxToolCalls` are enforced client-side before further tools
execute; neither becomes a ChatGPT request parameter.

The ChatGPT wire contract, the rig adapter, retries, tool results and usage
counters are in [Agent backends](docs/reference.md#agent-backends).

### Agent tools

`views.agentTools` is an explicit safe-name map. Each entry is either a flat
command declaration or a built-in reference; unknown and mixed fields are rejected.
There is no `metadata`, `script`, `resultKinds`, `artifactKind` or `observation`
wrapper on Agent tools.

```json
{
  "views": {
    "agentTools": {
      "inspect": {
        "description": "Inspect a section of {artifactName}.",
        "inputSchema": {
          "type": "object",
          "properties": {"section": {"type": "string"}},
          "required": ["section"],
          "additionalProperties": false
        },
        "protocol": "json",
        "command": "python3",
        "args": ["inspect.py"],
        "timeoutMs": 120000,
        "executionPaths": ["shared/rules.json"]
      },
      "search": {
        "description": "Search {artifactName}.",
        "inputSchema": {
          "type": "object",
          "properties": {"query": {"type": "string"}},
          "required": ["query"],
          "additionalProperties": false
        },
        "protocol": "plain",
        "command": "rg",
        "args": ["--", "{query}", "."]
      },
      "read": {"builtin": "read", "description": "Read {artifactName}."}
    }
  }
}
```

Field rules, the built-in tools, the `json` and `plain` protocols and their limits
are in the [reference](docs/reference.md#agent-tools).
[`tools check`](docs/reference.md#tool-diagnostics-and-mcp) validates declared
tools and runs one without a review.

### Human tools

`views.humanTools` is a separate safe-name map of predefined commands. Each entry
requires exactly `description`, `kind: "launch" | "output"`, `command` and `args`,
with optional `timeoutMs` (1–2147483647). No `inputSchema`, free call arguments,
`protocol`, `executionPaths`, `metadata` or `script` wrappers are accepted.
Descriptions follow the Agent description rules, including `{artifactName}`.

```json
{
  "views": {
    "humanTools": {
      "open": {
        "description": "Open {artifactName} for review.",
        "kind": "launch",
        "command": "code",
        "args": ["{artifactPath}"]
      },
      "show": {
        "description": "Show the review notes for {artifactName}.",
        "kind": "output",
        "command": "cat",
        "args": ["{artifactPath}/notes.txt"],
        "timeoutMs": 120000
      }
    }
  }
}
```

Placeholders, executable resolution and the `launch` and `output` kinds are in the
[reference](docs/reference.md#human-tools).

## Verify and Runs

`verify` requires exactly one selector: positional `ARTIFACT`, `--eval ID`,
`--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`,
or `--all`. Eval IDs are qualified (`green/check`). Selection order is preserved,
with duplicates removed at their first occurrence; Artifacts expand to their
evals in declaration order. READY runtime evals execute concurrently up to
`--jobs N` (default 4, minimum 1), with dispatch in that same ordered selection
then recursive configuration order. The graph is re-evaluated after each result;
newly READY evals do not wait for an entire batch to finish. Waiters on a
fingerprint claim occupy job slots while polling, but cache hits occupy none.
An Artifact selector runs only that Artifact's evals;
an individual eval selector runs only that eval. Both retain the full dependency
closure as a final obligation. `--recursive` includes every eval in that closure,
including other evals on the selected Artifact and cycle peers, in configuration
order. `--all` already includes every eval and all no-eval Artifact obligations.
A family name selects every instance, also in
`--artifacts` and `--artifacts-file`; overlapping family/instance entries are
deduplicated without selecting the family template itself.

RED blocks downstream execution; missing/operational evidence waits. Cycle peers
have no internal gates. `--ignore-gates` bypasses execution gates only: final
validation still requires actual GREEN evidence or explicit `basis: true` throughout
the required scope. A basis never waives its dependencies. Selected GREEN results
with missing obligations remain recorded in an INCOMPLETE Run; both text and JSON
output identify unmet obligations. Human evals record WAITING_HUMAN requests;
without a submission, ordinary verify exits INCOMPLETE (4). Use `--wait` to keep
the same Run scheduling after Human submissions.

Verification runs in the foreground, with or without `--wait`: GREEN exits 0,
RED 1, ERROR 2, Human wait timeout 3, and INCOMPLETE 4. Ctrl-C/SIGTERM cancels every owned process
group, waits for cleanup, and records ERROR/CANCELLED for running and queued
requests, never RED. Previously committed results remain unchanged. There is no
detached worker.

Text output prints one line per request and marks a result taken from another
request's execution (a fingerprint hit, or a joined execution) with its source Run
(results from a [remote review store](docs/reference.md#remote-review-store-client) also name their
producer or Human reviewer):

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed` counts the Run's own executions, including ERROR results and Human
requests it recorded; `reused` counts the rest that have a source. `Usage` (printed
only when some usage was reported) separates counters spent in this Run from
`saved`, the sum of the reused executions' original counters. JSON verify and
`run show` carry the same numbers as `summary.executed`, `summary.reused` and the
Run-level `usage: {spent, saved}`; `summary.usage` stays equal to `usage.spent`.

`--json` prints
full saved results, including payloads, argv, stdout/stderr and runtime details;
there is no compact projection or `--full` flag. `run show RUN_ID` always prints
full saved JSON and exits 0 on a successful read, regardless of the saved verdict;
`--wait` instead exits with the Run outcome code once it finishes.
It never discovers declarations or runs code, and does not need `--repo`.
Each Run saves its selection, effective policy, requested profile option, and
expanded Artifact/eval definitions for the selected dependency closure, including
family membership, schemas, tool views and graph relations. These are recorded
with the initial request rows, not reconstructed from current declarations.

`run list [--repo-only | --all]` lists saved Runs newest first as a text table, or
as a JSON array with `--json`. It defaults to the canonical `--repo` path (the
current directory when omitted); `--repo-only` makes that default explicit, and
`--all` includes every repository in the shared state. `--limit N` (default 50)
and `--offset N` (default 0) page the filtered results. Each row includes the Run
ID, repository, creation/completion timestamps, status, and request counts by
verdict/state (absent states have zero requests). Reads do not run owner code,
create missing state, or require a repository to still exist. There is no
separate `history` command.

Review policy, `--force`, `--max-executions`, selection files and `--profile`
variants are in the [reference](docs/reference.md#runtime-cli), with the
[command table](docs/reference.md#command-reference) and the
[state layout](docs/reference.md#state).

## Status and static graph

```sh
artifactize --repo PROJECT status
artifactize --repo PROJECT status ARTIFACT --recursive --json
artifactize --repo PROJECT config graph FAMILY --json
artifactize --repo PROJECT config check --json
```

`status` accepts the same exclusive selectors as `verify`, defaulting to `--all`,
and the same `--profile`, `--recursive`, `--force` and `--ignore-gates` policy
options. It reports current Artifact/eval states, unmet final obligations, and
`execute` / `reuse` / `wait` / `blocked` actions. All evals in the required closure
are shown; `selected` and `included` distinguish explicit selection from recursive
execution. Action counts cover included evals only. Exit 0 means current validation
is satisfied; 1 means obligations remain; invalid input or state errors exit 2.

Status prepares current fingerprints for the selected required closure, using
the same isolation and validation as verify. Fingerprint failures exit 2; an old
saved fingerprint is never substituted. It never runs tools or eval commands, creates
Runs, reserves work, creates a missing database or updates cache access times.
Fingerprint scripts use disposable output under the state directory, which may be
created even when no database exists. `config graph` and `config check` remain fully
static and never run owner code.

A current completed fingerprint/Eval-definition entry yields PASS or RED and a `reuse`
action. verify attaches cached results before their gates resolve, so the action
stays `reuse` while a dependency is pending or RED. The state then shows the gate
(WAIT_DEPENDENCY or BLOCKED) and the reason names it. An eval without a cached
result runs only once its gates are GREEN. It shows `blocked` behind a RED
dependency and `execute` when its dependencies are GREEN through cached results.
It shows `wait` when a dependency's result, directly or further upstream, exists
only after verify executes it (or after a live execution finishes). That is the
limit of the prediction: status cannot say whether a `wait` eval will execute. Text ends with, for example,
`Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`, so status run on
a merged checkout answers what verify will re-review there without running it.
Force still applies only to selected evals. Human execution actions
record a waiting request; an active Human fingerprint/Eval-definition pair projects WAITING_HUMAN and a
`wait` action, even after the original verifier exits. Saved attempts
are read for this canonical repository only: each eval's optional
`last: {runId, verdict, fingerprint?}` is historical, not current evidence. Use
`run show RUN_ID` for full attribution. Noncached GREEN/RED satisfies only its own
Run, so its later status is STALE rather than reuse (ENG-24). Basis-only scopes can
be satisfied; a basis with unmet dependencies is INCOMPLETE.

When an eval with a fingerprint has no current cached result, `status` explains why.
It compares the current fingerprint with the newest cached GREEN/RED result for the
same eval and Eval definition hash (from any repository in this state), and
reports `changes: {sinceRunId, files?, dependencies?, summary}`. In text this is a
`Fingerprint changed since Run RUN_ID: ...` line, for example
`changed: +docs/new.md, -old.md, src/a.py; dependency core changed`. Files and
dependencies are listed as `path` (changed), `+path` (added) or `-path` (removed).
A script fingerprint, or a manifest whose maps were dropped, can only report
`fingerprint changed` or `inputs changed`. No explanation appears when that eval
definition has never been cached, or for forced evals.

`config graph [ARTIFACT|FAMILY]` defaults to the whole project, or shows the selected
required closure including cycle peers. Text lists Artifacts, evals, families,
components and input-to-consumer relations. Full JSON includes expanded static
Artifact/eval definitions (including profiles, payloads, schemas and tool views),
child/mount/instruction/argv relation metadata, cycle markers, dependency-first
SCCs and family membership. Component IDs refer to the full graph and may be
noncontiguous in a selected projection. `config check` keeps its static validity
confirmation and JSON Artifact/eval counts. There is one text or full JSON output
level, with no `plan`, `--compact` or `--full`. The former top-level `graph` command
is hidden from help; it prints `graph moved to "artifactize config graph"` and
exits 2.

## Cache inspection

```sh
artifactize cache list --json
artifactize cache show FINGERPRINT [EVAL_HASH]
artifactize cache rm FINGERPRINT [EVAL_HASH]
```

These commands use the shared state home or `--state-dir PATH`, without loading a
repository. `list` shows fingerprint, Eval definition hash, original verdict/repository/eval, retained JSON
bytes and last use (a text table, or a JSON array). `show` always prints the full
saved execution with result, actual profile, provenance, usage and `producer`
(`user@host` and artifactize version; submitted Human results also record their
`reviewer`); a missing entry prints `null` and exits 4. Entries mirrored from a
[shared remote review store](docs/design/remote-store.md) carry an `origin`
(store, publisher, publication time), which `list` shows in place of the repository. Reads neither create missing state nor update
access times. `rm` prints `{"removed":true}` (false if absent), preserving saved
Runs and execution audit. For `show` and `rm`, the hash may be omitted when the
fingerprint has only one entry; multiple definitions require the full hash from
`cache list`. `rm` refuses a key with active executions or waiters (without a hash,
any active definition for that fingerprint prevents removal).

How reuse keys are built and what a hit returns is in
[Completed result reuse](docs/reference.md#completed-result-reuse); entry and byte
limits are in [Cache limits](docs/reference.md#cache-limits).

## Monitor

```sh
artifactize monitor [--repo PATH | --all] [--state-dir PATH]
```

A terminal UI for review progress. Like `run list`, it shows the canonical
`--repo` (default: the current directory) or, with `--all`, every repository.
The Run list (newest first: ID, repository, status, request counts, age) refreshes
every second and on `r`; `j`/`k` or arrows move (moving past the end loads older
Runs), Enter opens a Run, `q` quits. A Run shows state counts, validation,
durations, budgets, executed and reused counts, spent and saved usage, running
evals, waiting Human requests and errors above an
Artifact/eval tree built from the saved definitions: families group their
instances (collapsed until expanded with `l`/→ or Enter), each eval shows its
status glyph and dependency Artifacts, `⇐` rows show child/mount/reference inputs,
and `↻` marks cycles. The right pane details the selected Artifact, family or
request: result, actual and requested profile, fingerprint, reuse source, claim, tool
calls, usage and errors (PgUp/PgDn scroll; Esc returns to the list). The monitor
only reads the state database (read-only connections): it runs no owner code,
needs no repository, and keeps the last data with an error line if a read fails.

On a WAITING_HUMAN eval, `o` hands the terminal to
[`artifactize review REQUEST_ID`](#review) with the monitor's state directory and
its `--repo` or `--all` scope. The monitor leaves the alternate screen, waits for
the review to exit, then restores the screen and refreshes; a failed review leaves
its last error line until the next key. Claims, tools and submissions happen in
that separate process; the monitor itself keeps only read-only connections.

## Human reviews

READY Human evals persist WAITING_HUMAN and release their job slot. They consume
no `maxExecutions` budget, so even a zero budget admits a Human review. `verify`
without `--wait` exits INCOMPLETE and lists waiting requests; it does not fabricate
a verdict or keep a worker alive. Waiting executions with a fingerprint retain their exclusive
fingerprint/Eval-definition claim after the verifier exits. Cross-repository followers refer to that
same execution and forward Human actions to its original request and repository.

For a Human eval with a fingerprint, the next `verify` reuses the submitted result
and runs its dependents. **No fingerprint means no reuse**: submission settles only
that Run, and a later `verify` asks for a new Human review. Continuing the
dependents of a Human eval without a fingerprint requires keeping the same Run alive with `verify --wait`.

```sh
artifactize verify --all --wait --timeout-ms 600000
# In another terminal, using the same state directory:
artifactize request list [--run RUN_ID] [--json]
artifactize request show REQUEST_ID
artifactize request claim REQUEST_ID [--reviewer NAME]
artifactize request unclaim REQUEST_ID [--reviewer NAME]   # release without a verdict
artifactize request tool REQUEST_ID inspect_child [--reviewer NAME]
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true}'
# Alternatively: --fields-file /path/to/fields.json
# Or claim, run tools and submit in a terminal UI (see Review):
artifactize review [REQUEST_ID]
```

Claim, unclaim, tool and submit default the reviewer to `$USER`; `--reviewer NAME`
can select the same explicit reviewer for each action. Reviewer names are local
cooperative locks, not authenticated accounts. Only the claimant can run tools,
submit or unclaim. `request claim` prints the claim and `request unclaim` the
released claim; unclaiming a request someone else holds, or one that no longer
waits, exits 2. Tool names are `<operation>_<artifactId>` and take no free arguments.
Text output prints captured text or a launch notice; `--json` prints the tool
result. A tool failure exits 2 and does not invent a verdict.

Submission requires `--verdict GREEN|RED`. `--fields` and `--fields-file` are
mutually exclusive JSON objects of owner fields (default `{}`), not verdict
wrappers. Files must be regular files; fields and the complete result are bounded
at 256000 bytes. Invalid JSON, schemas, reviewer names or verdicts exit 2 and
leave the request correctable. A successful submission exits 0, even for RED;
it does not start a separate verifier.

`verify --wait` polls saved pending Human request states and resumes newly READY
dependents in the **same Run**, retaining its execution budget and earlier
results. While waiting the Run remains RUNNING. `--timeout-ms` requires `--wait`,
defaults to 600000, and accepts 1–2147483647. The deadline begins when scheduling
starts and is checked when foreground execution is idle with pending Human work;
it never interrupts running evals or cancels Human requests. On timeout the Run
ends INCOMPLETE with `waitTimedOut: true` and exit 3. Claims and submissions remain
available, but no background worker continues dependents. Without a fingerprint,
use a new waiting verify and submit its new request to complete those dependents.
Ctrl-C/SIGTERM exits 2, cleans owned processes, and ends the Run as cancelled;
previously created Human requests remain available. Missing non-Human obligations
without any pending Human request return INCOMPLETE (4) immediately.

The [reference](docs/reference.md#human-reviews) has the library API, the
`request list` and `request show` fields and Run summaries.

## Review

```sh
artifactize review [REQUEST_ID] [--repo PATH | --all] [--state-dir PATH] [--reviewer NAME]
```

A terminal UI for waiting Human reviews with the same lifecycle as `request`: it
claims, runs tools, unclaims and submits in-process, and
publishes a submission to the remote review store exactly like `request submit`.
Without an ID it lists the WAITING_HUMAN requests of the canonical `--repo`
(default: the current directory) or, with `--all`, of every repository (newest
Run first: eval, request, claim, waiting time, repository); Enter opens one. With
an ID it opens that request in any repository. It also runs on its own, for example
in a second terminal or tmux pane.

The request screen shows the request, Run, repository, status, claim, instruction,
GREEN and RED owner schemas and the declared Human tools with their commands and
args. Opening a request claims nothing.

- **Claim on first action.** Running a tool or submitting claims the request for
  the reviewer (`$USER` unless `--reviewer`) if it is unclaimed. A request claimed
  by someone else, or no longer waiting, is read-only. `u` releases your claim. On
  quit, claims this session took without submitting are listed: `k` keeps them,
  `u` releases them.
- **Tools.** `j`/`k` select a tool and Enter runs it. Before the first run of a
  command line in a session, a confirmation shows the resolved command, its
  directory and the repository it comes from (with `--all` it may be another
  repository); `y` runs it. A `launch` tool reports "launched" and the UI
  continues. An `output` tool runs with a spinner (Esc cancels) and its stdout and
  stderr fill the output pane (PgUp/PgDn scroll). A nonzero exit is shown as a tool
  error, not a verdict.
- **Submit.** `s`, then `g` (GREEN) or `r` (RED). A form opens when the verdict's
  owner schema is a flat object whose properties are `const`, `boolean`, `string`
  (with `minLength`, `maxLength` or `enum`), `integer` or `number`; `const` fields
  are prefilled and fixed. Tab or ↑/↓ move, Space or ←/→ choose, typing edits,
  Enter submits. Any other schema (nested objects, arrays, composition, `$ref`),
  or Ctrl-E in the form, opens `$EDITOR` (default `vi`) on a JSON template of the
  owner fields; saving submits it, and an empty file submits nothing. A validation
  error keeps the request waiting and returns to the form with the failing paths.
- After a submission the list returns if more requests wait; otherwise `review`
  exits, which returns to the monitor when the monitor opened it. Esc goes back
  and `q` quits.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|chatgpt|claude [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune` operates on the selected state's Runs across repositories, not the current
repository. It skips unfinished Runs, nonterminal/waiting requests and live execution
owners (PID plus process start time). `--older-than` compares Run completion time;
use a whole-number `s`, `m`, `h`, `d` or `w` duration. With no age filter, every
eligible Run is considered. `--dry-run` returns `wouldRemove` without deleting;
normal JSON returns `removed`, and both include `skippedRuns`.

What `doctor` checks, the `models` output and exactly which directories `prune`
removes are in the [reference](docs/reference.md#local-diagnostics-and-maintenance).

## Team review store

Machines and CI can reuse each other's verdicts through one shared review store,
`artifactize server` ([design](docs/design/remote-store.md)). Each machine keeps its
own state. The store holds one immutable record per (fingerprint, Eval definition
hash), and the first writer wins. The [team walkthrough](docs/team-walkthrough.md)
runs two machines and CI end to end.

**Server.** Run the store on a small host, with its own state directory:

```sh
artifactize --state-dir /srv/artifactize server token add alice-laptop --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add bob-laptop --scopes read,publish
artifactize --state-dir /srv/artifactize server token add ci --scopes read
artifactize --state-dir /srv/artifactize server run     # http://127.0.0.1:8417/
```

`token add` prints each token once. `server run` binds loopback by default. Put a
TLS reverse proxy or a tunnel in front, because clients require HTTPS except on
loopback. Back up `review-store.sqlite` with `sqlite3 .backup`. Upgrade the server
before its clients: a 0.4 server keeps serving 0.3 clients, so machines can follow
one at a time. See
[Review store server](docs/reference.md#review-store-server) for the full reference.

**Clients.** Each machine signs in once. The token is read from stdin and is not
echoed:

```sh
artifactize remote login https://reviews.example/     # paste the token
artifactize remote status
```

From then on:

- `verify` reuses results from the store and publishes its own GREEN/RED results;
- `status` predicts remote reuse;
- `request submit` publishes Human sign-offs;
- `remote push` sends results produced while the store was unreachable.

See [Remote review store client](docs/reference.md#remote-review-store-client) for the full reference.

**CI.** Configure CI through the environment only:

```yaml
- run: artifactize verify --all
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

- A read token reuses but never publishes.
- An outage never turns CI red: `verify` warns once and reviews locally.
- A rejected token, a TLS failure or an invalid configuration fails the job (exit 2),
  so a misconfiguration is never skipped silently.
- `ARTIFACTIZE_REMOTE=off` disables the store for one command.

**Share levels.** `summary`, the default, sends:

- the verdict;
- the schema-validated owner fields (for runtime evals, only the exit code,
  duration and truncation flag);
- the declared profile and usage counters;
- the producer (`user@host`), the Human reviewer and timestamps.

It never sends argv, stdout/stderr, the tool-call audit or repository paths.
`full` (`remote login --share full` or `ARTIFACTIZE_REMOTE_SHARE=full`) also sends
the saved execution as is, including captured output. Owner fields are free text
at both levels, so keep secrets out of the fields that `passSchema` and
`failSchema` allow.

**Trust.**

- The server stamps the authenticated `publisher`, the token name. The `producer`
  and the Human `reviewer` are what the publishing machine claims.
- Any `publish` token can assert any verdict for any key. Give untrusted CI (fork
  pull requests) `read` only, and `human` only to people who sign off. If a token
  leaks or is misused, `server token revoke NAME --purge` also deletes the
  entries it published.
- `verify --force` makes no remote call, which bypasses a suspect entry.
  `cache rm` drops a local mirror, and `server rm` removes the entry from the store.
- Only `$STATE/remote.json` and the environment configure the store, never
  `artifactize.json`, so a cloned repository cannot send your token elsewhere.
  Tokens are stored 0600, bound to the store they were issued for, and never printed.

## Reference

[docs/reference.md](docs/reference.md) is the detailed reference: every command
and flag, exact limits, schemas and protocols, internal library APIs, backend wire
details and the state layout.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
