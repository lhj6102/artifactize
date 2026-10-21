// The graph's place in the Main area is fixed, not something added only once a graph can
// render: with nothing chosen, an empty Run, unreadable Runs or a layout in progress it stays
// beside the divider and says so. Each starting condition owns a separate app and fixture.
import assert from "node:assert/strict";
import { createOutside, createUnreadableRuns, savedRuns } from "../fixture.ts";
import { createEmptyAudit, createLayout } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { listed, mainTitle, showRun } from "../pane.ts";

function slot(browser: WebdriverIO.Browser) {
  return browser.execute(() => {
    const stage = document.querySelector<HTMLElement>(".monitor .stage");
    const divider = document.querySelector<HTMLElement>(".monitor [data-pane-resizer]");
    if (!stage || !divider) return null;
    const box = stage.getBoundingClientRect();
    const line = divider.getBoundingClientRect();
    return { bounds: [box.left, box.top, box.width, box.height].map(Math.round),
      divider: line.width > 0 && line.left > 0 && line.right <= box.left + 1,
      text: stage.innerText, notice: stage.querySelector('section.panel[aria-label="Main area"] > header [role="status"]')?.textContent ?? "" };
  });
}

export const scenarios: Scenario[] = [
  // UX journey: J-MAIN-RUN
  scenario<Record<string, never>>(
    "main-nothing-chosen",
    "keeps the graph's place and its divider with nothing chosen, and says so",
    async () => ({ fixture: createOutside(), data: {} }),
    async (browser) => {
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("Choose a branch or a waiting review in the Pane.") === true,
        { timeout: 15000, timeoutMsg: "the Main area explains that nothing is chosen" });
      const shown = (await slot(browser))!;
      assert.ok(shown.divider && shown.bounds[2] > 0 && shown.bounds[3] > 0);
      assert.equal(await mainTitle(browser), "Nothing chosen");
      assert.equal(await browser.$(".monitor .graph").isExisting(), false);
    },
  ),
  // UX journey: J-MAIN-RUN
  scenario<{ run: string }>(
    "main-empty-graph",
    "keeps the graph's place and divider and explains a readable legacy Run with no Artifacts",
    async () => {
      const { fixture, restore } = createEmptyAudit();
      return { fixture, data: { run: savedRuns(fixture)[0].id }, release: async () => restore() };
    },
    async (browser, { data: { run } }) => {
      await listed(browser, "repo  master");
      await showRun(browser, "repo  master", run);
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("This Run has no Artifacts.") === true,
        { timeout: 15000, timeoutMsg: "the empty saved graph has its own explanation" });
      const shown = (await slot(browser))!;
      assert.ok(shown.divider && shown.bounds[2] > 0 && shown.bounds[3] > 0);
      assert.equal(await browser.$(".monitor .graph").isExisting(), false);
    },
  ),
  // UX journey: J-MAIN-RUN
  scenario<Record<string, never>>(
    "main-error",
    "keeps the graph's place and divider when no saved Run can be read, its notice saying why",
    async () => {
      const { fixture, restore } = createUnreadableRuns(true);
      return { fixture, data: {}, release: async () => restore() };
    },
    async (browser) => {
      await browser.waitUntil(async () => {
        const shown = await slot(browser);
        return !!shown && shown.text.includes("Choose a branch or a waiting review in the Pane.") && shown.notice.includes("Unreadable run");
      }, { timeout: 15000, timeoutMsg: "the read error is explained without losing the graph's place" });
      const shown = (await slot(browser))!;
      assert.ok(shown.divider && shown.bounds[2] > 0 && shown.bounds[3] > 0);
      assert.match(shown.notice, /unknown variant `dependency`/);
    },
  ),
  // UX journeys: J-MAIN-RUN J-GRAPH-LARGE
  scenario<{ run: string }>(
    "main-loading",
    "keeps the same graph place and divider while the real layout worker lays out a large Run",
    async () => {
      const fixture = createLayout();
      return { fixture, data: { run: savedRuns(fixture)[0].id } };
    },
    async (browser, { data: { run } }) => {
      await listed(browser, "repo  master");
      await showRun(browser, "repo  master", run);
      const action = () => browser.$("button=Lay out all 1200");
      await action().waitForExist({ timeout: 30000 });
      const before = (await slot(browser))!;
      assert.ok(before.divider);
      // Observe after the browser paints each frame, not a mutation or a guessed delay.
      await browser.execute(() => {
        const recorded: { bounds: number[]; divider: boolean }[] = [];
        (window as unknown as { catalogLoading: typeof recorded }).catalogLoading = recorded;
        const observe = () => {
          const stage = document.querySelector<HTMLElement>(".monitor .stage")!;
          const divider = document.querySelector(".monitor [data-pane-resizer]")!;
          if (stage.innerText.includes("Laying out…")) {
            const box = stage.getBoundingClientRect();
            recorded.push({ bounds: [box.left, box.top, box.width, box.height].map(Math.round), divider: divider.getBoundingClientRect().width > 0 });
          }
          if (!stage.querySelector(".graph .svelte-flow__node")) requestAnimationFrame(observe);
        };
        requestAnimationFrame(observe);
      });
      await action().click();
      await browser.waitUntil(() => browser.$(".monitor .graph .svelte-flow__node").isExisting(), { timeout: 60000, timeoutMsg: "the real large layout completed" });
      const loading = await browser.execute(() => (window as unknown as { catalogLoading: { bounds: number[]; divider: boolean }[] }).catalogLoading);
      assert.ok(loading.length > 0, "at least one painted frame explained the layout in progress");
      assert.ok(loading.every((shown) => shown.divider && JSON.stringify(shown.bounds) === JSON.stringify(before.bounds)), "loading kept the same place and its divider");
      assert.deepEqual((await slot(browser))?.bounds, before.bounds, "the completed graph fills that same place");
    },
  ),
];
