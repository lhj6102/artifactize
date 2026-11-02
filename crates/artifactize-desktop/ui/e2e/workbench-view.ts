// The workbench as a person sees it: the Git layer's history of the chosen worktree, and that
// worktree's terminal, the panel floating over the graph's left side or its tab, the screen's
// text and what the panel says above the screen. Readings are read-only; typing goes through a
// pointer press on the screen and the engine's text input.
import assert from "node:assert/strict";
import { TERMINAL } from "./graph-view.ts";
import { nativeInput } from "./platform.ts";

/** The collapsed terminal's tab. */
export const TAB = '.stage button[aria-label="Expand the terminal"]';

export type Terminal = {
  /** `Terminal · {project} / {branch}`. */
  heading: string;
  /** The screen's rows as they read, trailing blanks dropped. */
  rows: string[];
  /** What the panel says above the screen, each note's text. */
  notes: string[];
  /** The restore offer's text and its buttons, or null without one. */
  offer: { text: string; buttons: string[] } | null;
  /** The panel's box in the window. */
  box: { left: number; right: number; width: number };
};

/** The expanded terminal panel, or null while it is collapsed or not shown. */
export function terminal(browser: WebdriverIO.Browser): Promise<Terminal | null> {
  return browser.execute((query) => {
    const panel = document.querySelector<HTMLElement>(query);
    if (!panel) return null;
    const words = (element: Element) => (element.textContent ?? "").replace(/\s+/g, " ").trim();
    const offer = panel.querySelector('[aria-label="Restore"]');
    const box = panel.getBoundingClientRect();
    return {
      heading: words(panel.querySelector("h2")!),
      rows: [...panel.querySelectorAll(".xterm-rows > div")].map((row) => (row.textContent ?? "").replace(/\u00a0/g, " ").trimEnd()),
      notes: [...panel.querySelectorAll(".state, .offer, .failure")].map(words),
      offer: offer ? { text: words(offer), buttons: [...offer.querySelectorAll("button")].map((button) => button.getAttribute("aria-label") ?? words(button)) } : null,
      box: { left: box.left, right: box.right, width: box.width },
    };
  }, TERMINAL);
}

/** The screen's text, rows joined as a wrapped line reads. */
export const screenText = (shown: Terminal | null) => shown?.rows.join("") ?? "";

/** Wait until the expanded terminal satisfies `holds`; a timeout says what it showed. */
async function until(browser: WebdriverIO.Browser, holds: (shown: Terminal | null) => boolean, what: string, timeout: number): Promise<void> {
  const seen: { last: Terminal | null } = { last: null };
  await browser.waitUntil(async () => holds(seen.last = await terminal(browser)), { timeout, timeoutMsg: what })
    .catch((error: Error) => { throw new Error(`${error.message}: the screen showed ${JSON.stringify(seen.last?.rows ?? null)}`); });
}

/** Wait until the expanded terminal's screen shows `text`. */
export async function screenShows(browser: WebdriverIO.Browser, text: string, what: string, timeout = 15000): Promise<void> {
  await until(browser, (shown) => screenText(shown).includes(text), what, timeout);
}

/** Wait until a row of the expanded terminal's screen reads exactly `line`: what a program
 * printed, not the command line that was typed. */
export async function printed(browser: WebdriverIO.Browser, line: string, what: string, timeout = 15000): Promise<void> {
  await until(browser, (shown) => shows(shown, line), what, timeout);
}

/** Whether a row of the screen reads exactly `line`. */
export const shows = (shown: Terminal | null, line: string) => shown?.rows.some((row) => row.trim() === line) === true;

/** Wait until the expanded terminal of the worktree named `name` shows a live screen. */
export async function terminalOf(browser: WebdriverIO.Browser, name: string): Promise<Terminal> {
  await until(browser, (shown) => shown?.heading === `Terminal · ${name}` && shown.rows.some((row) => row.trim() !== ""), `${name}'s terminal shows a screen`, 30000);
  return (await terminal(browser))!;
}

/** Type `line` into the terminal as a person does, pressing on its screen first, and wait until
 * the screen shows it typed before Enter runs it. */
export async function typeLine(browser: WebdriverIO.Browser, line: string): Promise<void> {
  const screen = await browser.$(`${TERMINAL} .xterm-screen`);
  await browser.action("pointer").move({ origin: screen, x: 0, y: 0 }).down().up().perform();
  await browser.waitUntil(() => browser.execute((query) => !!document.activeElement?.closest(query), TERMINAL),
    { timeout: 15000, timeoutMsg: "the terminal has the keyboard" });
  // The engine's text input commits a composition, as an input method does.
  await nativeInput(browser, { type: "set", text: line });
  await nativeInput(browser, { type: "commit", text: line });
  await screenShows(browser, line, `typed ${JSON.stringify(line)}`);
  await browser.keys(["Enter"]);
}

/** Collapse the expanded terminal with its button and wait for its tab. */
export async function collapse(browser: WebdriverIO.Browser): Promise<void> {
  await browser.$(TERMINAL).$('button[aria-label="Collapse the terminal"]').click();
  await browser.$(TAB).waitForDisplayed({ timeout: 15000, timeoutMsg: "the terminal collapsed to its tab" });
  assert.equal(await terminal(browser), null, "no expanded terminal beside the tab");
}

/** Expand the collapsed terminal with its tab and wait for the panel. */
export async function expand(browser: WebdriverIO.Browser): Promise<void> {
  await browser.$(TAB).click();
  await browser.$(TERMINAL).waitForDisplayed({ timeout: 15000, timeoutMsg: "the terminal expanded" });
}

/** The Git layer, the window's far-left column. */
export const GIT_LAYER = 'section[aria-label="Git layer"]';

export type Commit = { summary: string; short: string; refs: string[]; marked: boolean };

/** The Git layer as it reads: its commits from the top, what it says otherwise (no worktree
 * chosen, reading, a worktree created), and its box. */
export function gitLayer(browser: WebdriverIO.Browser): Promise<{ commits: Commit[]; notes: string[]; box: { left: number; right: number } }> {
  return browser.execute((query) => {
    const layer = document.querySelector<HTMLElement>(query)!;
    const words = (element: Element | null) => (element?.textContent ?? "").replace(/\s+/g, " ").trim();
    const box = layer.getBoundingClientRect();
    return {
      commits: [...layer.querySelectorAll<HTMLElement>('[aria-label="Commits"] > li.commit')].map((item) => ({
        summary: words(item.querySelector(".subject")),
        short: words(item.querySelector(".meta code")),
        refs: [...item.querySelectorAll(".ref")].map(words),
        // The checked-out commit's dot carries a ring the others lack.
        marked: getComputedStyle(item.querySelector(".dot")!).boxShadow !== "none",
      })),
      notes: [...layer.querySelectorAll(":scope > p")].map(words),
      box: { left: box.left, right: box.right },
    };
  }, GIT_LAYER);
}

/** Ask for a new worktree from the commit whose summary is `summary`: the pointer goes to its
 * row, which brings its quiet action forward, and presses it. */
export async function newWorktreeFrom(browser: WebdriverIO.Browser, summary: string): Promise<void> {
  const rows = await browser.$$(`${GIT_LAYER} [aria-label="Commits"] > li.commit`);
  for (const row of rows) {
    if ((await row.$(".subject").getText()).trim() !== summary) continue;
    await browser.action("pointer").move({ origin: row, x: 0, y: 0 }).perform();
    const action = await row.$('button[aria-label^="New worktree from "]');
    await action.waitForDisplayed({ timeout: 15000, timeoutMsg: `the new worktree action shows on ${summary}` });
    await action.click();
    return;
  }
  assert.fail(`no commit ${summary} in the Git layer`);
}
