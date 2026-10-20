// The graph slot is a fixed pane, not something added only once a graph can render.
// Each external starting condition owns a separate native app and persisted fixture.
import assert from "node:assert/strict";
import { createOutside, createUnreadableRuns } from "../fixture.ts";
import { createEmptyAudit, createLayout } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { detail } from "../graph-view.ts";
import { afterRender } from "../keys.ts";

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
    "lays out a large Run at once in the same graph slot, paints it whole, opens a painted Artifact, and names those a zoom brings into view",
    async () => ({ fixture: createLayout(), data: {} }),
    async (browser) => {
      // Observe after the browser paints each frame, not a mutation or a guessed delay, from
      // before the graph shows until it has.
      await browser.execute(() => {
        const recorded: { bounds: number[]; divider: boolean; asked: boolean }[] = [];
        (window as unknown as { catalogLoading: typeof recorded }).catalogLoading = recorded;
        const observe = () => {
          const stage = document.querySelector<HTMLElement>(".monitor .stage");
          const divider = document.querySelectorAll(".monitor [data-pane-resizer]")[1];
          if (stage && divider) {
            const box = stage.getBoundingClientRect();
            recorded.push({ bounds: [box.left, box.top, box.width, box.height].map(Math.round), divider: divider.getBoundingClientRect().width > 0,
              asked: /Lay out all/.test(stage.innerText) });
          }
          if (!stage?.querySelector(".graph .paint")) requestAnimationFrame(observe);
        };
        requestAnimationFrame(observe);
      });
      await browser.waitUntil(() => browser.$(".monitor .graph .paint").isExisting(), { timeout: 60000, timeoutMsg: "the large Run's graph painted" });
      const loading = await browser.execute(() => (window as unknown as { catalogLoading: { bounds: number[]; divider: boolean; asked: boolean }[] }).catalogLoading);
      const after = (await slot(browser))!;
      assert.ok(loading.length > 0 && loading.every((shown) => !shown.asked), "the Run lays out without asking first");
      assert.ok(loading.every((shown) => shown.divider && JSON.stringify(shown.bounds) === JSON.stringify(after.bounds)), "loading kept the same pane geometry and its divider");
      // Whole in view, its 1200 Artifacts are painted rather than kept as elements.
      const elements = () => browser.execute(() => document.querySelectorAll(".monitor .graph .svelte-flow__node").length);
      assert.equal(await elements(), 0);
      // A click on a painted Artifact opens its Detail: the tree's first Artifact sits at the
      // layout's top-left corner.
      const corner = await browser.execute(() => {
        const viewport = document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport")!;
        const camera = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
        const canvas = document.querySelector(".monitor .graph .canvas")!.getBoundingClientRect();
        return { x: Math.round(canvas.left + camera.e + 84 * camera.a), y: Math.round(canvas.top + camera.f + 22 * camera.d) };
      });
      await browser.action("pointer").move({ x: corner.x, y: corner.y, origin: "viewport" }).down().up().perform();
      await browser.waitUntil(async () => /^Artifact n\d{4}\b/.test((await detail(browser))?.title ?? ""), { timeout: 15000, timeoutMsg: "the painted Artifact's Detail opened" });
      await browser.$('aside[aria-label="Detail"]').$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      // Zooming in until few enough are in view draws them as named elements again.
      const zoomIn = await browser.$(".monitor .svelte-flow__controls-zoomin");
      for (let step = 0; step < 20 && (await elements()) === 0; step += 1) {
        await zoomIn.click();
        await afterRender(browser);
      }
      await browser.waitUntil(async () => (await elements()) > 0, { timeout: 15000, timeoutMsg: "zooming in drew the Artifacts in view as elements" });
      assert.ok((await elements()) <= 500);
      assert.deepEqual((await slot(browser))?.bounds, after.bounds, "the graph fills that same slot");
    },
  ),
];
