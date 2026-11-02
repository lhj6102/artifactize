import type { Fixture } from "./fixture.ts";
import type { Capability, Environment, NativePath } from "./platform.ts";

/** Domain inputs prepared before the service launches a capability's app. */
export type Prepared<T = unknown> = {
  fixture: Fixture;
  args?: string[];
  env?: Environment;
  data: T;
  /** The address on the case's loopback server, or the start of the addresses, that the app
   * opens in the system browser; the case then gets a fixture browser. */
  browser?: string;
  /** The candidate app placed elsewhere ([`placeApp`]); the case launches it instead. */
  app?: NativePath;
  release?: () => Promise<void>;
};

/** A case is a whole independent app scenario, never an app lifecycle owner. */
export type Scenario = {
  id: string;
  title: string;
  prepare: () => Promise<Prepared>;
  run: (browser: WebdriverIO.Browser, input: Prepared) => Promise<void>;
  /** A second service-owned app lifetime, on this case's still-owned fixture. */
  relaunched?: (browser: WebdriverIO.Browser, input: Prepared) => Promise<void>;
  /** What the case needs of the system; it runs only where all of it is offered. */
  needs?: Capability[];
};

/** `item`, run only on systems that offer everything in `needs`. */
export function needing(needs: Capability[], item: Scenario): Scenario {
  return { ...item, needs };
}

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

/** The prepared inputs `build` makes while `server` already runs, released with it. When
 * `build` throws, the server closes before the failure goes on, so a failed preparation leaves
 * nothing listening. */
export async function servedBy<T>(server: { close: () => Promise<void> }, build: () => Omit<Prepared<T>, "release">): Promise<Prepared<T>> {
  let prepared: Omit<Prepared<T>, "release">;
  try { prepared = build(); } catch (error) {
    await server.close();
    throw error;
  }
  return { ...prepared, release: () => server.close() };
}
