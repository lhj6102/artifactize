// The public input-adapter boundary: a real option click followed by exactly the missing
// WebDriver-selectedness/input/change defaults, not an application handler invocation.
import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { clickOption } from "./input.ts";

test("option clicks supply the WebDriver default once and never change disabled or already-selected choices", async () => {
  for (const state of ["fresh", "selected", "disabled"] as const) {
    const page = new JSDOM('<select><option value="0" selected>stable</option><option value="1">preview</option></select>', { runScripts: "outside-only" });
    try {
      const select = page.window.document.querySelector("select")!;
      const option = select.options[1];
      if (state === "selected") option.selected = true;
      if (state === "disabled") select.disabled = true;
      const events: string[] = [];
      option.addEventListener("click", () => events.push("click"));
      for (const kind of ["input", "change"]) select.addEventListener(kind, () => events.push(kind));
      const target = { click: async () => { option.click(); } } as unknown as WebdriverIO.Element;
      const browser = { execute: async (action: (...args: unknown[]) => unknown) => {
        const inPage = page.window.eval(`(${action.toString()})`) as (target: HTMLOptionElement) => void;
        inPage(option);
      } } as unknown as WebdriverIO.Browser;
      await clickOption(browser, target);
      assert.deepEqual(events, state === "fresh" ? ["click", "input", "change"] : ["click"]);
      assert.equal(select.value, state === "disabled" ? "0" : "1");
    } finally { page.window.close(); }
  }
});
