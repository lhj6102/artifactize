// The Detail docked at the graph's right edge, as a person reads it: its title, its chips, its
// sections, an eval's Summary, an Artifact's tools and, on a Current, its Definition; each
// action waits for what it shows.
import { press } from "../keys.ts";
import { drawsEdge } from "./graph.ts";
import { holds, until } from "./wait.ts";

const DETAIL = 'main:not([hidden]) aside[aria-label="Detail"]';

/** The Detail as read: its title, its chips (on an eval's tab), the evals its Artifact's tab
 * lists with their status and, for a Human eval waiting for a person, its stage under it
 * (`claimable`, `yours`, `taken by {reviewer}`), its sections' names with whether each is
 * unfolded, and whether it is docked at the graph's right edge from the graph's top to the
 * main area's bottom. */
export type Shown = {
  title: string;
  tabs: { name: string; selected: boolean }[];
  evals: { id: string; name: string; status: string; stage: string }[];
  sections: { name: string; open: boolean }[];
  docked: boolean;
  starred: boolean;
};

export class Detail {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
  }

  reviewAction(name: "Claim to review" | "Release claim" | "Open in Review"): Promise<void> {
    return this.browser.$(DETAIL).$(`button=${name}`).click();
  }

  /** The open Detail, or null while none is open. */
  read(): Promise<Shown | null> {
    return this.browser.execute((detail) => {
      const aside = document.querySelector<HTMLElement>(detail);
      if (!aside) return null;
      const stage = aside.closest(".stage")!.getBoundingClientRect();
      const graph = aside.closest(".stage")!.querySelector("[data-graph-area]")?.getBoundingClientRect();
      const box = aside.getBoundingClientRect();
      return {
        title: aside.querySelector("h2")?.textContent?.trim() ?? "",
        tabs: [...aside.querySelectorAll<HTMLElement>('[role="tablist"][aria-label^="Tabs of "] [role="tab"]')].map((tab) => ({
          name: tab.querySelector(".label")?.textContent?.trim() ?? "", selected: tab.getAttribute("aria-selected") === "true",
        })),
        evals: [...aside.querySelectorAll<HTMLElement>("button[data-eval]")].map((row) => ({
          id: row.dataset.eval ?? "",
          name: row.querySelector(".name")?.firstChild?.textContent?.trim() ?? "",
          status: row.querySelector(".verdict-status")?.textContent?.trim() ?? "",
          stage: row.querySelector(".status .stage")?.textContent?.trim().replace(/^·\s*/, "") ?? "",
        })),
        sections: [...aside.querySelectorAll<HTMLElement>("section.section")].map((section) => ({
          name: section.getAttribute("aria-label") ?? "",
          open: section.querySelector(":scope > .head")?.getAttribute("aria-expanded") !== "false",
        })),
        docked: box.left > stage.left && Math.abs(stage.right - box.right) <= 1 && Math.abs(stage.bottom - box.bottom) <= 1
          && !!graph && Math.abs(box.top - graph.top) <= 1,
        starred: aside.querySelector('button.star[aria-label^="Star "]')?.getAttribute("aria-pressed") === "true",
      };
    }, DETAIL);
  }

  /** The open Detail once `ok` holds for it. */
  async until(ok: (shown: Shown) => boolean, what: string): Promise<Shown> {
    return (await until(this.browser, () => this.read(), (shown) => !!shown && ok(shown), what))!;
  }

  /** Wait until no Detail is open. */
  async closed(): Promise<void> {
    await holds(this.browser, async () => (await this.read()) === null, "the Detail closed");
  }

  /** Click the Detail's Close. */
  async close(): Promise<void> {
    await this.browser.$(DETAIL).$("button=Close").click();
  }

  /** Press `keys` in the Detail. */
  async press(keys: string[], done: () => Promise<boolean>, what: string): Promise<void> {
    await press(this.browser, keys, done, what);
  }

  /** Click the row of the eval `id` (`artifact/eval`) in the Artifact's Evals: its tab shows. */
  async openEval(id: string): Promise<Shown> {
    await this.browser.$(`${DETAIL} button[data-eval="e:${id}"]`).click();
    return this.until((shown) => shown.tabs.some((tab) => tab.selected && id.endsWith(`/${tab.name}`)), `${id}'s tab shown`);
  }

  /** Click the header row of the section `name`: it folds or unfolds. */
  async toggleSection(name: string): Promise<Shown> {
    const before = (await this.read())?.sections.find((section) => section.name === name);
    if (!before) throw new Error(`the Detail has no section ${name}`);
    await this.browser.$(`${DETAIL} section.section[aria-label="${name}"] > button.head`).click();
    return this.until((shown) => shown.sections.some((section) => section.name === name && section.open !== before.open), `${name} ${before.open ? "folded" : "unfolded"}`);
  }

  /** Switch the open eval's tab to its view `name` (Summary, Agent log, Runtime log), then
   * wait until what it shows below the switch satisfies `ok`. A log laid out in rows (the
   * Agent log) breaks its lines where its pane's width in characters ends, which differs with
   * each OS's font, even inside a word: its rows read as one text with no break between them,
   * so a check holds wherever the rows break. */
  async view(name: string, ok: (text: string) => boolean, what: string): Promise<string> {
    const tab = this.browser.$(`${DETAIL} [role="tablist"][aria-label="Views"]`).$(`button=${name}`);
    await tab.click();
    await holds(this.browser, async () => await tab.getAttribute("aria-selected") === "true", `${name} tab selected`);
    return until(this.browser, () => this.browser.execute((detail) => {
      const shown = document.querySelector<HTMLElement>(`${detail} [role="document"] .shown`);
      const rows = [...(shown?.querySelectorAll<HTMLElement>('[role="log"] .row') ?? [])];
      return rows.length > 0 ? rows.map((row) => row.textContent ?? "").join("") : shown?.innerText ?? "";
    }, DETAIL), ok, what);
  }

  /** Whether the Detail draws its Neighbours. */
  neighboursDrawn(): Promise<boolean> {
    return this.browser.$(`${DETAIL} [aria-label="Neighbours"] .svelte-flow__node`).isDisplayed();
  }

  /** The text the section `name` shows, its header row left out. */
  section(name: string): Promise<string> {
    return this.browser.execute((detail, section) => {
      const root = document.querySelector<HTMLElement>(`${detail} section.section[aria-label="${section}"]`);
      const head = root?.querySelector<HTMLElement>(":scope > .head")?.innerText ?? "";
      return (root?.innerText ?? "").slice(head.length).replace(/\s+/g, " ").trim();
    }, DETAIL, name);
  }

  /** An eval's Summary: its status line, its instruction, and the facts it lists by name. */
  summary(): Promise<{ status: string; instruction: string; facts: Record<string, string> }> {
    return this.browser.execute((detail) => {
      const shown = document.querySelector(`${detail} [role="document"]`);
      const facts: Record<string, string> = {};
      for (const term of shown?.querySelectorAll("dt") ?? []) facts[term.textContent?.trim() ?? ""] = term.nextElementSibling?.textContent?.trim() ?? "";
      return {
        status: shown?.querySelector(".summary")?.textContent?.replace(/\s+/g, " ").trim() ?? "",
        instruction: shown?.querySelector(".instruction")?.textContent?.trim() ?? "",
        facts,
      };
    }, DETAIL);
  }

  /** The eval's Summary once `ok` holds for it. */
  async summaryUntil(ok: (summary: Awaited<ReturnType<Detail["summary"]>>) => boolean, what: string): Promise<Awaited<ReturnType<Detail["summary"]>>> {
    return until(this.browser, () => this.summary(), ok, what);
  }

  /** Click the star beside the Detail's title: the Artifact is starred, or not any more. */
  async star(): Promise<void> {
    const before = (await this.read())?.starred;
    await this.browser.$(`${DETAIL} button.star[aria-label^="Star "]`).click();
    await this.until((shown) => shown.starred !== before, `the Artifact ${before ? "unstarred" : "starred"}`);
  }

  /** The Artifacts the Neighbours drawing shows: those its evals use, and those using it. */
  neighbours(): Promise<{ id: string; side: "left" | "right" | "self" }[]> {
    return this.browser.execute((detail) => {
      const nodes = [...document.querySelectorAll<HTMLElement>(`${detail} [aria-label="Neighbours"] .svelte-flow__node`)];
      const self = nodes.find((node) => node.querySelector(".artifact.open, .whole.open"))?.getBoundingClientRect();
      return nodes.map((node) => {
        const box = node.getBoundingClientRect();
        const side = !self || node.querySelector(".artifact.open, .whole.open") ? "self" : box.left < self.left ? "left" : "right";
        return { id: node.dataset.id ?? "", side: side as "left" | "right" | "self" };
      });
    }, DETAIL);
  }

  /** Wait until the Neighbours drawing draws the edge from `from` to `to`. */
  async neighbourEdge(from: string, to: string): Promise<void> {
    await holds(this.browser, () => this.browser.execute(drawsEdge, `${DETAIL} [aria-label="Neighbours"]`, from, to), `the Neighbours draw the edge from ${from} to ${to}`);
  }

  /** Click the neighbour `id` in the Neighbours drawing: its Detail opens, titled `title`. */
  async openNeighbour(id: string, title: string): Promise<void> {
    await this.browser.$(`${DETAIL} [aria-label="Neighbours"] .svelte-flow__node[data-id="${id}"]`).click();
    await this.until((shown) => shown.title === title, `${id}'s Detail opened from the Neighbours`);
  }

  /** How wide the Detail is, and how far it lies from the main area's left and right edges. */
  placed(): Promise<{ width: number; left: number; right: number } | null> {
    return this.browser.execute((detail) => {
      const aside = document.querySelector(detail);
      const dock = aside?.getBoundingClientRect();
      const stage = aside?.closest(".stage")?.getBoundingClientRect();
      return dock && stage ? { width: Math.round(dock.width), left: Math.round(dock.left - stage.left), right: Math.round(stage.right - dock.right) } : null;
    }, DETAIL);
  }

  /** Drag the Detail's left edge `dx` pixels to the left: its new width. */
  async widen(dx: number): Promise<number> {
    const before = (await this.placed())!.width;
    await this.browser.action("pointer", { id: "edge" }).move({ origin: await this.browser.$(`${DETAIL} [aria-label="Detail width"]`), x: 0, y: 0 }).down().perform(true);
    try {
      const pointer = this.browser.action("pointer", { id: "edge" });
      for (let step = 0; step < 4; step += 1) pointer.move({ origin: "pointer", x: -Math.round(dx / 4), y: 0 });
      await pointer.up().perform();
    } finally { await this.browser.releaseActions(); }
    return (await until(this.browser, () => this.placed(), (now) => !!now && Math.abs(now.width - before - dx) <= 8, `the Detail dragged ${dx}px wider`))!.width;
  }

  // -------------------------------------------------------------------------------
  // The Artifact's own tools

  /** Click the Artifact's tool `operation`: its run starts in the Detail's Tool log. */
  async runTool(operation: string): Promise<void> {
    const button = this.browser.$(`${DETAIL} section[aria-label^="Tools"]`).$(`button=${operation}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `the tool ${operation} is usable` });
    await button.click();
  }

  /** The Detail's Tool log, rows as they read with a run's time as HH:MM:SS. */
  toolLog(): Promise<string[]> {
    return this.browser.execute((detail) => [...document.querySelectorAll(`${detail} [role="log"][aria-label="Tool log"] p:not(.muted)`)]
      .map((row) => (row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS ")), DETAIL);
  }

  /** The Detail's Tool log once `ok` holds for it. */
  async toolLogUntil(ok: (rows: string[]) => boolean, what: string): Promise<string[]> {
    return until(this.browser, () => this.toolLog(), ok, what);
  }

  // -------------------------------------------------------------------------------
  // A Current's Definition and Source

  /** The Definition as read: its facts by their names (`Bound`, `Macro`, `Path`, `Tools`), the
   * missing target said beside the Path, its Parts, and how many fields it has; and the Source
   * once unfolded: its views (`Authored`, `Expanded`) with the one chosen, its line numbers and
   * its text. */
  definition(name: string): Promise<{
    facts: Record<string, string>; missing: string; parts: string[]; fields: number;
    views: string[]; view: string; numbers: number[]; source: string | null;
  }> {
    return this.browser.execute((detail, artifact) => {
      const root = document.querySelector(detail);
      const definition = root?.querySelector('section.section[aria-label="Definition"]');
      const facts: Record<string, string> = {};
      for (const term of definition?.querySelectorAll("dt") ?? []) {
        const value = term.nextElementSibling;
        facts[term.textContent?.trim() ?? ""] = (value?.querySelector(".mono") ?? value)?.textContent?.trim() ?? "";
      }
      const source = root?.querySelector(`[role="region"][aria-label="Source of ${artifact}"]`);
      const tabs = [...(root?.querySelectorAll('[role="tablist"][aria-label="Source views"] [role="tab"]') ?? [])];
      return {
        facts,
        missing: definition?.querySelector(".missing")?.textContent?.trim() ?? "",
        parts: [...(root?.querySelectorAll(`ul[aria-label="Parts of ${artifact}"] .part`) ?? [])].map((part) => part.textContent?.trim() ?? ""),
        fields: root?.querySelectorAll('section.section[aria-label="Definition"] :is(input, textarea, select, [contenteditable="true"]), section.section[aria-label="Source"] :is(input, textarea, select, [contenteditable="true"])').length ?? 0,
        views: tabs.map((tab) => tab.textContent?.trim() ?? ""),
        view: tabs.find((tab) => tab.getAttribute("aria-selected") === "true")?.textContent?.trim() ?? "",
        numbers: [...(source?.querySelectorAll(".margin li") ?? [])].map((line) => Number(line.textContent)),
        source: source?.querySelector("pre")?.textContent ?? null,
      };
    }, DETAIL, name);
  }

  /** The Definition once `ok` holds for it. */
  async definitionUntil(name: string, ok: (shown: Awaited<ReturnType<Detail["definition"]>>) => boolean, what: string): Promise<Awaited<ReturnType<Detail["definition"]>>> {
    return until(this.browser, () => this.definition(name), ok, what);
  }

  /** Click the Source's view `view` (Authored, Expanded): it shows. */
  async sourceView(name: string, view: "Authored" | "Expanded"): Promise<void> {
    await this.browser.$(`${DETAIL} [role="tablist"][aria-label="Source views"]`).$(`button=${view}`).click();
    await this.definitionUntil(name, (shown) => shown.view === view, `${name}'s Source shows ${view}`);
  }

  /** Click the part `part` under the Definition's Parts: its own Detail opens. */
  async openPart(name: string, part: string): Promise<void> {
    await this.browser.$(`${DETAIL} ul[aria-label="Parts of ${name}"]`).$(`button=${part}`).click();
    await this.until((shown) => shown.title === `Artifact ${part}`, `${part}'s Detail opened`);
  }
}
