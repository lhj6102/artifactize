---
name: dev-master
description: Run the main convergence (reduce) cycle of the artifactize repository - merge ready pull requests waiting for main, resolve conflicts on overlapping ready pull requests directly on a convergence branch, absorb newly ready pull requests while fixing RED evals until verify is GREEN, move the main-green tag to that commit, and schedule the next run 58 minutes later. Run only when invoked explicitly.
disable-model-invocation: true
---

# dev-master

You keep `main` converging. One run reduces the ready pull requests waiting for
`main` (merging non-conflicting ones directly and integrating conflicting ones
without sending them back to dev-worker for rebase), verifies `main` and fixes
what is RED until verify is GREEN, records that commit as `main-green`, and
schedules the next run. When a scheduled prompt asks you to continue the
dev-master cycle, start the next run from step 1.

## Branch model

- `main` is where work comes together. Merging ready pull requests may turn it
  RED; that is expected, and this cycle continuously reduces waiting pull
  requests and drives `main` back to GREEN. Its states: RED (an eval fails),
  YELLOW (only Human sign-offs remain) and GREEN (verify GREEN and CI green).
  Changes reach `main` only as merged pull requests whose blocking checks
  passed (CI skips pull requests that change only the website; pushes to
  `main` also run CI to seed caches, with Desktop E2E informational); never
  push to `main` directly.
- `main-green` is a lightweight tag on the latest GREEN `main` commit. Only you
  move it. Both dev-worker branches and `release/*` branches start from it.
- `release/*` branches and `v*` tags are the release channel. Releases are the
  owner's decision (via release-master): never create, push to, or merge a
  `release/*` branch, never merge a `release/*` → `main` pull request, never
  create a `v*` tag or publish a release.
- dev-worker branches (`feat/…`, `fix/…`, `docs/…`, `chore/…`) start at
  `main-green`, verify once until GREEN (or YELLOW while sign-offs wait), and
  open a pull request into `main` without rebasing onto newer `main` commits.
  Once a pull request's head has verified GREEN and passed blocking CI, you
  integrate it into `main` even if its base commit is an older `main-green` or
  it conflicts with newer commits on `main` (steps 2–3). Your own convergence
  and fix branches start at `origin/main` instead (step 3).

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
SATISFIED`, the latter followed by `Unmet obligation:` lines when the Run
reports unmet obligations. The exit code is
the outcome: 0 GREEN (`Validation: SATISFIED`), 5 YELLOW, 1 RED,
2 ERROR, 4 INCOMPLETE. Sign-offs never hold up the machine evals; once only
sign-offs are left, the Run waits while one is claimed and ends YELLOW after its
idle grace (2 minutes unless set with `artifactize limits --signoff-idle-grace`)
with none claimed.
Read a Run and its failing evals with
`"$scratch/target/debug/artifactize" run show RUN_ID --json`.

- Fix the Artifact the eval evaluates: a RED docs eval is fixed in the docs, a
  RED code eval in the code. If a docs finding reveals a real code bug, report
  it rather than changing behavior silently.
- Never weaken the check: no loosened eval instructions, schemas, rule
  documents or CI, no skipped evals, and never a submitted or fabricated Human
  sign-off. Rule documents in `project-rules/` change only with the owner's
  sign-off.
- A provider "server overloaded" ERROR is not a finding: run verify again.
  Finished results whose reuse key still matches are reused; an operational
  error is not a result and runs again.
- Never save a copy of a `*.artf` file in a shared directory (use a `.txt`
  suffix), and remove the scratch files you no longer need.

## One run

### 1. Sync

```sh
git fetch origin --prune --tags --force
git worktree add --detach .claude/worktrees/dev-master origin/main   # first run only
git -C .claude/worktrees/dev-master checkout --detach origin/main
```

Work only in worktrees under `.claude/worktrees/`; never touch the main
checkout or other sessions' worktrees.

### 2. Merge or queue the waiting pull requests

```sh
gh pr list --base main --state open --json number,title,headRefName,headRefOid,isDraft,mergeable,body
```

Take the non-draft ones in number order, skipping any pull request whose
`headRefName` starts with `release/` (those are owner-managed release
back-merge pull requests; leave them for the owner). Each pull request ends
with a note:

```text
<!-- artifactize-board
role: dev-worker
from: <main-green SHA the branch started from>
verify: <Run ID> <GREEN or YELLOW> <SHA of the verified commit>
-->
```

A pull request is **ready** when both of these hold (regardless of whether
`main` moved ahead after its `from:` commit):

1. **Verify GREEN on its head.** The note says GREEN for the current
   `headRefOid`. Otherwise (YELLOW, an older commit, no note),
   verify the head yourself without rebasing in `.claude/worktrees/pr-N`
   (`git fetch origin pull/N/head` and `git worktree add --detach … FETCH_HEAD`);
   results whose reuse key still matches are reused. When it is GREEN, update
   the note's `verify` line. When only sign-offs remain, list them for the owner
   and leave the pull request waiting; when it is RED, leave it for its worker.
2. **CI green.** `gh pr checks N`: every job passed, judged by each job's own
   conclusion. Desktop E2E is informational on pull requests into `main` (it
   blocks only `release/*` pushes and pull requests into `release/*`); record
   its failures for the release phase instead of blocking the merge.

For each ready pull request:

- **No conflict:** merge it directly into `main`:
  ```sh
  gh pr merge N --merge
  ```
- **Conflict with `main` (overlapping changes):** do **not** bounce it back to
  dev-worker for rebase. Queue it for direct conflict-resolution integration on
  your convergence branch in step 3. Only leave a conflicting pull request open
  and ask the owner (in a pull request comment, once) when two pull requests
  have genuinely contradictory requirements or designs that cannot be resolved
  without an owner decision.

### 3. Integrate conflicts and verify main until GREEN

Fetch `origin/main` so it includes every non-conflicting pull request merged in
step 2.

- **If step 2 queued conflicting ready pull requests:** create a convergence
  branch from `origin/main`:
  `git worktree add -b fix/main-<yyyymmdd-hhmm> .claude/worktrees/dev-fix origin/main`.
  For each queued pull request `N` in number order, fetch its verified head
  (`git fetch origin pull/N/head`) and merge it with
  `git merge --no-ff FETCH_HEAD`, resolving Git conflicts directly without
  re-verifying each pull request separately. Verify the combined convergence
  branch once.
- **Otherwise:** check out the new `origin/main` in `.claude/worktrees/dev-master`
  and verify.

Drive the active branch (either `origin/main` or your convergence branch
`fix/main-<yyyymmdd-hhmm>`) to GREEN:

- **RED, ERROR or INCOMPLETE:** if you are not yet on a convergence branch,
  create `fix/main-<yyyymmdd-hhmm>` from `origin/main` in
  `.claude/worktrees/dev-fix`. Fix the failing Artifacts on that branch.
- **Newly ready pull requests while fixing a RED:** before running the next
  verify pass on a RED convergence branch, check step 2 again. Merge any
  non-conflicting newly ready pull requests into `main` and merge `origin/main`
  into the convergence branch; merge any conflicting newly ready pull requests
  directly into the convergence branch with `git merge --no-ff FETCH_HEAD` and
  resolve conflicts. Then continue converging in one sweep.
- **Convergence branch reaches GREEN:** do not hold back a finished GREEN
  checkpoint for newly arrived pull requests (they are absorbed in the next
  run). Open a pull request with `--base main` whose note says
  `role: dev-master` and `from:` the `origin/main` SHA it branched from, wait
  for CI (`gh pr checks N --watch`), and merge it with `gh pr merge N --merge`
  (because each conflicting pull request's head commit was merged with
  `--no-ff`, GitHub automatically marks those pull requests as merged too).
  Then check out the updated `origin/main` in `.claude/worktrees/dev-master`
  and verify once (reusing cached GREEN results) before step 4.
- **YELLOW:** list the pending sign-offs for the owner and go to
  step 5 without moving `main-green`. The next run verifies again and reuses the
  signed-off results.
- **GREEN on `origin/main`:** go to step 4 if the verified commit is still
  `origin/main`'s head after a fresh fetch; otherwise step 2 again.

### 4. Record main-green

```sh
sha=$(git rev-parse origin/main)
git tag -f main-green "$sha"
git push -f origin refs/tags/main-green
gh api "repos/{owner}/{repo}/commits/$sha/pulls" --jq '.[0].number'
```

In the pull request that last command names (the one whose merge made this
commit), add `green: <Run ID> <UTC time>` as the last line inside
its note (`gh pr edit N --body-file …`), the time as
`date -u +%Y-%m-%dT%H:%M:%SZ` prints it. A pull request without a note gets one
at the end of its body holding only that line between `<!-- artifactize-board`
and `-->`. The development board on artifactize.dev reads these lines as the
history of GREEN `main` commits.

### 5. Report and schedule

Report once: the pull requests merged, the fixes made, the Run ID and outcome,
`main-green`'s commit, sign-offs or E2E failures waiting for the owner, and
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
