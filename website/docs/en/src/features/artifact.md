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
error: with its file, line and column for a mistake inside a declaration, or with the
path for a problem such as a missing target file. artifactize reads these declarations into one
dependency graph.

## Artifact Graph

The **Artifact Graph** connects inputs to the evals that consume them. Writing a
reference such as `{spec}` in an eval lets that eval read `spec`; when `spec` changes,
only the evals that name it are reviewed again, and every other result is reused. Sharing
a repository or a folder alone does not make an Artifact an input of another one's check.

Names add a second, separate relation: [composition](#composition). `game::combat` is
part of `game`, so `game` cannot pass while `game::combat` fails, yet no eval of `game`
reads `game::combat` unless it names it.

Each eval also waits for the evals that judge the inputs it names; see
[Eval dependencies](#eval-dependencies).

The graph itself can be reviewed: a nameless root `.artf` declares the Graph
Artifact. Its fingerprint covers extracted criteria and labeled relations, not
source bytes or execution tuning. Use it for a person's review of the project's
review structure. [Declarations](declarations.md#graph-artifact) gives the syntax;
the [graph tool](tools/builtin.md#current-project-graph) shows the full current
structure to an agent or opens it in Desktop for a person.

## Folder configuration

A folder with an `index.artf` is an Artifact. `name` is required, and its leading parts
say what it is part of: `game::combat` is a constituent of `game` (see
[composition](#composition) and [names](declarations.md#declaration-validation)); `basis`,
`views`, `fingerprint` and `tags` are optional. For older declarations,
see the [legacy policy upgrade reference](../getting-started/update.md#upgrading-from-011).

Evals are tables keyed by their local ID: `[evals.heading]` becomes the qualified ID
`guide::usage/heading` on the `guide::usage` Artifact. Do not declare an `id` field. Each eval
has a `title` and a `profile` whose `kind` is `runtime`, `agent`,
or `human`. All evals require `payload.instruction` and
may have `pass_schema`, `fail_schema` and `profile_variants`.

A runtime eval's exit code is its verdict (0 is GREEN); an Agent or a Human returns
GREEN or RED with owner fields.

This is the `guide` Artifact of the [runtime-relations example](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), whose README explains constituents,
`{artifact}` references and basis Artifacts:

```toml
name = "guide"

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{glossary}/terms.txt", "{guide::intro}/page.md", "{guide::usage}/page.md"], timeout_ms = 10000 }
payload.instruction = "Check that every bold term in {guide::intro} and {guide::usage} is defined in {glossary}."
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
unrelated siblings. The sidecar relates it to nothing, the surrounding folder Artifact
included, which still hashes the target as part of its own files. A folder declared
with `index.artf` inside another Artifact's folder takes its files out of that
Artifact's; that is all nesting does. Relations come from names and references only.
See the [file-Artifact reference](declarations.md#file-artifacts) for naming,
fingerprint forms, virtual paths and invalid-target/revalidation rules.

## Eval dependencies

A dependency belongs to the Eval that uses the input, not every Eval on its
Artifact. A `{name}` reference in `payload.instruction` or a runtime `args` operand
names an input Artifact; that Eval depends on the Evals judging the input. If an
Artifact uses another one, the eval that checks that use names it as `{name}`;
nothing else adds a dependency.

A [constituent](#composition) is not an input: an eval of `game` that runs the
whole game and so uses `game::combat` names `{game::combat}` like any other input.

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

Gates apply to each Eval. An Artifact shows ERROR or RED if one of its Evals is, else BLOCKED if one is blocked. A RED
or BLOCKED gate blocks the Eval in turn, so blocking propagates along these Eval gates.
A RED constituent blocks no Eval of its parent; it keeps the parent from passing
instead (see [composition](#composition)). An intermediate Artifact
with no Evals supplies no execution gates: referring to it does not gate execution
on its own inputs or constituents. Those still belong to the final approval obligations,
so the consumer may execute without the Run being able to finish GREEN.

Artifacts whose evals reference each other, directly or through a chain, form a
**cycle**. Evals in a cycle do not
gate each other. For example, if `docs`'s eval references `cli` and `cli`'s eval
references `docs`, a RED `cli/tests` does not block `docs/matches-cli` or replace its
verdict. Both run independently and must pass before consumers outside the cycle can
proceed.

An eval that references a cycle peer still waits for that cycle's outside inputs: if
`cli`'s eval also references `core`, a RED `core/tests` blocks `cli/tests` and also
`docs/matches-cli`, which references `cli`. An eval of these Artifacts that references
no cycle peer or input is not held back by them, though the Run still needs all of
their results to finish GREEN. An eval
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
Evals already pass. Until then it waits. A machine Eval whose own verdict is GREEN
counts even while it shows PROVISIONAL because a sign-off it depends on is still
outstanding. When one of those machine Evals is RED or
ERROR, the Human Eval does not start and is BLOCKED, and its reason names them, for
example `Sibling machine eval RED: spec/review`. Other Human Evals of the same
Artifact are not part of this condition, so two sign-offs never wait for each other.
The rule is the same in plain `verify` and with `--speculate`: even a speculative Run
does not ask a person before those machine Evals are GREEN.

## Composition

An Artifact can be made of others. Its name says what it is part of: `game::combat` is
a **constituent** of `game`, and `game::combat::skills::fireball` of `game::combat`.
The **parent** is the nearest leading part of the name that is declared, so if no
Artifact is named `game::combat::skills`, `game::combat` is the fireball's parent. An
undeclared leading part, such as `engine` in `engine::core` while no Artifact is named
`engine`, only groups names for people.

Each level is an Artifact in its own right, wherever its files are: names, not folders,
make the composition. A parent may hold no evals and only gather its constituents, or
have content and evals of its own, such as checks that its parts work together.
Constituents check what they own, so the parent need not repeat their checks:

| Artifact | Its own evals |
|---|---|
| `game` | The whole game starts and its parts work together |
| `game::combat` | Combat components work with each other |
| `game::combat::skills::fireball` | Damage, cooldown and effects |

Composition decides verdicts, not reviews:

- **A failing constituent fails its parent.** `game` shows RED while
  `game::combat::skills::fireball` is RED, and passes only when every constituent and
  its own evals pass. A parent without evals of its own has no review of its own to wait
  for: it shows its constituents' states, UNREVIEWED included, and passes once they pass.
- **Selecting a parent selects its constituents.** `verify game` runs or reuses the
  evals of `game` and of every Artifact under it.
- **Needing a parent needs its constituents.** An eval that names `{game}` needs
  `game` to pass to finish GREEN, so the Run needs the constituents' results too. Its
  execution gates are still only `game`'s own evals.
- **Nothing else follows.** Composition does not divide files: each Artifact keeps the
  files its declaration covers. With separate files, a change to the fireball reviews
  again only the evals that name it, and a change to `game`'s own files does not review
  the fireball again. A file Artifact or an aspect can share files with its parent, and a
  change to a shared file reviews both again. An eval of `game` that uses the fireball
  names `{game::combat::skills::fireball}`.

A constituent need not be a folder of its own. An [aspect](declarations.md#aspects)
selects the files of one part from a larger folder, such as the fireball's code, test
and sprite, so that part can be reviewed and reused on its own.

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
