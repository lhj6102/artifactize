// The Detail docked at the graph's right edge, as a person reads it: its title, its chips, its
// sections, an eval's Summary, an Artifact's tools and, on a Current, its Definition; each
// action waits for what it shows.
import { fill, press, typeText } from "../keys.ts";
import { dispatchInput, visibleCenter } from "../input.ts";
import { holds, until } from "./wait.ts";

const DETAIL = 'aside[aria-label="Detail"]';
const DIALOG = '[role="dialog"]';

/** The Detail as read: its title, its chips (on an eval's tab), the evals its Artifact's tab
 * lists with their status, its sections' names with whether each is unfolded, and whether it
 * is docked at the graph's right edge from the graph's top to the main area's bottom. */
export type Shown = {
  title: string;
  tabs: { name: string; selected: boolean }[];
  evals: { id: string; name: string; status: string }[];
  sections: { name: string; open: boolean }[];
  docked: boolean;
  starred: boolean;
};

export class Detail {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
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
          status: row.querySelector(".status")?.textContent?.trim() ?? "",
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

  /** Click the chip of the eval `name` on an eval's tab: its tab shows. */
  async chip(name: string): Promise<Shown> {
    const tabs = await this.browser.$$(`${DETAIL} [role="tab"][data-tab]`).getElements();
    for (const tab of tabs) {
      if ((await tab.$(".label").getText()).trim() !== name) continue;
      await tab.click();
      return this.until((shown) => shown.tabs.some((each) => each.name === name && each.selected), `${name}'s tab shown`);
    }
    throw new Error(`the Detail has no chip ${name}`);
  }

  /** Click the Artifact's own chip: its Artifact tab, without chips, shows its evals again. */
  async artifactTab(name: string): Promise<Shown> {
    await this.browser.$(`${DETAIL} [role="tab"][aria-label="Artifact ${name}"]`).click();
    return this.until((shown) => shown.tabs.length === 0, `${name}'s own tab shown`);
  }

  /** Click the header row of the section `name`: it folds or unfolds. */
  async toggleSection(name: string): Promise<Shown> {
    const before = (await this.read())?.sections.find((section) => section.name === name);
    if (!before) throw new Error(`the Detail has no section ${name}`);
    await this.browser.$(`${DETAIL} section.section[aria-label="${name}"] > button.head`).click();
    return this.until((shown) => shown.sections.some((section) => section.name === name && section.open !== before.open), `${name} ${before.open ? "folded" : "unfolded"}`);
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

  /** Click the neighbour `id` in the Neighbours drawing: its Detail opens, titled `title`. */
  async openNeighbour(id: string, title: string): Promise<void> {
    await this.browser.$(`${DETAIL} [aria-label="Neighbours"] .svelte-flow__node[data-id="${id}"]`).click();
    await this.until((shown) => shown.title === title, `${id}'s Detail opened from the Neighbours`);
  }

  /** Drag the Detail's left edge by `dx` pixels: its new width. */
  async dragEdge(dx: number): Promise<number> {
    const width = async () => this.browser.execute((detail) => Math.round(document.querySelector(detail)?.getBoundingClientRect().width ?? 0), DETAIL);
    const before = await width();
    const from = await visibleCenter(this.browser, await this.browser.$(`${DETAIL} [aria-label="Detail width"]`).getElement());
    await this.browser.action("pointer").move({ ...from, origin: "viewport" }).down()
      .move({ x: from.x + Math.round(dx / 2), y: from.y, origin: "viewport", duration: 100 })
      .move({ x: from.x + dx, y: from.y, origin: "viewport", duration: 100 }).up().perform();
    return until(this.browser, width, (shown) => Math.abs(shown - (before - dx)) <= 4, `the Detail dragged ${-dx}px wider`);
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

  /** What the Detail's tools say, in red, beside them. */
  toolNotice(): Promise<string> {
    return this.browser.execute((detail) => document.querySelector(`${detail} section[aria-label^="Tools"] p.error`)?.textContent?.trim() ?? "", DETAIL);
  }

  // -------------------------------------------------------------------------------
  // A Current's Definition

  /** The Definition as read: its Path field, its Parts, the refusal said under a field. */
  definition(name: string): Promise<{ path: string; parts: string[]; refusals: string[]; source: string; saving: boolean }> {
    return this.browser.execute((detail, artifact) => {
      const root = document.querySelector(detail);
      return {
        path: root?.querySelector<HTMLInputElement>(`input[aria-label="Path of ${artifact}"]`)?.value ?? "",
        parts: [...(root?.querySelectorAll(`ul[aria-label="Parts of ${artifact}"] > li`) ?? [])].map((part) => part.textContent?.trim() ?? ""),
        refusals: [...(root?.querySelectorAll('[role="alert"]') ?? [])].map((alert) => alert.textContent?.trim() ?? ""),
        source: root?.querySelector<HTMLTextAreaElement>(`textarea[aria-label="Source of ${artifact}"]`)?.value ?? "",
        saving: !![...(root?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.trim() === "Save"),
      };
    }, DETAIL, name);
  }

  /** Type `path` into the Path field and press Enter: it is written, or refused. */
  async writePath(name: string, path: string): Promise<void> {
    await fill(this.browser, `${DETAIL} input[aria-label="Path of ${name}"]`, path);
    await press(this.browser, ["Enter"], async () => true, "Enter in the Path field");
  }

  /** Type `path` into the Path field without writing it. */
  async typePath(name: string, path: string): Promise<void> {
    await fill(this.browser, `${DETAIL} input[aria-label="Path of ${name}"]`, path);
  }

  /** Unfold the Source and replace its text with `text`: Save and Revert show. */
  async editSource(name: string, text: string): Promise<void> {
    const field = `${DETAIL} textarea[aria-label="Source of ${name}"]`;
    if (!(await this.browser.$(field).isExisting())) await this.toggleSection("Source");
    await fill(this.browser, field, text);
    await holds(this.browser, async () => (await this.definition(name)).saving, "Save shows for the changed Source");
  }

  /** Click the Source's Save or Revert. */
  async sourceAction(action: "Save" | "Revert"): Promise<void> {
    await this.browser.$(DETAIL).$(`button=${action}`).click();
  }

  /** Click `+ Add` under Parts, type `part` and take it with Enter. */
  async addPart(name: string, part: string): Promise<void> {
    await this.browser.$(`${DETAIL} section[aria-label="Definition"]`).$("button*=Add").click();
    const field = `${DETAIL} input[aria-label="Add a part to ${name}"]`;
    await this.browser.$(field).waitForDisplayed({ timeout: 15000, timeoutMsg: "the field that adds a part" });
    await this.browser.$(field).click();
    await typeText(this.browser, part);
    await press(this.browser, ["Enter"], async () => (await this.definition(name)).parts.includes(part) || (await this.definition(name)).refusals.length > 0, `${part} added or refused`);
  }

  /** Click `✕` beside the part `part`. */
  async removePart(name: string, part: string): Promise<void> {
    await this.browser.$(`${DETAIL} button[aria-label="Remove ${part} from the parts of ${name}"]`).click();
    await until(this.browser, () => this.definition(name), (shown) => !shown.parts.includes(part), `${part} removed from ${name}'s parts`);
  }

  /** Click the Definition's `action` (Comment, Rename, Delete). */
  async definitionAction(action: "Comment" | "Rename" | "Delete"): Promise<void> {
    await this.browser.$(`${DETAIL} section[aria-label="Definition"]`).$(`button=${action}`).click();
    if (action !== "Comment") await this.browser.$(DIALOG).waitForDisplayed({ timeout: 15000, timeoutMsg: `the ${action} dialog` });
  }

  /** The open edit dialog: its title, what it says and the refusal under its field. */
  dialog(): Promise<{ title: string; text: string; refusal: string } | null> {
    return this.browser.execute((selector) => {
      const dialog = document.querySelector<HTMLElement>(selector);
      if (!dialog) return null;
      return {
        title: dialog.querySelector(".dialog-title")?.textContent?.trim() ?? "",
        text: dialog.innerText.replace(/\s+/g, " ").trim(),
        refusal: dialog.querySelector('[role="alert"]')?.textContent?.trim() ?? "",
      };
    }, DIALOG);
  }

  /** In the open dialog, type `values` into its fields in order (Name, then Path), then click
   * its `button`; the dialog closes, or says why it refused. */
  async submitDialog(values: string[], button: string): Promise<{ title: string; text: string; refusal: string } | null> {
    const fields = await this.browser.$$(`${DIALOG} input`).getElements();
    for (const [index, value] of values.entries()) {
      await fields[index].click();
      await fields[index].setValue(value);
    }
    await this.browser.$(DIALOG).$(`button=${button}`).click();
    return until(this.browser, () => this.dialog(), (shown) => shown === null || shown.refusal !== "", `the ${button} dialog closed or refused`);
  }

  /** Click Cancel in the open dialog: it closes. */
  async cancelDialog(): Promise<void> {
    await this.browser.$(DIALOG).$("button=Cancel").click();
    await holds(this.browser, async () => (await this.dialog()) === null, "the dialog closed");
  }

  /** The open comment field's text, or null. */
  commentField(name: string): Promise<string | null> {
    return this.browser.execute((artifact) => document.querySelector<HTMLTextAreaElement>(`textarea[aria-label="Comment on ${artifact}"]`)?.value ?? null, name);
  }

  /** In the open comment field of `name`, write `text` (lines joined by Shift+Enter) and press Enter. */
  async writeComment(name: string, text: string): Promise<void> {
    const field = `textarea[aria-label="Comment on ${name}"]`;
    await holds(this.browser, async () => this.browser.$(field).isFocused(), "the comment field has the keyboard");
    await fill(this.browser, field, "");
    const lines = text.split("\n");
    for (const [index, line] of lines.entries()) {
      if (index > 0) await press(this.browser, ["Shift", "Enter"], async () => ((await this.commentField(name)) ?? "").endsWith("\n"), "a new line in the comment");
      await typeText(this.browser, line);
    }
    await press(this.browser, ["Enter"], async () => (await this.commentField(name)) === null, "the comment written");
  }

  /** Click the comment drawn under `nodeSelector`: its field opens in place. */
  async openComment(nodeSelector: string, name: string): Promise<void> {
    const at = await visibleCenter(this.browser, await this.browser.$(`${nodeSelector} .comment`).getElement());
    await dispatchInput(this.browser, { kind: "click", ...at });
    await holds(this.browser, async () => (await this.commentField(name)) !== null, `${name}'s comment field opened`);
  }
}
