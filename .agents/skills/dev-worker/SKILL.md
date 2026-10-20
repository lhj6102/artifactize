---
name: dev-worker
description: Implement one development task in the artifactize repository, starting from the latest GREEN dev commit (the dev-green tag), verify it until GREEN and open a pull request into dev. Run only when invoked explicitly with the requirements.
disable-model-invocation: true
---

# dev-worker

You take one task in this repository from requirements to a pull request into
`dev` that is ready to merge. The requirements are the text given with the
invocation (`/dev-worker <requirements>` in Claude Code, `$dev-worker
<requirements>` in Codex). Without requirements, ask for them before doing
anything else.

## Branch model

- `main` is the release channel. Only the owner releases: a `dev` → `main` pull
  request, then a `v*` tag. Never push to `main`, open a pull request into it,
  tag or release.
- `dev` is where work comes together. It moves between RED and GREEN; the
  dev-master skill merges pull requests into it and drives it back to GREEN.
- `dev-green` is a tag on the latest `dev` commit whose full verify was GREEN
  and whose CI was green. Only dev-master moves it.
- Development branches (`feat/…`, `fix/…`, `docs/…`, `chore/…`) start at
  `dev-green`, reach verify GREEN and open a pull request into `dev`.

## 1. Start from dev-green

```sh
git fetch origin --prune --tags --force   # dev-green moves; --force updates it
git worktree add -b feat/<slug> .claude/worktrees/<slug> dev-green
git rev-parse dev-green                   # the start commit, for the pull request note
```

Work only in that worktree; never touch the main checkout or other worktrees.
If `dev-green` does not exist, ask which commit to start from.

Keep scratch files (the Cargo target directory, pull request drafts) in one
private directory outside the work tree, for example
`scratch=$(mktemp -d "${TMPDIR:-/tmp}/artifactize-<slug>-XXXX")`. Never write
into `/tmp` itself or the work tree, and never save a copy of a `*.artf` file in
a shared directory (use a `.txt` suffix): a stray declaration turns its folder
into an artifactize workspace for everyone. Remove your scratch directory when
you finish.

## 2. Read the rules for what you touch

- The `AGENTS.md` of each crate you change names its checklists in
  `project-rules/`. Rust code also follows `cargo fmt` and
  `cargo clippy --all-targets -- -D warnings`.
- The `.artf` declarations whose evals review your files say what each
  reviewer checks. Read them before writing, not after a RED.
- `website/README.md` for the site, `crates/artifactize-desktop/rules/` and
  `project-rules/DEV-FRONTEND.md` for the desktop UI.
- CI is fixed: never add jobs, steps, schedules or push triggers to
  `.github/workflows/`.

Rule documents in `project-rules/` and other Artifacts with a Human eval need
the owner's sign-off. When the task changes one, change only that first,
verify just that Artifact (the verify command in step 3 with its name in place
of `--all`), ask the owner to sign it off, and continue with the rest only after
the approval. Everything downstream of an unapproved rule might have to run
again.

## 3. Implement, then verify until GREEN

Commit with the git identity already configured; never change git
configuration or invent an identity. Then run the full verify from the work tree
root:

```sh
env -u DESKTOP_TEST_CANDIDATE CARGO_TARGET_DIR="$scratch/target" \
  cargo run -q -p artifactize -- verify --all --speculate --defer-human --wait
```

`env -u DESKTOP_TEST_CANDIDATE` makes the desktop end-to-end evals build their
candidate from this tree. The command prints the Run ID, the Run's outcome on
`Execution:`, and ends with `Validation: SATISFIED` or `Validation: NOT
SATISFIED`, the latter followed by `Unmet obligation:` lines. The exit code is
the outcome:

| Exit | Outcome | Next |
|---|---|---|
| 0 | GREEN (`Validation: SATISFIED`) | open the pull request |
| 5 | SIGNOFFS_PENDING: only Human sign-offs remain | open the pull request; name the sign-offs |
| 1 | RED | fix, verify again |
| 2 | ERROR | read it; fix or retry, verify again |
| 4 | INCOMPLETE | read the listed obligations, verify again |

Read a Run, its findings and the failing evals with:

```sh
"$scratch/target/debug/artifactize" run show RUN_ID --json
```

How to resolve a RED:

- **Fix the Artifact the eval evaluates.** Make it satisfy the eval's
  requirements: a RED docs eval is fixed in the docs, a RED code eval in the
  code. Never change another Artifact to match a wrong one, for example code to
  match docs. If a docs finding reveals a real code bug, report it instead of
  changing behavior silently.
- **A conflicting parent may change.** When a parent Artifact the eval depends
  on (a `{name}` it references, such as a spec or a rule document) conflicts
  with your task's goal, you may update that parent instead. A parent with a
  Human eval follows the sign-off order in step 2.
- **Never weaken the check.** Do not loosen eval instructions, schemas, rule
  documents or CI to get GREEN, do not skip evals, and never submit or
  fabricate a Human sign-off.
- **A provider "server overloaded" ERROR is not a finding.** Run verify again;
  finished results are reused.

Verify again after every fix until the outcome is GREEN or SIGNOFFS_PENDING.
Unchanged results are reused, so a rerun costs only what changed.

## 4. Open the pull request into dev

```sh
git push -u origin feat/<slug>
gh pr create --base dev --title "<type>(<area>): <summary>" --body-file "$scratch/pr.md"
```

Always pass `--base dev`: `main` is the default branch. The body says what
changed and why, lists your decisions, names the issues it closes (`Closes #N`;
they close when the change reaches `main`), and gives the verify Run ID and
outcome. It ends with this note, which dev-master and the development board on
artifactize.dev read:

```text
<!-- artifactize-board
role: dev-worker
from: <full SHA of the dev-green commit you started from>
verify: <Run ID> <GREEN or SIGNOFFS_PENDING> <full SHA of the commit you verified>
-->
```

Keep `verify` current: after every later verify of a new head, replace that
line (`gh pr edit N --body-file …`).

## 5. CI

```sh
gh pr checks N --watch
```

Every CI job must pass. Desktop E2E is informational on pull requests into
`dev`: report its result, but its failure is not yours to fix here. On a
failure, read the log (`gh run view RUN --log-failed`), fix, verify again,
push and watch again.

## 6. Report and wait

When the pull request is open, CI is green and only sign-offs may remain,
report once: the pull request URL, the Run ID and outcome, CI, the sign-offs
still needed, and the structural choices you made. Do not merge: dev-master
merges into `dev`. Then wait. On review feedback, fix, verify, push and update
the note.

Report only when done, blocked, or with a question the code and docs cannot
answer; no progress messages. Decide structure and workflow questions (file
placement, test layout, order of work) yourself and list them in the report.
