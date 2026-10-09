# Artifacts and evals

An Artifact is anything you review: code, docs, designs, images. A folder or a
single file declares its evals in a static TOML `.artf` file. artifactize reads
these declarations into one dependency graph.

## Folder configuration

A folder with an `index.artf` is an Artifact. `name` is required; `mounts`,
`basis`, `views`, `fingerprint` and `tags` are optional. `review_policy` belongs
only to the repository root's `index.artf`.

Evals are tables keyed by their local ID: `[evals.heading]` becomes the qualified ID
`usage/heading` on the `usage` Artifact. Do not declare an `id` field. Each eval
has a `title` and a `profile` whose `kind` is `runtime`, `agent`,
`human` or `dependency`. Runtime, Agent and Human evals require `payload.instruction` and
may have `pass_schema`, `fail_schema` and `profile_variants`.

A runtime eval's exit code is its verdict (0 is GREEN); an Agent or a Human returns
GREEN or RED with owner fields. A dependency eval derives its state from other
Artifacts without executing anything.

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
also fails discovery.

A file Artifact is not a child or dependency of the surrounding folder Artifact. The
folder's artifactsum still includes the target file and excludes all `*.artf`
declarations. By contrast, a subfolder's `index.artf` makes it a child of the nearest
Artifact folder above it. A file named `index` cannot have a sidecar: `index.artf`
always declares the folder. `.artf` files are never Artifact targets.

## Dependency evals

Use a dependency eval to name a readiness condition over other Artifacts:

```toml
# player/index.artf
name = "player"

[evals.ready]
title = "Movement, art and audio are ready"
profile = { kind = "dependency", depends_on = ["player-movement", "hero-art", "player-audio"] }
```

`depends_on` lists 1–64 unique Artifact names or the declaring Artifact's mount
aliases. Unknown targets, self-dependencies and aliases that resolve to duplicate
targets are rejected. Dependency evals cannot declare `payload`, `pass_schema`,
`fail_schema` or `profile_variants`; put any explanation in `title`.

The eval is GREEN when every listed Artifact's required evals are GREEN. A RED or
BLOCKED required eval makes it BLOCKED. Missing, stale, ERROR, cancelled or
Human-waiting evidence makes it WAIT_DEPENDENCY. A target with no evals, including a
basis Artifact, fulfills this dependency condition. A basis cannot own evals. The
dependency condition does not waive ordinary final Run obligations, including the
targets' dependency scopes.

`artifactize verify player/ready` selects the qualified eval directly and includes the required dependency
evals automatically, without `--recursive`. The dependency eval runs no command or
model, takes no execution budget or backend slot, and has no reuse key, cache record
or team-store result. Its verdict comes from current evidence, even with `review_policy.dependency_gates = "ignore"`
or `--ignore-gates`. Dependency-eval wait cycles are rejected; ordinary Artifact cycles
retain their existing scheduling rules.

[Runs, status and validation](runs-and-status.md) explains derived requests and unfulfilled dependency output.

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

While the eval definition and all fingerprints in its key are unchanged, the prior
GREEN or RED verdict is reused. See [Fingerprints and reuse](fingerprints-and-reuse.md). Dependency verdicts are derived
again rather than reused.

## Tags and validation

`tags = ["type:code", "scope:player"]` adds display-only metadata. Tags are at most 64 unique nonblank strings
without ASCII control characters. `status`, `config graph`, `monitor` and their JSON
Artifact output show them. Tags change no fingerprint, Eval definition hash, reuse
key, relation or selection; there are no tag filters or relationship rules.

Projects that want templates generate their own `.artf` files and commit them.
Discovery and validation rules are in [Declaration validation](../reference/declarations.md#declaration-validation).
