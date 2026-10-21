// The window's one screen as a person sees it: the Pane (its Search, the Branches with their
// chip rows, the Waiting stack) beside the Main area. Every reading is read-only; actions are
// real clicks on what the window shows.

/** A chip as read: the Run it shows (`current` for the Current chip), its text without the
 * glyph, its glyph, whether it is the one shown in the Main area, and its painted background
 * and glyph colour as red, green and blue channels on any one scale. */
export type Chip = { id: string; text: string; glyph: string; filled: boolean; paint: number[]; ink: number[] };

/** Red, green or blue (0, 1, 2) is the strongest channel of `colour`. */
export const leans = (colour: number[], channel: number) =>
  colour.length === 3 && colour.every((value, at) => at === channel || colour[channel] > value);
/** A branch's entry as read: `project  branch`, whether it is selected, and its chip row. */
export type Branch = { name: string; selected: boolean; chips: Chip[] };
/** A waiting review's row as read. */
export type WaitingRow = { id: string; eval: string; where: string; meta: string; selected: boolean };

export const ENTRIES = '[role="group"][aria-label="Pane entries"]';
export const SEARCH = 'input[aria-label="Search branches and reviews"]';
export const MAIN = 'section.panel[aria-label="Main area"]';

/** The Branches in the order shown. */
export function branches(browser: WebdriverIO.Browser): Promise<Branch[]> {
  return browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Branches"] [role="option"]')].map((row) => ({
    name: row.querySelector(".name")?.textContent ?? "",
    selected: row.getAttribute("aria-selected") === "true",
    chips: [...row.querySelectorAll<HTMLElement>("button[data-chip]")].map((chip) => {
      const glyph = chip.querySelector(".glyph");
      return {
        id: chip.dataset.chip ?? "",
        text: [...chip.childNodes].filter((node) => node !== glyph).map((node) => node.textContent ?? "").join("").trim(),
        glyph: glyph?.textContent?.trim() ?? "",
        filled: chip.getAttribute("aria-pressed") === "true",
        // `rgb(r, g, b)` or `color(srgb r g b)`: the first three numbers after the space name.
        paint: (getComputedStyle(chip).backgroundColor.replace("srgb", "").match(/[\d.]+/g) ?? []).slice(0, 3).map(Number),
        ink: glyph ? (getComputedStyle(glyph).color.replace("srgb", "").match(/[\d.]+/g) ?? []).slice(0, 3).map(Number) : [],
      };
    }),
  })));
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

/** Whether the Pane's entries, the Search or the Main area has the keyboard. */
export const entriesFocused = (browser: WebdriverIO.Browser) => browser.$(ENTRIES).isFocused();
export const searchFocused = (browser: WebdriverIO.Browser) => browser.$(SEARCH).isFocused();
export const mainFocused = (browser: WebdriverIO.Browser) => browser.$(MAIN).isFocused();

/** The XPath of the branch row named `name`. */
const row = (name: string) => `//ul[@aria-label="Branches"]/li[.//button[contains(@class,"name")][.=${JSON.stringify(name)}]]`;

/** Wait until `name` is listed. */
export async function listed(browser: WebdriverIO.Browser, name: string, timeout = 15000): Promise<void> {
  await browser.waitUntil(async () => (await branch(browser, name)) !== undefined,
    { timeout, timeoutMsg: `${name} is listed in the Branches: ${JSON.stringify(await branches(browser))}` });
}

/** Click a branch's name and wait until it is selected. */
export async function selectBranch(browser: WebdriverIO.Browser, name: string): Promise<void> {
  await listed(browser, name);
  await browser.$(`${row(name)}//button[contains(@class,"name")]`).click();
  await browser.waitUntil(async () => (await branch(browser, name))?.selected === true, { timeout: 15000, timeoutMsg: `${name} selected` });
}

/** Click the chip `chip` (a Run's ID, or `current`) of the branch `name`, and wait until it is
 * the filled one. */
export async function chooseChip(browser: WebdriverIO.Browser, name: string, chip: string): Promise<void> {
  await browser.waitUntil(async () => (await branch(browser, name))?.chips.some((entry) => entry.id === chip) === true,
    { timeout: 15000, timeoutMsg: `${name} has the chip ${chip}: ${JSON.stringify(await branch(browser, name))}` });
  await browser.$(`${row(name)}//button[@data-chip=${JSON.stringify(chip)}]`).click();
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

/** Open the waiting review `id` with a click on its row, and wait for its Review. */
export async function openWaiting(browser: WebdriverIO.Browser, id: string): Promise<void> {
  await browser.waitUntil(async () => (await waiting(browser)).rows.some((entry) => entry.id === id),
    { timeout: 15000, timeoutMsg: `${id} waits in the stack` });
  await browser.$(`[aria-label="Waiting reviews"] [data-key=${JSON.stringify(id)}]`).click();
  await browser.waitUntil(async () => (await mainTitle(browser)) === "Review", { timeout: 15000, timeoutMsg: `${id}'s Review shown` });
}
