// The window's one screen as a person sees it: the Pane (its Search, the Branches with their
// timelines, the Waiting stack) beside the Main area. Every reading is read-only; actions are
// real clicks on what the window shows.

/** A timeline entry as read: the Run it shows (`current` for the Current), its status word and
 * age as `text` (`GREEN 5m ago`, `Running 2m`, `Current`), the line under it (a Run's ID,
 * `Working tree`), the Run's comment below that (empty without one) with whether it is drawn on
 * one line and cut short, whether it is the one shown in the Main area, and its dot's colour as
 * red, green and blue channels on any one scale. */
export type Chip = {
  id: string;
  text: string;
  sub: string;
  note: string;
  noteOneLine: boolean;
  noteCut: boolean;
  filled: boolean;
  dot: number[];
};

/** Red, green or blue (0, 1, 2) is the strongest channel of `colour`. */
export const leans = (colour: number[], channel: number) =>
  colour.length === 3 && colour.every((value, at) => at === channel || colour[channel] > value);
/** A branch's entry as read: `project  branch` as the Main area's title names it, its label as
 * drawn (`project / branch`), whether the selection is on it or in its timeline, whether it is
 * expanded, the newest Run's status
 * under its name while collapsed (`RED 25m ago`), its star as drawn (`★` on, `☆` off) and
 * whether it is pressed, and its timeline, empty while collapsed. */
export type Branch = {
  name: string;
  label: string;
  selected: boolean;
  expanded: boolean;
  latest: string;
  star: string;
  starred: boolean;
  chips: Chip[];
};
/** A waiting review's row as read. */
export type WaitingRow = { id: string; eval: string; where: string; meta: string; selected: boolean };

export const ENTRIES = '[role="group"][aria-label="Pane entries"]';
export const SEARCH = 'input[aria-label="Search branches and reviews"]';
export const MAIN = 'section.panel[aria-label="Main area"]';

/** The Branches in the order shown. */
export function branches(browser: WebdriverIO.Browser): Promise<Branch[]> {
  return browser.execute(() => {
    const words = (element: Element | null | undefined) => (element?.textContent ?? "").replace(/\s+/g, " ").trim();
    // `rgb(r, g, b)` or `color(srgb r g b)`: the first three numbers after the space name.
    const channels = (colour: string) => (colour.replace("srgb", "").match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
    return [...document.querySelectorAll<HTMLElement>('[aria-label="Branches"] > [role="treeitem"]')].map((row) => {
      const project = words(row.querySelector(".project")).replace(/ \/$/, "");
      const branch = words(row.querySelector(".branch-name"));
      return {
        name: project ? `${project}  ${branch}` : branch,
        label: words(row.querySelector(".label")),
        // The selection is on the branch itself or on an entry of its timeline.
        selected: row.getAttribute("aria-selected") === "true" || row.querySelector('[role="treeitem"][aria-selected="true"]') !== null,
        expanded: row.getAttribute("aria-expanded") === "true",
        latest: words(row.querySelector(".latest")),
        star: row.querySelector(".star")?.textContent?.trim() ?? "",
        starred: row.querySelector(".star")?.getAttribute("aria-pressed") === "true",
        chips: [...row.querySelectorAll<HTMLElement>("button[data-chip]")].map((chip) => ({
          id: chip.dataset.chip ?? "",
          text: `${words(chip.querySelector(".status"))} ${words(chip.querySelector(".age"))}`.trim(),
          sub: words(chip.querySelector(".sub")),
          note: words(chip.querySelector(".note")),
          // One line: no taller than the Run ID's line above it.
          noteOneLine: ((note, id) => !note || !!id && note.getBoundingClientRect().height <= id.getBoundingClientRect().height + 1)(
            chip.querySelector<HTMLElement>(".note"), chip.querySelector<HTMLElement>(".sub")),
          noteCut: ((note) => !!note && note.scrollWidth > note.clientWidth)(chip.querySelector<HTMLElement>(".note")),
          filled: chip.getAttribute("aria-pressed") === "true",
          dot: channels(getComputedStyle(chip.querySelector(".dot")!).backgroundColor),
        })),
      };
    });
  });
}

/** The branch read as `name`, or undefined while it is not listed. */
export async function branch(browser: WebdriverIO.Browser, name: string): Promise<Branch | undefined> {
  return (await branches(browser)).find((entry) => entry.name === name);
}

/** The Waiting stack: its bar as it reads (`Waiting (N)`) and its rows, newest first. */
export function waiting(browser: WebdriverIO.Browser): Promise<{ bar: string; rows: WaitingRow[] }> {
  return browser.execute(() => ({
    bar: document.querySelector('section[aria-label="Waiting"] .bar')?.textContent?.trim() ?? "",
    rows: [...document.querySelectorAll<HTMLElement>('[aria-label="Waiting reviews"] [role="option"]')].map((row) => ({
      id: row.dataset.key ?? "",
      eval: row.querySelector(".eval")?.textContent ?? "",
      where: row.querySelector(".where")?.textContent ?? "",
      meta: row.querySelector(".meta")?.textContent ?? "",
      selected: row.getAttribute("aria-selected") === "true",
    })),
  }));
}

/** The Main area's title: `Run {id}`, `Current · {branch}`, `Review`, `Settings`, a
 * read-only request's title, or `Nothing chosen`. */
export function mainTitle(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute((main) => document.querySelector(`${main} > header h2`)?.textContent?.trim() ?? "", MAIN);
}

/** The shown Run's comment under its headline as read, and whether all of it is drawn, not
 * cut short; null without one. */
export function runComment(browser: WebdriverIO.Browser): Promise<{ text: string; whole: boolean } | null> {
  return browser.execute((main) => {
    const comment = document.querySelector<HTMLElement>(`${main} .run > .comment`);
    if (!comment) return null;
    const box = comment.getBoundingClientRect();
    const area = document.querySelector<HTMLElement>(main)!.getBoundingClientRect();
    return {
      text: comment.textContent?.trim() ?? "",
      whole: comment.scrollWidth <= comment.clientWidth && comment.scrollHeight <= comment.clientHeight + 1 &&
        box.left >= area.left && box.right <= area.right + 1,
    };
  }, MAIN);
}

/** Whether the Pane's entries, the Search or the Main area has the keyboard. */
export const entriesFocused = (browser: WebdriverIO.Browser) => browser.$(ENTRIES).isFocused();
export const searchFocused = (browser: WebdriverIO.Browser) => browser.$(SEARCH).isFocused();
export const mainFocused = (browser: WebdriverIO.Browser) => browser.$(MAIN).isFocused();

/** The branch row named `name` (as {@link Branch.name}), by its place in the list. */
async function row(browser: WebdriverIO.Browser, name: string): Promise<string> {
  const at = (await branches(browser)).findIndex((entry) => entry.name === name);
  if (at < 0) throw new Error(`${name} is not listed: ${JSON.stringify(await branches(browser))}`);
  return `[aria-label="Branches"] > [role="treeitem"]:nth-child(${at + 1})`;
}

/** Expand the branch `name` to its timeline with its chevron, if it is not already. */
export async function expand(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await listed(browser, name);
  if ((await branch(browser, name))?.expanded) return;
  await browser.$(`${await row(browser, name)} button.chevron`).click();
  await browser.waitUntil(async () => (await branch(browser, name))?.expanded === true, { timeout: 15000, timeoutMsg: `${name} expanded` });
}

/** Wait until `name` is listed. */
export async function listed(browser: WebdriverIO.Browser, name: string, timeout = 15000): Promise<void> {
  await browser.waitUntil(async () => (await branch(browser, name)) !== undefined,
    { timeout, timeoutMsg: `${name} is listed in the Branches: ${JSON.stringify(await branches(browser))}` });
}

/** Click a branch's name and wait until it is selected and expanded. */
export async function selectBranch(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await listed(browser, name);
  await browser.$(`${await row(browser, name)} button.name`).click();
  await browser.waitUntil(async () => {
    const entry = await branch(browser, name);
    return entry?.selected === true && entry.expanded;
  }, { timeout: 15000, timeoutMsg: `${name} selected and expanded` });
}

/** Click the timeline entry `chip` (a Run's ID, or `current`) of the branch `name`, expanding
 * it first, and wait until it is the one shown. */
export async function chooseChip(browser: WebdriverIO.Browser, name: string, chip: string): Promise<void> {
  await expand(browser, name);
  await browser.waitUntil(async () => (await branch(browser, name))?.chips.some((entry) => entry.id === chip) === true,
    { timeout: 15000, timeoutMsg: `${name} has the entry ${chip}: ${JSON.stringify(await branch(browser, name))}` });
  await browser.$(`${await row(browser, name)} button[data-chip=${JSON.stringify(chip)}]`).click();
  await browser.waitUntil(async () => (await branch(browser, name))?.chips.find((entry) => entry.id === chip)?.filled === true,
    { timeout: 15000, timeoutMsg: `${name}'s chip ${chip} shown` });
}

/** Show the Run `run` of the branch `name` in the Main area. */
export async function showRun(browser: WebdriverIO.Browser, name: string, run: string): Promise<void> {
  await chooseChip(browser, name, run);
  await browser.waitUntil(async () => (await mainTitle(browser)) === `Run ${run}`, { timeout: 15000, timeoutMsg: `Run ${run} shown` });
}

/** The Main area's Current header: `Scanned at …` once a scan showed, else what it says. */
export function scanned(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => document.querySelector('section[aria-label="Current"] .when')?.textContent?.trim() ?? "");
}

/** The instant the Current's header says it was scanned, as the app rendered it; null without one. */
export function scannedAt(browser: WebdriverIO.Browser): Promise<string | null> {
  return browser.execute(() => document.querySelector('section[aria-label="Current"] .when time')?.getAttribute("datetime") ?? null);
}

/** Open the waiting review `id` with a click on its row, and wait for its Review. */
export async function openWaiting(browser: WebdriverIO.Browser, id: string): Promise<void> {
  await browser.waitUntil(async () => (await waiting(browser)).rows.some((entry) => entry.id === id),
    { timeout: 15000, timeoutMsg: `${id} waits in the stack` });
  await browser.$(`[aria-label="Waiting reviews"] [data-key=${JSON.stringify(id)}]`).click();
  await browser.waitUntil(async () => (await mainTitle(browser)) === "Review", { timeout: 15000, timeoutMsg: `${id}'s Review shown` });
}
