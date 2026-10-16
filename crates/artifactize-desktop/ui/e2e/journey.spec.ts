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
    await item.run(browser, input);
  });
});
