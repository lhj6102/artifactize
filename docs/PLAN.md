# artifactize plan: full CCDD 7.0 port

artifactize is a Rust port and rebrand of [CCDD](https://github.com/lhj6102/ccdd) 7.0.0 (`cbf28b4`). It covers [ccdd#101](https://github.com/lhj6102/ccdd/issues/101). CCDD keeps living as its own program.

**Target: every CCDD 7.0 capability works in artifactize.** [`ccdd-7-inventory.md`](ccdd-7-inventory.md) lists all 334 items and is the parity checklist. Version 0.1.0 ships only when every non-DROP item is checked off. The phases below are runnable integration checkpoints on the way there, not release cut lines.

"Lean" means no process overhead: no parity or performance suites, no decision-record ceremony, no speculative abstractions. It does not mean fewer features. Issue #101's migration process (Node sidecar, TS/Rust contract suites, performance evidence, cutover review) does not apply.

## Agreed drops and replacements

These are the only exclusions. Each is marked DROP or REPLACED in the inventory.

| CCDD 7.0 | artifactize |
|---|---|
| Node-only runtime (`node --test`), Node import hooks, offline monkey-patch guard | Generic `{command, args, timeoutMs}` for runtime Critics and every hook; no guard |
| `load-check` benchmark, `prepare-demo` and demo tooling | Dropped |
| npm packages and umbrella facade | Cargo-installed binaries |
| Pi catalog and auth adapters, Pi re-exports | rig-core `=0.43.0` (OpenAI/Anthropic API keys), ChatGPT via official Sign in with ChatGPT, Claude via the unmodified official `claude` CLI |
| Vue/HTTP web monitor | Native desktop app `artifactize-desktop` (iced) with the same capabilities |
| CCDD config/state/cache compatibility | New format only: `artifactize.json`, fresh state and hashes |

CCDD 7.0 has no remote snapshot-transfer review (inventory HUM-20). Its local cross-process review and shared-execution forwarding are ported as they are; no remote transport is added.

## Fixed technical decisions

- Linux/WSL only.
- Cargo workspace:
  - `crates/artifactize`: library plus the `artifactize` CLI binary.
  - `crates/artifactize-desktop`: the iced monitor app, calling the same library.
- Crates: tokio; rusqlite (bundled, WAL) via tokio-rusqlite; serde/serde_json; jsonschema; rmcp (stdio); process-wrap; sha2 + serde_json_canonicalizer (JCS); petgraph; clap; thiserror; tracing; `rig-core = "=0.43.0"`. Add a crate only when a task needs it.
- rig relaxations accepted:
  - Duplicate tool-call IDs fail closed.
  - Session id is per request.
  - Anthropic wire `is_error` is dropped; the local audit flag is kept.
  - Model strings are explicit, with no catalog.
  - There is never a model or account fallback.
- SHA-256 over JCS authenticates stored objects and fingerprints. It never augments a reuse identity. An identity is the owner script's literal bounded string.
- Never hold a SQLite writer transaction across a subprocess, a filesystem deletion, or an `.await`.
- Background reading: [LLM providers](research/research-llm-providers.md), [core stack](research/research-core-stack.md), [subscription feasibility](research/research-subscription-feasibility.md). When they disagree with this plan, this plan wins.

## Final architecture

The module boundaries below are permanent; later phases add code inside them rather than restructuring. The CLI and the desktop app call the same library operations. Read-only observation uses read-only query connections. Explicit actions (inspect, claim, tool, submit) enter the owning module.

| Module | Owns | CCDD 7.0 source |
|---|---|---|
| `config`, `config::families` | Inert discovery, strict declarations/defaults, families/parameters/variants | `definitions.ts`, `contracts.ts`, `project/types.ts` |
| `scope` | Mounts, aliases, `{artifact}` references, canonical scoped paths, traversal/symlink rejection | `artifact-scope.ts`, `artifacts/scope.ts`, `execution-scope.ts` |
| `graph` | Edges, SCCs, shared external gates, RED blocking vs operational waiting, final obligations | `project/graph.ts`, `project/gates.ts`, `broker/graph.ts` |
| `project`, `project::selection`, `project::prepare` | Selection, selection files, profile variants, status/plan explanations, prepared handles, input manifests | `project/*`, `requester/` |
| `workspace` | Capture, content/metadata modes, watcher with scan fallback, mutation latch, runtime pinning | `workspaces/`, `runtime-paths.ts` |
| `process`, `runtime` | Literal argv, sanitized env, private HOME/TMP/output, bounded pipes, process groups, gated launch | `executors/process.ts`, `tools/environment.ts` |
| `broker`, `worker` | Run lifecycle, indexed gate propagation, detached `__worker`, ownership fencing, cancel/resume/drain | `broker/*`, `worker*.ts` |
| `resources`, `resources::budgets` | Weighted identity FIFO, provider/model/runtime pools, caps, leases, child tracking, `maxExecutions` ledger | `resources.ts`, `broker/admission.ts` |
| `cache`, `cache::compute`, `cache::gc` | Identity reuse, cross-process claim/join, subscriptions, owner handoff, capacity GC | `cache/*`, `broker/cache-execution.ts`, `project/owner-identity.ts` |
| `store`, `query`, `audit` | SQL schema, canonical objects, receipts, projections, audit and change cursors, prune | `broker/storage.ts`, `project/store.ts`, `result-view.ts`, `provenance.ts`, `project/prune.ts` |
| `tools`, `tools::defaults`, `mcp` | Registries, manifests, argument schemas, script protocol, images, default tools, view CLI, stdio MCP | `tools/*`, `artifacts/*`, `packages/default-tools` |
| `agent`, `llm`, `auth`, `provider_control` | Agent loop, strict verdicts and one repair, resultCheck, rig/ChatGPT/Claude backends, ChatGPT login, recovery and account lanes, usage | `executors/*`, `response-schema.ts`, `review-result.ts` |
| `human` | Waiting, alarms/inbox, two-phase claims, preparation, claimant tools/submission, forwarding | `review/*`, `executors/human-preparation.ts`, `broker/human-claims.ts` |
| `diagnostics` | doctor, tools check, prompt-size diagnostics | `doctor/*`, `diagnostics-cli.ts`, `artifacts/tool-check.ts` |
| `cli` | Every command, flag, projection and exit code | `project/cli.ts`, `cache/cli.ts`, `cli.ts` |

**Execution model.**
- There is no permanent daemon. As in CCDD, `artifactize __worker RUN_ID` outlives the submitting CLI. It keeps workspace observation through Human waiting and drains shared computation after its own Run ends.
- Competing workers are fenced by SQLite owner tokens plus pid/start-time identity.
- Children are registered before execution starts. Leases stay held until their process groups are gone.

**State.**
- Global state lives in `$ARTIFACTIZE_STATE_HOME` (default `~/.local/state/artifactize`): computation, cache, admission and default receipts.
- `--state-dir` moves Run receipts and history only; it never partitions global services.
- State, credential and output paths inside reviewed input are rejected.

**Results.**
- A result is published globally first, then copied idempotently (result, provenance, audit) into each receipt store before settlement is acknowledged. No transaction spans two databases.
- Cached results stay self-contained after the original repository or receipt store is gone.

**LLM backends.** There is one adapter with four concrete backends.
- **ChatGPT:**
  - `login chatgpt` performs dynamic registration and PKCE on `127.0.0.1`, keeping the issued client id and host id.
  - It validates state, nonce, granted scope and the ID token.
  - Refresh is serialized and written atomically to 0600 files.
  - Inference uses rig's Responses client at `api.openai.com/v1` with `store:false` and streaming, and rejects incomplete responses.
- **Claude:** the unmodified CLI is launched with:

  ```
  claude -p --output-format stream-json --verbose --model X --effort Y --tools "" --strict-mcp-config --mcp-config CFG --permission-mode dontAsk --setting-sources "" --allowedTools 'mcp__artifactize__*' --no-session-persistence
  ```

  - Model switching and fallback are disabled (verify setting names against `claude --help`), and provider/effort override env vars are stripped.
  - artifactize never reads Claude credentials.
  - The repair turn is a second invocation with an empty strict MCP config.
  - MCP tool calls wait for the matching assistant turn's model/usage before running. `maxToolCalls` counts issued calls.

## Phases

| Phase | Runnable exit |
|---|---|
| P1 Runtime loop | `artifactize verify --repo fixtures/runtime` runs discovery → graph → Critics → verdicts; `run show` reads them in a new process |
| P2 Complete project and input | Verify a cyclic, parameterized family selection; mutating input yields ERROR |
| P3 Durable shared execution | Concurrent verifies share one execution; detached runs, cancel/resume, budgets, admission and cache GC work |
| P4 Complete tool host | `artifactize view` reads text and images; `tools check --execute` exercises scoped scripts; MCP serves them |
| P5 Complete Agent execution | Real OpenAI-key, Anthropic-key, ChatGPT and Claude reviews use tools, strict validation, repair and provider control |
| P6 Human review | CLI claim → prepare → tools → submit completes an asynchronously waiting Run |
| P7 Desktop monitor | `artifactize-desktop` observes projects and completes local Human reviews |
| P8 Complete product | Management, diagnostics, streams, public APIs, install docs; **every inventory item checked off** |

## Tasks

Each task takes a worker about half a day to a day. It lands as one PR from `task/<id>-<slug>`. That PR ticks its box here and the inventory items it completes. A task is done when its check passes and `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

### P1 Runtime loop
- [x] **P1.1** Package skeleton: module tree from the architecture table, versioned config/state, clap CLI, `--version`. Check: `artifactize --version` prints 0.1.0.
- [x] **P1.2** Inert declarations, discovery, unique IDs, basis vs UNREVIEWED, `config check`. Check: duplicates rejected; no hook executed.
- [x] **P1.3** Children, mounts, aliases, `{artifact}` references in instructions and argv. Check: focused alias/traversal/symlink tests.
- [ ] **P1.4** SCC scheduling, shared external gates, verdict propagation. Check: focused cycle, external-gate, RED-blocks and ERROR-waits tests.
- [x] **P1.5** Generic runtime execution with gated process-group launch. Check: exit 0/nonzero → GREEN/RED; spawn/signal/timeout → ERROR.
- [x] **P1.6** Private environments, deadlines, bounded output, descendant cleanup. Check: secrets are not inherited; a timeout kills grandchildren.
- [ ] **P1.7** Receipts store and the end-to-end runtime `verify` plus `run show`. Check: `verify`, then `run show` from a fresh process.

### P2 Complete project and input
- [ ] **P2.1** Families: inline/file instances, material, parameter precedence, variants, `$param` pointers, per-instance identities and Critics. Check: the family fixture validates expansion and isolation.
- [ ] **P2.2** Selectors (artifact, critic, family, multiple, `--all`), bounded JSON/line selection files, profile variants. Check: JSON and line files select identical ordered sets.
- [ ] **P2.3** Recursion, force, gate policy (`--ignore-gates`), final obligations. Check: selected success with missing evidence stays INCOMPLETE.
- [ ] **P2.4** Canonical scoped and family fingerprints, mandatory execution inputs. Check: sibling material cannot change another instance's fingerprint.
- [ ] **P2.5** Workspace capture, watcher, metadata mode, boundary scans, mutation latch. Check: edit+restore, unsafe links and metadata changes fail as specified.
- [ ] **P2.6** Current inspection, `status`/`plan` explanations (REUSE/COALESCE/EXECUTE/WAIT/BLOCKED/FAILED), full/compact `graph`/config projections. Check: static queries never run owner code.
- [ ] **P2.7** Persist definitions, family membership and compact/full historical references. Check: saved results stay readable after the repository is removed.

### P3 Durable shared execution
- [ ] **P3.1** Broker lifecycle, indexed gate propagation, same-input ERROR retry, lightweight revision-only idle polling for Human/follower waits. Check: retry keeps completed results and the original requested profile.
- [ ] **P3.2** Detached `__worker`, saved settings, startup handshake, ownership recovery. Check: killing the submitter does not end its Run.
- [ ] **P3.3** Owner identity scripts with exact output validation. Check: malformed output fails preparation with no fallback.
- [ ] **P3.4** Global completed reuse, provenance, no-identity and force bypass. Check: cross-repo RED reuse; force leaves the entry unchanged.
- [ ] **P3.5** Transactional claim/join and stale-owner publication fencing. Check: two processes execute one identity once.
- [ ] **P3.6** Subscriber cancellation, owner handoff, shared draining. Check: the surviving subscriber completes after the initiating Run is cancelled.
- [ ] **P3.7** Capacity/FIFO admission and `resources.json` policy rereads. Check: stricter caps hold; reductions keep active work.
- [ ] **P3.8** Weighted identity FIFO and custom admission constraints. Check: a light preparation cannot overtake a heavy head.
- [ ] **P3.9** Budget reservations, start fencing, refunds (`maxExecutions`). Check: crash, pre-start and post-start cases keep execution counts exact.
- [ ] **P3.10** Releasing-child accounting and process-owner recovery. Check: a dead leader with a live descendant still holds capacity.
- [ ] **P3.11** Bounded GC (1 GiB / 10,000 entries / 16 MiB each), protected readers/subscribers, scratch retirement. Check: an oversized result reaches waiters without being retained.
- [ ] **P3.12** `cache show/list/compare/gc/delete`. Check: reads of a missing cache create nothing; deleting an active entry fails.
- [ ] **P3.13** Canonical objects, atomic publication, settlement, audit and change storage. Check: replaying settlement creates no duplicate changes or audit.
- [ ] **P3.14** Opaque prepared handles and submission revalidation. Check: forged or disposed handles are rejected; identity never reruns.
- [ ] **P3.15** `run show --wait`, `run resume [--wait]`, `run cancel`, and bounded shutdown. Check: a wait timeout leaves execution running.

### P4 Complete tool host
- [ ] **P4.1** Explicit tool registries and reconnectable scoped manifests. Check: a reconnect cannot widen scope.
- [ ] **P4.2** Bounded argument schemas and value-free diagnostics. Check: invalid arguments never spawn commands.
- [ ] **P4.3** Script protocol (JSON stdin, ToolResult stdout), text/JSON results, authored domain errors. Check: malformed output never becomes observation evidence.
- [ ] **P4.4** Native images and output references. Check: wrong signatures, escaping paths and oversized images fail.
- [ ] **P4.5** Default text reader, paged listings, family/mount navigation. Check: UTF-8 line and pagination boundary fixtures.
- [ ] **P4.6** Runtime file pinning and execution-path rebinding. Check: changed executable bytes or bits invalidate execution.
- [ ] **P4.7** Image, desktop and project-app default tools; author helpers; `view` CLI. Check: an explicit viewer handoff survives launching-tool cleanup.
- [ ] **P4.8** Readiness hooks, preflight, `tools check --execute`. Check: only explicit Human preparation runs environment hooks.
- [ ] **P4.9** rmcp stdio server and mandatory observation audit. Check: MCP text/image calls obey scope and audit rules.

### P5 Complete Agent execution
- [ ] **P5.1** rig OpenAI/Anthropic API-key backends, exact profiles, multimodal turns. Check: fake transport asserts model, effort and payloads.
- [ ] **P5.2** Issued-call, token and deadline gates; duplicate-ID failure. Check: crossing the token budget blocks that turn's tools.
- [ ] **P5.3** Strict verdict schemas, owner fields, observation requirements. Check: fenced JSON and listing-only evidence are rejected.
- [ ] **P5.4** resultCheck hooks and one shared tools-disabled repair. Check: a second failure ends in ERROR; a check failure cannot repair.
- [ ] **P5.5** ChatGPT registration, PKCE, protected credential storage. Check: callback-validation tests; a real `login chatgpt` (owner).
- [ ] **P5.6** Serialized refresh and Responses subscription inference. Check: refresh-race test; incomplete responses are rejected.
- [ ] **P5.7** Claude launch controls and stream-json parsing. Check: a fake CLI asserts flags and env; a real subscribed run succeeds (owner).
- [ ] **P5.8** Claude MCP gated by assistant turn; separate repair invocation. Check: reordered events cannot deadlock; repair has no tools; parent death releases waits.
- [ ] **P5.9** Bounded recovery, shared account lanes, `provider status/resume`. Check: cooldown is shared, with no replay and no auto-restart.
- [ ] **P5.10** Attempt usage independent of event windows, bounded optional telemetry. Check: missing usage stays unreported; a telemetry failure cannot change verdicts.

### P6 Human review
- [ ] **P6.1** Human waiting, alarm registration and delivery, JSONL inbox. Check: a failed alarm gives ERROR; waiting survives the caller's exit.
- [ ] **P6.2** Two-phase claim: reservation, renewal, expiry, release, confirmation. Check: stale or competing attempts cannot confirm or release successors.
- [ ] **P6.3** Preparation phases, receipts, scan progress, cancellation, retry diagnostics. Check: a failed preparation stays waiting and reclaimable.
- [ ] **P6.4** Claimant-only tools, exact-once schema-valid submission, shared-owner forwarding. Check: a wrong claimant, changed input or duplicate submission fails.
- [ ] **P6.5** `request list/show/claim/tool/submit/summary` CLI and desktop review handoff. Check: the CLI completes a waiting Run end to end.

### P7 Desktop monitor (`crates/artifactize-desktop`)
- [ ] **P7.1** Read-only monitor projections and registered project sources; bounded inspection/tool action concurrency with per-request mutation serialization. Check: pagination and filters run no scripts and no reconciliation.
- [ ] **P7.2** iced shell, project/Run selectors, Kanban lanes and counts, live freshness (auto/manual refresh, timestamps, stale-fetch cancel, last-known data on error), persistent local reviewer identity. Check: saved and active Runs across projects are shown.
- [ ] **P7.3** Graph and family navigation (containment/mount/instruction/cycle), layout, pan/zoom/fit. Check: every instance is reachable through bounded paging.
- [ ] **P7.4** Request details, references, results, provenance, timeline, worker overlays, open-by-request-id. Check: a historical request shows results without live actions.
- [ ] **P7.5** Explicit current-input inspection. Check: changed input shows current obligations beside saved evidence.
- [ ] **P7.6** Human forms (schema and JSON fallback), progress, text/JSON/image/launch renderers, submission; claimant isolation and no arbitrary workspace access; disconnected actions cancelled with actionable errors. Check: claim → view → verdict works natively; closing the window leaves the Run alive.

### P8 Complete product
- [ ] **P8.1** Cursor and NDJSON streams with resume and backpressure. Check: telemetry keeps the cursor; stopping the reader keeps execution.
- [ ] **P8.2** `history` selection, attempt summaries, cross-state comparisons, `run diff`. Check: removed families remain queryable; reuse is not double-counted.
- [ ] **P8.3** Ephemeral `doctor` with private provider nonce probes. Check: doctor creates no Run, verdict or alarm.
- [ ] **P8.4** Static and executing tools checks, prompt-size diagnostics. Check: static checks make no provider call or launch.
- [ ] **P8.5** Safe explicit `prune` with quarantine and crash recovery. Check: symlink replacement cannot redirect deletion; audit survives.
- [ ] **P8.6** Public Rust library API and existing extension points (admission, alarms). Check: a tiny library client prepares, submits and streams results.
- [ ] **P8.7** Every remaining CLI command, flag, projection and exit code. Check: wait codes 0/1/2/3/4 cover success/RED/error/timeout/incomplete.
- [ ] **P8.8** Port the CCDD examples as artifactize fixtures and docs (folders, mounts, computed views, families, default tools, custom reader). Check: each example runs from the installed binary.
- [ ] **P8.9** Install docs for both binaries; close the inventory. Check: a clean install completes runtime, Agent and desktop Human workflows; no unchecked non-DROP item remains.

## Final data model

SQLite `user_version = 1`. Ordinary tables plus canonical immutable objects.

| Store | Tables |
|---|---|
| Receipts (per state dir) | `objects(hash, kind, jcs)`, `runs`, `requests`, `run_members`, `gate_edges`; `attempt_audit`, `tool_call_audit`, `events`, `changes(cursor, …)`, `prune_claims` |
| Global computation | `executions`, `subscriptions`, `cache_entries(identity, result, bytes, last_used)`, `retirements` |
| Global admission | `submission_budgets`, `execution_attempts`, `resource_leases`, `resource_children`, `provider_lanes` |
| Human | `human_attempts` (reviewer, attempt, expiry, preparation progress/receipts) |

There is at most one active execution per identity, and one settlement per subscription generation.

Core types:
- Project and input: `Artifact`, `FamilyInstance`, `Critic::{Runtime, Agent, Human}`, `Profile`, `Scope`, `PreparedProject`, `InputManifest`.
- Execution: `Run`, `Request`, `Execution`, `Subscription`, `Identity`, `OwnerToken`, `ClaimAttempt`.
- Evidence: `ToolCall`, `Observation`, `Provenance`, `Usage`, `Outcome::{Completed(ReviewResult), OperationalError}`.

## Risks

1. **Subscription protocol drift.** Prove ChatGPT registration/refresh and Claude turn/MCP ordering early in P5. Keep tiny fake-transport tests plus one real review per backend run by the owner. Fail closed on incomplete output, a wrong model or a budget violation.
2. **Crashes during shared execution.** Use token/generation fencing, a durable start ledger and idempotent settlement. Focused two-process cancel/death tests cover the dangerous transitions.
3. **Workspace or process escape.** One scope resolver, watcher plus scans, pinned inputs, gated group launch, and adversarial path/link/descendant fixtures. Configured commands are trusted programs, not a sandbox.
4. **Human and desktop state divergence.** One authoritative claim lifecycle, read-only observation and explicit actions. Exercise expiry and window close against a live worker.
5. **Hidden feature loss.** Every PR ticks the inventory items it delivers. Release happens only at P8 with no unchecked non-DROP item.

## Working rules

- Read CCDD sources with `git -C ~/code/ccdd show cbf28b4:<path>`. That checkout's working tree is an older branch.
- Write tests only where behavior is subtle, plus a CLI check per phase. No test matrices, benchmarks or parity suites.
- Add no abstraction for a second implementation that does not exist yet. Traits exist only at CCDD's own extension points.
- Docs, comments and messages are in English.
