// An Artifact Viewer window against a mocked command boundary: each built-in viewer reads the
// Artifact through its viewer session and Point appends the place clicked; a plugin's page
// reaches the Artifact only through the bridge, answered for its own frame alone.
import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { listen } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ViewerContext, ViewerEntry } from "../ipc/client";
import ViewerWindow from "./ViewerWindow.svelte";

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
  window.matchMedia ??= ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  globalThis.DOMMatrixReadOnly ??= class {
    m22 = 1;
    constructor() {}
  } as unknown as typeof DOMMatrixReadOnly;
});

afterEach(() => {
  cleanup();
  clearMocks();
});

const context = (overrides: Partial<ViewerContext> = {}): ViewerContext => ({
  artifact: "checkout",
  type: "files",
  title: "File Explorer",
  target: "checkout",
  targetKind: "folder",
  review: "run-a-2",
  ...overrides,
});

type Call = { cmd: string; args: Record<string, unknown> };

/** A viewer session of `entry` over `files` (path → text; a path ending in `/` a folder). */
function backend(entry: ViewerEntry, files: Record<string, string>, shown: ViewerContext = context(), declarations = "{}") {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    const given = args as Record<string, unknown>;
    calls.push({ cmd, args: given });
    switch (cmd) {
      case "viewer_entry":
        return entry;
      case "viewer_context":
        return shown;
      case "viewer_files": {
        const under = given.path ? `${String(given.path)}/` : "";
        return Object.keys(files)
          .filter((path) => path.startsWith(under) && path !== under && !path.slice(under.length).replace(/\/$/, "").includes("/"))
          .map((path) => ({ name: path.slice(under.length).replace(/\/$/, ""), path: path.replace(/\/$/, ""), kind: path.endsWith("/") ? "directory" : "file" }));
      }
      case "viewer_read_text":
        return files[String(given.path)];
      case "viewer_declarations":
        return declarations;
      case "viewer_point":
        return String(given.text);
      case "human_open":
        return { id: given.id, target: { run: "run-a", node: "e:checkout/signoff" }, read: 0 };
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return calls;
}

const pointed = (calls: Call[]) => calls.filter((call) => call.cmd === "viewer_point").map((call) => call.args.text);

describe("File Explorer", () => {
  it("lists folders before files, unfolds a folder, shows a file with its lines and points at a line", async () => {
    const calls = backend({ kind: "builtin", viewer: "files" }, { "README.md": "# Checkout", "src/": "", "src/total.ts": "export const total = 1;\nexport const tax = 2;" });
    const told: unknown[] = [];
    await listen("notes-pointed", (event) => told.push(event.payload));
    const { container } = render(ViewerWindow, { session: 7 });
    // The strip names the active review by its eval.
    await waitFor(() => expect(container.querySelector("header.strip .review")?.textContent).toBe("checkout/signoff"));
    const tree = await screen.findByRole("list", { name: "Files" });
    await within(tree).findByText("README.md");
    expect(within(tree).getAllByRole("button").map((button) => button.textContent?.trim())).toEqual(["src", "README.md"]);
    await fireEvent.click(within(tree).getByRole("button", { name: "src" }));
    await fireEvent.click(await within(tree).findByRole("button", { name: "total.ts" }));
    const doc = screen.getByRole("region", { name: "Document" });
    const second = await within(doc).findByText("export const tax = 2;");
    // Point, then a click on a place: its file, line and column go to the draft.
    await fireEvent.click(screen.getByRole("button", { name: "Point" }));
    expect(screen.getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("true");
    await fireEvent.click(second);
    await waitFor(() => expect(pointed(calls)).toEqual(["src/total.ts L2:C1"]));
    expect(screen.getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false");
    // The windows showing the review's Notes are told what was pointed, to mark it there.
    await waitFor(() => expect(told).toEqual([{ review: "run-a-2", text: "src/total.ts L2:C1" }]));
  });

  it("shows no Point without an active review, and Esc stops pointing without appending", async () => {
    backend({ kind: "builtin", viewer: "files" }, { "a.txt": "a" }, context({ review: null }));
    render(ViewerWindow, { session: 1 });
    await screen.findByText("a.txt");
    expect(screen.queryByRole("button", { name: "Point" })).toBeNull();
    cleanup();
    clearMocks();
    const calls = backend({ kind: "builtin", viewer: "files" }, { "a.txt": "a" });
    render(ViewerWindow, { session: 1 });
    await fireEvent.click(await screen.findByRole("button", { name: "Point" }));
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false");
    expect(pointed(calls)).toEqual([]);
  });
});

describe("Markdown", () => {
  it("renders a file Artifact's Markdown and points at a block's line", async () => {
    const calls = backend({ kind: "builtin", viewer: "markdown" }, { "": "# Install\n\nRun the `installer`." }, context({ targetKind: "file", type: "markdown", title: "Markdown" }));
    render(ViewerWindow, { session: 2 });
    const heading = await screen.findByRole("heading", { name: "Install" });
    expect(heading.tagName).toBe("H1");
    const paragraph = screen.getByText("installer").parentElement!;
    await fireEvent.click(screen.getByRole("button", { name: "Point" }));
    await fireEvent.click(paragraph);
    await waitFor(() => expect(pointed(calls)).toEqual(["L3:C1"]));
  });
});

describe("Artifactize Viewer", () => {
  const graph = JSON.stringify({
    artifacts: { "front-design": { kind: "folder", type: "html" }, front: { kind: "folder", type: null } },
    evals: {
      "front-design/matches-layout": { target: "front-design", title: "The page matches the layout", criteria: { kind: "agent" }, payload: { instruction: "Compare the page with the layout." }, reads: ["front"] },
    },
    relations: [{ source: "front", target: "front-design", kind: "instruction" }],
  });

  it("draws the declared Artifacts, opens one's evals, and points at an eval by its ID", async () => {
    const calls = backend({ kind: "builtin", viewer: "artifactize" }, {}, context({ type: "artifactize", title: "Artifactize Viewer" }), graph);
    const { container } = render(ViewerWindow, { session: 3 });
    await waitFor(() => expect(container.querySelectorAll(".svelte-flow__node")).toHaveLength(2));
    const node = [...container.querySelectorAll<HTMLElement>(".svelte-flow__node")].find((each) => each.textContent?.includes("front-design"))!;
    expect(node.textContent).toContain("html");
    const other = [...container.querySelectorAll<HTMLElement>(".svelte-flow__node")].find((each) => !each.textContent?.includes("front-design"))!;
    expect(other.textContent).toContain("custom");
    await fireEvent.click(node);
    const panel = await screen.findByRole("region", { name: "Artifact front-design" });
    const evaluation = within(panel).getByRole("region", { name: "front-design/matches-layout" });
    expect(evaluation.textContent).toContain("Compare the page with the layout.");
    expect(evaluation.textContent).toContain("agent");
    await fireEvent.click(screen.getByRole("button", { name: "Point" }));
    await fireEvent.click(evaluation);
    await waitFor(() => expect(pointed(calls)).toEqual(["front-design/matches-layout"]));
  });
});

describe("a plugin's page", () => {
  it("loads from the viewer scheme in a sandbox and answers the bridge for its own frame only", async () => {
    mockConvertFileSrc("linux");
    const calls = backend({ kind: "web", html: "<p>page</p>" }, { "index.html": "<h1>Hi</h1>" }, context({ type: "html", title: "HTML Viewer" }));
    const { container } = render(ViewerWindow, { session: 4 });
    const frame = await waitFor(() => {
      const found = container.querySelector("iframe");
      expect(found).not.toBeNull();
      return found!;
    });
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.src).toContain("viewer");
    expect(frame.src).toContain("4");
    // The app's Point is not drawn over a plugin's page: the page draws its own.
    expect(screen.queryByRole("button", { name: "Point" })).toBeNull();
    const reply = vi.spyOn(frame.contentWindow!, "postMessage");
    const ask = { channel: "artifactize-viewer/v1", id: 1, method: "readText", args: { path: "index.html" } };
    // Another window's message is not answered.
    window.dispatchEvent(new MessageEvent("message", { data: ask, source: window }));
    window.dispatchEvent(new MessageEvent("message", { data: ask, source: frame.contentWindow }));
    await waitFor(() => expect(reply).toHaveBeenCalledWith({ channel: "artifactize-viewer/v1", id: 1, result: "<h1>Hi</h1>" }, "*"));
    expect(reply).toHaveBeenCalledTimes(1);
    expect(calls.filter((call) => call.cmd === "viewer_read_text")).toHaveLength(1);
    window.dispatchEvent(new MessageEvent("message", { data: { ...ask, id: 2, method: "invoke" }, source: frame.contentWindow }));
    await waitFor(() => expect(reply).toHaveBeenCalledWith({ channel: "artifactize-viewer/v1", id: 2, error: "The viewer bridge has no invoke." }, "*"));
  });
});
