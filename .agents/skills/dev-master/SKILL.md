---
name: dev-master
description: Run the dev convergence cycle of the artifactize repository - merge the pull requests waiting for dev, verify dev, fix what is RED until verify is GREEN, move the dev-green tag to that commit, and schedule the next run 58 minutes later. Run only when invoked explicitly.
disable-model-invocation: true
---

# dev-master

You keep `dev` converging. One run merges the pull requests waiting for `dev`,
then verifies `dev` and fixes what is RED until verify is GREEN, records that
commit as `dev-green`, and schedules the next run. When a scheduled prompt asks
you to continue the dev-master cycle, start the next run from step 1.

## Branch model

- `main` is the release channel. Releases are the owner's decision: never push
  to `main`, merge into it, open a `dev` → `main` pull request, create a `v*` tag
  or publish a release.
- `dev` is where work comes together. Merging a ready pull request may turn it
  RED; that is expected, and this cycle brings it back to GREEN. Its states:
  RED (an eval fails), YELLOW (only Human sign-offs remain) and GREEN
  (verify GREEN and CI green). Changes reach `dev` only as merged pull requests,
  so every `dev` commit has passed CI; never push to `dev` directly.
- `dev-green` is a lightweight tag on the latest GREEN `dev` commit. Only you
  move it. dev-worker branches start from it.
- Development branches (`feat/…`, `fix/…`, `docs/…`, `chore/…`) start at
  `dev-green`, reach verify GREEN and open a pull request into `dev`.

## Verify

Run from the root of the work tree being verified, with a scratch directory
outside every work tree (never `/tmp` itself) that you keep across runs so
builds stay incremental. In a new work tree, first install the desktop UI's
packages, which the desktop evals read without installing them:

```sh
pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile
```

Then verify:

```sh
env -u DESKTOP_TEST_CANDIDATE CARGO_TARGET_DIR="$scratch/target" \
  cargo run -q -p artifactize -- verify --all --speculate --wait
```

`env -u DESKTOP_TEST_CANDIDATE` makes the desktop end-to-end evals build their
candidate from this tree. The command prints the Run ID, the Run's outcome on
`Execution:`, and ends with `Validation: SATISFIED` or `Validation: NOT
SATISFIED`, the latter followed by `Unmet obligation:` lines. The exit code is
the outcome: 0 GREEN (`Validation: SATISFIED`), 5 YELLOW, 1 RED,
2 ERROR, 4 INCOMPLETE. Sign-offs never hold up the machine evals; once only
sign-offs are left, the Run waits while one is claimed and ends YELLOW after its
idle grace (2 minutes) with none claimed.
Read a Run and its failing evals with
`"$scratch/target/debug/artifactize" run show RUN_ID --json`.

- Fix the Artifact the eval evaluates: a RED docs eval is fixed in the docs, a
  RED code eval in the code. If a docs finding reveals a real code bug, report
  it rather than changing behavior silently.
- Never weaken the check: no loosened eval instructions, schemas, rule
  documents or CI, no skipped evals, and never a submitted or fabricated Human
  sign-off. Rule documents in `project-rules/` change only with the owner's
  sign-off.
- A provider "server overloaded" ERROR is not a finding: run verify again;
  finished results are reused.
- Never save a copy of a `*.artf` file in a shared directory (use a `.txt`
  suffix), and remove the scratch files you no longer need.

## One run

### 1. Sync

```sh
git fetch origin --prune --tags --force
git worktree add --detach .claude/worktrees/dev-master origin/dev   # first run only
git -C .claude/worktrees/dev-master checkout --detach origin/dev
```

Work only in worktrees under `.claude/worktrees/`; never touch the main
checkout or other sessions' worktrees.

### 2. Merge the waiting pull requests

```sh
gh pr list --base dev --state open --json number,title,headRefName,headRefOid,isDraft,mergeable,body
```

Take the non-draft ones in number order. Each pull request ends with a note:

```text
<!-- artifactize-board
role: dev-worker
from: <dev-green SHA the branch started from>
verify: <Run ID> <GREEN or YELLOW> <SHA of the verified commit>
-->
```

Merge one when all of these hold:

1. **Verify GREEN on its head.** The note says GREEN for the current
   `headRefOid`. Otherwise (YELLOW, an older commit, no note),
   verify the head yourself in `.claude/worktrees/pr-N`
   (`git fetch origin pull/N/head` and `git worktree add --detach … FETCH_HEAD`);
   unchanged results are reused. When it is GREEN, update the note's `verify`
   line. When only sign-offs remain, list them for the owner and leave the pull
   request waiting; when it is RED, leave it for its worker.
2. **CI green.** `gh pr checks N`: every job passed, judged by each job's own
   conclusion. Desktop E2E is informational on pull requests into `dev`; record
   its failures for the release phase instead of blocking the merge.
3. **No conflict.** A conflicting pull request stays open; say so in a pull
   request comment, once.

```sh
gh pr merge N --merge
```

### 3. Verify dev until GREEN

Check out the new `origin/dev` in `.claude/worktrees/dev-master` and verify.

- **RED, ERROR or INCOMPLETE:** fix it on a branch from `dev`'s head:
  `git worktree add -b fix/dev-<yyyymmdd-hhmm> .claude/worktrees/dev-fix origin/dev`.
  Verify there until GREEN, open a pull request with `--base dev` whose note
  says `role: dev-master` and `from:` that head, wait for CI
  (`gh pr checks N --watch`) and merge it under the rules of step 2.
- **Newly waiting pull requests:** before verifying again, run step 2 again.
- **YELLOW:** list the pending sign-offs for the owner and go to
  step 5 without moving `dev-green`. The next run verifies again and reuses the
  signed-off results.
- **GREEN:** go to step 4 if the verified commit is still `origin/dev`'s head
  after a fresh fetch; otherwise step 2 again.

### 4. Record dev-green

```sh
sha=$(git rev-parse origin/dev)
git tag -f dev-green "$sha"
git push -f origin refs/tags/dev-green
gh api "repos/{owner}/{repo}/commits/$sha/pulls" --jq '.[0].number'
```

In the pull request that last command names (the one whose merge made this
commit), add `green: <Run ID> <UTC time>` as the last line inside
its note (`gh pr edit N --body-file …`), the time as
`date -u +%Y-%m-%dT%H:%M:%SZ` prints it. A pull request without a note gets one
at the end of its body holding only that line between `<!-- artifactize-board`
and `-->`. The development board on artifactize.dev reads these lines as the
history of GREEN `dev` commits.

### 5. Report and schedule

Report once: the pull requests merged, the fixes made, the Run ID and outcome,
`dev-green`'s commit, sign-offs or E2E failures waiting for the owner, and
anything left blocked. Then schedule the next run (Host below). No progress
messages between runs; decide workflow questions yourself and list them in the
report.

Commit with the git identity already configured; never change git
configuration or invent an identity. CI is fixed: never add jobs, steps,
schedules or push triggers to `.github/workflows/`.

## Host: the next run in 58 minutes

**Claude Code.** End every run by scheduling one session task with `CronCreate`:
a one-shot task 58 minutes from now (pin its minute and hour), with the plain
prompt `Continue the dev-master cycle.` A scheduled fire cannot run this skill
(`disable-model-invocation`), so the prompt is plain text, and the skill is
already in the session's context. First remove any other pending dev-master
task (`CronList`, `CronDelete`) so exactly one remains. The task fires only
while the session is open and idle.

**Codex.** The CLI has no in-session scheduling. In the Codex app, create an
Automation for this project with an hourly schedule and the prompt
`$dev-master`; explicit invocation still works with implicit invocation off.
