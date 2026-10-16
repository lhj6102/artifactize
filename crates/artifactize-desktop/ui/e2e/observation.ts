import { readCandidate } from "./build.ts";
import { asNativePath, nameOf, nativePath, runProgramOk, variable } from "./platform.ts";
import type { Fixture } from "./fixture.ts";
import type { Identity } from "./runs.ts";

/** Read only the identity this E2E app captured for itself, never discover a PID
 * from a port, scan processes, or acquire ownership of the service's app. */
async function identity(browser: WebdriverIO.Browser): Promise<Identity> {
  const value: unknown = await browser.execute(() =>
    (window as Window & { __ARTIFACTIZE_E2E_PROCESS__?: unknown }).__ARTIFACTIZE_E2E_PROCESS__);
  if (!value || typeof value !== "object" || !("pid" in value) || !("start_time" in value)
    || typeof value.pid !== "number" || !Number.isSafeInteger(value.pid) || value.pid <= 0
    || typeof value.start_time !== "string" || !/^\d+$/.test(value.start_time)) {
    throw new Error("the test app did not expose its native birth identity");
  }
  return { pid: value.pid, start_time: value.start_time };
}

/** The existing Rust observer reads the saved birth identity without signalling,
 * launching, restarting or cleaning up the app. Observation failures are errors. */
function nativeAlive(owner: Identity): boolean {
  const value: unknown = JSON.parse(runProgramOk(readCandidate().observer, ["alive"], {
    input: JSON.stringify([owner]), timeout: 5000,
  }));
  if (!Array.isArray(value) || value.length !== 1 || typeof value[0] !== "boolean") {
    throw new Error("the Rust observer returned an invalid app-exit observation");
  }
  return value[0];
}

/** @wdio/runner always DELETEs its session, but the embedded server dies with the
 * app. Only the quit helper reads native metadata. Retire this client session only
 * after the same native process was observed alive before the action and exited
 * afterward; a vanished window/listener or the service's later kill cannot pass it.
 * The optional read-only predicate is the unit-test seam, never a runtime setting. */
export async function sessionEndedByApp(
  browser: WebdriverIO.Browser,
  quit: () => Promise<unknown>,
  observeAlive: (owner: Identity) => boolean | Promise<boolean> = nativeAlive,
): Promise<void> {
  const owner = await identity(browser);
  if (!(await observeAlive(owner))) throw new Error("the app had already exited before its quit action");
  let actionFailure: unknown;
  try { await quit(); }
  catch (error) { actionFailure = error; }
  try {
    await browser.waitUntil(async () => !(await observeAlive(owner)), {
      timeout: 15000,
      timeoutMsg: "the native app process did not exit after its quit action",
    });
  } catch (error) {
    if (actionFailure !== undefined) throw new AggregateError([actionFailure, error], "the quit action and native app-exit observation failed");
    throw error;
  }
  if (!Reflect.set(browser, "sessionId", undefined)) throw new Error("the ended client session could not be retired");
}

export async function saveEvidence(browser: WebdriverIO.Browser, fixture: Fixture, label: string): Promise<void> {
  const output = variable("HUMAN_FOCUS_EVIDENCE");
  if (output) await browser.saveScreenshot(nativePath(asNativePath(output), `${nameOf(fixture.root)}-${label.replaceAll(" ", "-")}.png`));
}
