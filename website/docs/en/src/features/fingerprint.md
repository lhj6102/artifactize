# Fingerprints and reuse

A fingerprint says what an Artifact's reviews depend on. An eval's earlier result
is reused when the eval is unchanged and the fingerprints of the Artifacts it
depends on are unchanged, so review cost follows the size of a change.

## Fingerprints

Folder and file Artifacts use **artifactsum** by default. Omit `fingerprint` to
hash their own files, or choose a script. The root graph Artifact always uses the
[structural graph fingerprint](#graph-fingerprint); it cannot declare `fingerprint`
to override or disable that value. For folder and file Artifacts, the configurable
forms below add no tool declarations or other Artifact's value; dependencies enter
through the [reuse key](#the-reuse-key).

- Omission uses artifactsum: `artifactsum:` plus SHA-256 over the Artifact's
  own input paths and bytes. `fingerprint = {}` also works for folders. A folder hashes its own files;
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

- `files`: 1–64 unique owner-relative paths of up to 1024 UTF-16 code units each,
  default `["."]` (the owner's own files). Each must exist and stay inside the Artifact: paths into child Artifacts
  are rejected. Directory walks skip child Artifact folders and every
  `*.artf` declaration. Explicit `.artf` file inputs fail `config check`; they are
  not silently skipped. A directory whose name ends in `.artf` is an invalid
  declaration and fails discovery.
  A neighboring file Artifact's target remains in the folder's hash; it creates
  no automatic child relation.
- `ignore`: up to 64 unique `.gitignore`-style globs relative to the owner, without
  negation; a pattern may not be blank, start with `#` or contain control characters. They always exclude, like the built-in ignores `.git`,
  `__pycache__/`, `*.pyc`, `target/` and `node_modules/`. `.gitignore` files
  exclude more, with git semantics: every one from the repository root (`--repo`)
  down through the Artifact and walked directories applies. A pattern is relative
  to its own file's folder; negation works, and deeper files override shallower
  ones. Explicitly named non-declaration `files` are never ignored.

File Artifacts hash only their target filename. An explicit artifactsum form
must name just that file, for example `fingerprint = { files = ["hero.png"] }`.
`fingerprint = {}`, `files = ["."]` and `ignore` (even `[]`) are rejected for a
file Artifact. Omit the field for its default; use a script if the fingerprint
needs to cover more than the file.

The old `dependencies` fingerprint option is rejected: a fingerprint covers only its own
Artifact. Walk limits and manifests are in the [reference](declarations.md#artifactsum); the script form is under
[fingerprint scripts](declarations.md#fingerprint-scripts).

## The reuse key

An eval depends on its target Artifact and on what the target directly connects:

- the target's child Artifacts;
- the Artifacts the eval names in its instruction or runtime args.

artifactize does not follow connections further than that. Whether a change two
connections away matters is up to how you define fingerprints: a fingerprint
script can read a file of any Artifact it names in its args. The key is

```text
hash(Eval definition hash, sorted (Artifact name, kind, fingerprint) of each covered Artifact)
```

Artifact names and kinds (`folder`, `file` or `graph`) are part of the key, for the
target and every covered dependency. A change from folder to file never reuses
the former result, even if a script emits the same fingerprint. Physical paths
are never key inputs, so copies with matching names, kinds and fingerprints reuse.
Changing a covered Artifact's name changes the key. Differently named targets can
still share a result when their evaluation criteria and complete covered Artifact
maps match: the key does not separately identify which covered Artifact is the target.
For example, two Artifacts whose identical evals reference each other can share results.
The **Eval definition hash** changes whenever one of the eval's **evaluation
criteria** changes:

- the eval kind (runtime, agent or human);
- `payload`, including the instruction;
- `pass_schema` and `fail_schema`;
- for runtime evals, the command and args.

**Evaluation execution options** are not part of the key: an Agent's backend, model, reasoning,
`timeout_ms`, `max_tool_calls` and `max_tokens`, a runtime `timeout_ms`, and the selected
profile variant. Neither are the eval id and title, tags, repository paths, unused
profile variants, or tool declarations (`views`): a tool is a way of viewing an
Artifact, so changing only a tool's description or schema does not review again. To
make a tool change matter, include the relevant files in the fingerprint.

An executable eval has a key only when its target and every Artifact it depends on
have a fingerprint. Omission supplies artifactsum for folder and file Artifacts,
including basis Artifacts of those kinds; graph Artifacts always supply their
structural graph fingerprint. If a folder or file Artifact declares
`fingerprint = false`, the eval has no reuse: every `verify` reviews it
again. For an unforced READY Runtime/Agent eval, `status` names the disabled target
or dependency in its execution reason. A READY Human eval instead gives the
request-awaiting-claim-and-submission reason; forced, blocked and waiting evals
use their respective execution, gate or wait reasons.

## Completed result reuse

A GREEN or RED result whose inputs were unchanged at the end of its review is kept
for its key ([cache inspection](../cli/maintenance.md#cache-inspection)). Errors and
cancellations are never kept. When a key holds more than one record,
the most recent by completion time is reused, GREEN or RED. Results from different
profiles therefore reuse each other: a review that `--profile fast` produced
satisfies the declared profile, and the other way around.

Records retain their producing profile and provenance; the [Run result
reference](run.md#verify-and-runs) owns the audit-field and source-kind inventory.

Before accepting reuse, artifactize validates admitted file targets again:
they must exist as regular files without symlink traversal. `fingerprint = false`
disables reuse, not these file-target safety checks. Targets are checked during
fingerprint preparation/recheck, at eval start/end and on every tool call too.

A hit returns the original result without running the eval or re-validating it
against the requested profile or schema. The producing profile remains distinct
from the requested one, and a reused request spends no new usage. A reused RED
remains RED for gates and final obligations. Dependencies outside execution
selection can supply cached evidence without running. Results remain readable
after the source repository is deleted; external paths embedded in result text
are not made portable.

With a [team review store](../cli/review-store.md#remote-review-store-client),
`verify` also asks the store for every key and reuses whichever record completed
last, the local latest or the store's latest; a store that cannot be reached leaves
the local latest.

No key means no cache lookup or publication. `--force` re-executes explicitly
selected executable evals without reading or joining earlier results, local or in
the store, and its completed GREEN or RED is appended as a newer record (and
published to a configured store), which later runs then reuse. Forced and unkeyed
results still satisfy their own Run and keep their execution.

## Upgrading to 0.9

For declaration, state-directory and terminal changes, see
[Install: Upgrading to 0.9](../getting-started/install.md#upgrading-to-09).
Reuse-key v2 matches no pre-0.9 result, including script fingerprints and team
review store records. The first full `verify` therefore reviews every executable
eval once, Human sign-offs included; later Runs reuse normally.

## Graph fingerprint

The root [`.artf` graph Artifact](declarations.md#graph-artifact)
uses `graph:<sha256>`, which changes when the project's structure or criteria change,
not when files move or their content changes. It covers:

- Artifact names and kinds, `basis`, and fingerprint coverage: artifactsum `files`
  and `ignore`; script `command`, `args` and `files`; or `fingerprint = false`.
  
- Input → consumer relations: child ownership (names, not child folder paths),
  instruction references, runtime argv references
  (including eval ID, argv index and logical suffix), and their input targets.
- Qualified eval IDs, owning Artifact, titles, optional `gate` and evaluation criteria: default
  profile kind, the full `payload` including instruction and owner context,
  `pass_schema`, `fail_schema`, and runtime `command`/`args`.

Excluded are evaluation execution options (backend, model, reasoning, all
`timeout_ms` values including fingerprint-script timeouts, `max_tool_calls`,
`max_tokens`, `profile_variants`, eval `mutex` tags), all tool views and sessions, Artifact tags, repository
paths and source file contents. Declared coverage paths and scoped argument
suffixes remain meaningful; a relocation that preserves those logical declarations
and ownership does not change the graph fingerprint. Execution options are still
saved in ordinary state records.

This is only a fingerprint algorithm. Existing reuse keys, Runs, status and review
flows decide whether to reuse or review again. The graph is evaluated against
current state, never a stored diff or structural history. A graph eval title
change invalidates its graph fingerprint even though titles are excluded from the
ordinary Eval definition hash. Both built-in `graph` tools use this same extraction.
