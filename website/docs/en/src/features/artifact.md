# Artifact

An Artifact is anything you review: code, docs, designs, images. Declare one by adding
a named definition in `.artifactize/` with the target's `path`:

```toml
path = "docs/"

[evals.links]
title = "Links resolve"
profile = { kind = "runtime", command = "python3", args = ["check_links.py"] }
payload.instruction = "Check that every link in the docs resolves."
```

Then run `artifactize config check`; it prints `Folder configuration is valid.` or the
error: with its file, line and column for a mistake inside a declaration, or with the
path in a note when a target is missing or has the wrong kind. Missing targets become per-eval preparation errors at verify, not invalid definitions. artifactize reads these declarations into one
dependency graph.

## Artifact Graph

The **Artifact Graph** connects inputs to the evals that consume them. Writing a
reference such as `{spec}` in an eval lets that eval read `spec`; when `spec` changes,
the evals that name it and the evals of its wholes are reviewed again; unrelated
results are reused. Sharing
a repository or a folder alone does not make an Artifact an input of another one's check.

Parts add a second, separate relation: [composition](#composition). When `combat` is a
part of `game`, `game` cannot pass while `combat` fails. Folder parts must be named to
read them; file and aspect parts remain readable through the whole. Naming a part also
admits its own tools and dependency gates.

Each eval also waits for the evals that judge the inputs it names; see
[Eval dependencies](#eval-dependencies).

The definitions themselves can be reviewed: an ordinary Artifact targets `.artifactize/`
or one of its subfolders. Its artifactsum covers those definition files, without their
annotations or TOML formatting. No special graph kind is needed. [Declarations](declarations.md#graph-artifact) gives the syntax;
the [graph tool](tools/builtin.md#current-project-graph) shows the full current
structure to an agent or opens it in Desktop for a person.

## Folder configuration

Define a folder with a trailing slash in its PATH. For example,
`.artifactize/usage.artf` with `path = "docs/usage/"` defines `usage`.
The file name is required as its identity; a `name` key is an error. `basis`, `views`,
`fingerprint`, `tags` and `parts` are optional. The definition's folder and the
nesting of target paths create no composition: list every part in `parts`.
Existing projects move with [`config migrate`](../getting-started/update.md).

Evals are tables keyed by their local ID: `[evals.heading]` becomes the qualified ID
`usage/heading` on the `usage` Artifact. Do not declare an `id` field. Each eval
has a `title` and a `profile` whose `kind` is `runtime`, `agent`,
or `human`. All evals require `payload.instruction` and
may have `pass_schema`, `fail_schema` and `profile_variants`.

A runtime eval's exit code is its verdict (0 is GREEN); an Agent or a Human returns
GREEN or RED with owner fields.

This is the `guide` Artifact of the [runtime-relations example](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), whose README explains parts,
`{artifact}` references and basis Artifacts:

```toml
path = "guide/"
parts = ["intro", "usage"]

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{glossary}/terms.txt", "{intro}/page.md", "{usage}/page.md"], timeout_ms = 10000 }
payload.instruction = "Check that every bold term in {intro} and {usage} is defined in {glossary}."
```

Declaration keys are snake_case. JSON Schema keywords such as `minLength` and
`additionalProperties` keep their standard names; owner-defined payload fields keep their chosen
names. `--json` output and stored state still use fields such as `passSchema`,
`timeoutMs` and `agentTools`. See [Declarations (.artf)](declarations.md) for the exact syntax.

## File Artifacts

A definition `.artifactize/hero-art.artf` with `path = "hero.png"` targets that
regular file. It uses the same schema and has its globally unique name from the file:

```toml
# .artifactize/hero-art.artf
path = "hero.png"
tags = ["type:image", "scope:player"]

[evals.approved]
title = "The hero sprite matches the style guide"
profile = { kind = "human" }
payload.instruction = "Check {hero-art} against {style-guide}."
```

A file Artifact hashes its one target by default. Its working directory is the
containing folder, but built-in tools see the file and its admitted scope, not
unrelated siblings. A file becomes a [part](#composition) only when a whole names it
in `parts`. The whole then excludes that target from its own artifactsum and combines
the part's fingerprint in its eval reuse keys. The same rule applies to folder and
aspect parts; target nesting alone does nothing.
See the [file-Artifact reference](declarations.md#file-artifacts) for naming,
fingerprint forms, virtual paths and invalid-target/revalidation rules.

## Eval dependencies

A dependency belongs to the Eval that uses the input, not every Eval on its
Artifact. A `{name}` reference in `payload.instruction` or a runtime `args` operand
names an input Artifact; that Eval depends on the Evals judging the input. If an
Artifact uses another one, the eval that checks that use names it as `{name}`;
nothing else adds a dependency.

A [part](#composition) is not an input: an eval of `game` that runs the
whole game and so uses `combat` names `{combat}` like any other input.

For example, application E2E execution belongs to the application:

```toml
# .artifactize/app.artf
path = "app/"

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

Gates apply to each Eval. An Artifact shows ERROR or RED if one of its Evals is, else
BLOCKED if one is blocked. With `app/e2e` above, which references `e2e-tests`:

- A RED or BLOCKED Eval of `e2e-tests` blocks `app/e2e`, and an Eval that references
  `app` is blocked in turn.
- A RED part blocks nothing: if `combat` is a part of `game` and `combat/tests` is RED,
  the Evals of `game` still run, but `game` cannot pass (see
  [composition](#composition)).
- An Artifact with no Evals gates nothing. If `app/e2e` referenced a `bundle` Artifact
  without Evals, it would run without waiting for the parts of `bundle`. The Run still
  needs their results, so it cannot finish GREEN while one of them is RED.

Artifacts whose evals reference each other, directly or through a chain, form a
**cycle**. Only references make a cycle; parts count only toward the
[cycle peers](run.md#verify-and-runs) an Artifact selector runs. Evals in a cycle do not
gate each other, but they still wait for the cycle's inputs from outside it.

For example, `docs/matches-cli` references `cli`, and `cli/tests` references `docs` and
`core`:

| Result | What happens |
|---|---|
| `cli/tests` is RED | `docs/matches-cli` still runs and keeps its own verdict. |
| `core/tests` is RED | `cli/tests` is blocked, and so is `docs/matches-cli`, because it references `cli`. |
| An eval of `docs` or `cli` references neither the other nor `core` | It runs without waiting for them. The Run still needs every result to finish GREEN. |
| An eval elsewhere references `docs` | It waits for every eval of `docs` and `cli`. |

An Eval's own Artifact is not a gate for it either, apart from the
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

An Artifact can be made of others, its **parts**. Only `parts` creates composition;
neither nested target paths nor definition subfolders do.

```toml
# .artifactize/game.artf
path = "game/"
parts = ["combat", "physics"]
```

Give `combat` its own definition and `parts = ["fireball"]` to compose the next level.
Their targets can live anywhere in the project. A module shared by two applications is
named in both applications' `parts`. An Artifact cannot be a part of itself, directly
or through other parts. A node that reviews definitions follows these same rules.

Each part is an Artifact in its own right. An Artifact may hold no evals and only
gather its parts, or have content and evals of its own, such as checks that its parts
work together. Parts check what they own, so the Artifact they make up need not repeat
their checks:

| Artifact | Its own evals |
|---|---|
| `game` | The whole game starts and its parts work together |
| `combat` | Combat components work with each other |
| `fireball` | Damage, cooldown and effects |

Parts affect verdicts, selection and reuse:

- **A failing part fails what it is a part of.** `game` shows RED while `fireball` is
  RED, and passes only when every part, its parts' parts included, and its own evals
  pass. An Artifact without evals of its own has no review of its own to wait for: it
  shows its parts' states, UNREVIEWED included, and passes once they pass.
- **Selecting an Artifact selects its parts.** `verify game` runs or reuses the
  evals of `game`, `combat`, `fireball` and `physics`.
- **Needing an Artifact needs its parts.** An eval that names `{game}` needs
  `game` to pass to finish GREEN, so the Run needs its parts' results too. Its
  execution gates are still only `game`'s own evals.
- **A part change reviews the whole again.** Each Artifact's artifactsum excludes
  its parts' files, whether they are files, folders or aspect selections. The reuse
  key of a whole's eval combines its own fingerprint with those of all its parts,
  their parts included. Changing the fireball reviews the evals of `fireball`,
  `combat` and `game` again, and those that name `{fireball}`, not unrelated evals.
  Changing only `game`'s own files does not review the fireball again. Reuse does
  not grant access to a folder part: an eval of `game` that reads a folder-based fireball
  still names `{fireball}`. File and aspect parts remain readable through the whole;
  name the part to admit its own tools and dependency gates.

A part need not be a folder of its own. A [file Artifact](declarations.md#file-artifacts)
is one file, and an [aspect](declarations.md#aspects) selects the files of one part from a larger folder, such as the fireball's code, test
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
