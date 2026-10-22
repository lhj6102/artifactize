# Declarations (.artf)

Write an `index.artf` next to the folder (or a `NAME.artf` next to the file) you want
to review, then check it:

```sh
artifactize config check
```

It prints `Folder configuration is valid.`, or the error with its file, line and
column, and exits 2 on an error. This page has the exact rules for declarations, folder
and file Artifacts, fingerprints, references and how commands run;
[Artifacts and evals](artifact.md) introduces them.

## Migrating removed dependency declarations

`kind = "dependency"` and `depends_on` are removed. Delete the dependency eval;
put the needed Artifacts as `{name}` references in the consuming eval's
`payload.instruction` (or runtime `args`). For example, instead of a derived
`ready` eval listing `spec` and `assets`, write the actual review instruction as
`"Review {app} against {spec} and {assets}."` Each consuming eval waits for those
inputs' required evals; independent sibling evals keep running.

A child folder counts as a reference of every eval of its parent. To require any
other Artifact's reviews before an eval, reference that Artifact explicitly in the
eval's instruction or argv.

An old profile fails configuration parsing with
`unknown variant `dependency`, expected one of `agent`, `human`, `runtime``.
A `depends_on` field on a supported profile is an unknown-field error. These are
configuration errors, not review verdicts; migrate the declaration before verifying.

## Declaration validation

`index.artf` declares its containing folder. `<filename>.artf` declares the
neighboring file whose complete name is `<filename>`, including its extension.
Both use standard TOML and require `name`. The repository root's nameless `.artf`
declares the [graph Artifact](#graph-artifact), with no source file.

Discovery walks from the repository root and skips `.git`, `node_modules`, and
subdirectories and sidecar paths listed in an optional root `.artfignore`
(gitignore syntax). In every visited directory, `index.artf` is read regardless of
an ignore pattern matching that filename alone. To suppress a subproject's folder
declaration, exclude its containing directory from the walk instead. An ignored
sidecar is not parsed or target-checked; its
target remains an ordinary file unless another declaration includes it. List folders
that hold separate projects, fixtures or examples:

```gitignore
crates/app/tests/
examples/
```

Names, eval IDs, tool/session names and profile variant names match
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`. Artifact names are globally unique. Evals are
ID-keyed tables, not an array; `[evals.check]` on `app` becomes `app/check`.
An explicit eval `id` field is rejected. Evals within each declaration are ordered
by ID, not by their position in the TOML source.

| Field | Rule |
|---|---|
| `name` | Required Artifact name |
| `tags` | Optional list of at most 64 unique nonblank strings, without ASCII controls |
| `basis` | Optional boolean; `true` declares a no-eval input, waiving its own review obligations but not its dependencies |
| `fingerprint` | Omission uses artifactsum; `false` disables reuse; otherwise an artifactsum or script table |
| `views.agent_tools`, `views.human_tools` | Optional maps of declared tools |
| `views.agent_sessions.<name>.start`, `.stop` | Tool-session commands: required `command` and `args`, optional `execution_paths`; no other fields |
| `views.agent_tools.<name>.session` | Optional name of a tool session declared in the same Artifact; command Agent tools only |
| `evals.<id>.title` | Required nonblank title |
| `evals.<id>.gate` | Optional `"blocking"`; keeps a Human sign-off strict under `--defer-human`. Omission follows the request mode; no other values |
| `evals.<id>.mutex` | Optional list of at most 64 unique hierarchical mutex tags; runtime and Agent evals only; no default |
| `evals.<id>.profile` | Required `runtime`, `agent` or `human` profile |
| `evals.<id>.payload.instruction` | Required nonblank instruction for runtime, Agent and Human evals |
| `evals.<id>.pass_schema`, `fail_schema` | Optional owner-field JSON Schemas for executable evals; a Human eval's take only [form shapes](#human-result-schemas) |
| `evals.<id>.profile_variants` | Up to 64 named complete profiles retaining the default kind |

[Tool sessions](tools/agent.md#tool-sessions) keep a harness running for one review.
Artifactize starts, stops and cleans them up; there are no lifecycle or timing options
to declare.

All declaration keys are snake_case: `timeout_ms`, `max_tool_calls`, `max_tokens`,
`input_schema`, `execution_paths`, `pass_schema`, `fail_schema`,
`profile_variants`, `gate` and `mutex`.
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
also name the typed key path, such as `evals.check.profile.args`.

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

## Owner-result schemas

Agent and Human results contain a `verdict` plus the owner's fields. Declare only
those owner fields in `pass_schema` (GREEN) and `fail_schema` (RED); artifactize
supplies `verdict`. For example, the `summary` schema above accepts
`{"verdict":"GREEN","summary":"All requirements are covered."}` from an Agent.
A Human supplies just `{"summary":"All requirements are covered."}` as their form
fields, with GREEN selected separately.

The result envelope is deliberately narrower than a general JSON Schema:

- `type`, when present, must be `"object"`. `additionalProperties`, when present,
  must be `false`; artifactize forces both object type and closed properties in
  the final envelope even when omitted.
- Every permitted top-level owner field must appear explicitly in `properties`.
  Neither `patternProperties` nor `$ref` can admit an undeclared top-level field.
- Top-level `allOf`, `anyOf`, `oneOf` and `not` are rejected. Compose schemas
  inside an owner field instead; nested objects, arrays and local references are
  supported by the schema validator.
- Reserved names cannot be declared or required: `verdict`, `reference`,
  `reusedFrom`, `executionProvenance`, `attemptId`, `provider`, `model`, `stdout`,
  `stderr`, `durationMs` and `exitCode`. Audit and execution data live outside
  owner fields.
- Omitting one verdict's schema permits **no owner fields** for that verdict:
  an Agent returns only `{"verdict":"GREEN"}` or `{"verdict":"RED"}`, and the
  Human form fields are `{}`. An empty object schema has the same closed envelope.

### Human result schemas

A person enters a Human result in a form, so a Human eval's `pass_schema` and
`fail_schema` may use only shapes a form can enter. Anything else is a declaration
error when the `.artf` loads, naming the key path and what is allowed instead.

| Owner field schema | How the review shows it |
|---|---|
| `{ type = "string" }`, optionally with `minLength`, `maxLength` | A text box |
| `{ type = "integer" }` or `"number"`, optionally with `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf` | A number box |
| `{ type = "boolean" }` | true / false (and – when optional) |
| `{ enum = ["a", "b"] }`, only strings, `type = "string"` optional | A choice |
| `{ const = … }` | Nothing: the value is filled in |
| `{ type = "array", items = { type = "string" } }`, items optionally with `minLength`, `maxLength` | A list of text boxes |
| `{ type = "array", items = { type = "object", properties = … } }` whose properties are the scalars above | Repeated groups of those fields |

Arrays take `minItems` and `maxItems`. The result object and each group's object
take `type = "object"`, `properties`, `required` and `additionalProperties`; every
name in `required` must be a declared property. Any schema may add `title`,
`description` and `$comment`; a property's `description` shows as its hint. A
required field is marked `*`; an empty optional field says *Optional* and is left
out of the result, as are empty list items and groups with nothing entered. The
desktop app adds and removes items and groups with `+ Add` and ✕ buttons; the
terminal UI uses keys, as its [Review form](../tui/index.md) describes.
Patterns, formats, nested objects, lists inside groups, `$ref` and combinators
such as `anyOf` are refused: a person could not enter them. Agent schemas keep the
full validator.

Schemas are limited to 8 MiB. An Agent's raw final response is limited to 1 MiB;
the normalized result is limited to 256000 UTF-16 code units. Invalid final
responses get one bounded repair attempt, not silent field removal. The
[Human submission interface](../cli/index.md#command-reference) separately bounds
its input and reports validation errors without settling the request.

## Graph Artifact

An optional nameless `.artf` at the repository root declares the whole project's
Artifact graph. `index.artf` still declares the root folder; `<filename>.artf`
still declares a neighboring file. A nameless `.artf` in another visited directory
is a declaration error. The root `.artf`, like `index.artf`, is read even when an
`.artfignore` pattern matches its filename. There can be only one graph Artifact.

```toml
# .artf at the repository root
name = "project-graph"

[views.agent_tools.structure]
builtin = "graph"
args = []

[views.human_tools.viewer]
builtin = "graph"
kind = "launch"
args = []

[evals.review]
title = "The Artifact boundaries and evaluation criteria fit the project"
profile = { kind = "human" }
payload = { instruction = "Use the graph viewer to review the current structure." }
```

It is an ordinary named Artifact: declare any runtime, Agent or Human
evals and tools, and select it by name in `verify`, `status`, CI or `--reuse-only`.
It has no default evals, child ownership or relations to other Artifacts. Explicit
instruction/argv references work normally. Commands run from the
repository root; declaring the graph does not make it the owner of that folder.
Its scope uses `kind = "graph"` and its declaration path is `.artf`.

Do not declare `fingerprint`, even `false` or an empty table: its fingerprint is
always the current extracted graph. Otherwise ordinary Artifact rules apply:
`basis = true` explicitly declares no own review obligations and cannot own evals;
it does not satisfy unmet dependencies. A no-eval graph without `basis = true`
remains UNREVIEWED, as any no-eval Artifact does. No source file is
created or required. Ordinary Artifact cycles are allowed; extraction is flat and
includes the graph Artifact's own declaration without recursive fingerprinting.

The [graph fingerprint](fingerprint.md#graph-fingerprint)
changes with structural definitions, not source file bytes or evaluation
execution options. There is no diff, history or automatic review policy: results
are reviewed or reused against the current state only.

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

The containing folder is the working directory of runtime commands, tools and
fingerprint scripts, so a relative executable may be a sibling file. It grants no
access to other siblings: built-in tools and scoped argv references see only the
target file and referenced Artifacts. `{name}` resolves to the target file path,
not the folder, and file references cannot have a `/path` suffix. `list .` names
the virtual file Artifact root and lists the target filename; paths within that
root stay relative to the containing folder. Default artifactsum covers only the
target file. A workspace can have only file Artifacts; it does not need a folder
declaration.

Omit `fingerprint` to hash the file. An explicit artifactsum form must be
`fingerprint = { files = ["hero.png"] }` for `hero.png.artf`: only the target
filename is allowed. `fingerprint = {}` and `files = ["."]` are rejected, as is
`fingerprint.ignore`, even an empty list. Script fingerprints and
`fingerprint = false` remain available.

JSON command-tool `context.artifactPath` names the file, while the process cwd is
its containing folder. Scope entries carry `kind = "file"`, `"folder"` or `"graph"`.
`execution_paths` stay workspace-relative provenance pins for all three kinds.

The target must remain an existing regular file with no symlink traversal, not
just at discovery. It is validated at fingerprint preparation/recheck, before
reuse, at eval start/end and on every Agent/Human tool call, including with
`fingerprint = false`. A file invalid before eval admission produces
ERROR/PREPARATION_FAILED. If accepting a completed Runtime/Agent result discovers
that a target has become invalid, it records ERROR/INPUT_CHANGED instead of a
semantic verdict. A Human action first reloads its recorded scope and declarations;
if discovery or reconnection fails, for example because a file target is missing
or invalid, the action fails without settling the waiting request. A changed
fingerprint value after successful Human reconnection still settles
ERROR/INPUT_CHANGED for a keyed request. Preparation failures abort before reuse
or execution; tool calls return an error without running the tool.

For removed legacy policy fields and their replacements, see the
[upgrade reference](../getting-started/install.md#removed-gate-bypass-settings).

## Eval mutex tags

A runtime or Agent eval can declare `mutex` beside `gate` and `profile`:

```toml
[evals.suite-passes]
title = "The end-to-end suite passes"
mutex = ["use-computer:display", "use-computer:callback"]
profile = { kind = "runtime", command = "pnpm", args = ["e2e"], timeout_ms = 1800000 }
payload.instruction = "Run the end-to-end suite."
```

A tag is `use-computer` or a colon-separated descendant such as
`use-computer:display`. Each descendant segment matches
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`. Other roots, empty segments, invalid segments,
duplicate tags and declarations on Human evals are errors.
Omitting `mutex` (or declaring `mutex = []`) adds no constraint.

Two evals conflict if any of their tags are equal or one is an ancestor of the
other. `use-computer` conflicts with every descendant;
`use-computer:db` conflicts with `use-computer:db:users`, but not with
`use-computer:browser` or `use-computer:database`. An eval waits until no conflicting
eval is running and then takes all its tags at once, so two evals never wait for each
other. Tags add no priority or ordering.

Tags apply to every Run, worktree and `verify` on the same state, not only within a Run;
a different `--state-dir` has its own tags. A waiting request stays `QUEUED`, and its
`blockedReason` names the eval and Run that hold the tag. Waiting uses no execution
budget, backend slot or job slot, and `timeout_ms` starts only when the eval starts.
Tags are released when the eval ends or is cancelled, also when its process dies or
its Run ends.

Tags describe the eval's own command or tools, never its Artifact: an Agent that
only reads test code need not wait because another eval runs those tests. There
are no implicit tags. Mutex tags are execution options, not evaluation criteria;
changing them changes neither fingerprints nor reuse keys, and cached results
can still be reused while a conflicting eval runs. Human try-claim behavior is
not implemented.

## Human gate policy

`gate = "blocking"` is an optional eval field, not a profile or global setting.
It preserves a Human eval's pending gate under `verify --defer-human`. Omission
retains ordinary strict behavior in default Runs and permits deferral only when
that request opts in. On runtime or Agent evals it has no additional
effect: only Human sign-offs can be deferred. Gate policy changes scheduling,
not the judgment definition or its reuse key.

```toml
[evals.signoff]
title = "The delivery is approved"
gate = "blocking"
profile = { kind = "human" }
payload.instruction = "Approve the delivery before dependent work proceeds."
```

See [verify modes](../cli/index.md#verify-modes) and
[deferred sign-offs](human.md#deferred-sign-offs).

## Eval dependencies

Write Artifact references as `{input}` or `{input}/path` in
`payload.instruction` and runtime `profile.args`. See
[Eval dependencies](artifact.md#eval-dependencies) for their obligations, child
Artifacts, cycles, scheduling and the Human prerequisites.

## Artifactsum

[Fingerprints and reuse](fingerprint.md#artifactsum)
describes `files` and `ignore`. Omitting `fingerprint` uses artifactsum. For a
folder, `fingerprint = {}` does the same. A file Artifact's explicit artifactsum
form must list its target filename. `fingerprint = false` disables reuse;
`fingerprint = true` is rejected.

The value changes when a covered file's name or content changes. It covers only the
Artifact's own files: a dependency's change reaches an eval through the dependency's fingerprint
in the [reuse key](fingerprint.md#the-reuse-key). Folder walks
exclude child Artifact folders and every `*.artf` file. Explicitly listing a
`.artf` file in `fingerprint.files` fails `config check`, even if the file does not
exist. Discovery also rejects non-regular `.artf` entries, including directories;
a directory named `bundle.artf` is not an ordinary hashable input. File Artifacts
hash only their target by default. Tags are not hashed and do not change the Eval definition hash or
reuse key.

Walks follow scoped path rules. Symlinks and special files fail closed unless they
are ignored. Every applicable `.gitignore` is read as rule metadata even when it is
itself ignored as content, so it must be a regular file, not a symlink, and at most
1 MiB. Nothing is followed out of the owner, and a walk stops with an error
after 10,000 entries or 1 GiB. Hashing runs on preparation and end-of-review
rechecks. Files a review writes into ignored paths, such as Python's `__pycache__`,
therefore never cause `INPUT_CHANGED`. Each result remembers which files it covered, so `status` can tell you which files
changed since then (`path`, `+path`, `-path`) and which dependency changed; a very large
folder may show only that something changed.

## Fingerprint scripts

Before executing evals, `verify` computes fingerprints in their selected required
dependency closure, including dependencies whose evals are not selected.
Up to `--fingerprint-jobs N`
fingerprints are computed at once (default: the number of CPUs), in `status` as in
`verify`, both before the evals and when Runtime and Agent reviews end. A Human tool
run or submission checks its fingerprints separately, as many at once as there are
CPUs, whatever the Run's `--fingerprint-jobs`. A failure reports the first failing
Artifact in name order and stops the rest. Each script gets its own private output
folder.

Every executable eval on an Artifact receives the same literal value, saved on
its request and in the Run's Artifact validation. artifactize adds no repository,
eval, profile, tool-view, dependency or content salt to script output. The eval's
[reuse key](fingerprint.md#the-reuse-key) combines it with the
Artifact names and kinds (`folder`/`file`/`graph`), dependency fingerprints and Eval
definition hash. Physical paths never enter the reuse key. Artifactsum
failures abort preparation in the same way.

The command runs from its owner's working directory with JSON on stdin:
`{"version":1,"artifactId":"example"}`. Bare commands resolve through PATH;
absolute executables run as given. Relative executables containing `/` (such as
`./fingerprint.sh`) resolve inside a folder Artifact's admitted scope. For file
Artifacts, relative executables instead resolve beneath the containing folder and
may name unrelated siblings. Traversal and symlinks are rejected in both cases.
Arguments stay literal except explicit scoped Artifact references: `{name}` is a
global Artifact name. Each referenced Artifact, with its child closure, enters the
script's scope without becoming a graph relation.
`config check` rejects unknown and malformed references; existence and symlinks
are checked when the command is prepared. There are no interpreter-specific
flags, wrappers or entry-file rules.

`fingerprint.script.files` accepts up to 64 unique owner-relative literal
file/directory paths of up to 1024 UTF-16 code units each. They must exist without symlink traversal on every call;
their contents are never hashed. Commands share the runtime environment, cancellation,
bounded raw output and a 30,000 ms default timeout (`timeout_ms` accepts
1–2,147,483,647). Private output and temporary directories are removed after
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

## Artifact references in commands

Runtime arguments, fingerprint scripts and tool arguments name other Artifacts with
`{name}`, `{name}/path` or `--flag={name}/path`. `{owner}/child/path` starts at a folder
Artifact; a file reference is the file itself and takes no path. Each one becomes an
absolute path when the command runs. In instructions and runtime arguments a reference
makes the eval depend on that Artifact; in a fingerprint script it only lets the script
read it; a tool argument may name only an Artifact its eval already references. Everything else is passed literally: escaped braces,
doubled braces, `${variables}`, JSON and unmatched braces stay as written, and artifactize
never builds a command line from a string. An eval may read its target and the Artifacts it references, with
their children. The instructions of the referenced Artifacts' evals are not added to its
review; a [`graph`](tools/builtin.md#current-project-graph) tool shows the whole project's criteria.

Paths in references may not be absolute, go up with `..`, have empty parts, backslashes,
colons or control characters, or be longer than 4096 characters; paths you declare
(`fingerprint.files`, `fingerprint.script.files` and `execution_paths`) are limited to 1024.
`config check` checks the names and syntax; whether a referenced file exists, and that no
symbolic link is followed, is checked when the command is prepared.

## Runtime execution

Artifactize runs the program with its arguments as written, without a shell, in the
Artifact's folder (the containing folder for a file Artifact).

Commands run in the author's own terminal environment, so a project declares them as
it would type them: the whole environment of the `artifactize` process is passed on,
including the real home and cache locations (`HOME`, `XDG_*`, and on Windows
`USERPROFILE`, `APPDATA`, `LOCALAPPDATA` and the system variables), so toolchains
that resolve through them, such as `cargo` or `pnpm` behind a version manager, work as
in a terminal. Artifactize's own variables stay out: every `ARTIFACTIZE_*` variable of the
caller (credentials such as `ARTIFACTIZE_REMOTE_TOKEN`, and settings) is removed. The
command gets `ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR` (a private folder for its
output, kept after the Run until you prune it), `ARTIFACTIZE_TMP_DIR`, and
`TMPDIR`/`TMP`/`TEMP` pointing at a private temporary folder removed when the command ends.
Only you can read these folders, and they are never inside the reviewed project.

Because commands see the author's environment, a result depends on it like any
terminal command does, and it is not part of the
[reuse key](fingerprint.md#the-reuse-key): a toolchain upgrade
does not invalidate a cached result. A fingerprint that must change with a tool's
version has to read that version.

The runtime deadline defaults to 30,000 ms and accepts 1 through 2,147,483,647 ms. The
result keeps the first 128 KiB of stdout and of stderr, with a flag when it was cut, and
the exit status and duration. Text is read as UTF-8 (invalid bytes replaced), with
terminal color codes and ASCII control characters other than tab, LF and CR removed (DEL is kept). The full
streams stay in the Run's output, where `request logs` and MCP `request_logs` read their
end, a range or matching lines. Once a capture is deleted, only the saved first 128 KiB
remains. `artifactize prune` removes the Run's `output` folder but keeps these stream
captures. Evidence that is missing, partial, or saved only by an older or remote
artifactize is reported as such.

Cancellation and a timeout stop the command and the programs it started. Commands are trusted local code, **not a sandbox**: they can do
whatever your account can, must not change the reviewed input, and a program that
deliberately detaches from the command may keep running.
