# Runs and Workflows

`artifactize verify` submits a durable **Run** that schedules and records your checks. `artifactize status` predicts what the next `verify` will `execute`, `reuse`, `wait` for, or mark `blocked` without running any evals (rows in the required closure that are outside the execution selection are marked `not included`: their action is a forecast under the current selection—including whether upstream sign-offs are selected and can be deferred—while only included evals are scheduled or counted in the action totals; rerun `status` with the intended selector to see how expanding the selection changes the plan).

## Selecting what to verify {#verify-and-runs}

`verify` requires a selector:

```sh
artifactize verify --all --wait                  # every Artifact and eval in the project
artifactize verify app --wait                    # app, its parts, and any cycle peers
artifactize verify app/tests --wait              # one eval (same as --eval app/tests)
artifactize verify --artifacts app,docs --wait   # multiple Artifacts (or --evals CSV)
artifactize verify app/tests --recursive --wait  # also execute dependencies in closure
```

- **Execution gates vs. final obligations**:
  - **Execution gates** decide when a selected eval can start; see [Per-eval gating](artifacts.md#eval-dependencies).
  - **Final obligations** decide whether the Run as a whole is satisfied. Even when you select a single eval (`verify app/tests`), the Run requires valid `GREEN` evidence across its entire **dependency closure**—including other checks on `app`, its parts, and the Artifacts it depends on (human sign-offs included; a selected Artifact with no evals, no `parts`, and no `basis = true` is also an unreviewed obligation). Unselected obligations are checked against saved cache/store evidence (`evidence` in the Run record); see [Sign-offs and the end of a Run](#sign-offs-and-the-end-of-a-run) for the Run outcome precedence and exit codes. Use `--recursive` or `--all` to select and run the whole closure.
- **Concurrency and queuing**: all `READY` evals start as soon as their gates clear, with **no default Run-wide job cap**. Evals can still queue (`QUEUED`) when they share a [`mutex` tag](../reference/declarations.md#eval-fields-evalsid), when a backend reaches its slot limit in [`limits.json`](../reference/cli-and-mcp.md#state-cache-and-limits) (shared by all Runs using the same state directory), or when you pass an opt-in `--jobs N` cap. Passing `--max-executions N` caps fresh `runtime` and `agent` starts for the Run (cache hits, joined executions, and Human sign-off requests do not count; when the cap prevents a ready start, that request is marked `BUDGET_EXHAUSTED` and the Run ends `INCOMPLETE` with exit 4).

## Submit, observe, and cancel {#submit-observe-and-cancel}

Artifactize owns each Run in the background independently of the terminal or MCP session that started it:

- Plain `artifactize verify SELECTOR` submits the Run, prints its `RUN_ID`, and exits `0` immediately on **acceptance**.
- `artifactize verify SELECTOR --wait` waits until the Run settles and exits with the Run's outcome code. `artifactize run show RUN_ID --wait [--timeout-ms MS]` also waits for an open Run (default 10-minute observation timeout; exits `3` if that timeout expires first, leaving the Run running).
- `artifactize verify SELECTOR --watch` (or `artifactize run watch RUN_ID`) streams JSON-line progress events (`update`, `eval_result`, `outcome`).
- Pressing `Ctrl+C` on `--wait` or `--watch` exits `130` and stops watching **without** cancelling the Run.
- Only `artifactize run cancel RUN_ID` (or MCP `verify_cancel`) requests cancellation of an open Run (`RUNNING` or `WAITING_HUMAN`). It returns immediately (exit `0`) once the cancellation request is recorded; the Run then stops starting new work, terminates the running work it started (cancelling a follower Run that joined an in-flight execution in another Run detaches the follower without stopping the producer Run's execution), and ends `ERROR/CANCELLED` while retaining any results already saved. Follow it with `run watch RUN_ID` or `run show RUN_ID` to confirm completion.
- Pass `--comment "Fixing auth"` to label the Run in `run list` and the Desktop app, or `--request-key KEY` to make retries return the same Run.

## Speculation (`--speculate`) {#speculation}

By default, an eval waits until its upstream dependency gates are satisfied (`GREEN`, `PROVISIONAL`, or a selected `DEFERRED` sign-off). Passing `--speculate` starts requested `runtime` and `agent` evals before unresolved upstream gates finish, within `--jobs`, `--max-executions`, backend capacity, and `mutex` limits (Human sign-offs are never started speculatively):

- Ready non-speculative work has priority: when it needs `--jobs` or backend capacity held by a speculative review, the speculative review is interrupted and requeued for later (each start counts toward `--max-executions`).
- If an upstream dependency turns `GREEN`, the speculative result is kept and saves wall time.
- If an upstream dependency turns `RED` or `ERROR`, Artifactize cancels any running speculative work that depends on it outside its own Artifact and cycle, invalidates any already completed speculative result so it cannot be reused, and marks downstream requests (including affected sign-offs) `BLOCKED`.
- During iterative debugging ([Focus then sweep](../agent-plugin/plugin.md#focus-then-sweep-iterate-on-red-evals)), `verify --eval ID --speculate` lets a builder rerun one eval even while its upstream inputs are stale.

## Sign-offs and the end of a Run {#sign-offs-and-the-end-of-a-run}

A Human eval starts only after every non-Human (`runtime` and `agent`) eval on its **same Artifact** is `GREEN` (even if `PROVISIONAL`), in addition to its input dependency gates; if a sibling machine eval is `RED` or `ERROR`, the Human eval is `BLOCKED`.

Once ready, a selected Human sign-off is recorded as a **`DEFERRED` request** that never holds up other evals: the open Run shows **`WAITING_HUMAN`**, downstream checks that depend on the sign-off proceed once their other prerequisites and scheduling limits allow, and their passing (`GREEN`) verdicts are marked **`PROVISIONAL`** until the sign-off is submitted (while `RED` or `ERROR` verdicts show as-is). Non-forced concurrent Runs in the same local state that need a sign-off with the same reuse key join the existing waiting request as **followers** instead of creating duplicate prompts.

Once all machine checks have finished and only unsubmitted sign-offs remain, the Run stays open while any sign-off is actively claimed by a reviewer, plus an **idle grace** of **2 minutes** (`signoffIdleGraceSeconds` in `limits.json`, configurable with `artifactize limits --signoff-idle-grace SECONDS`). When the grace expires with no active claim, the Run settles with its final outcome (`YELLOW` when pending sign-offs are the sole unmet obligations, or `RED`/`ERROR`/`INCOMPLETE` if another required obligation failed or is missing):

| Run outcome | Exit code | Meaning |
|---|---|---|
| `GREEN` | `0` | Every required eval, including human sign-offs, is `GREEN`. |
| `RED` | `1` | At least one eval (or required obligation) judged its Artifact and returned `RED` (with no cancellation, `ERROR`, or budget/`--reuse-only` `INCOMPLETE`). |
| `ERROR` | `2` | Explicit Run cancellation (`ERROR/CANCELLED`, highest precedence), a Run-level preparation failure (such as a failing fingerprint script or declarations changing mid-preparation with `INPUT_CHANGED`), or—when no budget/`--reuse-only` `INCOMPLETE` applies—an operational eval failure. |
| `INCOMPLETE` | `4` | In a Run that was not cancelled: `--max-executions` prevented a start (`BUDGET_EXHAUSTED`) or a [`--reuse-only` check missed](../guides/team-and-ci.md#ci) (both take precedence over eval `ERROR` and `RED`), or no eval is `ERROR`/`RED` and any required obligation remains unmet (such as an unselected obligation without valid evidence, or a selected Artifact with no evals, `parts`, or `basis = true`). |
| `YELLOW` | `5` | All machine checks passed (`GREEN` or `PROVISIONAL`) and **only unsubmitted human sign-offs remain** (not full approval). |

After a Run ends `YELLOW`, the waiting sign-off remains open in `artifactize review` and the Desktop app. Submitting `GREEN` settles that request (and any joined follower requests); while its [reuse key](reuse.md#the-reuse-key) still matches, the next `artifactize verify` reuses that sign-off (`fingerprint = false` disables reuse across Runs). If a newer Run in the same working tree requests the same Human eval with a different reuse key, the older waiting sign-off is retired as `ERROR/SUPERSEDED`.
