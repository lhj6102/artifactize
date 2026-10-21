// A Run's or a Current's graph in the Main area as a person sees it: the Artifact that has the
// keyboard, and the Detail a click opens over the graph's right side with its tabs. Every reading is read-only; a timeout
// reports what the window showed instead.

/** The Artifact node with keyboard focus (`a:<name>`), or null. */
export function focusedNode(browser: WebdriverIO.Browser): Promise<string | null> {
  return browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? null);
}

/** Wait until the live graph draws a visible, hit-testable node `id`, not a retained element. */
export async function graphShown(browser: WebdriverIO.Browser, id: string, timeout = 15000): Promise<void> {
  await browser.waitUntil(() => browser.execute((want) => {
    const node = document.querySelector(`.svelte-flow__node[data-id="${want}"]`);
    const box = node?.getBoundingClientRect();
    if (!node || !box || box.width <= 0 || box.height <= 0) return false;
    const target = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return target?.closest(".svelte-flow__node") === node;
  }, id), { timeout, timeoutMsg: `the graph draws ${id}` });
}

/** The camera (translation and scale) of the Main area's graph, read once it holds still over a
 * painted frame: a zoom control animates it. */
export async function camera(browser: WebdriverIO.Browser): Promise<string> {
  const read = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>(".svelte-flow__viewport")]
    .find((viewport) => !viewport.closest('aside[aria-label="Detail"]'))?.style.transform ?? "");
  let shown = await read();
  await browser.waitUntil(async () => {
    const before = shown;
    await browser.execute(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
    shown = await read();
    return before === shown;
  }, { timeout: 15000, timeoutMsg: "the graph's camera held still" });
  return shown;
}

/** The node selector of the Artifact `id`. */
export const nodeAt = (id: string) => `.svelte-flow__node[data-id="${id}"]`;

/** The open Detail's tab of the eval `id` (`e:<artifact>/<eval>`), and the same eval's row in
 * the Artifact tab's list. */
export const tabAt = (id: string) => `aside[aria-label="Detail"] [role="tab"][data-tab="${id}"]`;
export const evalAt = (id: string) => `aside[aria-label="Detail"] button[data-eval="${id}"]`;

/** A chip as read: its visible text without the glyph, what a screen reader names it, its verdict
 * glyph and colour, whether an icon is drawn in it, and whether it is the chosen one. */
export type Tab = { name: string; label: string; glyph: string; tone: string; colour: string; painted: boolean; icon: boolean; selected: boolean };
export type Detail = {
  /** The Detail's heading: the shown node's title, or "Human review". */
  title: string;
  /** The Artifact's chip (its icon and name) first, then one per eval; empty for the Run's Detail. */
  tabs: Tab[];
  /** The chip row as it reads, glyphs and icon left out: `{name} / eval eval …`. */
  path: string;
  /** The evals the Artifact tab lists, with their verdict text; empty on an eval's tab. */
  evals: { name: string; status: string }[];
  /** Whether the Detail lies within the Main area, right of the Pane, with a gap from the
   * area's right edge. */
  docked: boolean;
};

/** The open Detail, or null while none is open. */
export function detail(browser: WebdriverIO.Browser): Promise<Detail | null> {
  return browser.execute(() => {
    const aside = document.querySelector<HTMLElement>('.stage aside[aria-label="Detail"]');
    if (!aside) return null;
    const stage = aside.closest<HTMLElement>(".stage")!.getBoundingClientRect();
    const box = aside.getBoundingClientRect();
    const tone = (element: Element | null) => [...(element?.classList ?? [])].find((name) => name.startsWith("tone-")) ?? "";
    return {
      title: aside.querySelector("h2")?.textContent?.trim() ?? "",
      tabs: [...aside.querySelectorAll<HTMLElement>('[role="tab"]')].map((tab) => {
        const glyph = tab.querySelector(".glyph");
        const name = [...tab.childNodes].filter((node) => node !== glyph).map((node) => node.textContent ?? "").join("").trim();
        const icon = tab.querySelector("img");
        const drawn = icon?.getBoundingClientRect();
        return {
          name,
          label: tab.getAttribute("aria-label") ?? name,
          icon: !!icon && icon.complete && icon.naturalWidth > 0 && !!drawn && drawn.width > 0 && drawn.height > 0,
          glyph: glyph?.textContent?.trim() ?? "",
          tone: tone(glyph),
          colour: glyph ? getComputedStyle(glyph).color : "",
          painted: !!glyph && glyph.getBoundingClientRect().width > 0 && glyph.getBoundingClientRect().height > 0
            && getComputedStyle(glyph).visibility === "visible" && Number(getComputedStyle(glyph).opacity) > 0,
          selected: tab.getAttribute("aria-selected") === "true",
        };
      }),
      path: [...(aside.querySelector('[role="tablist"]')?.childNodes ?? [])].flatMap((node) => {
        const element = node as HTMLElement;
        const parts = element.matches?.(".head") ? [...element.children] : [element];
        return parts.map((part) => (part as HTMLElement).matches?.('[role="tab"]')
          ? [...part.childNodes].filter((child) => !(child as HTMLElement).classList?.contains("glyph")).map((child) => child.textContent ?? "").join("").trim()
          : part.textContent?.trim() ?? "");
      }).filter((text) => text !== "").join(" "),
      evals: [...aside.querySelectorAll<HTMLElement>("button[data-eval]")].map((row) => ({
        name: row.querySelector(".name")?.firstChild?.textContent?.trim() ?? "",
        status: row.querySelector(".status")?.textContent?.trim() ?? "",
      })),
      docked: box.left > stage.left && box.right < stage.right && box.top >= stage.top && box.bottom <= stage.bottom
        && stage.right - box.right + 1 < box.left - stage.left,
    };
  });
}

/** Click the Artifact `id` and wait for its Detail on the Artifact tab. */
export async function openArtifact(browser: WebdriverIO.Browser, id: string, title: string): Promise<Detail> {
  await browser.$(nodeAt(id)).click();
  let shown: Detail | null = null;
  await browser.waitUntil(async () => {
    shown = await detail(browser);
    return shown?.title === title && shown.tabs[0]?.selected === true;
  }, { timeout: 15000, timeoutMsg: `${id}'s Detail opened on its Artifact tab` })
    .catch((error: unknown) => { throw new Error(`${String(error)}; the Detail showed ${JSON.stringify(shown)}`); });
  return shown!;
}

/** Click the tab of the eval `id` (or its row on the Artifact tab, with `row`) and wait until
 * its tab is the shown one. */
export async function openTab(browser: WebdriverIO.Browser, id: string, { row = false } = {}): Promise<void> {
  await browser.$(row ? evalAt(id) : tabAt(id)).click();
  await browser.waitUntil(() => browser.$(tabAt(id)).getAttribute("aria-selected").then((selected) => selected === "true"),
    { timeout: 15000, timeoutMsg: `${id}'s tab shown` });
}

/** The open Detail. */
export const aside = (browser: WebdriverIO.Browser) => browser.$('aside[aria-label="Detail"]');

/** An eval's Summary as read: its status line, instruction, the titles of its blocks (Result,
 * Execution options, Execution metadata, Fingerprint), whether Debug is unfolded (null when it
 * has none), and one Debug fact by its name ("" while folded or absent). */
export type Summary = { status: string; instruction: string; blocks: string[]; debug: boolean | null };
export function summary(browser: WebdriverIO.Browser): Promise<Summary> {
  return browser.execute(() => {
    const shown = document.querySelector('aside[aria-label="Detail"] [role="document"]');
    const fold = shown?.querySelector("button.fold");
    return {
      status: shown?.querySelector(".summary")?.textContent?.replace(/\s+/g, " ").trim() ?? "",
      instruction: shown?.querySelector(".instruction")?.textContent?.trim() ?? "",
      blocks: [...(shown?.querySelectorAll(".block > h3") ?? [])].map((title) => title.textContent?.trim() ?? ""),
      debug: fold ? fold.getAttribute("aria-expanded") === "true" : null,
    };
  });
}
export function fact(browser: WebdriverIO.Browser, name: string): Promise<string> {
  return browser.execute((key) => [...document.querySelectorAll('aside[aria-label="Detail"] [role="document"] dt')]
    .find((entry) => entry.textContent === key)?.nextElementSibling?.textContent ?? "", name);
}
