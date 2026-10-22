# Artifact

An Artifact is anything you review: code, docs, designs, images. Declare one by adding
an `index.artf` to a folder (or `notes.md.artf` beside a file):

```toml
name = "docs"

[evals.links]
title = "Links resolve"
profile = { kind = "runtime", command = "python3", args = ["check_links.py"] }
payload.instruction = "Check that every link in the docs resolves."
```

Then run `artifactize config check`; it prints `Folder configuration is valid.` or the
error with its file, line and column. artifactize reads these declarations into one
dependency graph.

## Artifact Graph

The **Artifact Graph** connects inputs to the evals that consume them. A reference
such as `{spec}` admits that Artifact to an eval; changes to its fingerprint
invalidate only reviews whose keys cover it. Child folders organize inputs, but
sharing a repository alone does not make every Artifact a dependency of every check.

Execution gates belong to each eval’s own references. A child folder is part of its
parent's content, so it counts as a reference of every eval of the parent. See
[Eval dependencies](#eval-dependencies).

The graph itself can be reviewed: a nameless root `.artf` declares the Graph
Artifact. Its fingerprint covers extracted criteria and labeled relations, not
source bytes or execution tuning. Use it for a person's review of the project's
review structure. [Declarations](declarations.md#graph-artifact) gives the syntax;
the [graph tool](tools/builtin.md#current-project-graph) shows the full current
structure to an agent or opens it in Desktop for a person.

## Folder configuration

A folder with an `index.artf` is an Artifact. `name` is required; `basis`,
`views`, `fingerprint` and `tags` are optional. For older declarations,
see the [legacy policy upgrade reference](../getting-started/install.md#upgrading-from-011).

Evals are tables keyed by their local ID: `[evals.heading]` becomes the qualified ID
`usage/heading` on the `usage` Artifact. Do not declare an `id` field. Each eval
has a `title` and a `profile` whose `kind` is `runtime`, `agent`,
or `human`. All evals require `payload.instruction` and
may have `pass_schema`, `fail_schema` and `profile_variants`.

A runtime eval's exit code is its verdict (0 is GREEN); an Agent or a Human returns
GREEN or RED with owner fields. Dependencies come from each eval’s own references
and its Artifact's children.

This is the `guide` Artifact of the [runtime-relations example](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), whose README explains children,
`{artifact}` references and basis Artifacts:

```toml
name = "guide"

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{glossary}/terms.txt", "{guide}/intro/page.md", "{usage}/page.md"], timeout_ms = 10000 }
payload.instruction = "Check that every bold term in {intro} and {usage} is defined in {glossary}."
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
names an input Artifact; that Eval depends on the Evals judging the input. If an
Artifact uses another one, the eval that checks that use names it as `{name}`;
nothing else adds a dependency.

A child is the one link no eval writes. A folder Artifact's files include its child
folders, so a child counts exactly as if every eval of its parent referenced it:
each eval of the parent depends on the evals of its nearest marked children.

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

Gates apply to each Eval. An Artifact shows ERROR or RED if one of its Evals is, else BLOCKED if one is blocked. An
Eval's execution gates are the Evals of its referenced Artifacts and of its Artifact's
nearest child Artifacts. A RED
or BLOCKED gate blocks it in turn, so blocking propagates along these Eval gates.
A RED Eval of a child blocks every Eval of its parent. An intermediate Artifact
with no Evals supplies no execution gates: referring to it does not gate execution
on its own inputs or children. Those still belong to the final approval obligations,
so the consumer may execute without the Run being able to finish GREEN.

Artifacts whose evals reference each other, directly or through a chain, form a
**cycle**; a child that references its parent forms one with it. Evals in a cycle do not
gate each other. For example, if `docs`'s eval references `cli` and `cli`'s eval
references `docs`, a RED `cli/tests` does not block `docs/matches-cli` or replace its
verdict. Both run independently and must pass before consumers outside the cycle can
proceed.

Inputs from outside the cycle still gate all of it: if `cli`'s eval also references
`core`, a RED `core/tests` blocks both `cli/tests` and `docs/matches-cli`. An eval
elsewhere that references `docs` waits for every eval of `docs` and `cli`. An Eval's
own Artifact is likewise not a gate for it, apart from the
[Human sibling prerequisite](#human-sign-offs). These gates also decide what waits and
which speculative work is cancelled.

A BLOCKED Eval's reason names the RED Evals at the root of the block, for example
`Dependency verdict RED: core/tests`, never Evals that were merely still running.
In a speculative Run an upstream ERROR blocks too and is named as
`Dependency ERROR: …`. `blockedBy` lists the same Evals, and the settled Run names
every one of them.

### Human sign-offs

A Human Eval starts only when every other non-Human Eval of its own Artifact is
GREEN, on top of its dependency gates: a person signs off only on what the machine
Evals already pass. Until then it waits. When one of those machine Evals is RED or
ERROR, the Human Eval does not start and is BLOCKED, and its reason names them, for
example `Sibling machine eval RED: spec/review`. Other Human Evals of the same
Artifact are not part of this condition, so two sign-offs never wait for each other.
The rule is the same in plain `verify`, with `--speculate` and with `--defer-human`:
even a speculative Run does not ask a person before those machine Evals are GREEN.

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
