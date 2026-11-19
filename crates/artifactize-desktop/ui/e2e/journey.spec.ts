// One spec run per app: the scenario the runner gave this capability, or, from
// restarted.spec.ts, the phase of it that runs once its app was started again.
import { selectedScenarios } from "./catalog.ts";
import { recordKeys } from "./keys.ts";
import { takeSystemFocus, variable } from "./platform.ts";
import { asNativePath, exists, nativePath, readJson, writeJson, writeText } from "./portable.ts";
import { COMPLETED, FIRST, keepEvidence, noteWaits, type Context } from "./scenario.ts";
import { App } from "./views/app.ts";

// Globals of the configured WDIO Mocha adapter.
declare const describe: (title: string, body: () => void) => void;
declare const it: (title: string, body: () => Promise<void>) => void;
declare const browser: WebdriverIO.Browser;

const id = variable("DESKTOP_TEST_SCENARIO");
const context = variable("DESKTOP_TEST_CONTEXT");
const restarted = variable("DESKTOP_TEST_PHASE") === "restarted";
if (!id || !context) throw new Error("WDIO did not give the spec its scenario");
const [item] = selectedScenarios([id]);
const input = readJson(asNativePath(context)) as Context;

// An interrupt fails the scenario it stops (a terminal's Ctrl+C reaches every process of the
// batch): its evidence names the wait it was in and holds the window as the app shows it then;
// an app the same interrupt ended shows none, which the evidence says.
let running = false;
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    if (!running) return;
    keepEvidence(item.id, input.evidence, `interrupted by ${signal}`, browser)
      .catch((cause: unknown) => console.error(`no evidence of the interrupted ${item.id} was kept: ${String(cause)}`));
  });
}

describe(item.title, () => {
  it(restarted ? `${item.id} (started again)` : item.id, async () => {
    const app = new App(browser);
    running = true;
    noteWaits(browser);
    try {
      await app.loaded();
      await takeSystemFocus(browser);
      await recordKeys(browser);
      if (restarted) {
        const first = nativePath(input.root, FIRST);
        if (!exists(first)) throw new Error(`${item.id} did not pass before its app was started again`);
        await item.restarted!(app, input, (readJson(first) as { carried: unknown }).carried);
      } else {
        const carried = await item.run(app, input);
        if (item.restarted) {
          // Quit as a person does, by closing the window, before the app is started again.
          await app.exitsBy(() => app.closeWindow(), input.port);
          writeJson(nativePath(input.root, FIRST), { carried: carried ?? null });
          return;
        }
      }
      // The runner accepts the batch only when every scenario says it ran to its end.
      writeText(nativePath(input.root, COMPLETED), "");
    } catch (error) {
      // A failure carries what the window showed when it failed.
      const shown = await app.text().catch((cause: unknown) => `(the window could not be read: ${String(cause)})`);
      if (error instanceof Error) error.message += `\n--- the window showed ---\n${shown.slice(0, 4000)}`;
      throw error;
    } finally {
      running = false;
    }
  });
});
