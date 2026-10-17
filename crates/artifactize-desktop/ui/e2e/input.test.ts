// The public input-adapter boundary: a real option click followed by exactly the missing
// WebDriver-selectedness/input/change defaults, not an application handler invocation.
import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { clickOption, tab } from "./input.ts";

test("option clicks supply the WebDriver default once and never change disabled or already-selected choices", async () => {
  for (const state of ["fresh", "selected", "disabled", "prevented"] as const) {
    const page = new JSDOM('<select><option value="0" selected>stable</option><option value="1">preview</option></select>', { runScripts: "outside-only" });
    try {
      const select = page.window.document.querySelector("select")!;
      const option = select.options[1];
      if (state === "selected") option.selected = true;
      if (state === "disabled") select.disabled = true;
      if (state === "prevented") option.addEventListener("click", (event) => event.preventDefault());
      const events: string[] = [];
      option.addEventListener("click", () => events.push("click"));
      for (const kind of ["input", "change"]) select.addEventListener(kind, () => events.push(kind));
      const target = { click: async () => { option.click(); } } as unknown as WebdriverIO.Element;
      const browser = { execute: async (action: (...args: unknown[]) => unknown) => {
        const inPage = page.window.eval(`(${action.toString()})`) as (target: HTMLOptionElement) => void;
        return inPage(option);
      } } as unknown as WebdriverIO.Browser;
      await clickOption(browser, target);
      assert.deepEqual(events, state === "fresh" ? ["click", "input", "change"] : ["click"]);
      assert.equal(select.value, state === "disabled" || state === "prevented" ? "0" : "1");
    } finally { page.window.close(); }
  }
});

function focusPage(html: string) {
  const page = new JSDOM(html, { runScripts: "outside-only" });
  // jsdom has no layout: the fixture explicitly supplies whether a node has a client rect.
  page.window.HTMLElement.prototype.getClientRects = function () {
    return (this.hasAttribute("data-no-rect") ? [] : [{}]) as unknown as DOMRectList;
  };
  const browser = { execute: async (action: (...args: unknown[]) => unknown, ...args: unknown[]) => {
    const inPage = page.window.eval(`(${action.toString()})`) as (...args: unknown[]) => void;
    inPage(...args);
  } } as unknown as WebdriverIO.Browser;
  return { page, browser, focused: () => page.window.document.activeElement?.id };
}

test("Tab follows positive tabindex then tree order, Shift reverses and document ends wrap", async () => {
  const { page, browser, focused } = focusPage('<button id="zero-a">A</button><input id="two" tabindex="2"><input id="one" tabindex="1"><a id="link" href="#">link</a><button id="zero-b">B</button>');
  try {
    const events: { type: string; target: string; shift: boolean }[] = [];
    for (const type of ["keydown", "keyup"]) page.window.document.addEventListener(type, (event) => {
      const key = event as KeyboardEvent;
      assert.equal(key.key, "Tab");
      events.push({ type, target: (key.target as HTMLElement).id, shift: key.shiftKey });
    });
    for (const id of ["one", "two", "zero-a", "link", "zero-b", "one"]) {
      await tab(browser);
      assert.equal(focused(), id);
      assert.equal(events.at(-1)?.target, id, "keyup reaches the newly focused element");
    }
    await tab(browser, true);
    assert.equal(focused(), "zero-b");
    assert.ok(events.at(-1)?.shift);
    assert.deepEqual(events.slice(-2).map(({ type }) => type), ["keydown", "keyup"]);
  } finally { page.window.close(); }
});

test("Tab skips disabled, inert, hidden, closed contents and negative tabindex and defers to preventDefault", async () => {
  const { page, browser, focused } = focusPage(`<button id="first">first</button>
    <button disabled>disabled</button><div inert><button>inert</button></div>
    <button hidden>hidden</button><button data-no-rect>no rect</button>
    <button style="visibility:hidden">invisible</button><button style="display:none">not displayed</button>
    <button tabindex="-1">negative</button>
    <details><summary id="summary" tabindex="0">summary</summary><button>closed detail</button></details>
    <dialog><button>closed dialog</button></dialog><button id="last">last</button>`);
  try {
    await tab(browser);
    assert.equal(focused(), "first");
    const first = page.window.document.getElementById("first")!;
    first.addEventListener("keydown", (event) => event.preventDefault(), { once: true });
    await tab(browser);
    assert.equal(focused(), "first", "a page-managed focus trap keeps its own default prevention");
    await tab(browser);
    assert.equal(focused(), "summary");
    await tab(browser);
    assert.equal(focused(), "last");
    await tab(browser, true);
    assert.equal(focused(), "summary");
  } finally { page.window.close(); }
});
