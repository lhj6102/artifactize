// The graph's place in the Main area is fixed, not something added only once a graph can
// render: with nothing chosen, an empty Run, unreadable Runs or a layout in progress it stays
// beside the divider and says so. Each starting condition owns a separate app and fixture.
import assert from "node:assert/strict";
import { createOutside, createUnreadableRuns, savedRuns, spoilRun } from "../fixture.ts";
import { createEmptyAudit, createLayout } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { listed, mainTitle, showRun } from "../pane.ts";
import { aside, detail, graphShown, nodeAt } from "../graph-view.ts";
import { afterRender } from "../keys.ts";

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
  // UX journey: J-MAIN-RUN
  scenario<{ readable: string }>(
    "main-run-unreadable",
    "keeps the graph's place and divider when the shown Run can no longer be read, saying so in that place",
    async () => {
      const { fixture, readable, restore } = createUnreadableRuns();
      return { fixture, args: ["--repo", fixture.repo], data: { readable }, release: async () => restore() };
    },
    async (browser, { fixture, data: { readable } }) => {
      await listed(browser, "repo  master");
      await showRun(browser, "repo  master", readable);
      await graphShown(browser, "a:n000");
      const before = (await slot(browser))!;
      // Another version rewrites the shown Run: its graph can no longer be read.
      spoilRun(fixture, readable);
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("This Run cannot be read; choose another.") === true,
        { timeout: 15000, timeoutMsg: `the graph's place says the shown Run cannot be read: ${JSON.stringify(await slot(browser))}` });
      const after = (await slot(browser))!;
      assert.deepEqual(after.bounds, before.bounds, "the graph's place stays where it was");
      assert.ok(after.divider, "the divider stays");
      assert.match(after.notice, new RegExp(`Unreadable run ${readable}`), "the notice names the Run");
    },
  ),
  // UX journeys: J-MAIN-RUN J-GRAPH-LARGE; UX edge: E-GRAPH-BEYOND
  scenario<{ run: string }>(
    "main-loading",
    "keeps the graph's place and divider while a large Run loads and says so, then lays it out whole, opens an Artifact clicked in it and those a zoom brings into view",
    async () => {
      const fixture = createLayout();
      return { fixture, data: { run: savedRuns(fixture)[0].id } };
    },
    async (browser, { data: { run } }) => {
      await listed(browser, "repo  master");
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("Choose a branch or a waiting review in the Pane.") === true,
        { timeout: 15000, timeoutMsg: "nothing is chosen at launch" });
      const before = (await slot(browser))!;
      // Every state of the graph slot from now until the graph shows, with its geometry: one
      // record per change to the page, laid out as it was before anything replaced it.
      await browser.execute(() => {
        const recorded: { text: string; bounds: number[]; divider: boolean }[] = [];
        (window as unknown as { catalogLoading: typeof recorded }).catalogLoading = recorded;
        const stage = document.querySelector<HTMLElement>(".monitor .stage")!;
        const observer = new MutationObserver(() => {
          const divider = document.querySelector(".monitor [data-pane-resizer]")!;
          const box = stage.getBoundingClientRect();
          recorded.push({ text: stage.innerText, bounds: [box.left, box.top, box.width, box.height].map(Math.round), divider: divider.getBoundingClientRect().width > 0 });
          if (stage.querySelector(".graph")) observer.disconnect();
        });
        observer.observe(stage, { childList: true, subtree: true, characterData: true });
      });
      await showRun(browser, "repo  master", run);
      await browser.waitUntil(() => browser.$(".monitor .graph").isExisting(), { timeout: 60000, timeoutMsg: "the large Run's graph shown" });
      const loading = await browser.execute(() => (window as unknown as { catalogLoading: { text: string; bounds: number[]; divider: boolean }[] }).catalogLoading);
      const explained = loading.filter((shown) => /Loading the graph…|Laying out…/.test(shown.text));
      assert.ok(explained.length > 0, `the graph slot said it was loading: ${JSON.stringify(loading.map((shown) => shown.text.slice(-80)))}`);
      assert.ok(loading.every((shown) => !/Lay out all/.test(shown.text)), "the Run lays out without asking first");
      assert.ok(loading.every((shown) => shown.divider && JSON.stringify(shown.bounds) === JSON.stringify(before.bounds)), "loading kept the same pane geometry and its divider");
      // A click where the layout puts the tree's first Artifact, its top-left corner, opens
      // that Artifact's Detail.
      const corner = await browser.execute(() => {
        const viewport = document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport")!;
        const camera = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
        const canvas = document.querySelector(".monitor .graph .canvas")!.getBoundingClientRect();
        return { x: Math.round(canvas.left + camera.e + 84 * camera.a), y: Math.round(canvas.top + camera.f + 22 * camera.d) };
      });
      await browser.action("pointer").move({ x: corner.x, y: corner.y, origin: "viewport" }).down().up().perform();
      await browser.waitUntil(async () => /^Artifact n\d{4}\b/.test((await detail(browser))?.title ?? ""), { timeout: 15000, timeoutMsg: "the clicked Artifact's Detail opened" });
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "the Detail closed" });
      // Zooming in draws named Artifacts it brings into view, ones not drawn before it; one of
      // them takes the pointer like any other.
      const visible = () => browser.execute(() => {
        const canvas = document.querySelector(".monitor .graph .canvas")!.getBoundingClientRect();
        return [...document.querySelectorAll<HTMLElement>(".monitor .graph .svelte-flow__node")].filter((node) => {
          const box = node.getBoundingClientRect();
          return box.width > 0 && box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom;
        }).map((node) => node.dataset.id ?? "");
      });
      const drawnBefore = new Set(await visible());
      const brought = async () => (await visible()).filter((id) => !drawnBefore.has(id));
      const zoomIn = await browser.$(".monitor .svelte-flow__controls-zoomin");
      let zooms = 0;
      for (; zooms < 20 && (await brought()).length === 0; zooms += 1) {
        await zoomIn.click();
        await afterRender(browser);
      }
      await browser.waitUntil(async () => (await brought()).length > 0, { timeout: 15000, timeoutMsg: "zooming in drew Artifacts it brought into view" });
      assert.ok(zooms > 0, "the Artifacts were brought into view by zooming in");
      const [id] = await brought();
      assert.match(id, /^a:n\d{4}$/);
      await graphShown(browser, id);
      assert.equal(await browser.$(nodeAt(id)).getAttribute("aria-label"), `${id.slice(2)}: GREEN`, "the drawn Artifact is named with its verdict");
      await browser.$(nodeAt(id)).click();
      await browser.waitUntil(async () => (await detail(browser))?.title.startsWith(`Artifact ${id.slice(2)}`) === true,
        { timeout: 15000, timeoutMsg: `${id}'s Detail opened` });
      assert.deepEqual((await slot(browser))?.bounds, before.bounds, "the graph fills that same slot");
    },
  ),
];
