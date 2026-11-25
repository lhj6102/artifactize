// A scenario: one journey flow on one app, with the fixtures it prepares before the app starts.
import { systemName, variable, type Environment } from "./platform.ts";
import { asNativePath, exists, makeDirectory, nativePath, writeBytes, writeText, type NativePath } from "./portable.ts";
import type { Owned } from "./fixtures/owned.ts";
import type { Fixture } from "./fixtures/repo.ts";
import type { App } from "./views/app.ts";

/** What a scenario prepares: the fixture the app opens, data its run reads (JSON), the app's
 * extra arguments and variables (an `undefined` removes one), and another place to run the app
 * from. */
export type Prepared<T = unknown> = { fixture: Fixture; data: T; args?: string[]; env?: Environment; app?: NativePath };

/** What a scenario's run gets: what it prepared, its folder, and the port of the app's
 * embedded WebDriver server, which lives as long as the app. */
export type Context<T = unknown> = Prepared<T> & {
  root: NativePath; port: number;
  /** Where its failure and a screenshot of the window then are kept, should it fail. */
  evidence: NativePath;
};

export type Scenario = {
  id: string;
  title: string;
  /** The journeys (J- ids of rules/ux/) it covers (E2E-COVERAGE). */
  journeys: string[];
  /** It uses a once-per-machine resource, such as a fixed port or the display's focus, which a
   * window opened mid-scenario takes: no other app runs beside it (E2E-ISOLATED). */
  alone?: boolean;
  /** How long its run may take, in milliseconds. */
  timeout?: number;
  prepare: (own: Owned) => Promise<Prepared>;
  /** Its flow; what it returns (JSON) reaches `restarted`. */
  run: (app: App, input: Context) => Promise<unknown>;
  /** What it checks once the service has started the app again on the same state, after `run`
   * passed and its app was stopped: a later app of this machine's user. */
  restarted?: (app: App, input: Context, carried: unknown) => Promise<void>;
};

export function scenario<T>(definition: Omit<Scenario, "prepare" | "run" | "restarted"> & {
  prepare: (own: Owned) => Promise<Prepared<T>>;
  run: (app: App, input: Context<T>) => Promise<unknown>;
  restarted?: (app: App, input: Context<T>, carried: unknown) => Promise<void>;
}): Scenario {
  return definition as Scenario;
}

const WAITS = Symbol.for("artifactize-desktop-e2e.waits");
/** The waits this process's scenario is in, a wait inside another after it, and the wait it
 * started last. They hang on the process's global object: the configuration's hooks and the
 * spec may each load this module for themselves, and read the same. */
const waits = ((globalThis as { [WAITS]?: { running: string[]; last: string } })[WAITS] ??= { running: [], last: "" });

/** Note every wait of `browser` and of its elements while it lasts, by what its timeout would
 * say: a scenario stopped in a wait, or between two, names it in its evidence. */
export function noteWaits(browser: WebdriverIO.Browser): void {
  type Wait = (condition: unknown, options?: { timeoutMsg?: string }) => Promise<unknown>;
  const noted = async (wait: Wait, condition: unknown, options?: { timeoutMsg?: string }) => {
    const what = (options?.timeoutMsg ?? "a wait that names nothing").replace(/^timed out: /, "");
    waits.running.push(what);
    waits.last = what;
    try { return await wait(condition, options); }
    finally { waits.running.splice(waits.running.lastIndexOf(what), 1); }
  };
  browser.overwriteCommand("waitUntil", noted as never);
  browser.overwriteCommand("waitUntil", noted as never, true);
}

/** Keep the evidence of the scenario `id`, which failed (E2E-FAILURE-EVIDENCE), in its folder
 * `evidence`: the OS, the scenario and its failure, which names the wait that failed; for a
 * scenario stopped otherwise (a timeout of the whole scenario, an interrupt) the wait it was
 * in then, or the last it had ended; and a screenshot of the window, as `shown`, its browser,
 * shows it then, or, once its app has stopped, the image `shown` its flow left of it
 * (LAST_WINDOW); or why there is none. What failed first is kept: a later failure of the same
 * run of it changes nothing. */
export async function keepEvidence(id: string, evidence: NativePath, failure: string, shown: WebdriverIO.Browser | Uint8Array | undefined): Promise<void> {
  const restarted = variable("DESKTOP_TEST_PHASE") === "restarted";
  const kept = restarted ? asNativePath(`${evidence}-started-again`) : evidence;
  const report = nativePath(kept, "failure.txt");
  if (exists(report)) {
    const image = nativePath(kept, "window.png");
    if (shown instanceof Uint8Array && !exists(image)) writeBytes(image, shown);
    return;
  }
  const waiting = waits.running.length ? `the wait it was in then: ${waits.running.join(", within it: ")}\n` : waits.last ? `no wait ran then; its last wait, which had ended: ${waits.last}\n` : "";
  const said = `OS: ${systemName}\nscenario: ${id}${restarted ? " (started again)" : ""}\n${waiting}\n${failure}\n`;
  writeText(nativePath(makeDirectory(kept), "failure.txt"), said);
  console.error(`evidence of ${id} in ${kept}:\n${said}`);
  const window = nativePath(kept, "window.png");
  if (shown instanceof Uint8Array) return writeBytes(window, shown);
  await (shown ? shown.saveScreenshot(window) : Promise.reject(new Error("its app is not running, and its flow left no image of its window")))
    .catch((cause: unknown) => writeText(nativePath(kept, "window.txt"), `no screenshot: ${String(cause)}`));
}

/** The image a scenario's spec keeps in its folder once its flow has passed: the window as the
 * flow left it, for a failure that comes after its app has stopped (its teardown's). */
export const LAST_WINDOW = "last-window.png";

/** The file a scenario's spec writes into its folder once its run has passed to its end. */
export const COMPLETED = "completed";

/** The service did not confirm that it stopped all of the batch's apps. */
export const APPS_UNRELEASED = "apps-unreleased";

/** The file a scenario's first phase writes, with what its run returned, once it has passed:
 * its restarted phase starts from it. */
export const FIRST = "first.json";
