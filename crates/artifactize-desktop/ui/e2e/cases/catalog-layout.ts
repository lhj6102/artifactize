// The graph's place in the Main area is fixed, not something added only once a graph can
// render: with nothing chosen, an empty Run, unreadable Runs or a layout in progress it stays
// beside the divider and says so. Each starting condition owns a separate app and fixture.
import assert from "node:assert/strict";
import { createOutside, createUnreadableRuns, savedRuns, spoilRun } from "../fixture.ts";
import { CATALOG_NOTE, createEmptyAudit, createLayout } from "../catalog-fixtures.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { chooseChip, listed, mainTitle, scanned, showRun } from "../pane.ts";
import { aside, camera, detail, FIND, findField, graphShown, nodeAt, offCentre, ZOOM_IN, ZOOM_OUT } from "../graph-view.ts";
import { afterRender, fill, press } from "../keys.ts";
import { canvasSnapshot } from "../rendering.ts";
import { listFiles, nativePath, readText } from "../platform.ts";
import { dispatchInput } from "../input.ts";

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
      return { fixture, data: { run: savedRuns(fixture)[0].id }, restore: async () => restore() };
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
      return { fixture, data: {}, restore: async () => restore() };
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
      return { fixture, args: ["--repo", fixture.repo], data: { readable }, restore: async () => restore() };
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
  // UX journeys: J-MAIN-RUN J-GRAPH-LARGE
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
      // Only accept a loading note that is still visible on both sides of a
      // rendering opportunity. A note replaced in the first callback's frame
      // can never be evidence, even if it already had layout geometry.
      await browser.execute(() => {
        const trace = { frames: [] as { text: string; bounds: number[]; divider: boolean }[], done: false };
        (window as unknown as { catalogLoading: typeof trace }).catalogLoading = trace;
        const stage = document.querySelector<HTMLElement>(".monitor .stage")!;
        const read = () => {
          const divider = document.querySelector(".monitor [data-pane-resizer]")?.getBoundingClientRect();
          const box = stage.getBoundingClientRect();
          const note = [...stage.querySelectorAll<HTMLElement>("p.note")].find((note) => {
            if (!/Loading the graph…|Laying out…/.test(note.innerText)
              || !note.checkVisibility({ opacityProperty: true, visibilityProperty: true })) return false;
            const range = document.createRange();
            range.selectNodeContents(note);
            const text = range.getBoundingClientRect();
            return text.width > 0 && text.height > 0
              && note.contains(document.elementFromPoint(text.left + text.width / 2, text.top + text.height / 2));
          });
          return { note, frame: { text: note?.innerText ?? (stage.innerText.includes("Lay out all") ? "Lay out all" : ""),
            bounds: [box.left, box.top, box.width, box.height].map(Math.round),
            divider: !!divider && divider.width > 0 && divider.left > 0 && divider.right <= box.left + 1 } };
        };
        let previous: ReturnType<typeof read> | undefined;
        function sample() {
          if (trace.done) return;
          const now = read();
          if (previous && now.note === previous.note && now.frame.text === previous.frame.text
            && JSON.stringify(trace.frames.at(-1)) !== JSON.stringify(now.frame)) trace.frames.push(now.frame);
          previous = now;
          requestAnimationFrame(sample);
        }
        requestAnimationFrame(sample);
      });
      await showRun(browser, "repo  master", run);
      // This fixture puts n1199 at the top left. Click that block once the first view holds still.
      await camera(browser);
      // Read the visible canvas in its own backing-store coordinates and colour space.
      // The first block and its gap to the next row are only a few device pixels tall.
      const painted = async () => {
        const sample = await browser.execute(() => {
          const viewport = document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport");
          const surface = document.querySelector<HTMLCanvasElement>(".monitor .graph canvas.paint");
          const box = surface?.getBoundingClientRect();
          if (!viewport || !surface || !box?.width || !box.height || !surface.checkVisibility({ visibilityProperty: true })) return null;
          const view = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
          const x = view.e + 84 * view.a, y = view.f + 22 * view.d;
          const corner = { x: Math.round(box.left + x), y: Math.round(box.top + y) };
          if (!document.elementFromPoint(corner.x, corner.y)?.closest(".monitor .graph .canvas")) return null;
          const sx = surface.width / box.width, sy = surface.height / box.height;
          const token = getComputedStyle(document.documentElement).getPropertyValue("--green").trim();
          const expected = [1, 3, 5].map((at) => Number.parseInt(token.slice(at, at + 2), 16));
          // Exclude one backing pixel at the block's edge for resampling bleed.
          const left = Math.ceil((view.e + 4 * view.a) * sx), right = Math.floor((view.e + 150 * view.a) * sx);
          const top = Math.ceil((view.f + 44 * view.d) * sy) + 1, bottom = Math.floor((view.f + 78 * view.d) * sy);
          const hasGap = left >= 0 && right <= surface.width && top >= 0 && bottom <= surface.height && right > left && bottom > top;
          return { dataUrl: surface.toDataURL(), corner, expected, sampleX: Math.floor(x * sx), sampleY: Math.floor(y * sy),
            left, right, top, bottom, hasGap, scale: [sx, sy] };
        });
        if (!sample) return null;
        const { dataUrl, corner, expected, sampleX, sampleY, left, right, top, bottom, hasGap, scale } = sample;
        const image = canvasSnapshot(dataUrl);
        const color = [...image.at(sampleX, sampleY)];
        const green = color[3] === 255 && expected.every((value, i) => Math.abs(value - color[i]) <= 1);
        let noComment = hasGap;
        for (let y = top; noComment && y < bottom; y++) for (let x = left; x < right; x++) {
          if (image.at(x, y)[3] !== 0) noComment = false;
        }
        return { corner, color, expected, green, gap: [left, top, right, bottom], scale, noComment };
      };
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
      await browser.waitUntil(async () => (/^Artifact n1199(?:\s|$)/).test((await detail(browser))?.title ?? ""),
        { timeout: 15000, timeoutMsg: "the block clicked at the fixture's top left opens n1199's Detail" });
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
  // UX journeys: J-GRAPH-LARGE J-GRAPH-FIND
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
      // Find brings w00 from a graph drawn without names to the center as a named box.
      const node = nodeAt("a:w00");
      await browser.$(".monitor .graph canvas.paint").waitForDisplayed({ timeout: 15000 });
      assert.equal(await browser.$(`${node} .title .name`).isDisplayed(), false, "no named w00 box at the fitted zoom");
      await browser.$('[aria-label="Graph tools"]').$("button=Find").click();
      await browser.waitUntil(async () => (await findField(browser))?.focused === true, { timeout: 15000, timeoutMsg: "the find field open" });
      await fill(browser, FIND, "w00");
      let found = await findField(browser);
      await browser.waitUntil(async () => (found = await findField(browser))?.entries.some((entry) => entry.name === "w00") === true, { timeout: 15000, timeoutMsg: "w00 found" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(found)}`); });
      // Its parts match too; the whole itself is chosen by a click.
      const index = found!.entries.findIndex((entry) => entry.name === "w00");
      await (await browser.$$('.find [role="option"]'))[index].click();
      await browser.waitUntil(async () => (/^Artifact w00(?:\s|$)/).test((await detail(browser))?.title ?? ""), { timeout: 15000, timeoutMsg: "w00's Detail open" });
      await browser.waitUntil(async () => {
        const at = await offCentre(browser, "a:w00");
        const name = browser.$(`${node} .title .name`);
        return at !== null && Math.abs(at.x) <= 4 && Math.abs(at.y) <= 4 && await name.isDisplayed() && await name.getText() === "w00";
      }, { timeout: 15000, timeoutMsg: "Find centers w00 left of the Detail with its name drawn, not an outline" });
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed");
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
      const painted = async () => {
        const sample = await browser.execute((at) => {
          const canvas = document.querySelector<HTMLCanvasElement>(".monitor .graph canvas.paint");
          const viewport = document.querySelector<HTMLElement>(".monitor .svelte-flow__viewport");
          if (!canvas || !viewport) return null;
          const camera = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
          const box = canvas.getBoundingClientRect();
          const ratio = canvas.width / box.width;
          const [left, top] = [camera.e + at.x * camera.a, camera.f + at.y * camera.a];
          const [width, height] = [at.width * camera.a, at.height * camera.a];
          const middle = top + height / 2;
          return {
            dataUrl: canvas.toDataURL(), ratio, left, width, middle, zoom: camera.a,
            centre: { x: Math.round(box.left + left + width / 2), y: Math.round(box.top + middle) },
            token: getComputedStyle(document.documentElement).getPropertyValue("--green").trim(),
          };
        }, place);
        if (!sample) return null;
        const { dataUrl, ratio, left, width, middle, ...shown } = sample;
        const image = canvasSnapshot(dataUrl);
        const row = Array.from({ length: Math.max(2, Math.round(width) + 4) }, (_, step) =>
          [...image.at(Math.round((left - 2 + step) * ratio), Math.round(middle * ratio)).subarray(0, 3)]);
        return { ...shown, row };
      };
      const zoomedOut = async () => (await painted())?.zoom ?? Infinity;
      for (let step = 0; step < 12 && await zoomedOut() >= 0.3; step += 1) {
        await browser.$(ZOOM_OUT).click();
        await camera(browser);
      }
      let far: Awaited<ReturnType<typeof painted>> = null;
      await browser.waitUntil(async () => {
        far = await painted();
        if (!far || far.zoom >= 0.3) return false;
        const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(far.token)!.slice(1).map((part) => Number.parseInt(part, 16));
        const green = (pixel: number[]) => pixel.every((value, channel) => Math.abs(value - hex[channel]) <= 24);
        const first = far.row.findIndex(green);
        const last = far.row.findLastIndex(green);
        return first >= 0 && last > first + 2 && !far.row.slice(first + 2, last - 1).every(green);
      }, { timeout: 15000, timeoutMsg: "w00 paints both sides of the GREEN outline with an unfilled interior" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(far)}`); });
      // A click on it, as painted, opens it.
      await browser.action("pointer").move({ x: far!.centre.x, y: far!.centre.y, origin: "viewport" }).down().up().perform();
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

  // UX journey: J-NODE-MOVE
  scenario<null>(
    "current-block-pan",
    "drags a painted Artifact block on a Current to pan the camera without moving the Artifact or writing a definition",
    async () => ({ fixture: createLayout(), data: null }),
    async (browser, { fixture }) => {
      await listed(browser, "repo  master");
      await chooseChip(browser, "repo  master", "current");
      await browser.waitUntil(async () => /^Scanned at /.test(await scanned(browser)), { timeout: 15000, timeoutMsg: "the large Current scanned" });
      const current = 'section[aria-label="Current"]';
      const node = `${current} ${nodeAt("a:n0000")}`;
      // Find draws the block as a named node, so its place is learned from the real view.
      await browser.$(`${current} [aria-label="Graph tools"]`).$("button=Find").click();
      await browser.waitUntil(async () => (await findField(browser))?.focused === true);
      await fill(browser, FIND, "n0000");
      await browser.waitUntil(async () => (await findField(browser))?.entries[0]?.name === "n0000");
      await browser.$('.find [role="option"]').click();
      await graphShown(browser, "a:n0000");
      await browser.waitUntil(async () => (await detail(browser))?.title === "Artifact n0000");
      await aside(browser).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null);
      const centre = await browser.execute((selector, root) => {
        const node = document.querySelector(selector)!.getBoundingClientRect();
        const canvas = document.querySelector(`${root} canvas.paint`)!.getBoundingClientRect();
        const matrix = new DOMMatrixReadOnly(getComputedStyle(document.querySelector(`${root} .svelte-flow__viewport`)!).transform);
        return { x: (node.x + node.width / 2 - canvas.x - matrix.e) / matrix.a, y: (node.y + node.height / 2 - canvas.y - matrix.f) / matrix.a };
      }, node, current);
      for (let step = 0; step < 12 && await browser.$(node).isExisting(); step += 1) {
        await browser.$(ZOOM_OUT).click();
        await camera(browser);
      }
      const read = async () => {
        const sample = await browser.execute((root, at, selector) => {
          const canvas = document.querySelector<HTMLCanvasElement>(`${root} canvas.paint`)!;
          const clip = canvas.getBoundingClientRect();
          const matrix = new DOMMatrixReadOnly(getComputedStyle(document.querySelector(`${root} .svelte-flow__viewport`)!).transform);
          const [x, y] = [matrix.e + at.x * matrix.a, matrix.f + at.y * matrix.a];
          const point = { x: Math.round(clip.x + x), y: Math.round(clip.y + y) };
          return {
            point, zoom: matrix.a, canvasX: Math.round(x * canvas.width / clip.width), canvasY: Math.round(y * canvas.height / clip.height),
            data: canvas.toDataURL(), named: !!document.querySelector(selector),
            reachable: !!document.elementFromPoint(point.x, point.y)?.matches(`${root} .svelte-flow__pane`),
            direction: point.x + 72 <= clip.right ? 60 : -60,
          };
        }, current, centre, node);
        const { canvasX, canvasY, data, ...position } = sample;
        const bitmap = canvasSnapshot(data);
        const colour = (x: number) => [...bitmap.at(canvasX + x, canvasY)];
        return { ...position, middle: colour(0), beside: [colour(-3), colour(3)] };
      };
      let sample = await read();
      await browser.waitUntil(async () => {
        sample = await read();
        return !sample.named && sample.reachable && sample.middle[3] === 255
          && sample.beside.every((side) => side.join() === sample.middle.join());
      }, { timeout: 15000, timeoutMsg: "n0000 is visible as a solid painted block, not an element" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${JSON.stringify(sample)}`); });
      const before = sample;
      const files = listFiles(nativePath(fixture.repo, ".artifactize"));
      const definitions = files.map(readText);
      const { x, y } = before.point;
      await dispatchInput(browser, { kind: "pointer", phase: "down", x, y, target: `${current} .svelte-flow__pane` });
      for (const offset of [15, 30, 45, 60]) {
        await dispatchInput(browser, { kind: "pointer", phase: "move", x: x + offset * Math.sign(before.direction), y });
      }
      await dispatchInput(browser, { kind: "pointer", phase: "up", x: x + before.direction, y });
      await browser.waitUntil(async () => {
        sample = await read();
        return sample.point.x === x + before.direction && sample.point.y === y && sample.zoom === before.zoom
          && !sample.named && sample.middle.join() === before.middle.join() && (await detail(browser)) === null;
      }, { timeout: 15000, timeoutMsg: "the camera moved by the drag and the same block painted at its translated position" })
        .catch((error: Error) => { throw new Error(`${error.message}: before ${JSON.stringify(before)}, after ${JSON.stringify(sample)}`); });
      assert.deepEqual(listFiles(nativePath(fixture.repo, ".artifactize")), files, "panning does not add or remove definitions");
      assert.deepEqual(files.map(readText), definitions, "panning writes no definition");
    },
  ),
];
