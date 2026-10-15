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
  /** Chords whose real DOM event was consumed by an app handler. */
  handled?: () => Promise<string[]>;
};

/** Keep the original event so defaultPrevented is observed after the handlers run. */
function events(browser: WebdriverIO.Browser): Promise<{ received: string[]; handled: string[] }> {
  return browser.execute(() => {
    const page = window as unknown as { keyEvents?: KeyboardEvent[] };
    if (page.keyEvents === undefined) {
      const log: KeyboardEvent[] = [];
      page.keyEvents = log;
      window.addEventListener("keydown", (event) => log.push(event), { capture: true });
    }
    return {
      received: page.keyEvents.map((event) => event.key),
      handled: page.keyEvents.filter((event) => event.defaultPrevented).map((event) => [
        ...(event.ctrlKey ? ["Control"] : []), ...(event.shiftKey ? ["Shift"] : []),
        ...(event.altKey ? ["Alt"] : []), ...(event.metaKey ? ["Meta"] : []), event.key,
      ].join("+")),
    };
  });
}

/** The real app's page, logging keydowns on first use without changing their handling. */
export function page(browser: WebdriverIO.Browser): Page {
  return {
    keys: (keys) => browser.keys(keys),
    received: async () => (await events(browser)).received,
    handled: async () => (await events(browser)).handled,
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

type Options = { wait?: number; tries?: number; ms?: number; handled?: boolean };

/**
 * Press one action on `target` until its key arrives and `done` holds. With `handled`, the
 * exact chord must also have been consumed: useful when an action deliberately keeps state.
 * A dropped action key is retried at most `tries` times; a delivered one is never resent.
 */
export async function press(
  target: WebdriverIO.Browser | Page,
  keys: string[],
  done: () => Promise<boolean> | boolean,
  what: string,
  { wait = 2000, tries = 3, ms = 15000, handled = false }: Options = {},
): Promise<void> {
  const on = "received" in target ? target : page(target);
  const actions = keys.filter((key) => !MODIFIERS.has(key));
  assert.equal(actions.length, 1, `press one action at a time, not ${keys.join(", ")}`);
  const action = actions[0].toLowerCase();
  const name = [...MODIFIERS].filter((key) => keys.includes(key)).concat(actions).join("+");
  if (handled) assert.ok(on.handled, "the page observes consumed chords");
  for (let attempt = 0; attempt < tries; attempt += 1) {
    const before = (await on.received()).length;
    const beforeHandled = handled ? (await on.handled!()).length : 0;
    const arrived = async () => (await on.received()).slice(before).some((key) => key.toLowerCase() === action);
    const confirmed = async () => {
      if (!(await arrived())) return false;
      if (handled && !(await on.handled!()).slice(beforeHandled).some((key) => key.toLowerCase() === name.toLowerCase())) return false;
      return done();
    };
    await on.keys(keys);
    if (await within(confirmed, wait)) return;
    if (await arrived()) {
      // The page has the action key: the app decides now, however long it takes.
      if (await within(confirmed, ms - wait)) return;
      assert.fail(`timed out: ${what} (the page received ${name}${handled ? " but its handled effect was not confirmed" : ""})`);
    }
    console.log(`e2e: ${name} never reached the page; sending it again (${what})`);
  }
  assert.fail(`timed out: ${what} (${name} never reached the page in ${tries} tries)`);
}

/** Flush event-handler microtasks and a complete browser render, not an elapsed sleep.
 * This is only a boundary for synchronous DOM actions; native work needs its own oracle. */
export async function afterRender(browser: WebdriverIO.Browser): Promise<void> {
  await browser.executeAsync((done: () => void) => {
    requestAnimationFrame(() => requestAnimationFrame(() => done()));
  });
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
