// A scenario: one journey flow on one app, with the fixtures it prepares before the app starts.
import type { Environment } from "./platform.ts";
import type { NativePath } from "./portable.ts";
import type { Owned } from "./fixtures/owned.ts";
import type { Fixture } from "./fixtures/repo.ts";
import type { App } from "./views/app.ts";

/** What a scenario prepares: the fixture the app opens, data its run reads (JSON), the app's
 * extra arguments and variables (an `undefined` removes one), and another place to run the app
 * from. */
export type Prepared<T = unknown> = { fixture: Fixture; data: T; args?: string[]; env?: Environment; app?: NativePath };

/** What a scenario's run gets: what it prepared, its folder, the port of the app's embedded
 * WebDriver server, which lives as long as the app, and how the app was started. */
export type Context<T = unknown> = Prepared<T> & {
  root: NativePath; port: number; launch: { app: NativePath; args: string[]; env: Record<string, string> };
  /** Where its failure, a screenshot of the window then and its logs are kept, should it fail. */
  evidence: NativePath;
};

/** Quit the app by closing its window and start it again as it was started, on the same state:
 * a later app of this machine's user. Once per scenario. */
export type Again = () => Promise<App>;

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
  run: (app: App, input: Context, again: Again) => Promise<void>;
};

export function scenario<T>(definition: Omit<Scenario, "prepare" | "run"> & {
  prepare: (own: Owned) => Promise<Prepared<T>>;
  run: (app: App, input: Context<T>, again: Again) => Promise<void>;
}): Scenario {
  return definition as Scenario;
}

/** The file a scenario's spec writes into its folder once its run has passed to its end. */
export const COMPLETED = "completed";

/** The file holding the process ID of an app a scenario started again, while it runs. */
export const AGAIN = "again.pid";
