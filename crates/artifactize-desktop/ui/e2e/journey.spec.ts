// One spec run per app: the scenario the runner gave this capability.
import type { ChildProcess } from "node:child_process";
import { remote } from "webdriverio";
import { selectedScenarios } from "./catalog.ts";
import { recordKeys } from "./keys.ts";
import { startTree, stopTree, systemName, takeSystemFocus, variable } from "./platform.ts";
import { asNativePath, embeddedPorts, embeddedReady, makeDirectory, nativePath, readJson, removeTree, writeText } from "./portable.ts";
import { AGAIN, COMPLETED, type Context } from "./scenario.ts";
import { App } from "./views/app.ts";

// Globals of the configured WDIO Mocha adapter.
declare const describe: (title: string, body: () => void) => void;
declare const it: (title: string, body: () => Promise<void>) => void;
declare const browser: WebdriverIO.Browser;

const id = variable("DESKTOP_TEST_SCENARIO");
const context = variable("DESKTOP_TEST_CONTEXT");
if (!id || !context) throw new Error("WDIO did not give the spec its scenario");
const [item] = selectedScenarios([id]);
const input = readJson(asNativePath(context)) as Context;

/** Ready `session`'s window for a scenario: loaded, focused where the OS needs it, its keys
 * recorded. */
async function ready(session: WebdriverIO.Browser): Promise<App> {
  const app = new App(session);
  await app.loaded();
  await takeSystemFocus(session);
  await recordKeys(session);
  return app;
}

describe(item.title, () => {
  it(item.id, async () => {
    let app = new App(browser);
    let session = browser;
    // The app a scenario starts again is this spec's own to end.
    let later: { child: ChildProcess; session?: WebdriverIO.Browser } | undefined;
    const again = async (): Promise<App> => {
      if (later) throw new Error("a scenario starts its app again once");
      await app.exitsBy(() => app.closeWindow(), input.port);
      const [port] = await embeddedPorts(1);
      const env = { ...process.env, ...input.launch.env, TAURI_WEBDRIVER_PORT: String(port), WDIO_EMBEDDED_SERVER: "true" };
      later = { child: startTree(input.launch.app, input.launch.args, { cwd: input.root, env, capture: false }) };
      // Should this worker end before its teardown, the runner's teardown ends the app by this.
      writeText(nativePath(input.root, AGAIN), String(later.child.pid));
      const deadline = Date.now() + 60000;
      while (!(await embeddedReady(port))) {
        if (later.child.exitCode !== null) throw new Error(`the app started again exited with ${later.child.exitCode}`);
        if (Date.now() > deadline) throw new Error("the app started again did not serve WebDriver within a minute");
        await new Promise((done) => setTimeout(done, 100));
      }
      later.session = await remote({ hostname: "127.0.0.1", port, logLevel: "warn", capabilities: { browserName: "tauri", "wdio:enforceWebDriverClassic": true } });
      session = later.session;
      app = await ready(later.session);
      return app;
    };
    let failure: unknown;
    try {
      await ready(browser);
      await item.run(app, input, again);
    } catch (error) {
      // A failure carries what the window showed when it failed.
      const shown = await app.text().catch((cause: unknown) => `(the window could not be read: ${String(cause)})`);
      if (error instanceof Error) error.message += `\n--- the window showed ---\n${shown.slice(0, 4000)}`;
      failure = error;
      // Its evidence: the OS, the scenario and the failed wait, and the window at that point.
      const kept = makeDirectory(input.evidence);
      writeText(nativePath(kept, "failure.txt"), `OS: ${systemName}\nscenario: ${item.id} (${item.title})\n\n${error instanceof Error ? error.stack ?? error.message : String(error)}\n`);
      await session.saveScreenshot(nativePath(kept, "window.png")).catch((cause: unknown) => writeText(nativePath(kept, "window.txt"), `no screenshot: ${String(cause)}`));
      console.error(`evidence of ${item.id}: ${kept}`);
    }
    if (later) {
      try { await later.session?.deleteSession(); }
      catch (error) { failure ??= error; }
      finally {
        // Its record goes once the app is known to have ended; else the runner's teardown ends it.
        const ended = later.child.exitCode !== null ? Promise.resolve() : new Promise((done) => later!.child.once("exit", done));
        stopTree(later.child);
        await Promise.race([ended, new Promise((done) => setTimeout(done, 15000))]);
        if (later.child.exitCode !== null || later.child.signalCode !== null) removeTree(nativePath(input.root, AGAIN));
        else failure ??= new Error("the app started again did not end");
      }
    }
    if (failure) throw failure;
    // The runner accepts the batch only when every scenario says it ran to its end, its own
    // teardown included.
    writeText(nativePath(input.root, COMPLETED), "");
  });
});
