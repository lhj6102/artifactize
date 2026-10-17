// One spec execution, one independent scenario, one upstream-owned app capability.
import { selectedScenarios } from "./catalog.ts";
import { asNativePath, readJson, takeSystemFocus, variable } from "./platform.ts";
import type { Prepared } from "./scenario.ts";

// These globals are injected by the configured WDIO Mocha adapter, not node:test.
declare const describe: (title: string, body: () => void) => void;
declare const it: (title: string, body: () => Promise<void>) => void;
declare const browser: WebdriverIO.Browser;

const id = variable("DESKTOP_TEST_SCENARIO");
const context = variable("DESKTOP_TEST_CONTEXT");
if (!id || !context) throw new Error("WDIO did not provide the prepared scenario context");
const [item] = selectedScenarios([id]);
const input = readJson(asNativePath(context)) as Prepared;

describe(item.title, () => {
  it(item.id, async () => {
    try {
      // Setup is complete only when the app runs its own frontend; anything else fails here
      // once, by name, rather than as every later wait of the scenario.
      await browser.waitUntil(() => browser.execute(() => (document.getElementById("app")?.childElementCount ?? 0) > 0),
        { timeout: 30000, timeoutMsg: "setup failed: the test app did not load its frontend" });
      await takeSystemFocus(browser);
      if (variable("DESKTOP_TEST_RELAUNCH") === "1") {
        if (!item.relaunched) throw new Error("this scenario has no relaunch assertion");
        await item.relaunched(browser, input);
      } else await item.run(browser, input);
    } catch (error) {
      // CI keeps no screenshots: a failure carries what the window showed when it failed.
      const shown = await browser.execute(() => document.body.innerText)
        .catch((cause: unknown) => `(the window text could not be read: ${String(cause)})`);
      if (error instanceof Error) error.message += `\n--- the window showed ---\n${shown.slice(0, 4000)}`;
      throw error;
    }
  });
});
