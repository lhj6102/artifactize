// Domain setup observes the real Run API; this module never owns an app or driver.
import type { Fixture, Saved } from "./fixture.ts";
import { environment } from "./environment.ts";
import { runProgram, type NativePath } from "./platform.ts";
import { observe } from "./domain-fixtures/observe.ts";

/** The longest sign-off idle grace: a fixture's Run stays open until its sign-off is submitted. */
const OPEN_UNTIL_SUBMITTED = "86400";

/** Wait for the scheduler's authoritative Human boundary, not a file-write hint. The state's
 * idle grace keeps the Run open however long the test takes before it claims the sign-off. */
export async function waitingRun(program: NativePath, fixture: Fixture): Promise<string> {
  const limited = runProgram(program, ["--state-dir", fixture.state, "limits", "--signoff-idle-grace", OPEN_UNTIL_SUBMITTED], {
    env: environment(fixture),
  });
  if (limited.status !== 0) throw new Error(limited.stderr || limited.stdout || "cannot set the sign-off idle grace");
  const submitted = runProgram(program, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--json"], {
    env: environment(fixture),
  });
  if (submitted.status !== 0) throw new Error(submitted.stderr || submitted.stdout || "verify submission failed");
  const { run } = JSON.parse(submitted.stdout) as { run: string };
  let last = "no saved snapshot";
  await observe(() => {
    const shown = runProgram(program, ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "show", run, "--json"], {
      env: environment(fixture),
    });
    if (shown.status !== 0) throw new Error(shown.stderr || shown.stdout || `cannot observe Run ${run}`);
    const saved = JSON.parse(shown.stdout) as { status: string; control: { activity: string }; requests: Saved[] };
    last = JSON.stringify({ status: saved.status, control: saved.control,
      requests: saved.requests.map(({ id, evalId, status }) => ({ id, evalId, status })) });
    if (saved.status !== "RUNNING" && saved.status !== "WAITING_HUMAN") {
      throw new Error(`Run ${run} ended ${saved.status} before its Human boundary; last snapshot: ${last}`);
    }
    return saved.status === "WAITING_HUMAN" && saved.control.activity === "waiting"
      && saved.requests.some((request) => request.status === "DEFERRED");
  }, () => `Run ${run} did not reach its Human boundary; last saved snapshot: ${last}`);
  return run;
}
