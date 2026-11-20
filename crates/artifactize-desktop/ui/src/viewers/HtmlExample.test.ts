// @vitest-environment node
// Exercise the installable example's own scripts at its message boundary, without
// asking WebDriver to cross the opaque origin of the webview's sandbox.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { waitFor } from "@testing-library/dom";
import { JSDOM } from "jsdom";
import { expect, it, vi } from "vitest";

const example = fileURLToPath(new URL("../../../../../examples/plugins/html/viewer.html", import.meta.url));
const checkout = '<title>Checkout</title><main id="checkout"><p class="total">$42.00</p></main>';
const channel = "artifactize-viewer/v1";

type Request = { channel: string; id: number; method: string; args: Record<string, unknown> };

function exampleWindow(dom: JSDOM) {
  const host = dom.window as unknown as Window;
  const viewer = host.document.querySelector("iframe")!.contentWindow!;
  // jsdom does not supply MessageEvent.source for postMessage or load iframe srcdoc.
  // Deliver messages between the actual windows; the tests choose when to load srcdoc.
  const deliver = (to: Window, from: Window, data: unknown) =>
    to.dispatchEvent(new dom.window.MessageEvent("message", { source: from, data }));
  const sent = vi.fn((request: Request) => {
    const result = request.method === "context" ? { review: "run-a-2", target: "checkout/", targetKind: "folder" }
      : request.method === "readText" ? checkout : request.method === "pointing" ? request.args.text : null;
    deliver(viewer, host, { channel, id: request.id, result });
  });
  host.postMessage = sent;
  viewer.document.write(readFileSync(example, "utf8"));
  viewer.document.close();
  const frame = viewer.document.querySelector("iframe")!;
  const child = frame.contentWindow!;
  viewer.postMessage = (data: unknown) => { deliver(viewer, child, data); };
  child.postMessage = (data: unknown) => { deliver(child, viewer, data); };
  const outbound = (method: string) => sent.mock.calls.flatMap(([request]) => request.method === method ? [request] : []);
  const until = (read: () => void) => waitFor(read, { container: viewer.document.body });
  const point = (armed: boolean) => deliver(viewer, host, { channel, event: "point", armed });
  return { frame, child, outbound, until, point };
}

it("reports the loaded document and points at a CSS selector only while armed", async () => {
  const dom = new JSDOM("<iframe></iframe>", { runScripts: "dangerously" });
  try {
    const { frame, child, outbound, until, point } = exampleWindow(dom);
    await until(() => expect(frame.srcdoc).toContain(checkout));
    expect(outbound("readText")).toEqual([{ channel, id: expect.any(Number), method: "readText", args: { path: "index.html" } }]);
    expect(outbound("status")).toEqual([]);
    child.document.open();
    child.document.write(frame.srcdoc);
    child.document.close();
    await until(() => expect(outbound("status")).toEqual([
      { channel, id: expect.any(Number), method: "status", args: { text: "checkout/index.html · Checkout" } },
    ]));

    const total = child.document.querySelector<HTMLElement>("#checkout .total")!;
    const cursor = () => child.document.documentElement.style.cursor;
    total.click();
    expect(outbound("pointing")).toEqual([]);

    point(true);
    expect(cursor()).toBe("crosshair");
    total.click();
    expect(outbound("pointing")).toEqual([
      { channel, id: expect.any(Number), method: "pointing", args: { text: "#checkout .total" } },
    ]);
    total.click();
    expect(outbound("pointing")).toHaveLength(1);

    point(true);
    point(false);
    expect(cursor()).toBe("");
    total.click();
    expect(outbound("pointing")).toHaveLength(1);

    point(true);
    total.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(cursor()).toBe("");
    total.click();
    expect(outbound("pointing")).toHaveLength(1);
  } finally {
    dom.window.close();
  }
});

it("reports a failure if the framed document cannot load", async () => {
  const dom = new JSDOM("<iframe></iframe>", { runScripts: "dangerously" });
  try {
    const { frame, outbound, until } = exampleWindow(dom);
    await until(() => expect(frame.srcdoc).toContain(checkout));
    frame.dispatchEvent(new dom.window.Event("error"));
    expect(outbound("status")).toEqual([
      { channel, id: expect.any(Number), method: "status", args: { text: "checkout/index.html did not load." } },
    ]);
  } finally {
    dom.window.close();
  }
});
