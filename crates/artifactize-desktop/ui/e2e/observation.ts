import { readCandidate } from "./build.ts";
import { processId, processRunning, removeTree, runProgramOk, variable, newDirectory, writeJson } from "./platform.ts";
import { asNativePath, childDirectories, exists, nameOf, nativePath, readJson, saveScreenshot, writeTextOnce, type NativePath } from "./portable.ts";
import type { Fixture } from "./fixture.ts";
import type { Identity } from "./runs.ts";

/** Read only the identity this E2E app captured for itself, never discover a PID
 * from a port, scan processes, or acquire ownership of the service's app. */
async function identity(browser: WebdriverIO.Browser): Promise<Identity & { exitReceipt?: string }> {
  const value: unknown = await browser.execute(() =>
    (window as Window & { __ARTIFACTIZE_E2E_PROCESS__?: unknown }).__ARTIFACTIZE_E2E_PROCESS__);
  if (!value || typeof value !== "object" || !("pid" in value) || !("start_time" in value)
    || typeof value.pid !== "number" || !Number.isSafeInteger(value.pid) || value.pid <= 0
    || typeof value.start_time !== "string" || !/^\d+$/.test(value.start_time)) {
    throw new Error("the test app did not expose its native birth identity");
  }
  const exitReceipt = "exitReceipt" in value && typeof value.exitReceipt === "string" ? value.exitReceipt : undefined;
  return { pid: value.pid, start_time: value.start_time, exitReceipt };
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

function recordedOrderlyExit(owner: Identity, path: string | undefined): boolean {
  if (!path || !exists(asNativePath(path))) return false;
  const receipt = readJson(asNativePath(path)) as Partial<Identity>;
  return receipt.pid === owner.pid && receipt.start_time === owner.start_time;
}

/** The expected transport error from a driver shutting down during the action. */
function driverDisconnected(error: unknown): boolean {
  if (!(error instanceof Error)) return false;
  // The embedded driver can lose the webview or its response channel before HTTP shuts down.
  const webviewDisconnected = /^WebDriverError: (?:Channel closed|runtime error: failed to send message to the webview) when running "(?:actions|execute\/(?:a?sync))" with method "POST"(?:$| and args )/i;
  return webviewDisconnected.test(error.message)
    || /\b(?:ECONNREFUSED|ECONNRESET|EPIPE|UND_ERR_SOCKET)\b|fetch failed|socket hang up|other side closed/i.test(error.message)
    || driverDisconnected(error.cause);
}

/** Whether a failed request was refused before the driver received it. */
function neverDelivered(error: unknown): boolean {
  if (!(error instanceof Error)) return false;
  return /\bECONNREFUSED\b/.test(error.message) || neverDelivered(error.cause);
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
  orderlyExit = recordedOrderlyExit,
): Promise<void> {
  const { exitReceipt, ...owner } = await identity(browser);
  if (!(await observeAlive(owner))) throw new Error("the app had already exited before its quit action");
  let actionFailure: unknown;
  try { await quit(); }
  catch (error) { actionFailure = error; }
  try {
    await browser.waitUntil(async () => !(await observeAlive(owner)), {
      timeout: 15000,
      timeoutMsg: "the native app process did not exit after its quit action",
    });
    if (!orderlyExit(owner, exitReceipt)) throw new Error("native app exited without a successful, orderly shutdown");
  } catch (error) {
    if (actionFailure !== undefined) throw new AggregateError([actionFailure, error], "the quit action and native app-exit observation failed");
    throw error;
  }
  // A request refused at connect never reached the app: its exit, however orderly, was not
  // this action's doing.
  if (actionFailure !== undefined && (!driverDisconnected(actionFailure) || neverDelivered(actionFailure))) throw actionFailure;
  if (!Reflect.set(browser, "sessionId", undefined)) throw new Error("the ended client session could not be retired");
}

export async function saveEvidence(browser: WebdriverIO.Browser, fixture: Fixture, label: string): Promise<void> {
  const output = variable("HUMAN_FOCUS_EVIDENCE");
  if (output) await saveScreenshot(browser, nativePath(asNativePath(output), `${nameOf(fixture.root)}-${label.replaceAll(" ", "-")}.png`));
}

/** Explicitly separate an owned live app from an invalid observation. */
export class AppStillAlive extends Error {}

/** Service/runner exit alone is not proof that its native apps stopped. Missing birth
 * records or live owners keep fixture inputs intact; this observes, never signals them. */
export function assertAppsStopped(observer: NativePath, files: NativePath[]): void {
  const owners = files.flatMap((file) => {
    if (!exists(file)) throw new Error(`Native shutdown is unconfirmed; birth record missing: ${file}`);
    const value = readJson(file) as Partial<Identity> & { neverLaunched?: boolean };
    if (value.neverLaunched === true) return [];
    if (!Number.isSafeInteger(value.pid) || (value.pid ?? 0) <= 0 || typeof value.start_time !== "string" || !/^\d+$/.test(value.start_time)) {
      throw new Error(`Native shutdown is unconfirmed; invalid birth record: ${file}`);
    }
    return [value as Identity];
  });
  if (owners.length === 0) return;
  const alive: unknown = JSON.parse(runProgramOk(observer, ["alive"], { input: JSON.stringify(owners), timeout: 5000 }));
  if (!Array.isArray(alive) || alive.length !== owners.length || alive.some((value) => typeof value !== "boolean")) {
    throw new Error("Native shutdown is unconfirmed; invalid native observation");
  }
  if (alive.some(Boolean)) throw new AppStillAlive("A native app still owns its fixtures and executable inputs");
}

/** Settle an admitted app that its launcher (an app confirmed ended) never started, and that
 * is confirmed not running: its record says so, unless its own birth record got there first. */
export function recordNeverLaunched(file: NativePath): void {
  writeTextOnce(file, JSON.stringify({ neverLaunched: true }));
}

function admissions(root: NativePath): NativePath[] {
  const directory = nativePath(root, "admissions");
  return exists(directory) ? childDirectories(directory) : [];
}

/** Journal admission before yielding to the launcher; diagnostics may be deleted
 * independently, even after a runner was interrupted. No missing record is proof. */
export function recordAdmission(root: NativePath, ids: string[]): NativePath[] {
  const directory = newDirectory(nativePath(root, "admissions"), "launch-");
  const names = ids.map((id) => `${id}.owner.json`);
  try {
    writeJson(nativePath(directory, "expected.json"), { runner: processId(), names });
    if (exists(nativePath(root, "cleaning.json"))) throw new Error("candidate cleanup already started; refusing app admission");
  } catch (error) {
    // The caller has not received the owner paths yet, so no app was admitted.
    removeTree(directory);
    throw error;
  }
  return names.map((name) => nativePath(directory, name));
}

function readAdmission(directory: NativePath): { runner: number; owners: NativePath[] } {
  const { runner, names } = readJson(nativePath(directory, "expected.json")) as { runner: number; names: string[] };
  if (!Number.isSafeInteger(runner) || runner <= 0 || !Array.isArray(names) || !names.length
    || names.some((name) => typeof name !== "string" || !/^[a-z0-9-]+\.owner\.json$/.test(name))) {
    throw new Error(`Invalid app admission journal: ${directory}`);
  }
  return { runner, owners: names.map((name) => nativePath(directory, name)) };
}

export function assertAdmissionsStopped(root: NativePath, observer: NativePath): void {
  for (const directory of admissions(root)) {
    const { runner, owners } = readAdmission(directory);
    if (processRunning(runner)) throw new Error(`App admission still owned by a live runner: ${directory}`);
    assertAppsStopped(observer, owners);
  }
}

/** Once this runner has finished all service lifetimes, retire only its admissions
 * whose native exit we can prove. Retaining confirmed journals across batches would
 * make a recycled runner PID appear to own an already-ended launch. */
export function retireOwnAdmissions(root: NativePath, observer: NativePath): void {
  for (const directory of admissions(root)) {
    const { runner, owners } = readAdmission(directory);
    if (runner !== processId()) continue;
    assertAppsStopped(observer, owners);
    removeTree(directory);
  }
}

/** Ask the public Tauri window API to perform a genuine close request. The
 * pristine provider's DELETE /window force-destroys and bypasses this app event. */
export async function requestWindowClose(browser: WebdriverIO.Browser): Promise<void> {
  const error = await browser.executeAsync((done) => {
    const app = window as unknown as { __TAURI__: { window: { getCurrentWindow: () => { close: () => Promise<void> } } } };
    app.__TAURI__.window.getCurrentWindow().close().then(
      () => done(null),
      (failure: unknown) => done(String(failure)),
    );
  });
  if (error !== null) throw new Error(`native window close failed: ${String(error)}`);
}
