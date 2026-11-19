// Exercise the option and pointer input sequences at the page event boundary.
import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { clickOption, dispatchInput, tab } from "./input.ts";

test("option clicks choose as a user does, once, and never change disabled or already-selected choices", async () => {
  for (const state of ["fresh", "selected", "disabled", "prevented", "click-prevented", "covered", "offscreen"] as const) {
    const page = new JSDOM('<select><option value="0" selected>stable</option><option value="1">preview</option></select>', { runScripts: "outside-only" });
    try {
      const select = page.window.document.querySelector("select")!;
      select.getBoundingClientRect = () => new page.window.DOMRect(state === "offscreen" ? -100 : 0, 0, 80, 20);
      page.window.document.elementFromPoint = () => state === "covered" ? page.window.document.body : select;
      const option = select.options[1];
      if (state === "selected") option.selected = true;
      if (state === "disabled") select.disabled = true;
      if (state === "prevented") select.addEventListener("mousedown", (event) => event.preventDefault());
      if (state === "click-prevented") select.addEventListener("click", (event) => event.preventDefault());
      const events: string[] = [];
      for (const kind of ["mousedown", "input", "change", "mouseup", "click"]) {
        select.addEventListener(kind, () => events.push(`${kind}:${select.value}`));
      }
      const target = { waitForExist: async () => {} } as unknown as WebdriverIO.Element;
      const browser = { execute: async (action: (...args: unknown[]) => unknown) => {
        const inPage = page.window.eval(`(${action.toString()})`) as (target: HTMLOptionElement) => void;
        return inPage(option);
      } } as unknown as WebdriverIO.Browser;
      if (state === "covered" || state === "offscreen") {
        await assert.rejects(clickOption(browser, target), /outside the viewport or obscured/);
        assert.deepEqual(events, []);
        assert.equal(select.value, "0");
        continue;
      }
      await clickOption(browser, target);
      const changed = state === "fresh" || state === "click-prevented";
      const initial = state === "selected" ? "1" : "0";
      // As a user's choice: the selection changes before input and change; an unchanged one fires neither.
      assert.deepEqual(events, changed ? ["mousedown:0", "input:1", "change:1", "mouseup:1", "click:1"]
        : ["mousedown", "mouseup", "click"].map((kind) => `${kind}:${initial}`));
      assert.equal(select.value, changed ? "1" : initial);
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


test("a cancelled pointerdown keeps its gesture's compatibility mouse events from the page, its click kept", async () => {
  for (const cancelled of [false, true]) {
    const { page, browser } = focusPage('<div id="box">box</div>');
    try {
      const box = page.window.document.getElementById("box")!;
      page.window.document.elementFromPoint = () => box;
      if (cancelled) box.addEventListener("pointerdown", (event) => event.preventDefault());
      const events: string[] = [];
      const pointerMoves: number[][] = [];
      const presses: number[] = [];
      const clicks: [string, number][] = [];
      box.addEventListener("pointerdown", (event) => presses.push((event as PointerEvent).detail));
      box.addEventListener("pointermove", (event) => {
        const pointer = event as PointerEvent;
        pointerMoves.push([pointer.button, pointer.buttons, pointer.detail]);
      });
      box.addEventListener("click", (event) => {
        const click = event as PointerEvent;
        clicks.push([click.pointerType, click.detail]);
      });
      for (const kind of ["pointerdown", "pointermove", "pointerup", "mousedown", "mousemove", "mouseup", "click"]) {
        box.addEventListener(kind, () => events.push(kind));
      }
      for (const phase of ["down", "move", "up"] as const) await dispatchInput(browser, { kind: "pointer", phase, x: 5, y: 5 });
      assert.deepEqual(pointerMoves, [[-1, 1, 0]]);
      await dispatchInput(browser, { kind: "click", x: 5, y: 5 });
      assert.deepEqual(presses, [0, 0], "neither pointerdown carries a click count");
      assert.deepEqual(clicks, [["mouse", 1], ["mouse", 1]], "the drag and click end in mouse PointerEvent clicks with a click count");
      const pointers = ["pointerdown", "pointermove", "pointerup"];
      const gesture = cancelled ? pointers : ["pointerdown", "mousedown", "pointermove", "mousemove", "pointerup", "mouseup"];
      const click = cancelled ? ["pointerdown", "pointerup", "click"] : ["pointerdown", "mousedown", "pointerup", "mouseup", "click"];
      assert.deepEqual(events, [...gesture, "click", ...click]);
    } finally { page.window.close(); }
  }
});


test("an outside press releases on the new hit target without clicking the removed overlay", async () => {
  const { page, browser } = focusPage('<main id="behind">outside</main><div id="overlay">palette</div>');
  try {
    const overlay = page.window.document.getElementById("overlay")!;
    const behind = page.window.document.getElementById("behind")!;
    page.window.document.elementFromPoint = () => overlay.isConnected ? overlay : behind;
    const received: string[] = [];
    overlay.addEventListener("pointerdown", () => { received.push("overlay down"); overlay.remove(); });
    overlay.addEventListener("click", () => received.push("detached overlay click"));
    behind.addEventListener("pointerup", () => received.push("behind up"));
    behind.addEventListener("click", () => received.push("unpressed content click"));
    await dispatchInput(browser, { kind: "click", x: 5, y: 5 });
    assert.deepEqual(received, ["overlay down", "behind up"]);
  } finally { page.window.close(); }
});
