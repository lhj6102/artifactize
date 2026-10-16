// Domain setup observes the real Run API; this module never owns an app or driver.
import { spawnSync } from "node:child_process";
import type { Fixture, Saved } from "./fixture.ts";
import { environment } from "./environment.ts";
import { observe } from "./domain-fixtures/observe.ts";

/** Wait for the scheduler's authoritative Human boundary, not a file-write hint. */
export async function waitingRun(program: string, fixture: Fixture): Promise<string> {
  const submitted = spawnSync(program, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--json"], {
    encoding: "utf8", env: environment(fixture),
  });
  if (submitted.error) throw submitted.error;
  if (submitted.status !== 0) throw new Error(submitted.stderr || submitted.stdout || "verify submission failed");
  const { run } = JSON.parse(submitted.stdout) as { run: string };
  let last = "no saved snapshot";
  await observe(() => {
    const shown = spawnSync(program, ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "show", run, "--json"], {
      encoding: "utf8", env: environment(fixture),
    });
    if (shown.error) throw shown.error;
    if (shown.status !== 0) throw new Error(shown.stderr || shown.stdout || `cannot observe Run ${run}`);
    const saved = JSON.parse(shown.stdout) as { status: string; control: { activity: string }; requests: Saved[] };
    last = JSON.stringify({ status: saved.status, control: saved.control,
      requests: saved.requests.map(({ id, evalId, status }) => ({ id, evalId, status })) });
    if (saved.status !== "RUNNING") throw new Error(`Run ${run} ended ${saved.status} before its Human boundary; last snapshot: ${last}`);
    return saved.control.activity === "waiting" && saved.requests.some((request) => request.status === "WAITING_HUMAN");
  }, () => `Run ${run} did not reach its Human boundary; last saved snapshot: ${last}`);
  return run;
}
