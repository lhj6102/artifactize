import { createConnection } from "node:net";
import { basename, join } from "node:path";
import type { Fixture } from "./fixture.ts";

const windows = new WeakMap<object, { hostname: string; port: number; handle: string }>();

/** Establish the actual embedded endpoint and live native window before a case acts.
 * The WebDriver server lives inside this app, not in an independently-lived driver.
 */
export async function observeWindow(browser: WebdriverIO.Browser): Promise<void> {
  const handles = await browser.getWindowHandles();
  if (handles.length !== 1) throw new Error(`expected one native app window, found ${handles.length}`);
  const { hostname = "127.0.0.1", port } = browser.options;
  if (!port) throw new Error("the runner did not declare the embedded app endpoint");
  await browser.status();
  windows.set(browser, { hostname, port, handle: handles[0] });
}

/** Read-only observation: only refusal at the established in-app server endpoint
 * means it ended. Timeouts, resets, HTTP failures and unrelated transport errors fail.
 * A responsive server must report that this exact native window is no longer present.
 */
export async function windowClosed(browser: WebdriverIO.Browser): Promise<boolean> {
  const owner = windows.get(browser);
  if (!owner) throw new Error("the runner must observe the live app window before the scenario");
  const listening = await new Promise<boolean>((done, fail) => {
    const socket = createConnection({ host: owner.hostname, port: owner.port });
    socket.setTimeout(2000);
    socket.once("connect", () => { socket.destroy(); done(true); });
    socket.once("timeout", () => { socket.destroy(); fail(new Error("the embedded app endpoint timed out")); });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      if (error.code === "ECONNREFUSED") done(false);
      else fail(error);
    });
  });
  if (!listening) return true;
  return !(await browser.getWindowHandles()).includes(owner.handle);
}

/** @wdio/runner endSession always DELETEs a session, but the embedded WebDriver
 * dies with the app. Only a journey expecting real app exit may retire this client
 * session after observing that exit. Remove this bookkeeping when upstream handles it.
 */
export async function sessionEndedByApp(browser: WebdriverIO.Browser, quit: () => Promise<unknown>): Promise<void> {
  if (await windowClosed(browser)) throw new Error("the app had already exited before its quit action");
  let actionFailure: unknown;
  try { await quit(); }
  catch (error) { actionFailure = error; }
  try {
    await browser.waitUntil(() => windowClosed(browser), {
      timeout: 15000,
      timeoutMsg: "the app did not exit after its quit action",
    });
  } catch (error) {
    if (actionFailure !== undefined) throw new AggregateError([actionFailure, error], "the quit action and app-exit observation failed");
    throw error;
  }
  if (!Reflect.set(browser, "sessionId", undefined)) throw new Error("the ended client session could not be retired");
}

export async function saveEvidence(browser: WebdriverIO.Browser, fixture: Fixture, label: string): Promise<void> {
  const output = process.env.HUMAN_FOCUS_EVIDENCE;
  if (output) await browser.saveScreenshot(join(output, `${basename(fixture.root)}-${label.replaceAll(" ", "-")}.png`));
}
