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

Execution gates belong to each eval’s own references. Mounts and children alone
do not block sibling evals. See [Eval dependencies](#eval-dependencies).

The graph itself can be reviewed: a nameless root `.artf` declares the Graph
Artifact. Its fingerprint covers extracted criteria and labeled relations, not
source bytes or execution tuning. Use it for a person's review of the project's
review structure. [Declarations](declarations.md#graph-artifact) gives the syntax;
the [graph tool](tools/builtin.md#current-project-graph) shows the full current
structure to an agent or opens it in Desktop for a person.

## Folder configuration

A folder with an `index.artf` is an Artifact. `name` is required; `mounts`,
`basis`, `views`, `fingerprint` and `tags` are optional. For older declarations,
see the [legacy policy upgrade reference](../getting-started/install.md#removed-gate-bypass-settings).

Evals are tables keyed by their local ID: `[evals.heading]` becomes the qualified ID
`usage/heading` on the `usage` Artifact. Do not declare an `id` field. Each eval
has a `title` and a `profile` whose `kind` is `runtime`, `agent`,
or `human`. All evals require `payload.instruction` and
may have `pass_schema`, `fail_schema` and `profile_variants`.

A runtime eval's exit code is its verdict (0 is GREEN); an Agent or a Human returns
GREEN or RED with owner fields. Dependencies come from each eval’s own references.

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

An Eval is blocked, never an Artifact. An Eval is BLOCKED when an Artifact it
depends on has a RED Eval: that content will change, so the Eval's result would
not be fresh, and it does not run. The Eval depends on the Artifacts it references
and on everything upstream of them, so a BLOCKED input Eval blocks it in turn: a
RED sits further upstream.

Its own Artifact and the Artifacts in a dependency cycle with it are excluded.
Cycle peers are evaluated together: none can be fixed first. Cycles are the ones
Eval references form; a mount or child alone never makes Artifacts cycle peers,
just as it never gates. A RED sibling Eval on the same Artifact or a
RED Eval of a cycle peer never blocks an Eval or replaces its actual verdict, and
peers never wait for one another. A reference to a cycle peer stands for what the
cycle references outside itself; a reference to an Artifact in another cycle
stands for every Eval of that cycle. Waiting follows the same set: plain `verify`
holds an Eval only for those Evals, and `--speculate` cancels only Evals that a
RED makes invalid this way. Every result in a cycle must pass before consumers
outside the cycle can proceed.

A BLOCKED Eval's reason names the RED Evals at the root of the block, for example
`Dependency verdict RED: core/tests`, never Evals that were merely still running.
In a speculative Run an upstream ERROR blocks too and is named as
`Dependency ERROR: …`. `blockedBy` lists the same Evals, and the settled Run names
every one of them.

## The fingerprint field

Omit `fingerprint` for **artifactsum**, the built-in hash of the Artifact's own
files. `fingerprint = false` disables reuse; a script can define another value.
[Fingerprint](fingerprint.md) explains the forms, covered inputs and GREEN/RED
result reuse.

## Tags and validation

`tags = ["type:code", "scope:player"]` adds display-only metadata. Tags are at most 64 unique nonblank strings
without ASCII control characters. `status`, `config graph`, `monitor` and their JSON
Artifact output show them. Tags change no fingerprint, Eval definition hash, reuse
key, relation or selection of `verify` and `status`; there are no tag filters for those and no
relationship rules. Only the `artifactize review` search filters on tags, with `tag:` terms.

Projects that want templates generate their own `.artf` files and commit them.
Discovery and validation rules are in [Declaration validation](declarations.md#declaration-validation).
