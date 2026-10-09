# Fingerprints and reuse

A fingerprint says what an Artifact's reviews depend on. An eval's earlier result
is reused when the eval is unchanged and the fingerprints of the Artifacts it
depends on are unchanged, so review cost follows the size of a change.

## Fingerprints

Every Artifact uses **artifactsum** by default. Omit `fingerprint` to hash its own
files, or choose a script. A fingerprint adds no tool declarations or other
Artifact's value; dependencies enter through the [reuse key](#the-reuse-key).

- Omission, or `fingerprint = {}`, uses artifactsum: `artifactsum:` plus SHA-256
  over the Artifact's own input paths and bytes. A folder hashes its own files;
  a file Artifact hashes its one target file.
- `fingerprint = { script = { command = "hash", args = [] } }` uses the command's
  output as the fingerprint. It hashes nothing on its own, so the command must
  reflect every input and script change that should re-review.
- `fingerprint = false` disables reuse. `fingerprint = true` is rejected.

## Artifactsum

An Artifact is reviewed again when its own files change or when an Artifact its
eval depends on changes its fingerprint. To narrow a folder's inputs:

```toml
fingerprint = { files = ["src"], ignore = ["*.log"] }
```

- `files`: 1–64 unique owner-relative paths, default `["."]` (the owner's own
  files). Each must exist and stay inside the Artifact: paths into child Artifacts
  or mounts are rejected. Directory walks skip child Artifact folders and every
  `*.artf` declaration. Even explicitly named declaration files are excluded.
  A neighboring file Artifact's target remains in the folder's hash; it creates
  no automatic child relation.
- `ignore`: up to 64 `.gitignore`-style globs relative to the owner, without
  negation. They always exclude, like the built-in ignores `.git`,
  `__pycache__/`, `*.pyc`, `target/` and `node_modules/`. `.gitignore` files
  exclude more, with git semantics: every one from the repository root (`--repo`)
  down through the Artifact and walked directories applies. A pattern is relative
  to its own file's folder; negation works, and deeper files override shallower
  ones. Explicitly named non-declaration `files` are never ignored.

The old `dependencies` fingerprint option is rejected: a fingerprint covers only its own
Artifact. Walk limits and manifests are in the [reference](../reference/declarations.md#artifactsum); the script form is under
[fingerprint scripts](../reference/declarations.md#fingerprint-scripts).

## The reuse key

An eval depends on its target Artifact and on what the target directly connects:

- the target's mounts;
- the target's child Artifacts;
- the Artifacts the eval names in its instruction or runtime args.

artifactize does not follow connections further than that. Whether a change two
connections away matters is up to how you define fingerprints: a fingerprint
script can read a file of any Artifact it names in its args. The key is

```text
hash(Eval definition hash, sorted (Artifact name, fingerprint) of each Artifact the eval depends on)
```

Artifact names are part of the key, so two Artifacts never share a result, even
when their fingerprints are equal. The **Eval definition hash** is lowercase
SHA-256 of canonical JSON with recursively sorted keys over the eval strategy:

- the eval kind (runtime, agent or human);
- `payload`, including the instruction;
- `pass_schema` and `fail_schema`;
- for runtime evals, the command and args.

Execution options are not part of the key: an Agent's backend, model, reasoning,
`timeout_ms`, `max_tool_calls` and `max_tokens`, a runtime `timeout_ms`, and the selected
profile variant. Neither are the eval id and title, tags, repository paths, unused
profile variants, or tool declarations (`views`): a tool is a way of viewing an
Artifact, so changing only a tool's description or schema does not review again. To
make a tool change matter, include the relevant files in the fingerprint.

An executable eval has a key only when its target and every Artifact it depends on
have a fingerprint. Omission supplies artifactsum, including for basis Artifacts. If
any declares `fingerprint = false`, the eval has no reuse: every `verify` reviews it again,
and `status` names the disabled target or dependency.

Dependency evals have no reuse key. Their verdicts are derived from current required
evidence on every `status` or Run, never read from cache or published to the team
store. Their required executable evals still reuse in the usual way.

## Completed result reuse

A successful end-of-review recheck adds the GREEN or RED result to its key's
history: its self-contained `executions` row gets its completion time, size and last
use ([State](../reference/state-cache-limits.md#state)). Errors and cancellation are
never recorded there. When a key holds more than one record,
the most recent by completion time is reused, GREEN or RED. Results from different
profiles therefore reuse each other: a review that `--profile fast` produced
satisfies the declared profile, and the other way around.

Each record keeps how and by whom it was produced, next to its result. These are
JSON/state field names, not snake_case declaration keys:

- `options`: the backend, model, reasoning, `timeoutMs`, `maxToolCalls`,
  `maxTokens` and `variant` it ran with, as declared;
- `profile`: the effective profile;
- `producer` (`user@host` and artifactize version), the Human `reviewer`, and for
  a remote record its `origin` with the publisher;
- `completedAt`, `fingerprints` (each Artifact the key covers) and `key`.

A hit returns the original result without running the eval or re-validating it
against the requested profile or schema. The request saves the original execution
ID, the producing `profile` and `options`, `evalDefHash`, `key`, `provenance`
(repository, Run, request, eval, definition hash, completion time and, for Agent
results, the [pins of their tools' execution paths](../reference/agent-tools.md)) and the original attempts as `reusedUsage` when reported,
alongside its own `requestedProfile`. Its own `usage` is null: a reused request spent
nothing. Its `source`, `{"runId", "requestId", "kind"}`, names the request whose execution produced the
result and whether it was a completed record (`cache`), a live execution the
request waited for (`joined`) or a remote store record (`remote`). Dependency
requests instead use `kind: "derived"`, with their own Run/request IDs; they are not
reuse. `source` is null on a request that executed itself ([Runs](runs-and-status.md#verify-and-runs)). Runtime
usage is null, not an invented zero. A reused RED remains RED for gates and final
obligations. Dependencies outside execution selection can supply cached evidence
without running. Results remain readable after the source repository is deleted;
external paths embedded in result text are not made portable.

With a [team review store](../reference/review-store.md#remote-review-store-client),
`verify` also asks the store for every key and reuses whichever record completed
last, the local latest or the store's latest; a store that cannot be reached leaves
the local latest.

No key means no cache lookup or publication. `--force` re-executes explicitly
selected executable evals without reading or joining earlier results, local or in
the store, and its completed GREEN or RED is appended as a newer record (and
published to a configured store), which later runs then reuse. Forced and unkeyed
results still satisfy their own Run and keep their execution.

## Upgrading to 0.9

0.9 uses TOML `.artf` declarations: `index.artf` for folders and `<file>.artf` for
files. Declaration keys become snake_case and evals become `[evals.<id>]` tables. The
ignore file becomes `.artfignore`. JSON output, state fields and JSON Schema keywords
do not change spelling. There is no converter; have an agent rewrite and commit the
declarations, then run `artifactize config check`.

Families are removed. Projects that want templates generate `.artf` declarations
with their own generator and commit them.

State schema 6 starts a new state; earlier schemas are refused, not migrated. Set
`ARTIFACTIZE_STATE_HOME` or `--state-dir` to a new directory, or move the old state away. Reuse-key
v2 matches no earlier result, including a script fingerprint's result and records in
a team review store. The first full `verify` reviews every executable eval once,
Human sign-offs included. Dependency evals have no execution; they derive their
current verdicts. Later Runs reuse normally. See [State](../reference/state-cache-limits.md#state).
