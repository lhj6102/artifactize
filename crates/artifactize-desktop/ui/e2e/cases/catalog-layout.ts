// The graph slot is a fixed pane, not something added only once a graph can render.
// Each external starting condition owns a separate native app and persisted fixture.
import assert from "node:assert/strict";
import { createOutside, createUnreadableRuns } from "../fixture.ts";
import { createEmptyAudit, createLayout } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";

function slot(browser: WebdriverIO.Browser) {
  return browser.execute(() => {
    const stage = document.querySelector<HTMLElement>(".monitor .stage");
    const divider = document.querySelectorAll<HTMLElement>(".monitor [data-pane-resizer]")[1];
    if (!stage || !divider) return null;
    const box = stage.getBoundingClientRect();
    const line = divider.getBoundingClientRect();
    return { bounds: [box.left, box.top, box.width, box.height].map(Math.round),
      divider: line.width > 0 && line.left > 0 && line.right <= box.left + 1,
      text: stage.innerText, notice: stage.querySelector('[role="status"]')?.textContent ?? "" };
  });
}

export const scenarios: Scenario[] = [
  // UX journey: J-MON-CATALOG
  scenario<Record<string, never>>(
    "catalog-no-selected-run",
    "keeps the graph slot and its divider when there is no Run to select",
    async () => ({ fixture: createOutside(), data: {} }),
    async (browser) => {
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("Select a Run.") === true,
        { timeout: 15000, timeoutMsg: "the graph area explains that no Run is selected" });
      const shown = (await slot(browser))!;
      assert.ok(shown.divider && shown.bounds[2] > 0 && shown.bounds[3] > 0);
      assert.equal(await browser.$(".monitor .graph").isExisting(), false);
    },
  ),
  // UX journey: J-MON-CATALOG
  scenario<Record<string, never>>(
    "catalog-empty-graph",
    "keeps the graph slot and divider and explains a readable legacy Run with no Artifacts",
    async () => {
      const { fixture, restore } = createEmptyAudit();
      return { fixture, data: {}, release: async () => restore() };
    },
    async (browser) => {
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("This Run has no Artifacts.") === true,
        { timeout: 15000, timeoutMsg: "the empty saved graph has its own explanation" });
      const shown = (await slot(browser))!;
      assert.ok(shown.divider && shown.bounds[2] > 0 && shown.bounds[3] > 0);
      assert.equal(await browser.$(".monitor .graph").isExisting(), false);
    },
  ),
  // UX journey: J-MON-CATALOG
  scenario<Record<string, never>>(
    "catalog-error",
    "keeps the graph slot and divider when unreadable saved Runs leave no graph to show",
    async () => {
      const { fixture, restore } = createUnreadableRuns(true);
      return { fixture, data: {}, release: async () => restore() };
    },
    async (browser) => {
      await browser.waitUntil(async () => {
        const shown = await slot(browser);
        return !!shown && shown.text.includes("Select a Run.") && shown.notice.includes("Unreadable run");
      }, { timeout: 15000, timeoutMsg: "the read error is explained without losing the graph area" });
      const shown = (await slot(browser))!;
      assert.ok(shown.divider && shown.bounds[2] > 0 && shown.bounds[3] > 0);
      assert.match(shown.notice, /unknown variant `dependency`/);
    },
  ),
  // UX journey: J-MON-CATALOG; UX edge: E-GRAPH-BEYOND
  scenario<Record<string, never>>(
    "catalog-loading",
    "keeps the same graph slot and divider while the real layout worker lays out a large Run",
    async () => ({ fixture: createLayout(), data: {} }),
    async (browser) => {
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
          const divider = document.querySelectorAll(".monitor [data-pane-resizer]")[1];
          if (stage.innerText.includes("Laying out…")) {
            const box = stage.getBoundingClientRect();
            recorded.push({ bounds: [box.left, box.top, box.width, box.height].map(Math.round), divider: divider.getBoundingClientRect().width > 0 });
          }
          if (!stage.querySelector(".graph")) requestAnimationFrame(observe);
        };
        requestAnimationFrame(observe);
      });
      await action().click();
      await browser.waitUntil(() => browser.$(".monitor .graph .svelte-flow__node").isExisting(), { timeout: 60000, timeoutMsg: "the real large layout completed" });
      const loading = await browser.execute(() => (window as unknown as { catalogLoading: { bounds: number[]; divider: boolean }[] }).catalogLoading);
      assert.ok(loading.length > 0, "at least one painted frame explained the layout in progress");
      assert.ok(loading.every((shown) => shown.divider && JSON.stringify(shown.bounds) === JSON.stringify(before.bounds)), "loading kept the same pane geometry and its divider");
      assert.deepEqual((await slot(browser))?.bounds, before.bounds, "the completed graph fills that same slot");
    },
  ),
];
