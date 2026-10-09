# Declarations (.artf)

The exact rules for TOML declarations, folder and file Artifacts, fingerprints,
dependency evals, scoped references and the runtime execution environment.
[Artifacts and evals](../concepts/artifacts-and-evals.md) introduces them.

## Declaration validation

`index.artf` declares its containing folder. `<filename>.artf` declares the
neighboring file whose complete name is `<filename>`, including its extension.
Both use standard TOML and require `name`.

Discovery walks from the repository root and skips `.git`, `node_modules`, and
paths listed in an optional root `.artfignore` (gitignore syntax), including
individual sidecar files. An ignored sidecar is not parsed or target-checked; its
target remains an ordinary file unless another declaration includes it. List folders
that hold separate projects, fixtures or examples:

```gitignore
crates/app/tests/
examples/
```

Names, eval IDs, mount aliases, tool names and profile variant names match
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`. Artifact names are globally unique. Evals are
ID-keyed tables, not an array; `[evals.check]` on `app` becomes `app/check`.
An explicit eval `id` field is rejected. Evals within each declaration are ordered
by ID, not by their position in the TOML source.

| Field | Rule |
|---|---|
| `name` | Required Artifact name |
| `tags` | Optional list of at most 64 unique nonblank strings, without ASCII controls |
| `mounts` | Optional alias-to-Artifact-name map |
| `basis` | Optional boolean; `true` declares a no-eval input, waiving its own review obligations but not its dependencies |
| `fingerprint` | Omission uses artifactsum; `false` disables reuse; otherwise an artifactsum or script table |
| `views.agent_tools`, `views.human_tools` | Optional maps of declared tools |
| `review_policy.dependency_gates` | `green` (default) or `ignore`; only in the repository root's `index.artf` |
| `evals.<id>.title` | Required nonblank title |
| `evals.<id>.profile` | Required `runtime`, `agent`, `human` or `dependency` profile |
| `evals.<id>.payload.instruction` | Required nonblank instruction for runtime, Agent and Human evals |
| `evals.<id>.pass_schema`, `fail_schema` | Optional owner-field JSON Schemas for executable evals |
| `evals.<id>.profile_variants` | Up to 64 named complete profiles retaining the default kind; not allowed for dependency evals |

All declaration keys are snake_case: `timeout_ms`, `max_tool_calls`, `max_tokens`,
`input_schema`, `execution_paths`, `pass_schema`, `fail_schema`,
`profile_variants`, `review_policy`, `dependency_gates` and `depends_on`.
Unknown fields and camelCase spellings at these typed boundaries are rejected.
JSON Schema keywords such as `additionalProperties`, `minLength` and `maxItems`
keep their standard names. Owner-defined payload fields keep their chosen names.
`--json`, tool protocols and stored state keep their existing camelCase fields,
such as `timeoutMs`, `passSchema`, `agentTools` and `blockedBy`; they are not
declaration syntax.

Inline tables, dotted keys and multiline strings are standard TOML:

```toml
name = "app"
tags = ["type:code"]

[evals.review]
title = "The implementation matches the spec"
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", reasoning = "high", timeout_ms = 240000 }

[evals.review.payload]
instruction = """
Read {spec}, then review {app}.
Return GREEN when the implementation matches the spec.
"""

[evals.review.pass_schema]
type = "object"
required = ["summary"]
additionalProperties = false
properties.summary = { type = "string", minLength = 1 }
```

TOML has no null value. Omit optional fields; do not use `null`. A schema may say
`type = "null"`, but literal null values such as a null `const` cannot be written.
Unquoted TOML dates/times and non-finite numbers (`inf`, `nan`) are rejected
anywhere, including payloads and schemas, because they have no JSON equivalent.
Quote dates to use them as strings. Declaration errors name the `.artf` file,
line and column when parsing or validating their contents, including syntax,
deserialization, semantic validation and cross-Artifact reference errors. Field validation and unsupported-value errors
also name the typed key path, such as `evals.ready.profile.depends_on`.

`config check` validates without executing fingerprint scripts or hashing files.
Use it after rewriting declarations. Legacy declaration and ignore files are not
read. A non-ignored legacy declaration file fails discovery. A directory with the
legacy declaration filename is walked normally, unless a directory ignore pattern
excludes it. `.artfignore` can ignore a legacy file individually or skip its whole
folder. The old ignore file at the repository root always fails. The diagnostics
are:

```text
artifactize.json is no longer read; declare this Artifact in index.artf (TOML).
.artifactizeignore was renamed to .artfignore.
```

There is no converter. Have an agent rewrite the declarations and commit the new
`.artf` files. Families are removed: generate any repeated declarations with your
own project generator and commit its `.artf` output. Older fields such as
`staleKey`, `stale`, `critics`, `envRequirements`, `resultCheck`,
`reviewPolicy.maxConcurrentExecutors` and tool metadata `observation` are rejected;
they are not aliases for the new fields. The artifactsum form's old `dependencies`
option is also rejected.

## File Artifacts

`hero.png.artf` declares the regular file `hero.png` in the same folder. Its
`name` is required and independent of any surrounding folder Artifact. Missing
targets, directories, symlinks and special files are not valid targets.
Target filenames containing `:` are rejected at discovery because scoped tool
paths cannot represent them.
The `.artf` declaration itself must also be a regular file: symlinks, special files
and directories such as `bundle.artf` fail discovery. A file named `index` cannot have a sidecar because `index.artf` always declares the folder;
`.artf` files are declarations and are never targets.

A file Artifact creates no child or dependency relation to the containing folder
Artifact. The nearest-folder ownership rule still applies to subfolder
`index.artf` declarations. A folder's artifactsum includes neighboring file
Artifact targets and excludes every `*.artf` declaration.

Runtime commands, tools and fingerprint scripts use the containing folder as cwd.
`{name}` resolves to the target file path, not the folder; file references cannot
have a `/path` suffix. Built-in tools expose only the target file, mounts and
referenced Artifacts, not unrelated siblings. `list .` names the virtual file
Artifact root; it lists the target filename and mounts. Paths within that root
remain relative to the containing folder.
Mount aliases must not shadow the target filename, but may shadow unrelated
sibling entries: those siblings are not visible in the file Artifact's virtual
root. Ordinary global-name ambiguity checks still apply. Default artifactsum covers
only the target file. The containing folder is a working directory, not an
implicit grant of built-in-tool access to its other files. A workspace can have
only file Artifacts; it does not need a folder declaration.

Omit `fingerprint` to hash the file. An explicit artifactsum form must be
`fingerprint = { files = ["hero.png"] }` for `hero.png.artf`: only the target
filename is allowed. `fingerprint = {}` and `files = ["."]` are rejected, as is
`fingerprint.ignore`, even an empty list. Script fingerprints and
`fingerprint = false` remain available.

JSON command-tool `context.artifactPath` names the file, while the process cwd is
its containing folder. Scope entries carry `kind = "file"` or `"folder"`.
`execution_paths` stay workspace-relative provenance pins for both kinds.

The target must remain an existing regular file with no symlink traversal, not
just at discovery. It is validated at fingerprint preparation/recheck, before
reuse, at eval start/end and on every Agent/Human tool call, including with
`fingerprint = false`. A file invalid before eval admission produces
ERROR/PREPARATION_FAILED; a target that becomes invalid during an eval produces
ERROR/INPUT_CHANGED instead of a semantic verdict. Preparation failures abort
before reuse or execution; tool calls return an error without running the tool.

## Dependency evals

```toml
name = "player"
mounts = { art = "hero-art" }

[evals.ready]
title = "Movement and art are ready"
profile = { kind = "dependency", depends_on = ["player-movement", "art"] }
```

`depends_on` is a list of 1–64 unique Artifact names or owner mount aliases. Names
resolve as instruction references do. Unknown names, the declaring Artifact itself
and duplicate resolved targets are rejected. Dependency evals cannot declare
`payload`, `pass_schema`, `fail_schema` or `profile_variants`, even empty ones.
Dependency-eval wait cycles fail configuration validation; ordinary Artifact SCCs
retain their existing behavior.

A dependency eval checks each listed Artifact's required evals. GREEN evidence
for every requirement produces GREEN. RED or BLOCKED evidence produces BLOCKED;
missing, stale, ERROR, cancelled or waiting Human evidence produces
WAIT_DEPENDENCY. A target without evals, including a basis Artifact, fulfills the
dependency condition. Basis Artifacts cannot own evals. Ordinary final Run
obligations, including upstream dependency scopes, are unchanged.

The verdict is derived from current evidence, not executed or cached. There is no
command, provider call, execution row, capacity slot, reuse key, cache record,
team-store lookup or publication for the dependency eval itself. A dependency-only
Run prepares no fingerprints solely for derivation, including script fingerprints;
included runtime/Agent/Human reviews keep their normal fingerprint preparation.

Selecting `verify player/ready` includes the dependency requirements' evals without
`--recursive`. `--force` never forces the derived eval. `--reuse-only dependency`
still derives it; `--reuse-only agent,human` constrains its requirements in the
usual way. `--profile NAME` skips dependency evals and still validates the named
variant on included executable evals; an explicit per-eval library profile
selection for a dependency eval is rejected. `--ignore-gates` and
`review_policy.dependency_gates = "ignore"` do not override its verdict.

`blocked_by` is an ordered flat list: each unfulfilled listed Artifact, followed
by its non-GREEN qualified eval IDs. JSON requests, validation and status expose
it as `blockedBy`; text status and monitor details show the same blockers. Requests
have `source.kind = "derived"`, and Run summaries count them separately as
`summary.derived`. Later evidence changes do not rewrite a historical request;
a subsequent `status` or `verify` derives the new current verdict.

## Artifactsum

[Fingerprints and reuse](../concepts/fingerprints-and-reuse.md#artifactsum)
describes `files` and `ignore`. Omitting `fingerprint` uses artifactsum. For a
folder, `fingerprint = {}` does the same. A file Artifact's explicit artifactsum
form must list its target filename. `fingerprint = false` disables reuse;
`fingerprint = true` is rejected.

The value is `artifactsum:` plus the SHA-256 over each input file's owner-relative
path and the SHA-256 of its bytes, in path order. It covers only the Artifact's own
files: a dependency's change reaches an eval through the dependency's fingerprint
in the [reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key). Folder walks
exclude child Artifact folders and every `*.artf` file. Explicitly listing a
`.artf` file in `fingerprint.files` fails `config check`, even if the file does not
exist. Discovery also rejects non-regular `.artf` entries, including directories;
a directory named `bundle.artf` is not an ordinary hashable input. File Artifacts
hash only their target by default. Tags are not hashed and do not change the Eval definition hash or
reuse key.

Walks follow scoped path rules. Symlinks and special files fail closed unless they
are ignored, nothing is followed out of the owner, and a walk stops with an error
after 10,000 entries or 1 GiB. Hashing runs on preparation and end-of-review
rechecks. Files a review writes into ignored paths, such as Python's `__pycache__`,
therefore never cause `INPUT_CHANGED`. Artifactsum records a manifest with the
execution: per-file digests (16 hex digits) and the inputs digest. A file map that
would exceed 64 KiB is dropped from the manifest but still covered by the
fingerprint. `status` diffs this manifest against the current one, and the key's
recorded `fingerprints` against current dependency fingerprints.

## Fingerprint scripts

Before executing evals, `verify` computes fingerprints in their selected required
dependency closure, including dependencies whose evals are not selected. Derived
evals alone need no fingerprint preparation. Up to `--fingerprint-jobs N`
fingerprints are computed at once (default: the CPUs available to the process),
in `status` as in `verify`; the same bound covers end-of-review rechecks.
Values are keyed by Artifact, not completion order. A failure reports the first
failing Artifact in name order and cancels fingerprints after it. Scripts run in
separate private output directories; a script that must run alone needs
`--fingerprint-jobs 1`.

Every executable eval on an Artifact receives the same literal value, saved on
its request and in the Run's Artifact validation. artifactize adds no repository,
eval, profile, tool-view, dependency or content salt to script output. The eval's
[reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key) combines it with the
Artifact names and kinds (`folder`/`file`), dependency fingerprints and Eval
definition hash. Physical paths never enter the reuse key. Artifactsum
failures abort preparation in the same way.

The command runs from its owner's working directory with JSON on stdin:
`{"version":1,"artifactId":"example"}`. Bare commands resolve through PATH;
absolute executables run as given. Relative executables containing `/` (such as
`./fingerprint.sh`) must resolve inside the admitted scope without symlinks.
Arguments stay literal except explicit scoped Artifact references: `{name}` may
be a mount alias or global Artifact name. Each referenced Artifact, with its child
and mount closure, enters the script's scope without becoming a graph relation.
`config check` rejects unknown and malformed references; existence and symlinks
are checked when the command is prepared. There are no interpreter-specific
flags, wrappers or entry-file rules.

`fingerprint.script.files` accepts up to 64 unique owner-relative literal
file/directory paths. They must exist without symlink traversal on every call;
their contents are never hashed. Commands share runtime isolation, cancellation,
bounded raw output and a 30,000 ms default timeout (`timeout_ms` accepts
1–2,147,483,647). Private output, HOME and temporary directories are removed after
each invocation. Stdout must be exactly 1–128 ASCII characters from
`[A-Za-z0-9._:-]`, optionally followed by one LF, or one CRLF on Windows. No other
whitespace is allowed; the value is not trimmed. Nonzero exit, malformed output,
timeout, cancellation, missing files or cleanup failure abort preparation with an
operational error, without starting any eval or falling back to uncached review.
Stderr is not forwarded as a fingerprint diagnostic.

After runtime or Agent review, covered fingerprints are recomputed before
accepting GREEN or RED. A changed value records ERROR/INPUT_CHANGED with no
semantic result; a failed recheck records ERROR. Force skips neither preparation
nor recheck. An Artifact with `fingerprint = false` has no fingerprint to prepare;
unkeyed evals skip the fingerprint-value recheck. File-target validation remains
mandatory at preparation, eval start/end and tool calls, including for unkeyed
evals. Other required Artifacts can still have fingerprints prepared. There is no workspace monitoring: artifactsum hashes
only at preparation and recheck.

## Scoped input library

`config::read_workspace_config` (also used by `config check`) resolves nearest
child ownership, validates mounts and explicit references, and returns typed
input-to-consumer `relations` for graph scheduling. Instruction references resolve
an owner's mount alias or a global Artifact name. Backslash-escaped braces, doubled
braces, `${variables}`, nested/JSON groups and unmatched braces stay literal.
Payloads are never changed and references never expand file content.

`scope::eval_scope` admits the target and explicit references plus their child
and mount closure, not the referenced Artifacts' eval instructions.
`Scope::resolve_path` follows logical child/mount paths to canonical Artifact
ids; `Scope::resolve_input` additionally requires existing files/directories
without symlink traversal. Logical paths reject absolute paths, traversal, empty
components, backslashes, colons, controls and lengths above 4096 characters.

Before preparing a runtime command, call `scope::resolve_argv` with the admitted
scope. Explicit `{name}`, `{name}/path` and `--flag={name}/path` operands resolve to
absolute scoped input paths and add dependencies even without instruction
references. Use `{owner}/mount/path` for logical paths starting at a folder owner.
A file reference resolves to the file itself. Other arguments (including escaped
references) remain literal, and the command is never interpolated. `config check`
validates names and syntax but does not open runtime operands or execute programs.
Input existence and symlink checks happen during argument preparation. Graph
closure, runtime CLI execution and the Agent tool registry use these resolvers.

## Runtime execution library

`runtime::Command::prepare(program, args, workspace, run_dir, timeout_ms)` prepares
one invocation for `runtime::execute`. Arguments are literal; the runner never adds
a shell. The default cwd is the canonical workspace; a scoped caller can set
`command.cwd` to the Artifact's working directory (the containing folder for a
file Artifact). `process::run` is the lower-level API for resolved commands with
a complete explicit environment. `verify` uses the owner working directory as cwd.

Only `PATH` and `LANG` are inherited (`LANG` defaults to `en_US.UTF-8`). Each prepared
runtime command gets a fresh 0700 directory below the caller's external `run_dir`,
with 0700 `output`, `tmp`, `home`, and `cache` subdirectories. Children receive
`ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, `ARTIFACTIZE_TMP_DIR`, private
`HOME` and `XDG_CACHE_HOME`, and `TMPDIR`/`TMP`/`TEMP` pointing at private temporary
storage. Existing output ancestors are canonicalized before creation; output
inside the canonical workspace, including through symlinks, is rejected. The
caller owns the run directory; prepared output persists after execution for
receipts and explicit pruning. Failed preparation removes its newly owned
invocation directory, not the caller's root.

The runtime deadline defaults to 30,000 ms and accepts 1 through 2,147,483,647 ms.
It covers inert-child registration and execution without a reset. Each raw stdout
and stderr stream is capped at 128 KiB while excess bytes are drained. Runtime
results decode UTF-8 lossily, remove ANSI CSI sequences and C0 controls except tab,
LF and CR (DEL is preserved), and retain the truncation flag and actual exit
status/duration. Low-level process results remain unmodified bytes.

Cancellation and timeout signal the entire owned process group with SIGTERM,
then SIGKILL after at most one second; ordinary leader exit also kills its group.
Capture stops waiting at most one second after cleanup if a pipe remains open.
Dropping the caller cancels the supervisor, which still cleans up and reaps the
leader. Configured programs are trusted local code, **not an OS sandbox**: they
can use ordinary OS access, must keep reviewed input unchanged, and descendants
that deliberately detach into another process group can escape cleanup. These
controls do not promise confinement or detection of every adversarial transient
write.
