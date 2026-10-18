// Keyboard input that survives a WebDriver transport dropping a key now and then (a person's
// keyboard never does this). Each call presses one action: one key, or one key held with
// modifiers (Control+k). The page logs every keydown it receives in the capture phase, before
// any handler: an action key that reached the page and did nothing is the app's failure and
// fails the test; only an action key that never reached the page is sent again. Several keys
// in a row are several calls, each with its own expected effect, so no delivered key is ever
// sent twice.
import assert from "node:assert/strict";
import { dispatchInput, tab } from "./input.ts";

const WAIT_EXPIRED = "the key delivery or outcome did not complete";

/** Keys that only modify the action key they are held with. */
const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

/** What `press` needs of a page: send keys, and read the keydowns it received so far. */
export type Page = {
  keys: (keys: string[]) => Promise<unknown>;
  received: () => Promise<string[]>;
  until: (check: () => Promise<boolean> | boolean, timeout: number) => Promise<boolean>;
  /** Chords whose real DOM event was consumed by an app handler. */
  handled?: () => Promise<string[]>;
  /** Read-only failure evidence: where the delivered action actually landed. */
  diagnostics?: () => Promise<unknown>;
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
    keys: async (keys) => {
      // The pristine embedded provider omits these W3C special-key mappings. Use
      // WebDriver's page command for the same keyboard fields the app handles.
      if (keys.includes("Tab") && keys.every((key) => key === "Tab" || key === "Shift")) {
        await tab(browser, keys.includes("Shift"));
        return;
      }
      const key = keys.length === 1 ? keys[0] : undefined;
      if (key === "PageUp" || key === "PageDown") {
        await dispatchInput(browser, { kind: "key", key });
      } else await browser.keys(keys);
    },
    until: async (check, timeout) => {
      let failure: unknown;
      try {
        await browser.waitUntil(async () => {
          try { return await check(); }
          catch (error) { failure = error; throw error; }
        }, { timeout, timeoutMsg: WAIT_EXPIRED });
        return true;
      } catch (error) {
        if (failure !== undefined) throw failure;
        if (error instanceof Error && error.message === WAIT_EXPIRED) return false;
        throw error;
      }
    },
    received: async () => (await events(browser)).received,
    handled: async () => (await events(browser)).handled,
    diagnostics: () => browser.execute(() => {
      const describe = (target: EventTarget | null) => target instanceof Element
        ? { tag: target.tagName, label: target.getAttribute("aria-label"), role: target.getAttribute("role"), id: target.id }
        : null;
      const log = (window as unknown as { keyEvents?: KeyboardEvent[] }).keyEvents ?? [];
      return {
        focused: describe(document.activeElement),
        recent: log.slice(-5).map((event) => ({ key: event.key, prevented: event.defaultPrevented, target: describe(event.target) })),
      };
    }),
  };
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
    if (await on.until(confirmed, wait)) return;
    if (await arrived()) {
      // The page has the action key: the app decides now, however long it takes.
      if (await on.until(confirmed, ms - wait)) return;
      const evidence = on.diagnostics ? `; ${JSON.stringify(await on.diagnostics())}` : "";
      assert.fail(`timed out: ${what} (the page received ${name}${handled ? " but its handled effect was not confirmed" : ""})${evidence}`);
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

/** Set the field `selector` names to `text` and wait until the field, found again, shows it.
 * A form rebuilt since the field was last found has a new element with the same selector. */
export async function fill(browser: WebdriverIO.Browser, selector: string, text: string): Promise<void> {
  const shown = () => browser.execute((query) => {
    const field = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(query);
    const active = document.activeElement;
    return { value: field?.value ?? null, focused: !!field && active === field, active: active ? `${active.tagName}#${active.id}` : null };
  }, selector);
  await browser.$(selector).setValue(text);
  try {
    await browser.waitUntil(async () => (await shown()).value === text, { timeout: 15000, timeoutMsg: `timed out: ${selector} shows ${JSON.stringify(text)}` });
  } catch (error) {
    throw new Error(`${selector} does not show ${JSON.stringify(text)}; it showed ${JSON.stringify(await shown())}`, { cause: error });
  }
}

/** The focused field's value. */
const value = (browser: WebdriverIO.Browser) =>
  browser.execute(() => (document.activeElement as HTMLInputElement | null)?.value ?? "");

/**
 * Type content into the focused field, confirming each resulting character. Ordinary
 * letters still exercise the actual key handlers, including shortcut suppression. Literal
 * whitespace uses WebDriver's text-input API: the embedded provider's synthetic Space key
 * has no native text-insertion default action. No page-side input behavior is replaced.
 */
export async function typeText(browser: WebdriverIO.Browser, text: string): Promise<void> {
  for (const key of text) {
    const before = await value(browser);
    if (key === " ") {
      const active = await browser.getActiveElement();
      const elementId = active["element-6066-11e4-a52e-4f735466cecf"];
      if (typeof elementId !== "string") throw new Error("WebDriver did not identify the focused text field");
      await browser.$({ "element-6066-11e4-a52e-4f735466cecf": elementId }).addValue(key);
      await browser.waitUntil(async () => (await value(browser)) === before + key, { timeout: 15000, timeoutMsg: "the entered space did not appear in the focused field" });
    } else {
      await press(browser, [key], async () => (await value(browser)) === before + key, `typing ${JSON.stringify(key)}`);
    }
  }
}
