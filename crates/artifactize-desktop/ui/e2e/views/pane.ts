// The left pane as a person reads it: the search field, the branches with their timelines, and
// the waiting reviews; each action waits for what it shows.
import { afterRender, press, typeText } from "../keys.ts";
import { clickAt, visibleCenter } from "../input.ts";
import { holds, until } from "./wait.ts";

/** A timeline entry: the Run it shows, its status and age as they
 * read (`GREEN 5m ago`), the line under it, and whether it is the one shown. */
export type Entry = { id: string; text: string; sub: string; shown: boolean };

/** A branch as listed: `project  branch`, its label as drawn (`project / branch`), whether it is
 * selected, expanded and starred, its newest Run's status while folded, and its timeline. */
export type Branch = { name: string; label: string; selected: boolean; expanded: boolean; starred: boolean; latest: string; entries: Entry[] };

const PANE = 'section[aria-label="Pane"]';
const SEARCH = 'input[aria-label="Search the branches"]';
const ENTRIES = '[role="group"][aria-label="Pane entries"]';

export class Pane {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
  }

  /** The branches in the order listed. */
  branches(): Promise<Branch[]> {
    return this.browser.execute(() => {
      const words = (element: Element | null | undefined) => (element?.textContent ?? "").replace(/\s+/g, " ").trim();
      return [...document.querySelectorAll<HTMLElement>('[aria-label="Branches"] > [role="treeitem"]')].map((row) => {
        const project = words(row.querySelector(".project")).replace(/ \/$/, "");
        const branch = words(row.querySelector(".branch-name"));
        return {
          name: project ? `${project}  ${branch}` : branch,
          label: words(row.querySelector(".label")),
          selected: row.getAttribute("aria-selected") === "true",
          expanded: row.getAttribute("aria-expanded") === "true",
          starred: row.querySelector(".star")?.getAttribute("aria-pressed") === "true",
          latest: words(row.querySelector(".latest")),
          entries: [...row.querySelectorAll<HTMLElement>("button[data-chip]")].map((chip) => ({
            id: chip.dataset.chip ?? "",
            text: `${words(chip.querySelector(".status"))} ${words(chip.querySelector(".age"))}`.trim(),
            sub: words(chip.querySelector(".sub")),
            shown: chip.getAttribute("aria-pressed") === "true",
          })),
        };
      });
    });
  }

  /** The branch `name` once it is listed and `ok` holds for it. */
  async branch(name: string, ok: (branch: Branch) => boolean = () => true, what = `${name} is listed`): Promise<Branch> {
    const found = await until(this.browser, async () => (await this.branches()).find((entry) => entry.name === name), (entry) => !!entry && ok(entry), what);
    return found!;
  }

  /** Wait until the branches read `names`, in that order. */
  async listed(names: string[], what = `the branches read ${names.join(", ")}`): Promise<void> {
    await until(this.browser, () => this.branches(), (shown) => JSON.stringify(shown.map((entry) => entry.name)) === JSON.stringify(names), what);
  }

  private async row(name: string): Promise<string> {
    const at = (await this.branches()).findIndex((entry) => entry.name === name);
    if (at < 0) throw new Error(`${name} is not listed`);
    return `[aria-label="Branches"] > [role="treeitem"]:nth-child(${at + 1})`;
  }

  /** Click the branch's name: it is selected and expanded. */
  async selectBranch(name: string): Promise<Branch> {
    await this.branch(name);
    await this.browser.$(`${await this.row(name)} button.name`).click();
    return this.branch(name, (entry) => entry.selected && entry.expanded, `${name} selected and expanded`);
  }

  /** Fold or expand the branch with its chevron. */
  async toggle(name: string): Promise<Branch> {
    const before = await this.branch(name);
    await this.browser.$(`${await this.row(name)} button.chevron`).click();
    return this.branch(name, (entry) => entry.expanded !== before.expanded, `${name} ${before.expanded ? "folded" : "expanded"}`);
  }

  /** Click Older runs at the end of the branch's timeline: more entries are listed. */
  async older(name: string): Promise<Branch> {
    const before = (await this.branch(name)).entries.length;
    await this.browser.$(await this.row(name)).$("button=Older runs").click();
    return this.branch(name, (entry) => entry.entries.length > before, `${name} lists older Runs`);
  }

  /** Click the timeline entry `id` (a Run's ID, or `current`) of the branch, expanding it first:
   * it is the one shown. */
  async choose(name: string, id: string): Promise<void> {
    if (!(await this.branch(name)).expanded) await this.toggle(name);
    await this.branch(name, (entry) => entry.entries.some((chip) => chip.id === id), `${name} lists ${id}`);
    await this.browser.$(`${await this.row(name)} button[data-chip=${JSON.stringify(id)}]`).click();
    await this.branch(name, (entry) => entry.entries.some((chip) => chip.id === id && chip.shown), `${name}'s ${id} shown`);
  }

  /** Click the branch's star: it is starred, or not any more. */
  async star(name: string): Promise<void> {
    const before = await this.branch(name);
    await this.browser.$(`${await this.row(name)} .star`).click();
    await this.branch(name, (entry) => entry.starred !== before.starred, `${name} ${before.starred ? "unstarred" : "starred"}`);
  }

  /** The pane's parts top to bottom, the search field's text, and whether its divider shows. */
  layout(): Promise<{ parts: string[]; query: string; divider: boolean; scope: boolean }> {
    return this.browser.execute((pane) => {
      const root = document.querySelector(pane);
      const divider = document.querySelector<HTMLElement>(".monitor [data-pane-resizer]");
      return {
        parts: [...(root?.querySelectorAll('input[aria-label="Search the branches"], [aria-label="Branches"]') ?? [])]
          .map((part) => part.getAttribute("aria-label") ?? ""),
        query: root?.querySelector<HTMLInputElement>("input")?.value ?? "",
        divider: !!divider && divider.getBoundingClientRect().width > 0,
        scope: !!document.querySelector('[aria-label="Scope"], [aria-label="Runs"], [aria-label="Screens"]'),
      };
    }, PANE);
  }

  /** Whether the search, the branches and the waiting reviews are stacked in one column, in
   * that order, each below the one before at the same left edge. */
  stacked(): Promise<boolean> {
    return this.browser.execute((pane) => {
      const root = document.querySelector(pane);
      const boxes = ['input[aria-label="Search the branches"]', '[aria-label="Branches"]']
        .map((selector) => root?.querySelector(selector)?.getBoundingClientRect());
      if (boxes.some((box) => !box || box.width === 0)) return false;
      const [search, branches] = boxes as DOMRect[];
      return branches.top >= search.bottom - 1 && search.left >= branches.left - 2 && search.right <= branches.right + 2;
    }, PANE);
  }

  /** Whether the pane shows, and how wide it is. */
  shown(): Promise<{ visible: boolean; width: number }> {
    return this.browser.execute((pane) => {
      const box = document.querySelector(pane)?.getBoundingClientRect();
      return { visible: !!box && box.width > 0, width: Math.round(box?.width ?? 0) };
    }, PANE);
  }

  /** Drag the divider between the pane and the main area by `dx` pixels: the pane's new width. */
  async dragDivider(dx: number): Promise<number> {
    const before = (await this.shown()).width;
    // The divider listens for moves only from the render after its press: the press is held
    // across that render before the pointer moves.
    await this.browser.action("pointer", { id: "divider" }).move({ origin: await this.browser.$(".monitor [data-pane-resizer]"), x: 2, y: 0 }).down().perform(true);
    try {
      await afterRender(this.browser);
      const pointer = this.browser.action("pointer", { id: "divider" });
      for (let step = 0; step < 4; step += 1) pointer.move({ origin: "pointer", x: Math.round(dx / 4), y: 0 });
      await pointer.perform(true);
      await this.browser.action("pointer", { id: "divider" }).up().perform();
      // Released: the width the pane keeps.
      const shown = await until(this.browser, () => this.shown(), (now) => Math.abs(now.width - before - dx) <= 8, `the pane dragged ${dx}px wider`);
      return shown.width;
    } finally { await this.browser.releaseActions(); }
  }

  // -------------------------------------------------------------------------------
  // Search

  /** The search field's text and whether it has the keyboard. */
  search(): Promise<{ query: string; focused: boolean; suggestions: string[] }> {
    return this.browser.execute((selector) => {
      const field = document.querySelector<HTMLInputElement>(selector);
      return {
        query: field?.value ?? "",
        focused: !!field && document.activeElement === field,
        suggestions: [...document.querySelectorAll('#search-suggestions [role="option"]')].map((option) => option.textContent?.trim() ?? ""),
      };
    }, SEARCH);
  }

  /** Click into the search field: it has the keyboard. */
  async focusSearch(): Promise<void> {
    await this.browser.$(SEARCH).click();
    await holds(this.browser, async () => (await this.search()).focused, "the search field has the keyboard");
  }

  /** Replace the search with `query`, typed into the field: the branches follow it. */
  async searchFor(query: string): Promise<void> {
    await this.browser.$(SEARCH).setValue(query);
    await until(this.browser, () => this.search(), (shown) => shown.query === query, `the search reads ${query}`);
  }

  /** Type `text` at the end of the focused search field, a key at a time. */
  async typeSearch(text: string): Promise<void> {
    const before = (await this.search()).query;
    await typeText(this.browser, text);
    await until(this.browser, () => this.search(), (shown) => shown.query === before + text, `${text} typed into the search`);
  }

  /** Click the pane's entries below the search: the search lets go of the keyboard. */
  async leaveSearch(): Promise<void> {
    await this.browser.$(ENTRIES).click();
    await holds(this.browser, async () => !(await this.search()).focused, "the search let go of the keyboard");
  }

  /** Click the suggestion `text`: it takes the place of the term being typed. */
  async takeSuggestion(text: string): Promise<string> {
    await until(this.browser, () => this.search(), (shown) => shown.suggestions.includes(text), `${text} is suggested`);
    const before = (await this.search()).query;
    const option = await this.browser.$("#search-suggestions").$(`[role="option"]=${text}`).getElement();
    await clickAt(this.browser, await visibleCenter(this.browser, option));
    return (await until(this.browser, () => this.search(), (shown) => shown.query !== before, `the suggestion ${text} taken`)).query;
  }

  /** Press `keys` in the pane: the search, the branches and the waiting reviews react to them. */
  async press(keys: string[], done: () => Promise<boolean>, what: string): Promise<void> {
    await press(this.browser, keys, done, what);
  }

}
