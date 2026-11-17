# Fingerprints and reuse

In a project with reusable, file-backed checks, run `artifactize verify --all --wait`
twice: the second time its GREEN results are reused and nothing runs again. Live
runtime observations run each time, as explained below. Change one file and verify again: only the evals
that depend on that file run. A fingerprint says what an Artifact's reviews depend on. An
eval's earlier result is reused when the eval is unchanged and the fingerprints of the
Artifacts it depends on are unchanged, so review cost follows the size of a change.

## Fingerprints

Folder, file and aspect Artifacts use **artifactsum** by default. Omit `fingerprint` to
hash their own files, or choose a script. This includes nodes targeting
[definition files](#graph-fingerprint); there is no separate graph kind. The
configurable forms below add no tool declarations or other Artifact's value; dependencies enter
through the [reuse key](#the-reuse-key).

- Omission uses artifactsum: `artifactsum:` plus SHA-256 over the Artifact's
  own input paths and bytes. `fingerprint = {}` also works for folders. A folder hashes its own files;
  a file Artifact hashes its one target file, and an [aspect](declarations.md#aspects) its selected paths.
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

- `files`: 1–64 unique target-relative paths, up to 1024 characters each (a character
  outside the Basic Multilingual Plane counts as two). The default is `["."]`.
  Every part's target is excluded, including its parts, whether it is a folder, file or
  aspect selection. A folder part's files cannot be used as the whole's explicit inputs.
  Overlapping Artifacts that are not parts hash the shared files independently.
  `.artf` files are ordinary inputs, with the definition view described below.
- `ignore`: up to 64 unique `.gitignore`-style globs relative to the owner, without
  negation; a pattern may not be blank, start with `#` or contain control characters. They always exclude, like the built-in ignores `.git`,
  `__pycache__/`, `*.pyc`, `target/` and `node_modules/`. `.gitignore` files
  exclude more, with git semantics: every one from the repository root (`--repo`)
  down through the Artifact and walked directories applies. A pattern is relative
  to its own file's folder; negation works, and deeper files override shallower
  ones. Explicitly named `files` are never ignored.

File Artifacts hash only their target filename. An explicit artifactsum form
must name just that file, for example `fingerprint = { files = ["hero.png"] }`.
`fingerprint = {}`, `files = ["."]` and `ignore` (even `[]`) are rejected for a
file Artifact. Omit the field for its default; use a script if the fingerprint
needs to cover more than the file.

The old `dependencies` fingerprint option is rejected: a fingerprint covers only its own
Artifact. Walk limits are in the [reference](declarations.md#artifactsum); the script form is under
[fingerprint scripts](declarations.md#fingerprint-scripts).

## The reuse key

An eval's key combines the fingerprints of its target Artifact, all of the target's
[parts](artifact.md#composition) (their parts included), and the Artifacts the eval
names in its instruction or runtime args. Each covered Artifact contributes its
name, kind and own fingerprint once. Changing a file part or a nested folder part
reviews the whole's evals again, just as changing the whole's own files does. `status` can name the changed part separately from the whole's own
changed files. An eval that needs only a part should target that part.

References still stop at one hop: naming another Artifact does not add its parts
or the Artifacts its evals name to this key. A fingerprint script can read a file
of any Artifact it names in its args. A result is reviewed again when the eval's
criteria change, or the name, kind or fingerprint of any covered Artifact changes.
Composition adds key inputs, not tool access. To use a part's tools or refer to
its files as `{part}`, name it in the eval.

After upgrading to composition-aware keys, the first `verify` reviews evals whose
target has parts once, including Human sign-offs, even if their scripts return
unchanged values or the eval already names those parts. Later Runs reuse normally.
Saved Runs and results remain readable; no state reset is needed.

Artifact names and kinds (`folder`, `file`, `aspect` or `live`) are part of the key, for the
target and every covered dependency. A change from folder to file never reuses
the former result, even if a script emits the same fingerprint. Physical paths
are never key inputs, so copies with matching names, kinds and fingerprints reuse.
Changing a covered Artifact's name changes the key. Two differently named Artifacts can
still share a result: for example, when each declares the same eval that reads the other,
both evals cover the same Artifacts with the same criteria and reuse one result.
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
have a fingerprint. Omission supplies artifactsum for folder, file and aspect
Artifacts, including basis Artifacts of those kinds. Live Artifacts have no
snapshot unless they declare a script, and runtime evals covering Live never
reuse even with one. If a covered Artifact declares `fingerprint = false`,
the eval has no reuse: every `verify` reviews it
again. `status` explains each eval's state and action; see
[Status states](../cli/index.md#status-states).

## Completed result reuse

A GREEN or RED result whose inputs were unchanged at the end of its review is kept
for its key ([cache inspection](../cli/maintenance.md#cache-inspection)). Errors and
cancellations are never kept. When a key holds more than one record,
the most recent by completion time is reused, GREEN or RED. Results from different
profiles therefore reuse each other when their criteria and inputs are the same: a review
that an Agent variant such as `--profile fast` produced satisfies the declared profile, and
the other way around. A runtime variant whose command or arguments differ has its own key,
and may name other inputs.

A reused result still names the command or model that produced it and where it came
from; [Verify and Runs](run.md#verify-and-runs) shows where `run show` lists both.

Before accepting reuse, artifactize validates admitted file targets again:
they must exist as regular files without symlink traversal. `fingerprint = false`
disables reuse, not these file-target safety checks. Targets are checked during
fingerprint preparation/recheck, at Runtime/Agent start/end and on every tool call.
An unkeyed Human submission without Live inputs does not recheck target availability.

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

## Definition-file fingerprints {#graph-fingerprint}

Definition files use ordinary artifactsum, including those inside nested projects.
Their default [artf view](declarations.md#graph-artifact) leaves positions, notes and
formatting out of reuse decisions. See that page for which edits count, what file tools
show and how to limit a definition review's scope.

## Live snapshots and observations

For a [Live Artifact](declarations.md#live-artifacts), a script supplies the observed-state
snapshot. Omitting it means there is no snapshot and no reusable key. Agent and Human
reviews reuse while their covered snapshots stand; a snapshot changing during review
produces `INPUT_CHANGED`. Every runtime eval that covers a Live Artifact runs on every
verify, unchanged snapshot or not. A snapshot does not prove the service still works.
