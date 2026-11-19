import type { Fixture } from "./fixture.ts";
import type { BrowserEntry } from "./browser-fixtures.ts";
import type { Environment } from "./platform.ts";
import type { NativePath } from "./portable.ts";

/** Domain inputs prepared before the service launches a capability's app. */
export type Prepared<T = unknown> = {
  fixture: Fixture;
  args?: string[];
  env?: Environment;
  data: T;
  /** Only this fixture’s exact origins or files are eligible for browser cleanup. */
  browser?: BrowserEntry;
  /** The candidate app placed elsewhere ([`placeApp`]); the case launches it instead. */
  app?: NativePath;
  /** Stops what the fixture runs (servers, jobs); always called at teardown. */
  release?: () => Promise<void>;
  /** Puts the fixture's files back before they are deleted; skipped when they are kept for
   * diagnosis because the app's end could not be confirmed. */
  restore?: () => Promise<void>;
};

/** A case is a whole independent app scenario, never an app lifecycle owner. */
export type Scenario = {
  id: string;
  title: string;
  prepare: () => Promise<Prepared>;
  run: (browser: WebdriverIO.Browser, input: Prepared) => Promise<void>;
  /** A second service-owned app lifetime, on this case's still-owned fixture. */
  relaunched?: (browser: WebdriverIO.Browser, input: Prepared) => Promise<void>;
};

export function scenario<T>(
  id: string,
  title: string,
  prepare: () => Promise<Prepared<T>>,
  run: (browser: WebdriverIO.Browser, input: Prepared<T>) => Promise<void>,
  relaunched?: (browser: WebdriverIO.Browser, input: Prepared<T>) => Promise<void>,
): Scenario {
  return {
    id,
    title,
    prepare,
    run: (browser, input) => run(browser, input as Prepared<T>),
    ...(relaunched ? { relaunched: (browser: WebdriverIO.Browser, input: Prepared) => relaunched(browser, input as Prepared<T>) } : {}),
  };
}

/** The file a scenario's spec writes into its fixture root once its run (or its relaunch) has
 * passed to its end; a batch without it for every scenario and phase failed. */
export function completionMarker(relaunch: boolean): string {
  return relaunch ? "completed-relaunch" : "completed-run";
}
