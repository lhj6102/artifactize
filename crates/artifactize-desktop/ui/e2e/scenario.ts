// A scenario: one journey flow on one app, with the fixtures it prepares before the app starts.
import { systemName, variable, type Environment } from "./platform.ts";
import { asNativePath, exists, makeDirectory, nativePath, writeText, type NativePath } from "./portable.ts";
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
  /** The journeys (J- ids of UX-FEATURES.md) it covers (E2E-COVERAGE). */
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

export function scenario<T>(definition: Omit<Scenario, "prepare" | "run"> & {
  prepare: (own: Owned) => Promise<Prepared<T>>;
  run: (app: App, input: Context<T>) => Promise<unknown>;
  restarted?: (app: App, input: Context<T>, carried: unknown) => Promise<void>;
}): Scenario {
  return definition as Scenario;
}

/** Keep the evidence of the scenario `id`, which failed (E2E-FAILURE-EVIDENCE), in its folder
 * `evidence`: the OS, the scenario and its failure, which names the wait that failed, and a
 * screenshot of the window as `browser` shows it then, or why there is none. What failed first
 * is kept: a later failure of the same run of it changes nothing. */
export async function keepEvidence(id: string, evidence: NativePath, failure: string, browser: WebdriverIO.Browser | undefined): Promise<void> {
  const restarted = variable("DESKTOP_TEST_PHASE") === "restarted";
  const kept = restarted ? asNativePath(`${evidence}-started-again`) : evidence;
  const report = nativePath(kept, "failure.txt");
  if (exists(report)) return;
  writeText(nativePath(makeDirectory(kept), "failure.txt"), `OS: ${systemName}\nscenario: ${id}${restarted ? " (started again)" : ""}\n\n${failure}\n`);
  console.error(`evidence of ${id}: ${kept}`);
  await (browser ? browser.saveScreenshot(nativePath(kept, "window.png")) : Promise.reject(new Error("its app was not started")))
    .catch((cause: unknown) => writeText(nativePath(kept, "window.txt"), `no screenshot: ${String(cause)}`));
}

/** The file a scenario's spec writes into its folder once its run has passed to its end. */
export const COMPLETED = "completed";

/** The file a scenario's first phase writes, with what its run returned, once it has passed:
 * its restarted phase starts from it. */
export const FIRST = "first.json";
