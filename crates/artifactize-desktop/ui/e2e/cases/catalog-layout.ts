// The graph's place in the Main area is fixed, not something added only once a graph can
// render: with nothing chosen, an empty Run, unreadable Runs or a layout in progress it stays
// beside the divider and says so. Each starting condition owns a separate app and fixture.
import assert from "node:assert/strict";
import { createOutside, createUnreadableRuns, savedRuns, spoilRun } from "../fixture.ts";
import { CATALOG_NOTE, createEmptyAudit, createLayout } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { listed, mainTitle, showRun } from "../pane.ts";
import { aside, camera, detail, FIND, findField, graphShown, nodeAt, ZOOM_IN, ZOOM_OUT } from "../graph-view.ts";
import { afterRender, fill, press } from "../keys.ts";

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
      // The graph's tool strip stays above the sentence; there is no canvas.
      assert.equal(await browser.$('.monitor .graph [aria-label="Graph tools"]').isExisting(), true, "the tool strip above the sentence");
      assert.equal(await browser.$(".monitor .graph .canvas").isExisting(), false, "no canvas");
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
      const fixture = createLayout(0, true);
      return { fixture, data: { run: savedRuns(fixture)[0].id } };
    },
    async (browser, { data: { run } }) => {
      await listed(browser, "repo  master");
      await browser.waitUntil(async () => (await slot(browser))?.text.includes("Choose a branch or a waiting review in the Pane.") === true,
        { timeout: 15000, timeoutMsg: "nothing is chosen at launch" });
      const before = (await slot(browser))!;
      // Sample render frames, not each DOM mutation: an explanation replaced before
      // the browser gets a rendering opportunity does not count.
      await browser.execute(() => {
        const trace = { frames: [] as { text: string; bounds: number[]; divider: boolean }[], done: false };
        (window as unknown as { catalogLoading: typeof trace }).catalogLoading = trace;
        const stage = document.querySelector<HTMLElement>(".monitor .stage")!;
        function sample() {
          if (trace.done) return;
          const divider = document.querySelector(".monitor [data-pane-resizer]")!;
          const box = stage.getBoundingClientRect();
          const text = stage.innerText;
          const frame = { text, bounds: [box.left, box.top, box.width, box.height].map(Math.round), divider: divider.getBoundingClientRect().width > 0 };
          requestAnimationFrame(() => {
            // Keep the completed frame, including its geometry; repeats add no evidence.
            if (JSON.stringify(trace.frames.at(-1)) !== JSON.stringify(frame)) trace.frames.push(frame);
            sample();
          });
        }
        requestAnimationFrame(sample);
      });
      await showRun(browser, "repo  master", run);
      // A click where the layout puts the tree's first Artifact, its top-left corner, opens
      // that Artifact's Detail, once the first view holds still.
      await camera(browser);
      // Read the visible canvas in its own backing-store coordinates and colour space.
      // The first block and its gap to the next row are only a few device pixels tall.
      const painted = () => browser.execute(() => {
        const viewport = document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport")!;
        const view = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
        const surface = document.querySelector<HTMLCanvasElement>(".monitor .graph canvas.paint");
        const box = surface?.getBoundingClientRect();
        const context = surface?.getContext("2d");
        if (!surface || !box?.width || !box.height || !context || !surface.checkVisibility({ visibilityProperty: true })) return null;
        const x = view.e + 84 * view.a, y = view.f + 22 * view.d;
        const corner = { x: Math.round(box.left + x), y: Math.round(box.top + y) };
        if (!document.elementFromPoint(corner.x, corner.y)?.closest(".monitor .graph .canvas")) return null;
        const sx = surface.width / box.width, sy = surface.height / box.height;
        const colourSpace = context.getContextAttributes().colorSpace;
        const swatch = document.createElement("canvas").getContext("2d", { colorSpace: colourSpace })!;
        const token = getComputedStyle(document.documentElement).getPropertyValue("--green").trim();
        if (!CSS.supports("color", token)) throw new Error(`invalid GREEN swatch ${token}`);
        swatch.fillStyle = token;
        swatch.fillRect(0, 0, 1, 1);
        const expected = [...swatch.getImageData(0, 0, 1, 1).data];
        const color = [...context.getImageData(Math.floor(x * sx), Math.floor(y * sy), 1, 1).data];
        const green = color[3] === 255 && expected.slice(0, 3).every((value, i) => Math.abs(value - color[i]) <= 1);
        // At this fixture's top left there is a 44-unit node and then the comment
        // area. Sample the gap, excluding one *backing pixel* at the block's edge
        // for resampling bleed. Do not subtract a CSS pixel from a subpixel range.
        const left = Math.ceil((view.e + 4 * view.a) * sx), right = Math.floor((view.e + 150 * view.a) * sx);
        const top = Math.ceil((view.f + 44 * view.d) * sy) + 1;
        const bottom = Math.floor((view.f + 78 * view.d) * sy);
        const hasGap = left >= 0 && right <= surface.width && top >= 0 && bottom <= surface.height && right > left && bottom > top;
        const under = hasGap ? context.getImageData(left, top, right - left, bottom - top).data : null;
        return { corner, color, expected, green, gap: [left, top, right, bottom], scale: [sx, sy],
          noComment: !!under && under.every((alpha, i) => i % 4 !== 3 || alpha === 0) };
      });
      let pixels = await painted();
      await browser.waitUntil(async () => (pixels = await painted())?.green === true,
        { timeout: 15000, timeoutMsg: "the visible canvas paints the first Artifact as a GREEN block" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(pixels)}`); });
      assert.ok(pixels?.noComment, `no pixels of a comment are painted under the block: ${JSON.stringify(pixels)}`);
      const { corner } = pixels;
      const loading = await browser.execute(() => {
        const trace = (window as unknown as { catalogLoading: { frames: { text: string; bounds: number[]; divider: boolean }[]; done: boolean } }).catalogLoading;
        trace.done = true;
        return trace.frames;
      });
      assert.ok(loading.some((shown) => /Loading the graph…|Laying out…/.test(shown.text)),
        `a completed render frame explained the loading: ${JSON.stringify(loading.map((shown) => shown.text.slice(-80)))}`);
      assert.ok(loading.every((shown) => !/Lay out all/.test(shown.text)), "the Run lays out without asking first");
      assert.ok(loading.every((shown) => shown.divider && JSON.stringify(shown.bounds) === JSON.stringify(before.bounds)), "loading kept the same pane geometry and its divider");
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
      const zoomIn = await browser.$(ZOOM_IN);
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
      // Drawn as an element, it shows its comment under it.
      await browser.waitUntil(async () => (await browser.$(`${nodeAt(id)} .comment`).isExisting()) && (await browser.$(`${nodeAt(id)} .comment`).getText()) === CATALOG_NOTE,
        { timeout: 30000, timeoutMsg: `${id}'s comment under it: ${await browser.execute(() => document.querySelectorAll(".monitor .comment").length)} comments drawn` });
      const named = `${id.slice(2)}: GREEN`;
      const label = (await browser.$(nodeAt(id)).getAttribute("aria-label")) ?? "";
      assert.ok(label.startsWith(named) && /^(, \d+ of \d+ evals GREEN)?$/.test(label.slice(named.length)), `the drawn Artifact is named with its verdict: ${label}`);
      await browser.$(nodeAt(id)).click();
      await browser.waitUntil(async () => (await detail(browser))?.title.startsWith(`Artifact ${id.slice(2)}`) === true,
        { timeout: 15000, timeoutMsg: `${id}'s Detail opened` });
      assert.deepEqual((await slot(browser))?.bounds, before.bounds, "the graph fills that same slot");
    },
  ),
  // UX journey: J-GRAPH-LARGE; UX edges: E-GRAPH-BEYOND E-GRAPH-FIND-GO E-GRAPH-CAMERA
  scenario<{ run: string }>(
    "main-large-wholes",
    "draws a whole of a large graph, too far out for names, as its box's outline in its status's colour, opens it on a click, and draws it as a named box again when zoomed in",
    async () => {
      const fixture = createLayout(4);
      return { fixture, data: { run: savedRuns(fixture)[0].id } };
    },
    async (browser, { data: { run } }) => {
      await listed(browser, "repo  master");
      await showRun(browser, "repo  master", run);
      await browser.waitUntil(() => browser.$(".monitor .graph").isExisting(), { timeout: 60000, timeoutMsg: "the large Run's graph shown" });
      await camera(browser);
      // Find brings w00 into view, near enough to read: its box is an element, at its place in the graph.
      await browser.$('[aria-label="Graph tools"]').$("button=Find").click();
      await browser.waitUntil(async () => (await findField(browser))?.focused === true, { timeout: 15000, timeoutMsg: "the find field open" });
      await fill(browser, FIND, "w00");
      let found = await findField(browser);
      await browser.waitUntil(async () => (found = await findField(browser))?.entries.some((entry) => entry.name === "w00") === true, { timeout: 15000, timeoutMsg: "w00 found" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(found)}`); });
      // Its parts match too; the whole itself is chosen by a click.
      const index = found!.entries.findIndex((entry) => entry.name === "w00");
      await (await browser.$$('.find [role="option"]'))[index].click();
      await browser.waitUntil(async () => /^Artifact w00/.test((await detail(browser))?.title ?? ""), { timeout: 15000, timeoutMsg: "w00's Detail open" });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      const node = nodeAt("a:w00");
      await browser.$(node).waitForExist({ timeout: 15000, timeoutMsg: "w00 drawn as an element" });
      /** w00's place in the graph: its corner and size in graph units. */
      const place = await browser.execute((selector) => {
        const element = document.querySelector<HTMLElement>(selector)!;
        const zoom = new DOMMatrixReadOnly(getComputedStyle(document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport")!).transform).a;
        const at = new DOMMatrixReadOnly(getComputedStyle(element).transform);
        const box = element.getBoundingClientRect();
        return { x: at.e, y: at.f, width: box.width / zoom, height: box.height / zoom };
      }, node);
      // Zoomed out until names cannot be read, it is painted: its box's outline in GREEN's colour
      // around a face that is not filled.
      const painted = () => browser.execute((at) => {
        const canvas = document.querySelector<HTMLCanvasElement>(".monitor .graph canvas.paint");
        const viewport = document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport");
        if (!canvas || !viewport) return null;
        const camera = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
        const box = canvas.getBoundingClientRect();
        const ratio = canvas.width / box.width;
        const [left, top] = [camera.e + at.x * camera.a, camera.f + at.y * camera.a];
        const [width, height] = [at.width * camera.a, at.height * camera.a];
        const pixel = (x: number, y: number) => [...canvas.getContext("2d")!.getImageData(Math.round(x * ratio), Math.round(y * ratio), 1, 1).data.slice(0, 3)];
        const middle = top + height / 2;
        const row = Array.from({ length: Math.max(2, Math.round(width) + 4) }, (_, step) => pixel(left - 2 + step, middle));
        return {
          zoom: camera.a,
          centre: { x: Math.round(box.left + left + width / 2), y: Math.round(box.top + middle) },
          row,
          token: getComputedStyle(document.documentElement).getPropertyValue("--green").trim(),
        };
      }, place);
      let far = await painted();
      for (let step = 0; step < 12 && !(far && far.zoom < 0.3); step += 1) {
        await browser.$(ZOOM_OUT).click();
        await camera(browser);
        far = await painted();
      }
      assert.ok(far && far.zoom < 0.3, `w00 painted too far out for names: ${JSON.stringify(far?.zoom)}`);
      const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(far.token)!.slice(1).map((part) => Number.parseInt(part, 16));
      const green = (pixel: number[]) => pixel.every((value, channel) => Math.abs(value - hex[channel]) <= 24);
      const first = far.row.findIndex(green);
      const last = far.row.findLastIndex(green);
      assert.ok(first >= 0 && last > first + 2, `its outline in GREEN's colour on both sides: ${JSON.stringify(far.row)}`);
      assert.ok(!far.row.slice(first + 2, last - 1).every(green), `an outline, not a filled block: ${JSON.stringify(far.row)}`);
      // A click on it, as painted, opens it.
      await browser.action("pointer").move({ x: far.centre.x, y: far.centre.y, origin: "viewport" }).down().up().perform();
      await browser.waitUntil(async () => /^Artifact w00/.test((await detail(browser))?.title ?? ""), { timeout: 15000, timeoutMsg: "the painted whole opened w00's Detail" });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
      // Zoomed in again, it is a named box.
      for (let step = 0; step < 12 && !(await browser.$(node).isExisting()); step += 1) {
        await browser.$(ZOOM_IN).click();
        await camera(browser);
      }
      await browser.$(node).waitForExist({ timeout: 15000, timeoutMsg: "w00 a box again" });
      assert.equal(await browser.$(node).$(".name").getText(), "w00", "named");
    },
  ),
];
