// One spec execution, one independent scenario, one upstream-owned app capability.
import { selectedScenarios } from "./catalog.ts";
import { asNativePath, readJson, variable } from "./platform.ts";
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
      await item.run(browser, input);
    } catch (error) {
      // CI keeps no screenshots: a failure carries what the window showed when it failed.
      const shown = await browser.execute(() => document.body.innerText)
        .catch((cause: unknown) => `(the window text could not be read: ${String(cause)})`);
      if (error instanceof Error) error.message += `\n--- the window showed ---\n${shown.slice(0, 4000)}`;
      throw error;
    }
  });
});
