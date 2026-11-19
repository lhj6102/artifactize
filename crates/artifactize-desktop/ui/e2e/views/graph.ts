// The main area as a person reads it: its header, and the graph of a Run or a Current with its
// tool strip, its find field and its navigator; each action waits for what it shows.
import { PNG } from "pngjs";
import { clickAt, dispatchInput, visibleCenter } from "../input.ts";
import { afterRender, press, typeText } from "../keys.ts";
import { holds, until } from "./wait.ts";

const MAIN = 'section.panel[aria-label="Main area"]';
/** The main area's graph, never the one a Detail draws of an Artifact's neighbours. */
const GRAPH = `${MAIN} .graph`;
const TOOLS = '[aria-label="Graph tools"]';
const FIND = 'input[aria-label="Find in the graph"]';

/** A drawn Artifact: its id (`a:name`, a part `a:whole/a:part`), what it reads aloud (its name
 * and verdict), its verdict's icon, its comment and whether a whole is minimized. */
export type Node = { id: string; label: string; icon: string; light: string; comment: string };

/** The find field: whether it has the keyboard, its text, what it lists (name and whether it
 * is chosen) and its note. */
export type Found = { focused: boolean; value: string; entries: { name: string; chosen: boolean }[]; note: string };

export const nodeAt = (id: string) => `${GRAPH} .svelte-flow__node[data-id="${id}"]`;

export class Graph {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
  }

  // -------------------------------------------------------------------------------
  // The main area's header

  /** The header's title: the Run's ID, `Current · {branch}`, `Review`, or `Nothing chosen`. */
  title(): Promise<string> {
    return this.browser.execute((main) => document.querySelector(`${main} > header h2`)?.textContent?.trim() ?? "", MAIN);
  }

  /** Wait until the header's title is `title`. */
  async titled(title: string): Promise<void> {
    await until(this.browser, () => this.title(), (shown) => shown === title, `the main area shows ${title}`);
  }

  /** The Run's headline as it reads aloud: its verdict and what needs attention. */
  headline(): Promise<string> {
    return this.browser.execute((main) => document.querySelector(`${main} > header button.headline`)?.getAttribute("aria-label") ?? "", MAIN);
  }

  /** What the header's row says beside the title, as text. */
  header(): Promise<string> {
    return this.browser.execute((main) => (document.querySelector<HTMLElement>(`${main} > header`)?.innerText ?? "").replace(/\s+/g, " ").trim(), MAIN);
  }

  /** A Current's scan as its header says it: `Scanned at …` once a scan ended, `N left` while
   * one runs, then a failure's words if any. */
  scan(): Promise<string> {
    return this.browser.execute((main) => {
      const scan = document.querySelector(`${main} > header .scan`);
      const said = scan?.querySelector("time")?.getAttribute("aria-label") ?? scan?.querySelector(".checking")?.getAttribute("aria-label") ?? "";
      const failure = scan?.querySelector(".failure")?.textContent?.trim();
      return failure ? `${said} ${failure}` : said;
    }, MAIN);
  }

  /** What the main area says where its view would be (nothing chosen, a Run it cannot read). */
  empty(): Promise<string> {
    return this.browser.execute((main) => document.querySelector(`${main} .empty`)?.textContent?.trim() ?? "", MAIN);
  }

  /** Wait until `check`, a reading of the graph, holds. */
  async until(check: () => Promise<boolean>, what: string, timeout = 15000): Promise<void> {
    await holds(this.browser, check, what, timeout);
  }

  /** A request the main area shows read-only: its status line and instruction. */
  record(): Promise<{ status: string; instruction: string }> {
    return this.browser.execute((main) => {
      const root = document.querySelector(`${main} [role="document"]`);
      const words = (selector: string) => (root?.querySelector<HTMLElement>(selector)?.innerText ?? "").replace(/\s+/g, " ").trim();
      return { status: words(".summary .status"), instruction: words(".instruction") };
    }, MAIN);
  }

  /** The facts under Debug of a request the main area shows read-only, by name, unfolding Debug
   * with a click on its header row when it is folded. */
  async recordDebug(): Promise<Record<string, string>> {
    const head = this.browser.$(`${MAIN} [role="document"] section.section[aria-label="Debug"] > button.head`);
    await head.waitForExist({ timeout: 15000, timeoutMsg: "the request's Debug section" });
    if ((await head.getAttribute("aria-expanded")) === "false") await head.click();
    return until(this.browser, () => this.browser.execute((main) => Object.fromEntries([...document.querySelectorAll(`${main} [role="document"] section.section[aria-label="Debug"] dt`)]
      .map((term) => [term.textContent?.trim() ?? "", term.nextElementSibling?.textContent?.trim() ?? ""])), MAIN), (facts) => Object.keys(facts).length > 0, "Debug unfolded");
  }

  /** Click the Run's headline: the Run's own Detail opens. */
  async openRunDetail(): Promise<void> {
    await this.browser.$(`${MAIN} > header button.headline`).click();
    await holds(this.browser, async () => (await this.browser.$('aside[aria-label="Detail"] h2').isExisting()), "the Run's Detail opened");
  }

  /** Whether the main area has the keyboard. */
  focused(): Promise<boolean> {
    return this.browser.$(MAIN).isFocused();
  }

  /** Press `keys` with the main area's view as it is. */
  async press(keys: string[], done: () => Promise<boolean>, what: string): Promise<void> {
    await press(this.browser, keys, done, what);
  }

  /** How wide the main area is. */
  width(): Promise<number> {
    return this.browser.execute((main) => Math.round(document.querySelector(main)?.getBoundingClientRect().width ?? 0), MAIN);
  }

  // -------------------------------------------------------------------------------
  // Nodes

  /** The drawn Artifacts. */
  nodes(): Promise<Node[]> {
    return this.browser.execute((graph) => [...document.querySelectorAll<HTMLElement>(`${graph} .svelte-flow__node`)].map((node) => ({
      id: node.dataset.id ?? "",
      label: node.getAttribute("aria-label") ?? "",
      icon: node.querySelector(".light svg")?.getAttribute("data-icon") ?? "",
      light: node.dataset.light ?? "",
      comment: (node.querySelector<HTMLElement>(":scope > * > .comment, :scope .artifact > .comment, :scope .whole > .comment")?.innerText ?? "").trim(),
    })), GRAPH);
  }

  /** The node `id` once it is drawn, hit-testable and `ok` holds for it. */
  async node(id: string, ok: (node: Node) => boolean = () => true, what = `the graph draws ${id}`): Promise<Node> {
    const found = await until(this.browser, async () => {
      const covered = await this.browser.execute((selector) => {
        const node = document.querySelector(selector);
        const box = node?.getBoundingClientRect();
        if (!node || !box) return "not drawn";
        if (box.width <= 0 || box.height <= 0) return "an empty box";
        const hit = document.elementFromPoint(box.left + box.width / 2, box.top + Math.min(box.height / 2, 12));
        if (hit?.closest(".svelte-flow__node") === node) return "";
        return `covered at ${Math.round(box.left)},${Math.round(box.top)} by ${hit?.closest("[data-id]")?.getAttribute("data-id") ?? hit?.className ?? "nothing"}`;
      }, nodeAt(id));
      return covered ? covered : (await this.nodes()).find((node) => node.id === id) ?? "gone";
    }, (node) => typeof node !== "string" && ok(node), what);
    return found as Node;
  }

  /** Wait until the node `id` is not drawn. */
  async gone(id: string): Promise<void> {
    await holds(this.browser, async () => !(await this.browser.$(nodeAt(id)).isExisting()), `${id} is not drawn`);
  }

  /** Click the node `id` (on its name): its Detail opens, titled `title`. */
  async open(id: string, title: string): Promise<void> {
    await this.node(id);
    await this.browser.$(`${nodeAt(id)} .name`).click();
    await until(this.browser, () => this.browser.execute(() => document.querySelector('aside[aria-label="Detail"] h2')?.textContent?.trim() ?? ""),
      (shown) => shown === title, `${id}'s Detail opened`);
  }

  /** Click the graph's background: an open Detail closes. */
  async clickBackground(): Promise<void> {
    const point = await this.browser.execute((graph) => {
      const canvas = document.querySelector(`${graph} .canvas`)!.getBoundingClientRect();
      const detail = document.querySelector('aside[aria-label="Detail"]')?.getBoundingClientRect();
      const right = detail && detail.left > canvas.left ? detail.left : canvas.right;
      // The first empty point along the canvas's bottom edge, left of a Detail.
      for (let x = canvas.left + 12; x < right - 12; x += 16) {
        for (const y of [canvas.bottom - 12, canvas.top + 40]) {
          const hit = document.elementFromPoint(x, y);
          if (hit?.classList.contains("svelte-flow__pane")) return { x: Math.round(x), y: Math.round(y) };
        }
      }
      return null;
    }, GRAPH);
    if (!point) throw new Error("the graph shows no background to click");
    await clickAt(this.browser, point);
  }

  /** Give the main area the keyboard as a person does from the pane: Enter on its entries. */
  async focus(): Promise<void> {
    await this.browser.$('[role="group"][aria-label="Pane entries"]').click();
    await holds(this.browser, () => this.browser.$('[role="group"][aria-label="Pane entries"]').isFocused(), "the pane's entries have the keyboard");
    await press(this.browser, ["Enter"], () => this.focused(), "Enter gave the main area the keyboard");
  }

  /** Drag the node `id` (by its title row for a whole) by `dx`, `dy` pixels and release it. */
  async drag(id: string, dx: number, dy: number, handle = ".name"): Promise<void> {
    const from = await visibleCenter(this.browser, await this.browser.$(`${nodeAt(id)} ${handle}`).getElement());
    await this.pointerDrag(from, { x: from.x + dx, y: from.y + dy });
  }

  /** Drag the node `id` onto the node `onto` and release it there. */
  async dragOnto(id: string, onto: string, handle = ".name"): Promise<{ x: number; y: number }> {
    const from = await visibleCenter(this.browser, await this.browser.$(`${nodeAt(id)} ${handle}`).getElement());
    const to = await visibleCenter(this.browser, await this.browser.$(`${nodeAt(onto)} .name`).getElement());
    await this.pointerDrag(from, to);
    return to;
  }

  /** Drag the comment drawn under the node `id` onto the node `onto` and release it there. */
  async dragComment(id: string, onto: string): Promise<void> {
    const from = await visibleCenter(this.browser, await this.browser.$(`${nodeAt(id)} .comment`).getElement());
    const to = await visibleCenter(this.browser, await this.browser.$(`${nodeAt(onto)} .name`).getElement());
    await this.pointerDrag(from, to);
  }

  /** Press at `from`, move through a few points to `to`, release there once the moves have
   * been drawn, as a mouse does. */
  async pointerDrag(from: { x: number; y: number }, to: { x: number; y: number }): Promise<void> {
    await dispatchInput(this.browser, { kind: "pointer", phase: "down", ...from });
    for (let step = 1; step <= 8; step += 1) {
      await dispatchInput(this.browser, { kind: "pointer", phase: "move", x: Math.round(from.x + ((to.x - from.x) * step) / 8), y: Math.round(from.y + ((to.y - from.y) * step) / 8) });
    }
    await afterRender(this.browser);
    await dispatchInput(this.browser, { kind: "pointer", phase: "up", ...to });
  }

  /** Where the node `id` is drawn, in window pixels. */
  box(id: string): Promise<{ x: number; y: number; width: number; height: number } | null> {
    return this.browser.execute((selector) => {
      const box = document.querySelector(selector)?.getBoundingClientRect();
      return box ? { x: Math.round(box.left), y: Math.round(box.top), width: Math.round(box.width), height: Math.round(box.height) } : null;
    }, nodeAt(id));
  }

  /** Click the whole's Minimize or Expand button; the whole is drawn as `minimized` says. */
  async minimize(id: string, name: string, minimized: boolean): Promise<void> {
    await this.browser.$(`${nodeAt(id)} button[aria-label="${minimized ? "Minimize" : "Expand"} ${name} (m)"]`).click();
    await this.node(id, (node) => /, minimized,/.test(node.label) === minimized, `${name} ${minimized ? "minimized" : "expanded"}`);
  }

  /** Whether the tool strip shows `name` (Find, Fit, New Artifact, Reset layout…). */
  async hasTool(name: string): Promise<boolean> {
    return this.browser.$(TOOLS).$(`button*=${name}`).isExisting();
  }

  /** Click the tool strip's `name`. */
  async tool(name: string): Promise<void> {
    await this.browser.$(TOOLS).$(`button*=${name}`).click();
  }

  // -------------------------------------------------------------------------------
  // Camera

  /** The graph's camera: its translation and zoom, once it holds still. */
  async camera(): Promise<string> {
    let last = "";
    return until(this.browser, async () => {
      const before = last;
      last = await this.browser.execute((graph) => document.querySelector<HTMLElement>(`${graph} .svelte-flow__viewport`)?.style.transform ?? "", GRAPH);
      return before === last ? last : "";
    }, (shown) => shown !== "", "the graph's camera held still");
  }

  /** The zoom the tool strip says, in percent. */
  async zoom(): Promise<number> {
    return Number((await this.browser.$(TOOLS).$('[aria-label="Zoom"]').getText()).replace("%", ""));
  }

  /** Click a camera tool (Zoom out, Zoom in, Fit): the camera moves and comes to rest. */
  async cameraTool(name: "Zoom out" | "Zoom in" | "Fit"): Promise<string> {
    const before = await this.camera();
    await (name === "Fit" ? this.browser.$(TOOLS).$("button*=Fit") : this.browser.$(TOOLS).$(`button[aria-label="${name}"]`)).click();
    return until(this.browser, () => this.camera(), (shown) => shown !== before, `${name} moved the camera`);
  }

  /** Whether the Navigator shows. */
  async navigatorShown(): Promise<boolean> {
    return this.browser.$(`${GRAPH} [data-navigator]`).isDisplayed();
  }

  /** The drawn Artifact nearest the middle of the graph's view, and how far from the middle it
   * is drawn (as `offCentre` says it). */
  async central(): Promise<{ id: string; x: number; y: number }> {
    const ids = (await this.nodes()).map((node) => node.id);
    let nearest: { id: string; x: number; y: number } | undefined;
    for (const id of ids) {
      const off = await this.offCentre(id);
      if (off && (!nearest || Math.hypot(off.x, off.y) < Math.hypot(nearest.x, nearest.y))) nearest = { id, ...off };
    }
    if (!nearest) throw new Error("the graph draws no Artifact in view");
    return nearest;
  }

  /** The part in view as the Navigator frames it: the frame's middle as fractions of the map's
   * width and height, and how many pixels of the view one pixel of the map stands for. */
  navigatorFrame(): Promise<{ fx: number; fy: number; step: number }> {
    return this.browser.execute((graph) => {
      const map = document.querySelector(`${graph} [data-navigator] .map`)!.getBoundingClientRect();
      const frame = document.querySelector(`${graph} [data-navigator] .frame`)!.getBoundingClientRect();
      const view = document.querySelector(`${graph} .canvas`)!.getBoundingClientRect();
      return { fx: (frame.left + frame.width / 2 - map.left) / map.width, fy: (frame.top + frame.height / 2 - map.top) / map.height, step: view.width / frame.width };
    }, GRAPH);
  }

  /** Click the Navigator's map at `fx`, `fy` (fractions of its width and height), and wait
   * until the frame of the part in view is drawn around that place. Returns the clicked point
   * and the frame's middle. */
  async navigate(fx: number, fy: number): Promise<{ clicked: { x: number; y: number }; framed: { x: number; y: number }; camera: string }> {
    const before = await this.camera();
    const point = await this.browser.execute((graph, x, y) => {
      const box = document.querySelector(`${graph} [data-navigator] .map`)!.getBoundingClientRect();
      return { x: Math.round(box.left + box.width * x), y: Math.round(box.top + box.height * y) };
    }, GRAPH, fx, fy);
    await dispatchInput(this.browser, { kind: "pointer", phase: "down", ...point });
    await dispatchInput(this.browser, { kind: "pointer", phase: "up", ...point });
    const frame = () => this.browser.execute((graph) => {
      const box = document.querySelector(`${graph} [data-navigator] .frame`)!.getBoundingClientRect();
      return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
    }, GRAPH);
    const framed = await until(this.browser, frame, (middle) => Math.hypot(middle.x - point.x, middle.y - point.y) <= 3, "the Navigator framed the clicked place");
    // The graph itself moved there.
    const camera = await until(this.browser, () => this.camera(), (shown) => shown !== before, "the graph's camera moved");
    return { clicked: point, framed, camera };
  }

  /** Whether the large graph's canvas paints anything in the middle of the graph: some pixels
   * there differ from its background. This is a canvas, which only its pixels show. */
  async paints(): Promise<boolean> {
    return (await this.painted()).length > 20;
  }

  /** The points in the middle of the large graph's canvas whose pixels differ from its
   * background: where it paints blocks. */
  async painted(): Promise<{ x: number; y: number }[]> {
    const area = await this.browser.execute((graph) => {
      const box = document.querySelector(`${graph} canvas.paint`)?.getBoundingClientRect();
      return box ? { x: box.left, y: box.top, width: box.width, height: box.height, scale: devicePixelRatio } : null;
    }, GRAPH);
    if (!area) return [];
    const image = PNG.sync.read(Buffer.from(await this.browser.takeScreenshot(), "base64"));
    const at = (x: number, y: number) => {
      const offset = (Math.round(y * area.scale) * image.width + Math.round(x * area.scale)) * 4;
      return [image.data[offset], image.data[offset + 1], image.data[offset + 2]];
    };
    const background = at(area.x + 2, area.y + area.height - 2);
    const points: { x: number; y: number }[] = [];
    for (let y = area.y + area.height * 0.25; y < area.y + area.height * 0.75; y += 4) {
      for (let x = area.x + area.width * 0.25; x < area.x + area.width * 0.75; x += 4) {
        const pixel = at(x, y);
        if (pixel.some((value, channel) => Math.abs(value - background[channel]) > 40)) points.push({ x: Math.round(x), y: Math.round(y) });
      }
    }
    return points;
  }

  /** Click a block the large graph paints, inside it: one whose neighbours on every side are
   * painted too. */
  async clickBlock(): Promise<void> {
    const points = await this.painted();
    const key = (x: number, y: number) => `${x},${y}`;
    const all = new Set(points.map((point) => key(point.x, point.y)));
    const inside = points.find((point) => [[-4, 0], [4, 0], [0, -4], [0, 4]].every(([dx, dy]) => all.has(key(point.x + dx, point.y + dy))));
    if (!inside) throw new Error("the large graph paints no block wide enough to click");
    await clickAt(this.browser, inside);
  }

  // -------------------------------------------------------------------------------
  // Find

  /** The find field, or null while it is closed. */
  found(): Promise<Found | null> {
    return this.browser.execute((selector) => {
      const input = document.querySelector<HTMLInputElement>(selector);
      const box = input?.closest(".find");
      if (!input || !box) return null;
      return {
        focused: document.activeElement === input,
        value: input.value,
        entries: [...box.querySelectorAll<HTMLElement>('[role="option"]')].map((option) => ({
          name: option.querySelector(".name")?.textContent?.trim() ?? "", chosen: option.getAttribute("aria-selected") === "true",
        })),
        note: box.querySelector('[role="status"]')?.textContent?.trim() ?? "",
      };
    }, FIND);
  }

  /** Click Find: the find field opens with the keyboard, once what it lists satisfies `ok`. */
  async openFind(ok: (found: Found) => boolean = () => true, what = "the find field opened with the keyboard"): Promise<Found> {
    await this.tool("Find");
    return (await until(this.browser, () => this.found(), (shown) => !!shown?.focused && ok(shown), what))!;
  }

  /** Type `text` into the open find field: what it lists follows. */
  async findText(text: string, ok: (found: Found) => boolean, what: string): Promise<Found> {
    await typeText(this.browser, text);
    return (await until(this.browser, () => this.found(), (shown) => !!shown && shown.value.endsWith(text) && ok(shown), what))!;
  }

  /** Click the listed `name`: the find field closes and its Detail opens, titled `title`. */
  async chooseFound(name: string, title: string): Promise<void> {
    await this.browser.$('.find [role="option"]').waitForExist();
    const options = await this.browser.$$('.find [role="option"]').getElements();
    for (const option of options) {
      if ((await option.$(".name").getText()).trim() === name) {
        await option.click();
        await until(this.browser, () => this.browser.execute(() => document.querySelector('aside[aria-label="Detail"] h2')?.textContent?.trim() ?? ""),
          (shown) => shown === title, `${name} found and its Detail opened`);
        return;
      }
    }
    throw new Error(`the find field does not list ${name}`);
  }

  /** How far the node `id` is drawn from the middle of the graph left of an open Detail. */
  offCentre(id: string): Promise<{ x: number; y: number } | null> {
    return this.browser.execute((selector, graph) => {
      const node = document.querySelector(selector)?.getBoundingClientRect();
      const canvas = document.querySelector(`${graph} .canvas`)?.getBoundingClientRect();
      if (!node || !canvas) return null;
      const detail = document.querySelector('aside[aria-label="Detail"]')?.getBoundingClientRect();
      const right = detail && detail.left > canvas.left && detail.left < canvas.right ? detail.left : canvas.right;
      return { x: Math.round(node.left + node.width / 2 - (canvas.left + right) / 2), y: Math.round(node.top + node.height / 2 - (canvas.top + canvas.height / 2)) };
    }, nodeAt(id), GRAPH);
  }
}
