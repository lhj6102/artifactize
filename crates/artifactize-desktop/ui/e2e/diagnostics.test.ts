import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { it } from "node:test";
import { runInNewContext } from "node:vm";
import { failureEvidence } from "./diagnostics.ts";

it("a failed document observation does not suppress URL or source evidence", async () => {
  const observations: string[] = [];
  const browser = {
    async getUrl() { observations.push("url"); return "http://tauri.localhost/"; },
    async execute() { observations.push("document"); throw new Error("script context unavailable"); },
    async getPageSource() { observations.push("source"); return '<html><body><div id="app"></div></body></html>'; },
  } as unknown as WebdriverIO.Browser;
  assert.deepEqual(await failureEvidence(browser), {
    url: "http://tauri.localhost/",
    document: { observationFailure: "Error: script context unavailable" },
    source: '<html><body><div id="app"></div></body></html>',
  });
  assert.deepEqual(observations, ["url", "document", "source"]);
});

it("the E2E startup observer records errors without cancelling or changing them", () => {
  const window = new EventTarget() as EventTarget & { __ARTIFACTIZE_E2E_ERRORS__: unknown[] };
  runInNewContext(readFileSync(new URL("../../src/e2e_startup.js", import.meta.url), "utf8"), { window });
  const emit = (type: string, fields: object) => {
    const event = Object.assign(new Event(type, { cancelable: true }), fields);
    assert.equal(window.dispatchEvent(event), true);
    assert.equal(event.defaultPrevented, false);
  };
  emit("error", { message: "module failed", filename: "app.js", lineno: 7, colno: 2, error: { stack: "module stack" } });
  emit("unhandledrejection", { reason: "launch failed" });
  emit("securitypolicyviolation", { violatedDirective: "script-src", blockedURI: "app.js", sourceFile: "index.html", lineNumber: 3 });
  assert.deepEqual(JSON.parse(JSON.stringify(window.__ARTIFACTIZE_E2E_ERRORS__)), [
    { kind: "error", message: "module failed", source: "app.js", line: 7, column: 2, stack: "module stack" },
    { kind: "unhandledrejection", message: "launch failed", stack: "" },
    { kind: "csp", directive: "script-src", blocked: "app.js", source: "index.html", line: 3 },
  ]);
  assert.throws(() => window.__ARTIFACTIZE_E2E_ERRORS__.push("rewrite"), /not extensible/);
  assert.equal(Reflect.set(window, "__ARTIFACTIZE_E2E_ERRORS__", []), false);
});
