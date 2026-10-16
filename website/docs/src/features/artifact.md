# Artifact

An Artifact is anything you review: code, docs, designs, images. A folder or a
single file declares its evals in a static TOML `.artf` file. artifactize reads
these declarations into one dependency graph.

## Artifact Graph

The **Artifact Graph** connects inputs to the evals that consume them. A reference
such as `{spec}` admits that Artifact to an eval; changes to its fingerprint
invalidate only reviews whose keys cover it. Child folders and mount aliases
organize inputs, but sharing a repository alone does not make every Artifact a
dependency of every check.

These relations also create shared execution gates for the Artifact (or its cycle
component). A reference in one eval can therefore delay another eval on that
Artifact without becoming part of that other eval's reuse key. See
[shared execution gates](run.md#shared-execution-gates) for an example.

The graph itself can be reviewed: a nameless root `.artf` declares the Graph
Artifact. Its fingerprint covers extracted criteria and labeled relations, not
source bytes or execution tuning. Use it for a person's review of the project's
review structure. [Declarations](declarations.md#graph-artifact) gives the syntax;
the [graph tool](tools/builtin.md#current-project-graph) shows the full current
structure to an agent or opens it in Desktop for a person.

## Folder configuration

A folder with an `index.artf` is an Artifact. `name` is required; `mounts`,
`basis`, `views`, `fingerprint` and `tags` are optional. Gate bypass declarations
(`review_policy.dependency_gates`, including the `review_policy` table) are removed;
choose `--speculate` and `--defer-human` per verification request instead.

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
`timeoutMs` and `agentTools`. See [Declarations (.artf)](declarations.md) for the exact syntax.

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

A file Artifact hashes its one target by default. Its working directory is the
containing folder, but built-in tools see the file and its admitted scope, not
unrelated siblings. The sidecar does not make it a child of the surrounding folder;
that folder still hashes the target as part of its own files. A folder declared
with `index.artf`, by contrast, is a child of the nearest Artifact folder above it.
See the [file-Artifact reference](declarations.md#file-artifacts) for naming,
fingerprint forms, virtual paths and invalid-target/revalidation rules.

## Dependency evals

Use a dependency eval to name a readiness condition over other Artifacts:

```toml
# player/index.artf
name = "player"

[evals.ready]
title = "Movement, art and audio are ready"
profile = { kind = "dependency", depends_on = ["player-movement", "hero-art", "player-audio"] }
```

The listed Artifacts supply readiness evidence: all required GREEN means GREEN;
a required RED or BLOCKED result blocks it; missing or pending evidence waits.
`artifactize verify player/ready` selects it and includes the required evals,
without needing `--recursive`. It runs no reviewer and caches no verdict. The
[dependency-eval reference](declarations.md#dependency-evals) gives allowed fields,
selection, cycle and provisional-result rules.

## The fingerprint field

Omit `fingerprint` for **artifactsum**, the built-in hash of the Artifact's own
files. `fingerprint = false` disables reuse; a script can define another value.
[Fingerprint](fingerprint.md) explains the forms, covered inputs and GREEN/RED
result reuse. Dependency verdicts are derived again rather than reused.

## Tags and validation

`tags = ["type:code", "scope:player"]` adds display-only metadata. Tags are at most 64 unique nonblank strings
without ASCII control characters. `status`, `config graph`, `monitor` and their JSON
Artifact output show them. Tags change no fingerprint, Eval definition hash, reuse
key, relation or selection of `verify` and `status`; there are no tag filters for those and no
relationship rules. Only the `artifactize review` search filters on tags, with `tag:` terms.

Projects that want templates generate their own `.artf` files and commit them.
Discovery and validation rules are in [Declaration validation](declarations.md#declaration-validation).
