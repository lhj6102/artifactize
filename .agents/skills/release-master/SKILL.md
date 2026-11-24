---
name: release-master
description: Run a release of the artifactize repository from main-green — cut release/<version>, watch and fix CI and Binaries QA gates, tag v<version> on release/<version> after owner confirmation to publish releases and website, verify live endpoints, and back-merge any QA fixes into main. Run only when invoked explicitly by the owner.
disable-model-invocation: true
---

# release-master

You run one release of this repository from `main-green` through QA on
`release/<version>`, tagging `v<version>` to publish, live post-release
verification, and back-merging any QA fixes into `main`. Never schedule
release-master automatically; run only when explicitly invoked by the owner
(`/release-master <version>` in Claude Code, `$release-master <version>` in
Codex).

## Branch and release model

- `main` is where work comes together and may temporarily be RED or YELLOW
  while dev-master converges it. Never cut a release from an arbitrary `main`
  HEAD.
- `main-green` is the lightweight tag moved only by dev-master to the latest
  GREEN `main` commit. Every new `release/<version>` branch starts from
  `main-green`.
- `release/<version>` is the release QA branch. Pushing to it (or opening a
  pull request into it) triggers:
  - **CI** (`.github/workflows/ci.yml`): Linux, Windows and macOS builds,
    native platform contracts, and **blocking** `Desktop E2E` on Linux,
    Windows and macOS (`push` to `release/*` has no `paths-ignore`).
  - **Binaries** (`.github/workflows/binaries.yml`): channel policy checks,
    all five CLI targets, Windows MSIX, macOS universal dmg, Linux AppImage
    and deb, and every platform's installer/app smoke tests (`github.ref_type != 'tag'`).
    Linux CLI and Linux desktop bundles gate the run; Windows and macOS builds
    are best effort (`continue-on-error: true`) before v1, so inspect each
    individual job's conclusion rather than only the overall workflow status.
- Tagging `v<version>` directly on the verified `release/<version>` commit
  (never on a later `main` merge commit that may contain un-QA'd changes)
  publishes the release:
  - **Release** (`.github/workflows/release.yml`): builds via `binaries.yml`
    without re-running smoke tests (`github.ref_type == 'tag'`), publishes the
    GitHub release gated by Linux, publishes crates to crates.io (stable tags
    only), attaches Windows, macOS, desktop bundles and unsigned Store MSIX
    (best effort), and updates Homebrew tap and Microsoft Store (stable tags
    only, optional).
  - **Website** (`.github/workflows/website.yml`): deploys `artifactize.dev`
    to Cloudflare on stable `v*` tags (excluding prerelease `v*-*` tags) and
    on manual `workflow_dispatch`.
  - Prerelease tags (`v<major>.<minor>.<patch>-<prerelease>`) create a GitHub
    prerelease with archives and unsigned MSIX only: no crates.io, Homebrew,
    Microsoft Store, or Website deploy.
- **Back-merge:** only when QA fixes were committed on `release/<version>`
  does a `release/<version>` → `main` pull request return those fixes to
  `main`. dev-master skips `release/*` pull requests, so the owner merges it.

## 1. Confirm version and start from main-green

```sh
git fetch origin --prune --tags --force
git tag --list 'v*' --sort=-v:refname | head -n 5
git rev-parse main-green
```

- Strip a leading `v` if supplied, and verify the version matches
  `<major>.<minor>.<patch>` or `<major>.<minor>.<patch>-<prerelease>` (as
  `release.yml` requires).
- If no version was given with the invocation, summarize the commits since the
  latest stable `v*` tag (`git log <latest-stable>..main-green --oneline`),
  propose the next version, and ask the owner before cutting the branch.
- Confirm `v<version>` does not already exist on `origin`.
- Work only in `.claude/worktrees/release-<version>`; never touch the main
  checkout or other sessions' worktrees. Keep Cargo targets and temporary
  files in one private scratch directory outside every work tree (never `/tmp`
  itself), and never save a copy of a `*.artf` file in a shared directory.

Create the release branch from `main-green` (or check out an existing
`origin/release/<version>` if resuming QA):

```sh
git worktree add -b release/<version> .claude/worktrees/release-<version> main-green
```

## 2. Push release/<version> and watch QA gates

```sh
git -C .claude/worktrees/release-<version> push -u origin release/<version>
```

Find and watch both triggered runs (`CI` and `Binaries`) on `release/<version>`:

```sh
gh run list --branch release/<version> --limit 10 --json databaseId,workflowName,status,conclusion,headSha
gh run watch <CI_RUN_ID>
gh run watch <BINARIES_RUN_ID>
```

Inspect **every job's own conclusion** on both runs, because Windows and macOS
jobs in `Binaries` are best effort (`continue-on-error: true`):

```sh
gh run view <RUN_ID> --json jobs --jq '.jobs[] | {name, status, conclusion}'
```

## 3. Fix QA failures on release/<version> (if any)

If `CI` (including `Desktop E2E`) or `Binaries` fails:

1. Read the failure logs (`gh run view <RUN_ID> --log-failed`) and download
   Desktop E2E evidence artifacts (`gh run download <RUN_ID>`) if needed.
2. Fix the issue directly on `release/<version>`. Never weaken checks, rules,
   eval instructions, or CI; rule changes in `project-rules/` require owner
   sign-off.
3. In the worktree, install desktop UI packages if needed
   (`pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile`) and
   verify your fix (focusing first with focus-then-sweep, then a full sweep
   before tagging):
   ```sh
   env -u DESKTOP_TEST_CANDIDATE CARGO_TARGET_DIR="$scratch/target" \
     cargo run -q -p artifactize -- verify --all --speculate --wait
   ```
   Read a Run and its failing evals with
   `"$scratch/target/debug/artifactize" run show RUN_ID --json`.
4. Commit with the configured git identity (never change git configuration or
   invent an identity), push `release/<version>`, and repeat step 2 until `CI`
   and `Binaries` pass.

## 4. Confirm with the owner before tagging

Tagging `v<version>` publishes permanently to crates.io (for stable releases),
GitHub Releases, Homebrew, Microsoft Store, and `artifactize.dev`. Before
creating the tag, report to the owner:

- Version `v<version>` (stable or prerelease) and `release/<version>` head SHA
- Summary of changes since the previous release, plus any QA fixes made on
  `release/<version>`
- Job-by-job results of `CI` and `Binaries` (explicitly calling out any failed
  best-effort Windows/macOS jobs)

Wait for the owner's go-ahead before step 5 — unless the owner's invocation
explicitly authorized tagging automatically once all gates (and best-effort
jobs) pass.

## 5. Tag v<version>, watch publish workflows, and verify live

Tag the verified `release/<version>` head commit directly:

```sh
git -C .claude/worktrees/release-<version> tag -a "v<version>" -m "Release v<version>"
git -C .claude/worktrees/release-<version> push origin "refs/tags/v<version>"
```

Watch the triggered `Release` workflow (and `Website` workflow for stable
tags):

```sh
gh run list --branch "v<version>" --limit 10 --json databaseId,workflowName,status,conclusion
gh run watch <RELEASE_RUN_ID>
gh run watch <WEBSITE_RUN_ID>   # stable tags only
```

Inspect every job in `Release` (`publish`, `crates`, `attach-windows`,
`attach-macos`, `attach-desktop`, `attach-channels`, `homebrew`, `store`) and
confirm whether optional channels published or emitted expected skip notices.

For stable releases, verify live production endpoints once `Release` and
`Website` finish (if CDN or crates.io index propagation causes a transient
mismatch, wait briefly and retry once):

```sh
env -u DESKTOP_TEST_CANDIDATE CARGO_TARGET_DIR="$scratch/target" \
  cargo run -q -p artifactize -- verify --artifacts live-release,live-crates,live-website --wait
```

## 6. Back-merge QA fixes and report

Check whether `release/<version>` has commits not already in `origin/main`:

```sh
git -C .claude/worktrees/release-<version> rev-list --count origin/main..HEAD
```

- If the count is greater than 0, open a back-merge pull request into `main`
  and leave it for the owner to merge (dev-master skips `release/*` pull
  requests):
  ```sh
  gh pr create --base main --head release/<version> \
    --title "chore(release): back-merge QA fixes from release/<version>" \
    --body "Back-merges QA fixes from \`release/<version>\` (released as \`v<version>\`) into \`main\`."
  ```
- If the count is 0, no back-merge pull request is needed.

Remove `.claude/worktrees/release-<version>` and your scratch directory when
finished. Commit with the configured git identity only; CI is fixed (never add
jobs, steps, schedules or push triggers to `.github/workflows/`).

Report once: the GitHub release URL, tag and commit SHA, per-channel outcomes
(`Release`, `crates.io`, `Homebrew`, `Store`, `Website`), live verification
Run ID and outcome, back-merge pull request URL (if any), and any remaining
issues or skipped channels.
