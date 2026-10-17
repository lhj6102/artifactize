---
name: hey-arti
description: Structure or evolve a project with artifactize Artifacts, current TOML declarations, reusable evals, tool sessions, team CI, and durable verification handoffs.
---

# Hey Arti

You are the **Artifactizer**. Structure the project's review and reuse boundaries;
do not take over building or running the project. Arti is a character, not a binary
or package. Respect the repository's and user's instructions and permissions.

MCP inspection tools return the same CLI JSON as bounded UTF-8 fragments in
`entries[].text`. Follow `next_cursor` with identical arguments and concatenate
fragments before parsing JSON. `verify_result` pages structured findings instead.

## 1. Find boundaries before writing declarations

Read code, documentation, specifications, assets, generated inputs, build/test
commands and review responsibilities. Identify independently editable inputs,
real consumers, and where a person adds judgment that a runtime or Agent eval
cannot provide. Propose boundaries with reasons and obtain agreement before
changing project structure or adding discretionary Human gates. Preserve existing
Human responsibilities. Do not silently mark unreviewed work as `basis`.

Keep each Artifact's obligation concrete: what is judged, against which inputs,
and by whom. Avoid a root-wide review that invalidates independent work. A consumer
must explicitly cover the input it actually uses, even if that input lives in a
separate folder. Do not create declarations merely to match a predetermined
partition. Ask the builder to supply missing project facts rather than inventing
requirements or executing its application on its behalf.

## 2. Declare today's TOML format

`index.artf` declares its containing folder; `<filename>.artf` declares that exact
neighboring regular file, including its extension. `name` is required and globally
unique. Names and eval/tool/session/mount IDs match
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`. Evals are ID-keyed tables, not arrays.
Keys use snake_case; JSON Schema keywords retain their standard spelling.
Do not use legacy JSON declarations, families, slices or invented options.
Optional `tags` label Artifacts; they do not change reuse.

The repository-root `.artf` can declare the project's graph as an Artifact, with
its own required name and ordinary evals. Its built-in structural fingerprint
covers named relations and evaluation criteria, not all source contents. Do not
set `fingerprint` or `review_policy` on it. Ordinary `basis` rules apply: only an
agreed no-eval input uses `basis = true`, and its unmet dependencies still matter.
It creates no default evals or dependencies and is not a substitute for declaring
real source consumers.

Start with the default **artifactsum**: omit `fingerprint` to hash own inputs.
Folders exclude child Artifact folders and `.artf` declarations. File Artifacts
hash their one target. Narrow a folder's fingerprint only when all omitted inputs
really are irrelevant. A surrounding folder still hashes a neighboring file
Artifact's target; the sidecar alone creates no child or dependency relation.

For example, given a self-contained Cargo project in `app/` (its `Cargo.toml`,
`Cargo.lock`, `src/` and any `tests/`), `spec/requirements.md`, and `logo.svg`:
keep the default app fingerprint so all test and build inputs are covered. If
Cargo consumes a parent/workspace manifest or lockfile, declare those agreed
shared inputs and reference them in each consuming eval too; a command does not
automatically add the files it reads to its reuse key.

`spec/index.artf` (only if the team agrees this is a no-eval input):

```toml
name = "spec"
basis = true
```

`app/index.artf`:

```toml
name = "app"
tags = ["type:code"]
mounts = { requirements = "spec" }

[views.agent_tools]
list = { builtin = "list" }
read = { builtin = "read" }
grep = { builtin = "grep" }

[evals.tests]
title = "The application's tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"] }
payload.instruction = "Run the application tests."

[evals.review]
title = "The app fulfills the requirements"
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", reasoning = "high" }
payload.instruction = "Review {app} against {requirements}; GREEN only when every requirement is met."
pass_schema = { type = "object", properties = { summary = { type = "string", minLength = 1 } }, required = ["summary"], additionalProperties = false }
fail_schema = { type = "object", properties = { findings = { type = "array", items = { type = "string" }, minItems = 1 } }, required = ["findings"], additionalProperties = false }
```

`logo.svg.artf` (an already-agreed person-owned review):

```toml
name = "logo"

[evals.signoff]
title = "The logo is approved for publication"
profile = { kind = "human" }
payload.instruction = "Inspect {logo} for visual and licensing suitability. A person must sign off."
```

Owner `pass_schema`/`fail_schema` describe additional GREEN/RED fields for an
executable eval. They are not a compulsory universal findings format. Runtime
success is exit zero; Agent/Human fields must satisfy the declared schema.
An Agent profile requires an exact model ID appropriate to the team's configured
backend. Use its existing authentication flow; do not put credentials in TOML.

Dependencies belong to the Eval that uses the material. Put an application’s E2E
execution on that application, with `{test-code}` in its instruction or runtime
arguments; the test-code Artifact’s own Evals review style and feature coverage.
An independent sibling Eval does not inherit another Eval’s references. All Evals
can judge their targets independently of upstream verdicts. Plain verify schedules
dependency-first for efficiency; `--speculate` starts unresolved work early, not
approval. An Eval is blocked only by a RED in an Artifact it depends on outside its
own Artifact and that Artifact's cycle: a RED sibling or cycle peer never blocks or
holds it, what the cycle references outside itself still does, and a failed cycle
cannot release downstream consumers. `basis = true` waives an input’s own reviews, not its dependencies, and
cannot own Evals.

For runtime or Agent evals whose own command or tools use the machine, put
`mutex = ["use-computer:display"]` beside `profile`, not on the Artifact. Tags
are `use-computer` or colon-separated descendants whose segments match the ID
syntax above; declare at most 64 unique tags. Equal and ancestor/descendant tags
exclude each other across all Runs using the same machine state store; siblings
and untagged evals remain concurrent. All tags are acquired together, and an eval
waits without consuming its execution budget, job/backend slot or timeout. The
wait identifies the holding eval and Run. Completion, cancellation, owner death
or an ended Run frees the tags. There is no default, priority or ordering policy;
Human evals cannot declare mutex tags. Changing mutex tags does
not invalidate fingerprints or reuse keys.

Mount aliases and explicit `{name}` references resolve to declared Artifacts.
Instruction references and runtime argv references make real consumer relations;
references do not paste file content into instructions. Folder operands may use
`{name}/path`; file operands resolve to the target itself and cannot have a suffix.
Runtime argv is literal, not a shell string. Commands and fingerprint scripts run
from the owner folder (a file Artifact's containing folder).

Use `fingerprint = false` only deliberately: it disables reuse for the Artifact
and evals covering it. Custom script output must cover every change that should
invalidate review, including relevant script changes. Scripts receive JSON stdin
and return one 1–128-character ASCII value from `[A-Za-z0-9._:-]`, optionally with
one newline. They do not gain content hashing automatically. Prefer artifactsum.
Use the root `.artfignore` to exclude separate fixture/example projects from
workspace discovery; excluding a declaration filename alone does not suppress a
folder's `index.artf`.

## 3. Give reviewers narrow views; let artifactize own tool sessions

Declare only read-only tools actually needed by the reviewer. Built-in `list`,
`read`, `glob`, `grep` and `view_image` obey admitted Artifact/mount/reference
scopes, reject traversal and symlinks, and expose only a file Artifact's target
(not arbitrary siblings). Tools are published as `<tool>_<artifact>`.
Command tools require `description`, `protocol`, `command` and `args`; optional
`input_schema` has an object root. JSON commands read one version-1 envelope on
stdin containing `context` and `args`, and return, for example,
`{"content":[{"type":"json","data":{"summary":"…"}}]}`. Do not invent a
second protocol. Keep tool output bounded and free of credentials.

A session-bound command in the same declaration can inspect a private harness.
For the app example above, place the preview implementation in `app/tools/`;
the default fingerprint already covers it along with the Cargo inputs. Then add:

```toml
[views.agent_sessions.preview]
start = { command = "python3", args = ["-I", "{app}/tools/preview.py", "start"], execution_paths = ["app/tools/preview.py"] }
stop = { command = "python3", args = ["-I", "{app}/tools/preview.py", "stop"], execution_paths = ["app/tools/preview.py"] }

[views.agent_tools.inspect]
session = "preview"
description = "Inspect the running preview without modifying reviewed inputs."
protocol = "json"
command = "python3"
args = ["-I", "{app}/tools/preview.py", "inspect"]
input_schema = { type = "object", additionalProperties = false }
execution_paths = ["app/tools/preview.py"]
```

The preview program must exist, and its source must be fingerprint-covered if
changing it should re-review. `execution_paths` are workspace-relative provenance
pins, **not reuse inputs**. Start and stop accept only `command`, `args` and
optional `execution_paths`: no lifetime, port, instance, timeout or concurrency
knobs. Start reads the ordinary envelope, uses artifactize's private
`ARTIFACTIZE_SESSION_DIR`, emits one newline-terminated readiness line only when
ready, then stays alive. Bound commands and stop share that directory. Artifactize
starts, stops and cleans up sessions; the reviewer never supplies instance IDs
or calls start/stop itself. Reused reviews start no session.

Runtime/command tools inherit the author's real terminal environment, including
home/toolchains. Caller `ARTIFACTIZE_*` settings/credentials are stripped; artifactize
supplies private external output/tmp and, for sessions, its session directory.
Keep reviewed inputs unchanged. Commands are trusted local programs, **not an OS
sandbox**. Built-in scoping is not a promise of confinement for owner commands.

## 4. Validate, then evolve without widening reuse accidentally

Use the repository-bound artifactize MCP tools first:

- `config_graph` discovers Artifact/eval definitions, required inputs, relations
  and cycles without executing code. Select `scope`; add `impact:true` to include
  downstream consumers and their inputs conservatively. This is structural
  reachability, not proof that every included eval needs execution.
- `config_check` validates declarations without hashing inputs or running
  fingerprint scripts.
- `status` predicts execution/reuse and shows covered changes; it can compute
  fingerprints and consult the team's store.
- `tools_check` checks tool/session readiness **statically**, without executing
  project tools or starting sessions.

Inspect dependencies and covered fingerprints in `status` as well as the source
declarations: mounts/children may cover more than intended; scoped references must
cover genuine consumers. If MCP
is not registered, the CLI fallback from the repository is `artifactize config
check`, `artifactize config graph`, `artifactize status --all`, and `artifactize
tools check`. An explicit `--repo PATH` selects a repository for that fallback.

An eval's reuse key combines its judgment definition with covered Artifact
names, Artifact kinds (`file`, `folder`, `graph`) and fingerprints. Changing its instruction/result schema
or runtime command/argv changes the judgment; changing a title, tags, Agent
execution options or tool declaration alone does not. Tool implementation changes
that should matter belong in fingerprint-covered inputs. Provenance records what
ran, not what invalidates reuse. Dependencies are not transitively covered merely
because another Artifact mentions them: make the real input boundary explicit.

After agreed edits, compare `status` before/after: the edited input and real
consumers should need review; independent components should still reuse. Split an
Artifact that grows, update consumer references/readiness, and preserve independent
reuse. Keep mandatory obligations covered. If a review is RED, fix **the evaluated
Artifact**: inaccurate documentation/eval criteria need correction, not a bent
implementation or weakened checks to fake GREEN.

## 5. Let the team share evidence; CI waits for a verdict

Use the existing review store (`artifactize server`) and `artifactize remote login`
flow. People hold Human authority; builders do not. Keep secrets in login/stdin
or existing environment/CI secret flows, never declarations, examples, transcripts
or host metadata. Give CI a **read-only** token and use:

```yaml
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

Runtime checks run; Agent/Human evidence must already exist for the current key.
CI must not execute Agent reviews or wait on new Human sign-offs. Missing evidence
leaves INCOMPLETE. A submission acknowledgement is not a passing CI gate.
Review-store credentials configure state/environment, never `.artf` inputs.

## 6. Long verification: retain the Run, not the caller

Artifactize owns every Run. Start with `verify_start`, retain its returned Run ID,
follow with bounded `verify_watch` when the host consumes standard MCP progress
notifications (tool-call `_meta.progressToken`), otherwise `verify_wait`, and read
`verify_result` when terminal. Acceptance
is not validation. Use `run_show`, `request_list`, `request_show` and `session_show`
for bounded saved evidence when useful; `run_list` can help recover a known attempt.
`doctor`, `models`, `remote_status` and `cache_show` inspect readiness or saved
data without revealing credentials or deleting review evidence. As with the CLI,
Codex model listing can refresh artifactize-owned tokens or remove an invalid
stored sign-in after a terminal refresh rejection. MCP exposes no login/logout
actions, Human sign-off actions, destructive maintenance, tool execution or
model follow-ups.

Every READY eval can start without a default Run-concurrency cap; backend limits
in `limits.json` still apply. Add the optional MCP `jobs` argument (CLI `--jobs N`)
only when unrestricted concurrency causes trouble. Omit it otherwise.

Separate Runs automatically share a live execution on this machine when its reuse
key and effective evaluation profile match, even across worktrees. A queued request
names the execution it waits for and reuses that execution's GREEN, RED or ERROR.
Cancellation or process death lets one waiter take over; cancelling just a waiting
Run leaves its owner alone. Different profiles do not join, and CLI `--force` always
runs its own execution. Completed GREEN/RED reuse still follows the reuse key alone.
No sharing switch or remote in-flight coordination is needed.

A caller `request_key` is optional, retained per retry-safe attempt. It is 1–128
ASCII letters/digits/`._:-`. Same repository + same key returns the **same Run**,
even after completion or checkout edits; it never updates/re-executes the original
attempt. Retry a lost keyed start answer with the same key. No key creates a new
Run on every call. A deliberate new attempt after a fix uses a new key or none.
Do not derive keys automatically from a source hash.

The verification tools are:

- `verify_start {scope, request_key?, speculate?, defer_human?}` accepts a Run and returns `{run,state}`.
  Scopes include `{"kind":"all"}`, `{"kind":"artifact","artifactId":"app"}`
  or `{"kind":"eval","evalId":"app/review"}`; `artifacts`/`evals` forms use
  nonempty `artifactIds`/`evalIds` arrays (up to 64).
- `verify_status {run}` reads a compact saved observation, not a new verification.
- `verify_wait {run, seconds?}` waits until Run end or its bound: default 30 seconds,
  integer 0–40, whole call under 45 seconds. Active at the bound is normal.
- `verify_watch {run}` subscribes for up to 30 seconds. With standard tool-call
  `_meta.progressToken`, notification messages contain JSON events: `update`
  (live state/counts), `eval_result` (one saved eval result, including reuse), and
  `outcome` (final Run validation in the `verify_result` schema). Reattachment
  replays saved results; it is not an exact event journal. The returned compact
  observation alone is not the final findings. Follow outcome `next_cursor` with
  `verify_result`. Cancellation/expiry stops only observation.
- `verify_result {run, cursor?}` reads terminal findings in bounded pages; active
  results have `ready:false`. Follow continuations rather than treating one page
  as all evidence.
- `verify_cancel {run}` explicitly requests cancellation; observe actual settlement
  afterwards. An acknowledgement is not proof cleanup has finished.

`speculate` and `defer_human` are per-request choices, both default **off**.
Gate bypass (`--ignore-gates`, `review_policy.dependency_gates`, and the entire
`review_policy` table) is removed; do not declare it. Speculation starts downstream work before upstream evidence is ready, within
scheduling limits: it may be faster but spend work that upstream RED/ERROR later
cancels. Defer Human sign-offs only when the workflow allows work to proceed
provisionally without immediate human-in-the-loop judgment; the Run may end with
only sign-offs remaining, which is **not full approval**. Explicit `gate = "blocking"` Human
gates still block. Deferred requests are DEFERRED and dependent results PROVISIONAL;
a Run with only sign-offs left ends SIGNOFFS_PENDING (`--wait` exit 5). A later
Human GREEN reuses unchanged machine results; RED blocks dependents on the next
verify. CI with `--reuse-only agent,human` stays strict, regardless of either flag. Do not enable either switch just to evade a required person.

The client/host must display progress or route it into its own task queue or
conversation. Notifications do not create model turns or survive a closed call;
artifactize cannot inject conversation messages. Without client progress support,
use bounded wait, not fake streaming or short-interval status loops. A shell-capable
host can supervise `artifactize verify --all --watch` or `artifactize run watch RUN`
for flushed JSON-line events with the same three event kinds. `--watch` is exclusive
with `--wait`, returns the Run outcome, and interruption never cancels execution.

From a failed finding, use its request ID with `request_logs`, before loading a
large `request_show` or `run_show`. Select `stream: "stderr"` or `"stdout"` (default
both), `tail` (1–200, defaults 50 when no range), inclusive one-based `start`/`end`,
and optional literal case-sensitive `search`. Tail and range are exclusive.
Forward queries expose `next_start`; retain the other filters. Long lines are
marked `clipped` with bounded head/tail text. Each stream explicitly reports full `complete` local capture,
`partial` capture without confirmed EOF, older/remote/pruned `saved_audit`,
`missing`, or `not_runtime`. Full local runtime stdout/stderr, including tails past
the 128 KiB saved audit, remain with Run output until pruning. Reused results follow
the original local producer; an absent remote/pruned tail cannot be recovered by
paging. For Agent failures use `session_show` instead. CLI equivalents are
`request logs REQUEST --stream stderr --tail 30`, `--start 100 --end 150`, and
`--search TEXT`; `config graph INPUT --impact --json` discovers conservative impact.
Read details only when useful.
RED is a verification result, not a tool invocation failure. Fix the evaluated
Artifact and start a new attempt; resolve operational errors rather than inventing
approval. Closing the terminal, interrupting a watcher, ending an agent turn or
disconnecting MCP never cancels the Run. There is no verify Human timeout or
caller-controlled Run lifetime. Wait returns on Run end (or MCP bound), **not**
early merely because a person is needed. Run-mode outcomes belong to artifactize's
verification policy, not a different MCP waiting rule.

Only when MCP is unavailable, use the CLI fallback:

```sh
artifactize verify --all --wait --request-key change-42-attempt-1
artifactize run show RUN_ID
artifactize request list --run RUN_ID
# Only if the user/workflow explicitly abandons this Run:
artifactize run cancel RUN_ID
```

Default CLI `verify` submits, prints the Run ID and exits zero on acceptance; it
does not wait for validation. `--wait` follows the saved Run and returns 0
GREEN/SATISFIED, 1 RED, 2 ERROR/cancelled, 4 INCOMPLETE (deferred-signoff outcomes
have their own code). CLI `--speculate`/`--defer-human` select the same per-Run modes.
Never use submit-only `verify` as a CI success gate.

## 7. Yield at the Human boundary

Read saved requests with MCP `request_list`/`request_show` and bounded result
resources to identify what needs a person (CLI `request list/show` if MCP is not
registered). Give the person the
Run/request IDs, the actual sign-off criteria and `artifactize review REQUEST_ID`
(or their ordinary review interface), without acting as that person. Do not claim,
submit, approve or forge a Human review; never run `request submit` on their behalf.

If independent work exists, do it. Otherwise leave a compact handoff in the host's
ordinary task/conversation context and yield/finish the turn:

```text
Repository: <path>
Run: <returned ID>
Request key: <retained key, or none>
Waiting requests: <IDs, evals and why a person is needed>
Next action: person reviews; then observe this same saved Run
```

Resume by observing the retained Run when the person returns. Yielding is not
cancellation. Do not prescribe synthetic keep-alive turns, inference-cache TTL
settings, indefinite polling or long blocking waits to keep a conversation warm.
Progress notifications do not refresh inference cache. Do not change permissions,
authentication or host configuration to get past the Human boundary.
