// Keyboard input that survives WebKitWebDriver dropping a key now and then (seen under load
// on keys sent one by one as well as in bursts; a person's keyboard never does this). A key is
// sent again only when it never reached the page: a key the page received but did nothing
// with is the app's failure and fails the test, so this never hides a key the app loses.
import assert from "node:assert/strict";

const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** How many keydown events the page has received, counted before any handler runs. */
async function received(browser: WebdriverIO.Browser): Promise<number> {
  return browser.execute(() => {
    const page = window as unknown as { receivedKeys?: number };
    if (page.receivedKeys === undefined) {
      page.receivedKeys = 0;
      window.addEventListener("keydown", () => (page.receivedKeys = (page.receivedKeys ?? 0) + 1), { capture: true });
    }
    return page.receivedKeys;
  });
}

/** Poll `check` until `ms` have passed. */
async function within(check: () => Promise<boolean> | boolean, ms: number): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return true;
    await pause(50);
  }
  return check();
}

/**
 * Send `keys` (one key, or a chord such as Control+k) until `done` holds. When `done` has not
 * held after `wait` ms and the page never received the keys, they are sent again (at most
 * `tries` times); once the page received them, the effect gets `ms` in all.
 */
export async function press(
  browser: WebdriverIO.Browser,
  keys: string[],
  done: () => Promise<boolean> | boolean,
  what: string,
  { wait = 2000, tries = 3, ms = 15000 }: { wait?: number; tries?: number; ms?: number } = {},
): Promise<void> {
  const keydowns = keys.length;
  for (let attempt = 0; attempt < tries; attempt += 1) {
    const before = await received(browser);
    await browser.keys(keys);
    if (await within(done, wait)) return;
    if ((await received(browser)) >= before + keydowns) {
      // The page has the keys: the app decides now, however long it takes.
      if (await within(done, ms - wait)) return;
      assert.fail(`timed out: ${what} (the page received ${keys.join("+")})`);
    }
    console.log(`e2e: ${keys.join("+")} never reached the page; sending it again (${what})`);
  }
  assert.fail(`timed out: ${what} (${keys.join("+")} never reached the page in ${tries} tries)`);
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
