// @vitest-environment node
// Exercise the installable example's own scripts at its message boundary, without
// asking WebDriver to cross the opaque origin of the webview's sandbox.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { waitFor } from "@testing-library/dom";
import { JSDOM } from "jsdom";
import { expect, it, vi } from "vitest";

const example = fileURLToPath(new URL("../../../../../examples/plugins/html/viewer.html", import.meta.url));
const checkout = '<main id="checkout"><p class="total">$42.00</p></main>';
const channel = "artifactize-viewer/v1";

type Request = { channel: string; id: number; method: string; args: Record<string, unknown> };

it("posts the selected element's CSS selector once, and posts nothing when Point is off or cancelled", async () => {
  const dom = new JSDOM("<iframe></iframe>", { runScripts: "dangerously" });
  try {
    const host = dom.window as unknown as Window;
    const viewer = host.document.querySelector("iframe")!.contentWindow!;
    // jsdom does not supply MessageEvent.source for postMessage or load iframe srcdoc.
    // Deliver messages between the actual windows and load the example's generated srcdoc.
    const deliver = (to: Window, from: Window, data: unknown) =>
      to.dispatchEvent(new dom.window.MessageEvent("message", { source: from, data }));
    const sent = vi.fn((request: Request) => {
      switch (request.method) {
        case "context":
          deliver(viewer, host, { channel, id: request.id, result: { review: "run-a-2", targetKind: "file" } });
          break;
        case "readText":
          deliver(viewer, host, { channel, id: request.id, result: checkout });
          break;
        case "pointing":
          deliver(viewer, host, { channel, id: request.id, result: request.args.text });
          break;
      }
    });
    host.postMessage = sent;
    viewer.document.write(readFileSync(example, "utf8"));
    viewer.document.close();
    const frame = viewer.document.querySelector("iframe")!;
    const child = frame.contentWindow!;
    viewer.postMessage = (data: unknown) => { deliver(viewer, child, data); };
    child.postMessage = (data: unknown) => { deliver(child, viewer, data); };
    await waitFor(() => expect(frame.srcdoc).toContain(checkout), { container: viewer.document.body });
    child.document.open();
    child.document.write(frame.srcdoc);
    child.document.close();

    const point = viewer.document.querySelector("button")!;
    const total = child.document.querySelector<HTMLElement>("#checkout .total")!;
    const outboundPoints = () => sent.mock.calls.flatMap(([request]) => request.method === "pointing" ? [request] : []);
    const arm = async () => {
      expect(point.hidden).toBe(false);
      point.click();
      await waitFor(() => expect(point.getAttribute("aria-pressed")).toBe("true"), { container: viewer.document.body });
    };
    total.click();
    expect(outboundPoints()).toEqual([]);

    await arm();
    total.click();
    expect(outboundPoints()).toEqual([
      { channel, id: expect.any(Number), method: "pointing", args: { text: "#checkout .total" } },
    ]);
    total.click();
    expect(outboundPoints()).toHaveLength(1);

    await arm();
    total.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(point.getAttribute("aria-pressed")).toBe("false");
    total.click();
    expect(outboundPoints()).toHaveLength(1);
  } finally {
    dom.window.close();
  }
});
