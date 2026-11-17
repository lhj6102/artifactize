# Declarations (.artf)

A project is a folder with `.artifactize/`. Each regular `*.artf` file below it defines
one Artifact. The file's stem is its name: `.artifactize/engine/app.artf` defines `app`.
Subfolders organize definitions for people; they create no namespace or relationship.

```toml
# .artifactize/app.artf
path = "crates/app/"
parts = ["app-tests"]

[evals.tests]
title = "The application tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"] }
payload.instruction = "Test {app} with {app-tests}."
```

Check the definitions with `artifactize config check`. A mistake names the definition
file, line and column and exits 2. A missing or wrong-kind target is different: the
check exits 0 with a note. `status` and `verify` report `ERROR/PREPARATION_FAILED` only
on evals covering that target, naming the path; unrelated evals retain their forecasts
or can still run.

## Declaration validation

`path` is required and is relative to the project root, not to the definition file.
Its spelling decides what the Artifact is, even before the target exists:

| PATH | Target |
|---|---|
| `"src/"` | A folder; the trailing `/` matters |
| `"./"` | The project root |
| `"README.md"` | One regular file |
| `"src/"` with `select = ["api", "model.rs"]` | Selected paths below that folder, an aspect |
| `"https://example.com"` | Live external state; a URI scheme has at least two letters |

Local paths use `/`, no absolute root, `..`, empty component or backslash. Several
Artifacts may target the same or overlapping paths. Commands and relative declarations
use the target folder as their base: the containing folder for a file, the selected
folder for an aspect, and the project root for a Live Artifact. `execution_paths`
remain project-relative.

A `name` key is refused: the file name is the Artifact's name. Names match
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`, are unique ignoring case across all definition
subfolders, and cannot be device names such as `con`, `nul` or `com1`.
Eval, tool, session and profile-variant IDs use the same identifier grammar. An eval
`[evals.check]` in `app.artf` is `app/check`; an eval cannot declare its own `id` field.
Eval order is by ID, not source order.

| Field | Rule |
|---|---|
| `path` | Required target, as above |
| `parts` | Optional 1–64 unique Artifact names; no self-reference or composition cycle |
| `select` | Optional 1–64 unique paths below a folder target, making an aspect |
| `basis` | `true` waives only this Artifact's own reviews; it cannot own evals |
| `tags` | At most 64 unique nonblank strings, without control characters |
| `fingerprint` | Default artifactsum for local targets; `false` disables reuse; a table selects artifactsum or a script |
| `evals.<id>.title` | Required nonblank title |
| `evals.<id>.profile` | Required `runtime`, `agent` or `human` profile |
| `evals.<id>.payload.instruction` | Required nonblank instruction |
| `evals.<id>.pass_schema`, `fail_schema` | Optional owner-result schemas; see below |
| `evals.<id>.profile_variants` | Up to 64 named complete profiles of the same kind |
| `evals.<id>.mutex` | Optional hierarchical mutex tags for runtime and Agent evals |
| `views.agent_tools`, `views.human_tools` | Tools published by this Artifact |
| `views.agent_sessions` | Tool sessions; see [Agent tools](tools/agent.md#tool-sessions) |
| `annotations` | Presentation and notes, never structure; see below |

Declaration keys use snake_case (`timeout_ms`, `input_schema`, `execution_paths`,
`pass_schema`); JSON Schema keywords and owner-defined payload fields retain their
names. Unknown declaration keys are errors. Everything inside `annotations` is
accepted without validation. TOML null values are not supported. Outside annotations,
quote dates and use finite numbers so the values have JSON equivalents. JSON output
and tool protocols keep their camelCase names; they are not declaration syntax.

## Upgrading an in-tree project

Run `artifactize config migrate`, or select another project with `--repo`. This is the
only command that reads the old `index.artf`, file sidecars, root `.artf` and
`.artfignore`. Normal commands read only `.artifactize/`; files elsewhere are ordinary
content, not definitions, and `.artfignore` no longer controls discovery.

Migration makes the former folder composition explicit in `parts`, adds `path`, removes
`name`, preserves comments and the remaining declarations, then removes the old files.
A root-folder Artifact using artifactsum gains an ignore for `.artifactize/`. The former
root graph becomes an ordinary folder Artifact on `.artifactize/`. Separate projects
kept out by the old ignore file are left alone and reported; migrate each with `--repo`.

Migration writes nothing if definitions already exist, names collide ignoring case,
a name is reserved, or the old or converted project is invalid. Local fingerprints
and eval criteria are preserved except for the former graph and folders whose content
now includes a nested project's `.artf` files. Check the printed changes before verifying.

## Owner-result schemas

Agent and Human results contain a `verdict` plus the owner's fields. Declare only
those owner fields in `pass_schema` (GREEN) and `fail_schema` (RED); artifactize
supplies `verdict`. For example:

```toml
[evals.review.pass_schema]
type = "object"
properties.summary = { type = "string", minLength = 1 }
required = ["summary"]
additionalProperties = false
```

The saved result is `{"verdict":"GREEN","summary":"All requirements are covered."}`.
The [Agent response](../cli/index.md#agent-sessions) instead wraps the owner's fields:
`{"verdict":"GREEN","verdictFields":{"summary":"All requirements are covered."}}`.
A Human supplies just `{"summary":"All requirements are covered."}` as form fields,
with GREEN selected separately.

These schemas are JSON Schema: rules for which fields a result may have and which
values they take. The schema of the whole result object, the **result envelope**, allows
less than general JSON Schema does, including for references (`$ref`, which points to
another schema) and composition (`allOf`, `anyOf`, `oneOf` and `not`, which combine
schemas):

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

Schemas are limited to 8 MiB. An Agent's final response is checked once complete: over
1 MiB it is invalid. The result artifactize keeps from it is limited to 256000 characters, where an emoji or
another character outside the Basic Multilingual Plane counts as two. Invalid final
responses get one bounded repair attempt, not silent field removal. The
[Human submission interface](../cli/index.md#command-reference) separately bounds
its input and reports validation errors without settling the request.

## Reviewing definitions {#graph-artifact}

Definitions are ordinary files. To review all of them, declare an Artifact with
`path = ".artifactize/"`; to review one family, use a subfolder, and to review one
definition use its complete file path. None needs a special kind or a special
fingerprint. A definition may cover itself without a recursive fingerprint.
The `graph` builtin still shows the whole project's criteria and relations from any
Artifact.

```toml
# .artifactize/graph.artf
path = ".artifactize/"

[views.human_tools.structure]
builtin = "graph"
args = []

[evals.review]
title = "The definitions describe the intended project"
profile = { kind = "human" }
payload.instruction = "Review the definitions and their relationships."
```

Every `.artf` file uses the default **artf view** in artifactsum and file builtins.
Definition values count toward its fingerprint; `annotations`, formatting and `#`
comments do not. A field inside a user-defined schema, such as
`pass_schema.properties.comment`, is definition content, not an annotation. If the file
is not valid TOML or UTF-8, changes to any of its bytes count instead.

File builtins show the definition without annotations, formatting or `#` comments.
Runtime commands and fingerprint scripts read the actual file. This behavior needs no
declaration option.

A review of definitions reruns for a changed model, timeout, tool or criterion, but not
for an annotation or formatting edit. A root-folder Artifact also covers `.artifactize/`
unless its `fingerprint.ignore` excludes it or it is a part's target.

## Positions and comments

A node may keep positions and a plain-text note in its own definition:

```toml
[annotations.style]
at = [120, 40]
parts = { app-tests = [40, 80] }

[annotations.comment]
text = "An accessibility review will go here next."
```

`at` is the top-level position. A whole's `parts` positions belong to that whole's box;
a shared part can have a position in each whole. A comment belongs to one node, appears
with every copy of it, and moves with it. There is no separate layout file or free
canvas comment. Positions and notes never change verification or reuse.

The reader ignores malformed or future annotation values. The workbench draws that
box automatically or hides that comment. An edit of an unreadable section is refused,
with its file, line and column. Edits retain unknown annotation keys and untouched text.
Stale positions are dropped when that style is next written; empty sections are removed.
See [the desktop](../desktop/index.md) for placing nodes, editing notes and conflicts
with an agent's file edits.

## File Artifacts

A PATH without a trailing `/` names exactly one file. Commands run in its containing
folder, and `{name}` is the file itself; `{name}/suffix` is an error. Its default
artifactsum covers only that file. An explicit `fingerprint.files` may name only the
file's name, and `fingerprint.ignore` is not supported for a file Artifact.

Symlinks, directories and special files are not regular file targets. A missing or
wrong-kind target is a note at `config check` and an eval preparation error at verify,
not an invalid definition. Runtime and Agent evals check targets again at start and end,
and tool calls check them too, even when fingerprinting is disabled or a script is constant.
An unkeyed Human submission without Live inputs does not recheck target availability.

## Aspects

A folder PATH with `select` defines an aspect. Its own files are the selected paths
below that folder, minus its parts' targets. The default artifactsum covers its
selection; explicit `fingerprint.files` must stay within it. Aspects may overlap and
may select `.artf` files. Their commands run from the selected folder, and `{name}`
refers to that folder. A file or Live target cannot have `select`.

Composition comes only from `parts`. Merely placing a target below another target, or
placing a definition in a subfolder, creates no relationship. A folder part is hidden
from the whole's tools until the eval names it; file and aspect parts remain readable
through the whole. [Artifacts and evals](artifact.md#composition) explains verdicts.

## Live Artifacts

A URI PATH names external state. Artifactize never opens it: `{name}` supplies the URI
to runtime, tool and fingerprint-script arguments, and `{name}/x` appends `/x`.
Commands use the project root and the author's environment. File builtins and artifactsum
fingerprints are invalid on a Live Artifact.

Omitting `fingerprint` means there is no observed-state snapshot. To reuse Agent or
Human reviews, declare a fingerprint script that reports a stable snapshot, such as a
deployed commit, using the script contract below. Runtime evals that cover a Live
Artifact always run, even when its snapshot is unchanged; Agent and Human reviews reuse
while the snapshot stands. A snapshot that changes during a review gives `INPUT_CHANGED`.

Evals observe; they do not deploy or repair the target. Compare production with the
remote release or `origin/main`, not a feature worktree's HEAD, so ordinary branch work
does not make a healthy deployment fail.

## Eval mutex tags

A runtime or Agent eval can declare `mutex` beside `profile`:

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
duplicate tags and declarations on Human evals are errors. An eval accepts at most
64 unique tags; exceeding that limit is a declaration error.
Omitting `mutex` (or declaring `mutex = []`) adds no constraint.

Two evals conflict if any of their tags are equal or one is an ancestor of the
other. `use-computer` conflicts with every descendant;
`use-computer:db` conflicts with `use-computer:db:users`, but not with
`use-computer:browser` or `use-computer:database`. Give the same tag, or an ancestor and its
descendant, to evals that must not run together, such as two that drive the same display.
An eval whose tag is held shows `QUEUED` with the holder named (below) and starts when it
is released; tags add no priority or ordering, and evals waiting on each other's tags
cannot deadlock.

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
can still be reused while a conflicting eval runs.

## Eval dependencies

Write Artifact references as `{input}` or `{input}/path` in
`payload.instruction` and runtime `profile.args`. See
[Eval dependencies](artifact.md#eval-dependencies) for their obligations, cycles,
scheduling and the Human prerequisites, and [composition](artifact.md#composition) for
parts.

## Artifactsum

Omitting `fingerprint` uses artifactsum; for a folder, `fingerprint = {}` does the same.
`fingerprint = false` disables reuse and `fingerprint = true` is rejected. What it covers,
`files`, `ignore` and the file-Artifact form are in
[Fingerprints and reuse](fingerprint.md#artifactsum). Tags do not enter an ordinary target's reuse key. A definition review still sees them in its `.artf` content.

Artifactsum reads the files below the Artifact by the scoped path rules. A symlink or
special file it would read stops verification with an error unless it is ignored. Every
applicable `.gitignore` is read for its rules even when it is itself ignored as content,
so it must be a regular file, not a symlink, and at most 1 MiB. Artifactsum never hashes a
file outside the Artifact it belongs to; to know what is ignored, it also reads the
`.gitignore` files of the folders above it, up to the repository root. It stops with an
error after 10,000 entries or 1 GiB. Hashing runs on preparation and end-of-review
rechecks. Files a review writes into ignored paths, such as Python's `__pycache__`,
therefore never cause `INPUT_CHANGED`. Each result remembers which files it covered, so `status` can tell you which files
changed since then (`path`, `+path`, `-path`) and which dependency changed; a very large
folder may show only that something changed.

## Fingerprint scripts

Before executing evals, `verify` computes the fingerprint of every Artifact the selected
evals need, also of the Artifacts they depend on whose own evals are not selected.
How many are computed at once is set by
[`--fingerprint-jobs`](../cli/index.md#runtime-cli). A failure reports the first failing
Artifact in name order and stops the rest. Each script gets its own private output
folder.

The script's output is used unchanged as the Artifact's fingerprint, the same for
every eval of the Artifact, and saved with each request. How it combines with the
criteria and dependencies is in [the reuse key](fingerprint.md#the-reuse-key). Artifactsum
failures abort preparation in the same way.

The command runs from its owner's working directory with JSON on stdin:
`{"version":1,"artifactId":"example"}`. Its executable follows
[Program lookup](others/platforms.md#program-lookup). Arguments stay literal except
[Artifact references](#artifact-references-in-commands); each referenced Artifact enters
the script's scope without becoming a graph relation. `config check` rejects unknown
and malformed references; file existence and symlinks are checked at preparation.
There are no interpreter-specific flags, wrappers or entry-file rules.

`fingerprint.script.files` accepts up to 64 unique owner-relative literal
file/directory paths of up to 1024 characters each (an emoji or another character outside the Basic Multilingual Plane counts as two). They must exist without symlink traversal on every call;
their contents are never hashed. Commands share the runtime environment, cancellation,
raw output bounded to 128 KiB per stream, and a 30,000 ms default timeout (`timeout_ms` accepts
1–2,147,483,647). Private output and temporary directories are removed after
each invocation. Stdout must be exactly 1–128 ASCII characters from
`[A-Za-z0-9._:-]`, optionally followed by one LF, or one CRLF on Windows. No other
whitespace is allowed; the value is not trimmed. Nonzero exit, malformed output,
more than 128 KiB on stdout or on stderr (even after a valid fingerprint), timeout, cancellation, missing files or cleanup failure abort preparation with an
operational error, without starting any eval or falling back to uncached review.
Stderr is not forwarded as a fingerprint diagnostic.

After runtime or Agent review, covered fingerprints are recomputed before
accepting GREEN or RED. A changed value records ERROR/INPUT_CHANGED with no GREEN or
RED verdict; a failed recheck records ERROR. Force skips neither preparation
nor recheck. An Artifact with `fingerprint = false` has no fingerprint to prepare.
An unkeyed eval covering Live state still rechecks every available snapshot; other
evals without a complete reuse key skip the fingerprint-value recheck. Target availability
and the unkeyed Human exception are covered in [File Artifacts](#file-artifacts).
Other required Artifacts can still have fingerprints prepared. There is no workspace monitoring: artifactsum hashes
only at preparation and recheck.

## Artifact references in commands

A reference is `{name}`, `{name}/path` or `--flag={name}/path`. An eval's **scope** is
the Artifacts it may read: its target and the Artifacts named in its instruction or
runtime arguments.

| Context | Rule | Example |
|---|---|---|
| Instruction | Makes the eval depend on the named Artifact | `Review {spec}.` |
| Runtime arguments | Substitutes the reference and adds an eval dependency; reference a folder part directly rather than through its whole | `args = ["{spec}/api.md"]` |
| Fingerprint-script arguments | Lets the script read the named Artifact, without adding an eval dependency | `args = ["{rules}/STYLE.md"]` |
| Tool and tool-session arguments | May name only Artifacts already in the eval's scope; the tool does not add dependencies | Instruction `Review {spec}.`, tool `args = ["{spec}"]` |
| Local folder or aspect | Starts at its folder and becomes an absolute path; an aspect stays within its selection | `{code}/src/lib.rs` |
| File | Names the file itself, with no `/path` suffix | `{logo}` |
| Live | Supplies an opaque URI and appends any suffix literally, without filesystem access | `{service}/health` |

A fingerprint-script or tool path through a folder part needs that part in scope too;
otherwise preparation or the call fails. An aspect's selection may reach into a folder
part. An admitted folder still exposes file-part targets and aspect-selected files
through its paths, but not unadmitted folder parts. Composition does not add a part's
own tools: name the part in the consuming eval's instruction or runtime arguments.

Other arguments stay literal: escaped or doubled braces, `${variables}`, JSON and
unmatched braces are not interpolated, and artifactize never builds a shell command
from a string. Referencing an Artifact does not add its evals' instructions to the
review. The [`graph`](tools/builtin.md#current-project-graph) tool shows the whole
project's criteria.

Paths in references may not be absolute, go up with `..`, have empty parts, backslashes,
colons or control characters, or be longer than 4096 characters; paths you declare
(`fingerprint.files`, `fingerprint.script.files` and `execution_paths`) are limited to 1024.
`config check` checks the names and syntax of the profile each eval declares. The syntax and
path rules above apply to every `profile_variants` entry too, so a malformed reference such
as `{app}/../outside` in any variant fails `config check`. Whether a reference that appears
only in a variant names an existing Artifact is checked when that variant is selected, so
run `status SELECTOR --profile NAME` before relying on a variant. Whether a referenced file
exists, and that no symbolic link is followed, is checked when the command is prepared.

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

When the command ends, whether normally, by cancellation or by a timeout, the programs
it started are stopped too, so nothing it started in the background outlives it; use an
Agent [tool session](tools/agent.md#tool-sessions) or a Human
[launch tool](tools/human.md#running) for a helper that must keep running. Commands are trusted local code, **not a sandbox**: they can do
whatever your account can, must not change the reviewed input, and a program that
deliberately detaches from the command may keep running.
