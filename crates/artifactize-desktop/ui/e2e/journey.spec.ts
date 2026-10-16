// One spec execution, one independent scenario, one upstream-owned app capability.
import { readFileSync } from "node:fs";
import { selectedScenarios } from "./catalog.ts";
import { failureEvidence } from "./diagnostics.ts";
import type { Prepared } from "./scenario.ts";

// These globals are injected by the configured WDIO Mocha adapter, not node:test.
declare const describe: (title: string, body: () => void) => void;
declare const it: (title: string, body: () => Promise<void>) => void;
declare const browser: WebdriverIO.Browser;

const id = process.env.DESKTOP_TEST_SCENARIO;
const context = process.env.DESKTOP_TEST_CONTEXT;
if (!id || !context) throw new Error("WDIO did not provide the prepared scenario context");
const [item] = selectedScenarios([id]);
const input = JSON.parse(readFileSync(context, "utf8")) as Prepared;

describe(item.title, () => {
  it(item.id, async () => {
    try { await item.run(browser, input); }
    catch (error) {
      console.error(`Desktop E2E failure evidence (${item.id}): ${JSON.stringify(await failureEvidence(browser))}`);
      throw error;
    }
  });
});
