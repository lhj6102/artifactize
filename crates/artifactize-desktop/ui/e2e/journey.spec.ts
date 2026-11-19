// One spec run per app: the scenario the runner gave this capability.
import { selectedScenarios } from "./catalog.ts";
import { recordKeys } from "./keys.ts";
import { takeSystemFocus, variable } from "./platform.ts";
import { asNativePath, nativePath, readJson, writeText } from "./portable.ts";
import { COMPLETED, type Context } from "./scenario.ts";
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

describe(item.title, () => {
  it(item.id, async () => {
    const app = new App(browser);
    try {
      await app.loaded();
      await takeSystemFocus(browser);
      await recordKeys(browser);
      await item.run(app, input);
      // The runner accepts the batch only when every scenario says it ran to its end.
      writeText(nativePath(input.root, COMPLETED), "");
    } catch (error) {
      // A failure carries what the window showed when it failed.
      const shown = await app.text().catch((cause: unknown) => `(the window could not be read: ${String(cause)})`);
      if (error instanceof Error) error.message += `\n--- the window showed ---\n${shown.slice(0, 4000)}`;
      throw error;
    }
  });
});
