# Artifacts and evals

An Artifact is anything you review: code, docs, designs, images. A folder or a
single file declares its evals in a static TOML `.artf` file. artifactize reads
these declarations into one dependency graph.

## Folder configuration

A folder with an `index.artf` is an Artifact. `name` is required; `mounts`,
`basis`, `views`, `fingerprint` and `tags` are optional. Gate bypass declarations
(`review_policy.dependency_gates`, including the `review_policy` table) are removed;
choose `--speculate` and `--defer-human` per verification request instead.

Evals are tables keyed by their local ID: `[evals.heading]` becomes the qualified ID
`usage/heading` on the `usage` Artifact. Do not declare an `id` field. Each eval
has a `title` and a `profile` whose `kind` is `runtime`, `agent`,
or `human`. All evals require `payload.instruction` and
may have `pass_schema`, `fail_schema` and `profile_variants`.

A runtime eval's exit code is its verdict (0 is GREEN); an Agent or a Human returns
GREEN or RED with owner fields. Dependencies come from each Eval’s own references.

This is the `guide` Artifact of the [runtime-relations example](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), whose README explains children,
mounts, `{artifact}` references and basis Artifacts:

```toml
name = "guide"
mounts = { terms = "glossary" }

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{terms}/terms.txt", "{guide}/intro/page.md", "{usage}/page.md"], timeout_ms = 10000 }
payload.instruction = "Check that every bold term in {intro} and {usage} is defined in {terms}."
```

Declaration keys are snake_case. JSON Schema keywords such as `minLength` and
`additionalProperties` keep their standard names; owner-defined payload fields keep their chosen
names. `--json` output and stored state still use fields such as `passSchema`,
`timeoutMs` and `agentTools`. See [Declarations (.artf)](../reference/declarations.md) for the exact syntax.

## File Artifacts

A sidecar `hero.png.artf` next to a regular file `hero.png` declares that file as an
Artifact. It uses the same schema and requires its own globally unique `name`:

```toml
# hero.png.artf
name = "hero-art"
tags = ["type:image", "scope:player"]

[evals.approved]
title = "The hero sprite matches the style guide"
profile = { kind = "human" }
payload.instruction = "Check {hero-art} against {style-guide}."
```

The working directory is the containing folder, but built-in tools see only the
file, its mounts and the referenced Artifacts, not unrelated sibling files.
`{hero-art}` resolves to the file path and cannot have a `/path` suffix.
`list .` uses a virtual root containing the filename and mounts; paths inside it
are containing-folder-relative. Its default artifactsum hashes just that file.
An explicit artifactsum `files` list may name only `hero.png`; `ignore`, even an
empty list, is rejected. `fingerprint = {}` is not a file default: omit the field.
The sidecar itself must be a regular file, so a directory named `bundle.artf`
also fails discovery. Filenames containing `:` are rejected. `.artfignore` can
exclude an individual sidecar as well as whole folders.

A mount alias may shadow unrelated sibling entries, but not the target filename.
File targets must remain regular, present and free of symlink traversal. They are
validated at preparation/recheck, before reuse, at eval start/end and on every
tool call, even with `fingerprint = false`. Invalid input is an operational error,
not a RED review.

A file Artifact is not a child or dependency of the surrounding folder Artifact. The
folder's artifactsum still includes the target file and excludes all `*.artf`
declarations. By contrast, a subfolder's `index.artf` makes it a child of the nearest
Artifact folder above it. A file named `index` cannot have a sidecar: `index.artf`
always declares the folder. `.artf` files are never Artifact targets.

## Eval dependencies

A dependency belongs to the Eval that uses the input, not every Eval on its
Artifact. A `{name}` reference in `payload.instruction` or a runtime `args` operand
names an input Artifact; that Eval depends on the Evals judging the input.
Mount aliases resolve to their named Artifacts. Mounts and children define visible
material and fingerprint inputs; visibility alone does not gate sibling Evals.

For example, application E2E execution belongs to the application:

```toml
name = "app"

[evals.e2e]
title = "The application passes its end-to-end suite"
profile = { kind = "runtime", command = "node", args = ["{e2e-tests}/run.mjs", "{app}"] }
payload.instruction = "Run {e2e-tests} against {app}."
```

The `e2e-tests` Artifact evaluates the test code itself: its style and fidelity to
the feature documents. It does not execute the application suite as a review of
its own test code. An independent `app` style Eval does not wait for the test-code
reviews unless its own instruction references them.

All Evals can judge their own targets independently of their inputs' verdicts.
Plain `verify` chooses dependency-first scheduling for efficiency: pending input
reviews delay the consumer Eval; RED input reviews block it. `--speculate` lets
work start before unresolved input gates, without approving missing requirements.
Final GREEN still requires all required evidence, including upstream reviews.

Cycles are condensed at the Eval level. Internal peers do not wait for each other,
and an internal RED does not cancel a peer or replace its actual verdict with
BLOCKED. External gates still apply to the entire cycle. Every internal result
must pass before downstream consumers outside the cycle can proceed.

## The fingerprint field

Omit `fingerprint` to use **artifactsum**, the built-in hash of the Artifact's own
files, with the `artifactsum:` prefix. A folder hashes its own files and excludes child
Artifact folders; a file Artifact hashes its one target file.

- `fingerprint = { files = ["src"], ignore = ["*.log"] }` narrows artifactsum's
  folder inputs. With an empty table, a folder's defaults apply. A file Artifact
  can explicitly list only its target filename and cannot declare `ignore`.
- `fingerprint = { script = { command = "/bin/sh", args = ["fingerprint.sh"] } }`
  uses an owner-written [fingerprint script](../reference/declarations.md#fingerprint-scripts),
  optionally with `files` (paths that must exist, never hashed) and `timeout_ms`.
- `fingerprint = false` disables reuse for this Artifact and executable evals
  whose keys would depend on it. `fingerprint = true` is rejected.

While the eval definition, Artifact kinds and all fingerprints in its key are
unchanged, the prior
GREEN or RED verdict is reused. See [Fingerprints and reuse](fingerprints-and-reuse.md).

## Tags and validation

`tags = ["type:code", "scope:player"]` adds display-only metadata. Tags are at most 64 unique nonblank strings
without ASCII control characters. `status`, `config graph`, `monitor` and their JSON
Artifact output show them. Tags change no fingerprint, Eval definition hash, reuse
key, relation or selection of `verify` and `status`; there are no tag filters for those and no
relationship rules. Only the `artifactize review` search filters on tags, with `tag:` terms.

Projects that want templates generate their own `.artf` files and commit them.
Discovery and validation rules are in [Declaration validation](../reference/declarations.md#declaration-validation).
