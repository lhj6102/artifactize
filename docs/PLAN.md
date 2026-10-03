# artifactize plan: lean CCDD 7.0 port

artifactize is a Rust port and rebrand of [CCDD](https://github.com/lhj6102/ccdd) 7.0.0 (`cbf28b4`). It covers [ccdd#101](https://github.com/lhj6102/ccdd/issues/101). CCDD keeps living as its own program.

**Target: CCDD 7.0's core logic, light.**

**Language.** CCDD's *Critic* is an **eval** in artifactize: the config field is `evals`, the CLI uses `--eval`/`--evals`/`--evals-file`, and the type is `Eval`. The inventory keeps CCDD's original terms.

 artifactize keeps what makes CCDD useful and drops the machinery around it.

[`ccdd-7-inventory.md`](ccdd-7-inventory.md) lists every CCDD 7.0 capability. Each item is in one of three states:
- assigned to a task;
- assigned to a task and marked SIMPLIFIED, with what remains;
- marked DROP, with the reason.

Version 0.1.0 ships when every non-DROP item is checked off. Lean means no process overhead (no parity or performance suites, no ceremony, no speculative abstractions) and no machinery that the core logic does not need.

## Core logic

- Static `artifactize.json` declarations, families, selection and profile variants.
- The dependency graph: children, mounts, `{artifact}` references, SCCs, external GREEN gates, RED blocking, ERROR waiting, final obligations.
- Three eval kinds:
  - **Runtime:** a generic command; the exit code is the verdict.
  - **Agent:** OpenAI or Anthropic API key, ChatGPT subscription, or the Claude CLI. A strict JSON verdict with one repair.
  - **Human:** claim, then submit.
- Tools, declared separately per audience:
  - **Agent tools:** take free arguments validated by JSON Schema. They are declared CLI commands or the built-ins `read`, `list`, `glob`, `grep` and `view_image`, and are also served over MCP.
  - **Human tools:** predefined commands that launch a program for the reviewer or show a command's output.
- The identity cache:
  - An explicit identity maps to a completed GREEN/RED result.
  - No identity means no reuse.
  - The same identity runs only once at a time.
  - Each review ends with an identity recheck.
- Budgets: `maxExecutions`, `maxTokens`, `maxToolCalls`, deadlines.
- The CLI, including `models` to list a configured provider's models, and an `artifactize monitor` TUI for review progress.

## Drops and replacements

| CCDD 7.0 | artifactize |
|---|---|
| Node-only runtime (`node --test`), Node import hooks, offline monkey-patch guard | Generic `{command, args}` for runtime evals and every hook; no guard |
| `load-check` benchmark, `prepare-demo` and demo tooling | Dropped |
| npm packages and umbrella facade | Cargo-installed binaries |
| Pi catalog and auth adapters, Pi re-exports | rig-core `=0.43.0` (OpenAI/Anthropic API keys), ChatGPT via official Sign in with ChatGPT, Claude via the unmodified official `claude` CLI |
| CCDD config/state/cache compatibility | New format only: `artifactize.json`, fresh state |
| Continuous workspace monitoring | End-of-review identity recheck ([ccdd#104](https://github.com/lhj6102/ccdd/issues/104)) |
| Content fingerprints, runtime pinning, canonical object store and record authentication | The identity is the only input key; plain SQLite rows |
| Separate receipt and global databases with copy-on-settle | One state database |
| Detached `__worker`, resume, owner handoff and fencing generations, prepared submission handles | Foreground `verify`; Ctrl-C cancels |
| Admission pools, weighted identity FIFO, `resources.json`, caller caps, custom admission, budget reservation ledger | `--jobs N` and plain `maxExecutions`/`maxToolCalls` counters |
| Provider account lanes, cooldowns, `provider status/resume`, telemetry events | At most 2 transient retries before any output; per-attempt usage |
| Change cursors, NDJSON streams, history comparisons, `run diff`, quarantined prune | `run show [--wait]`, `run list`, plain prune |
| Human reservation/renewal/expiry, preparation phases and progress, readiness hooks, alarms, `resultCheck` | Claim lock and submit; owner checks belong in `passSchema` |
| Same-input ERROR retry, coalescing grace | Dropped |
| Public library API and extension points, doctor nonce probes, prompt-size diagnostics | Internal library, plain `doctor`, `tools check` |
| Default tools package (text/files/image readers, desktop/project openers, factories, author helpers, `view` CLI) | Built-in Agent tools `read`, `list`, `glob`, `grep`, `view_image`, available when an Artifact declares them; Human tools are declared `launch`/`output` commands |
| Observation receipts and required-observation checks | Dropped |
| CCDD's restricted JSON Schema subset and diagnostic budgets | `jsonschema` validation with a size limit |
| Web monitor (Kanban, worker overlays, current-input inspection, multi-project registry, Human review UI) | `artifactize monitor` TUI (ratatui): live Run progress, Artifact/eval tree with status and dependencies, request details; Human review stays in the CLI |
| Separate `status`/`plan` commands, compact vs `--full` projections, `history`, `run cancel` | `status` shows state and what `verify` would do; one output level (text, or full `--json`); `run list`; Ctrl-C |
| Cache GC protected readers and scratch retirement | LRU cap on entries and bytes |

CCDD 7.0 has no remote snapshot-transfer review (inventory HUM-20); none is added.

## Fixed technical decisions

- Linux/WSL only.
- Cargo workspace:
  - `crates/artifactize`: library plus the `artifactize` CLI binary, including the `monitor` TUI.
- Crates: tokio, rusqlite (bundled, WAL), serde/serde_json, jsonschema, rmcp (stdio), process-wrap, petgraph, clap, thiserror, tracing, `rig-core = "=0.43.0"`; ratatui/crossterm/tui-tree-widget for the monitor. Add a crate only when a task needs it.
- rig relaxations accepted:
  - Duplicate tool-call IDs fail closed.
  - Session ids are per request.
  - Anthropic wire `is_error` is dropped; the local audit flag is kept.
  - Model strings are explicit, with no catalog.
  - There is never a model or account fallback.
- An identity is the owner script's literal bounded string. Nothing else ever becomes part of a reuse key.
- Never hold a SQLite writer transaction across a subprocess, a filesystem deletion, or an `.await`.
- Background reading: [LLM providers](research/research-llm-providers.md), [core stack](research/research-core-stack.md), [subscription feasibility](research/research-subscription-feasibility.md). When they disagree with this plan, this plan wins.

## Architecture

The module boundaries are fixed; later tasks add code inside them. The CLI and the monitor call the same library operations.

| Module | Owns |
|---|---|
| `config`, `config::families` | Inert discovery, strict declarations, families/parameters/variants |
| `scope` | Mounts, aliases, `{artifact}` references, scoped paths, traversal/symlink rejection |
| `graph` | Edges, SCCs, external gates, final obligations |
| `project`, `project::selection` | Selection, selection files, profile variants, `verify` orchestration, `status` |
| `workspace` | Supplied-workspace rules, external state/output checks, execution-path resolution |
| `process`, `runtime` | Literal argv, sanitized env, private HOME/TMP/output, bounded pipes, process groups, gated launch |
| `broker` | Run lifecycle, scheduling with `--jobs`, cancellation, budget counters |
| `cache` | Identity commands, end-of-review recheck, reuse, cross-process claim, LRU GC |
| `store`, `query` | The state database, read projections, plain prune |
| `tools`, `tools::builtin`, `mcp` | Agent tool registry (declared commands with the `json` or `plain` protocol, built-ins), JSON Schema arguments, text/JSON/image results, Human `launch`/`output` tools, stdio MCP |
| `agent`, `llm`, `auth` | Agent loop, strict verdict and one repair, the four backends, ChatGPT login, transient retries, usage |
| `human` | Waiting state, claim lock, claimant tools, submission |
| `diagnostics` | `doctor`, `tools check` |
| `monitor` | ratatui TUI over read-only queries |
| `cli` | Commands, flags, output and exit codes |

**Execution.**
- `verify` runs in the foreground. It runs READY evals up to `--jobs N` (default 4), re-evaluates the graph after each result, and exits when nothing more can run.
- Ctrl-C or SIGTERM kills the running process groups and records ERROR (cancelled).
- There is no daemon and no detached worker.
- Human evals are recorded as WAITING_HUMAN. `request submit` completes them, and the next `verify` runs their dependents. `verify --wait` keeps waiting for pending Human results.

**Tools.**
- Agent tools run under the runtime isolation policy: PATH/LANG only, private HOME/TMP/output. They are read-only by contract and scoped to the eval's Artifacts.
- Declared Agent commands use one of two protocols:
  - `json`: JSON context and arguments on stdin; ToolResult content blocks on stdout.
  - `plain`: arguments are substituted into argv placeholders such as `{query}`, and stdout becomes a text result. A nonzero exit is a tool error.
- Human tools take no free arguments; their only placeholders are scope values such as `{artifactPath}`. They run in the reviewer's real environment (HOME, DISPLAY/WAYLAND, config).
  - `launch` opens a program (e.g. `code {artifactPath}`) and records only that it launched.
  - `output` runs a command and shows its output to the reviewer.

**Identity cache.**
- A process claims an identity by inserting the single active execution row, holding its pid and start time.
- Other processes poll and take the published result.
- If the owner process is dead, its row becomes ERROR and the next caller claims the identity again.
- Only GREEN and RED are published. Hits return the original result and profile.
- Status prepares current identities for the required closure, then reads completed entries without executing evals or updating saved evidence. Graph and config checks remain static.

**Input changes.** When a review completes (runtime or Agent exit, Human submission), artifactize re-runs the Artifact's identity command. If the output differs from the identity computed at preparation, the review becomes ERROR and nothing is published. A review without an identity function gets no input-change check.

**State.**
- There is one SQLite database, `state.sqlite`, in `$ARTIFACTIZE_STATE_HOME` (default `~/.local/state/artifactize`). It holds Runs, requests, executions, cache entries and Human claims.
- Run output directories live next to it.
- `--state-dir` moves the whole state.
- State, credential and output paths inside the reviewed repository are rejected.
- Cached results are self-contained rows, readable after the repository is gone.

**LLM backends.** There is one adapter with four concrete backends.
- **ChatGPT:**
  - `login chatgpt` performs dynamic registration and PKCE on `127.0.0.1`, keeping the issued client id and host id.
  - It validates state, nonce, granted scope and the ID token.
  - Refresh is serialized and written atomically to 0600 files.
  - Inference uses rig's Responses client at `api.openai.com/v1` with `store:false` and streaming. Incomplete responses are rejected.
- **Claude:** the unmodified CLI is launched with:

  ```
  claude -p --output-format stream-json --verbose --model X --effort Y --tools "" --strict-mcp-config --mcp-config CFG --permission-mode dontAsk --setting-sources "" --allowedTools 'mcp__artifactize__*' --no-session-persistence
  ```

  - Model switching and fallback are disabled (verify the setting names against `claude --help`), and provider/effort override env vars are stripped.
  - artifactize never reads Claude credentials.
  - The repair turn is a second invocation with an empty strict MCP config.
- A transient provider failure is retried at most twice, and only before any output or tool call. Auth and quota failures are ERROR with the provider's message.

## Phases

| Phase | Runnable exit |
|---|---|
| P1 Runtime loop | `verify --all` runs discovery → graph → evals → verdicts; `run show` reads them in a new process |
| P2 Project | Verify a cyclic family selection with profile variants; `status` explains it; saved Runs stay readable without the repo |
| P3 Identity cache | Concurrent verifies of one identity execute once; an input change during review is ERROR; `--jobs` and budgets hold |
| P4 Tools | Scoped text/image tools run through the script protocol and over MCP |
| P5 Agents | Real OpenAI-key, Anthropic-key, ChatGPT and Claude reviews use tools, strict verdicts and repair |
| P6 Human | Claim → tools → submit completes a waiting request; the next `verify` continues |
| P7 Monitor | `artifactize monitor` shows live Run progress and the Artifact/eval tree |
| P8 Finish | `doctor`, prune, remaining CLI, examples, install docs; every non-DROP inventory item checked off |

## Tasks

Each task is one PR from `task/<id>-<slug>`. That PR ticks its box here and the inventory items it completes. Done means its check passes and `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

### P1 Runtime loop
- [x] **P1.1** Package skeleton. Check: `artifactize --version` prints 0.1.0.
- [x] **P1.2** Inert declarations, discovery, unique IDs, basis vs UNREVIEWED, `config check`. Check: duplicates rejected; no hook executed.
- [x] **P1.3** Children, mounts, aliases, `{artifact}` references in instructions and argv. Check: alias/traversal/symlink tests.
- [x] **P1.4** SCC scheduling, external gates, verdict propagation. Check: cycle, external-gate, RED-blocks and ERROR-waits tests.
- [x] **P1.5** Generic runtime execution with gated process-group launch. Check: exit 0/nonzero → GREEN/RED; spawn/signal/timeout → ERROR.
- [x] **P1.6** Private environments, deadlines, bounded output, descendant cleanup. Check: secrets are not inherited; a timeout kills grandchildren.
- [x] **P1.7** Receipts and the end-to-end runtime `verify` plus `run show`. Check: `verify`, then `run show` from a fresh process.

### P2 Project
- [x] **P2.1** Families. Check: the family fixture validates expansion and isolation.
- [x] **P2.2** Selectors, selection files, profile variants. Check: JSON and line files select identical ordered sets.
- [x] **P2.3** Recursion, force, `--ignore-gates`, final obligations. Check: selected success with missing evidence stays INCOMPLETE.
- [x] **L.1** Lean cleanup:
  - Remove the `stale` `file-hash`/`always` kinds and `stale.paths`; `stale` accepts only `identity`.
  - Remove the family entry digest and obsolete readiness, result-check, observation and admission config.
  - Delete skeleton modules that left the architecture.
  - Rename CCDD’s Critic to eval in config (`evals`), CLI (`--eval`, `--evals`, `--evals-file`), types, output and docs.
  - Drop `plan`, `--full` and `history` where they already exist.
  - Move receipts to the single `state.sqlite` with `--state-dir` moving the whole state.

  Check: existing tests pass on the new layout; removed fields are rejected.
- [x] **P2.4** `status` (state plus what `verify` would do: reuse/execute/wait/blocked), `graph` and config output, one output level with `--json`. Check: graph/config queries never run owner code; status prepares current identities without running evals or tools.
- [ ] **P2.5** Persisted definitions and family membership; `run list`. Check: saved results stay readable after the repository is removed.

### P3 Identity cache
- [x] **P3.1** Identity commands (`stale: {kind: identity, script, inputs?, timeoutMs?}`; the unused `weight` field is removed) with exact output validation, and the end-of-review recheck. Check: malformed output fails preparation with no fallback; editing input during a review makes it ERROR and unpublished.
- [x] **P3.2** Reuse of completed GREEN/RED results across repositories; no-identity and `--force` bypass. Check: cross-repo RED reuse; force leaves the entry unchanged.
- [x] **P3.3** Cross-process claim, polling waiters, dead-owner reclaim. Check: two processes execute one identity once; a killed owner is reclaimed.
- [x] **P3.4** `--jobs N` scheduling and `maxExecutions` counter. Check: concurrency never exceeds N; the budget stops new starts.
- [ ] **P3.5** `cache list/show/rm` and LRU GC on entries and bytes. Check: GC never removes an active execution.

### P4 Tools
- [x] **P4.1** Agent tool declarations and registry:
  - Declared commands with the `json` and `plain` protocols, and executable and declared execution-path resolution.
  - `jsonschema` argument validation with a size limit.
  - Text/JSON results and authored errors.

  Check: invalid arguments never spawn; malformed output is a tool error.
- [ ] **P4.2** Image results (PNG/JPEG/WebP, size cap) through the `json` protocol and the built-in `view_image` (moved from P4.3). Check: wrong signatures, escaping paths and oversized images fail.
- [x] **P4.3** Built-in Agent tools `read` (line ranges), `list`, `glob`, `grep`; `view_image` moved to P4.2 with image results. They are read-only, scoped to the eval's Artifacts, and available only when declared. Check: scope/symlink escapes are rejected; UTF-8/CRLF/BOM and paging fixtures; bounded glob/grep and binary skipping.
- [ ] **P4.4** Human tools: predefined `launch`/`output` commands with scope placeholders only, run in the reviewer's real environment. Check: `launch` returns once the program starts; `output` captures stdout.
- [ ] **P4.5** `tools check [--execute]` and the stdio MCP server serving Agent tools, with call audit. Check: MCP text/image calls obey scope and audit.

### P5 Agents
- [ ] **P5.1** rig OpenAI/Anthropic API-key backends, explicit profiles, multimodal turns, transient retries, per-attempt usage. Check: fake transport asserts model, effort and payloads.
- [ ] **P5.2** `maxToolCalls`, token and deadline gates; duplicate-ID failure. Check: crossing a budget blocks further tools.
- [ ] **P5.3** Strict verdict schemas, owner fields, one tools-disabled repair. Check: fenced JSON is rejected; a second failure is ERROR.
- [x] **P5.4** ChatGPT login: registration, PKCE, protected storage, serialized refresh. Check: callback-validation and refresh-race tests; a real `login chatgpt` (owner).
- [ ] **P5.5** ChatGPT subscription inference via rig Responses. Check: incomplete responses are rejected; a real review (owner).
- [ ] **P5.6** Claude backend: launch controls, stream-json parsing, MCP gated by assistant turn, separate repair invocation. Check: a fake CLI asserts flags and env; a real review (owner).

### P6 Human
- [ ] **P6.1** WAITING_HUMAN state, claim lock, claimant-only tools, schema-valid submission with the identity recheck, dependents continuing on the next `verify`. Check: a wrong claimant, changed input or duplicate submission fails.
- [ ] **P6.2** `request list/show/claim/tool/submit` and `verify --wait` for Human results. Check: the CLI completes a waiting request end to end.

### P7 Monitor (`artifactize monitor`, ratatui)
- [ ] **P7.1** TUI shell over read-only state queries with periodic refresh, and the Run list. Check: saved and running Runs appear and update without touching the repository.
- [ ] **P7.2** Run progress: counts per state, running evals, durations, errors. Check: a running `verify` is reflected live.
- [ ] **P7.3** Artifact/eval tree with status, dependency and cycle markers, and family grouping. Check: every Artifact and instance is reachable.
- [ ] **P7.4** Request detail pane: result, provenance, tool calls, usage, errors. Check: a historical request shows its saved result.

### P8 Finish
- [ ] **P8.1** `doctor` (backends, auth presence, `claude` binary), `models <backend>` and plain `prune` of finished Run output. Check: doctor creates no Run; prune never touches active Runs or the repository.
- [ ] **P8.2** Remaining CLI flags and exit codes. Check: wait codes 0/1/2/3/4 cover success/RED/error/timeout/incomplete.
- [ ] **P8.3** Three example projects with docs:
  - runtime evals with children, mounts and references;
  - an Agent eval using built-in and declared tools;
  - a family.

  Check: each example runs from the installed binary.
- [ ] **P8.4** Install docs; close the inventory. Check: a clean install completes runtime, Agent and Human workflows and the monitor; no unchecked non-DROP item remains.

## Data model

One SQLite database, `user_version = 1`, plain tables.

| Table | Contents |
|---|---|
| `runs` | id, repo, selection, policy, status, timestamps |
| `requests` | run, artifact, eval, status, execution, requested profile |
| `executions` | identity (nullable), owner pid and start time, status, verdict, result JSON, error, actual profile, usage JSON, timestamps |
| `tool_calls` | execution, order, tool, arguments, result summary, error |
| `cache_entries` | identity → completed execution, bytes, last used |
| `human_claims` | request, reviewer, claimed at |

At most one execution per non-null identity is active at a time.

Core types:
- Project: `Artifact`, `FamilyInstance`, `Eval::{Runtime, Agent, Human}`, `Profile`, `Scope`.
- Execution: `Run`, `Request`, `Execution`, `Identity`.
- Evidence: `ToolCall`, `Usage`, `Outcome::{Completed(ReviewResult), OperationalError}`.

## Risks

1. **Subscription protocol drift.** Prove ChatGPT login/refresh and the Claude turn/MCP ordering early in P5, with one real review per backend run by the owner. Fail closed on incomplete output, a wrong model or a budget violation.
2. **Duplicate or stuck identity execution.** One active row per identity, pid/start-time liveness, no lease timers; two-process tests.
3. **Path or process escape.** One scope resolver, gated group launch, adversarial path/link/descendant fixtures. Configured commands are trusted programs, not a sandbox.
4. **Hidden feature loss.** Every PR ticks the inventory items it delivers; anything not delivered is either assigned or explicitly DROP.

## Working rules

- Read CCDD sources with `git -C ~/code/ccdd show cbf28b4:<path>`. That checkout's working tree is an older branch.
- Write tests only where behavior is subtle, plus a CLI check per phase. No test matrices, benchmarks or parity suites.
- Add no abstraction for a second implementation that does not exist yet.
- Docs, comments and messages are in English.
