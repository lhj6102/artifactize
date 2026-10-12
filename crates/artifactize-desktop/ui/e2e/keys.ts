// Keyboard input that survives WebKitWebDriver dropping a key now and then (a person's
// keyboard never does this). Each call presses one action: one key, or one key held with
// modifiers (Control+k). The page logs every keydown it receives in the capture phase, before
// any handler: an action key that reached the page and did nothing is the app's failure and
// fails the test; only an action key that never reached the page is sent again. Several keys
// in a row are several calls, each with its own expected effect, so no delivered key is ever
// sent twice.
import assert from "node:assert/strict";

const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Keys that only modify the action key they are held with. */
const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

/** What `press` needs of a page: send keys, and read the keydowns it received so far. */
export type Page = {
  keys: (keys: string[]) => Promise<unknown>;
  received: () => Promise<string[]>;
};

/** The real app's page, logging keydowns on first use. */
export function page(browser: WebdriverIO.Browser): Page {
  return {
    keys: (keys) => browser.keys(keys),
    received: () =>
      browser.execute(() => {
        const page = window as unknown as { receivedKeys?: string[] };
        if (page.receivedKeys === undefined) {
          const log: string[] = [];
          page.receivedKeys = log;
          window.addEventListener("keydown", (event) => log.push(event.key), { capture: true });
        }
        return [...page.receivedKeys];
      }),
  };
}

/** Poll `check` until `ms` have passed. */
async function within(check: () => Promise<boolean> | boolean, ms: number): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return true;
    await pause(25);
  }
  return check();
}

type Options = { wait?: number; tries?: number; ms?: number };

/**
 * Press one action on `target` until `done` holds. When `done` has not held after `wait` ms
 * and the action key never reached the page, it is sent again (at most `tries` times in all);
 * once it reached the page, the app's effect gets `ms` in all, and no resend.
 */
export async function press(
  target: WebdriverIO.Browser | Page,
  keys: string[],
  done: () => Promise<boolean> | boolean,
  what: string,
  { wait = 2000, tries = 3, ms = 15000 }: Options = {},
): Promise<void> {
  const on = "received" in target ? target : page(target);
  const actions = keys.filter((key) => !MODIFIERS.has(key));
  assert.equal(actions.length, 1, `press one action at a time, not ${keys.join(", ")}`);
  const action = actions[0].toLowerCase();
  const name = keys.join("+");
  for (let attempt = 0; attempt < tries; attempt += 1) {
    const before = (await on.received()).length;
    await on.keys(keys);
    if (await within(done, wait)) return;
    const arrived = (await on.received()).slice(before).some((key) => key.toLowerCase() === action);
    if (arrived) {
      // The page has the action key: the app decides now, however long it takes.
      if (await within(done, ms - wait)) return;
      assert.fail(`timed out: ${what} (the page received ${name})`);
    }
    console.log(`e2e: ${name} never reached the page; sending it again (${what})`);
  }
  assert.fail(`timed out: ${what} (${name} never reached the page in ${tries} tries)`);
}

/** The focused field's value. */
const value = (browser: WebdriverIO.Browser) =>
  browser.execute(() => (document.activeElement as HTMLInputElement | null)?.value ?? "");

/**
 * Type `text` into the focused field one key at a time, each confirmed by the field's value
 * growing by that character.
 */
export async function typeText(browser: WebdriverIO.Browser, text: string): Promise<void> {
  for (const key of text) {
    const before = await value(browser);
    await press(browser, [key], async () => (await value(browser)) === before + key, `typing ${JSON.stringify(key)}`);
  }
}
