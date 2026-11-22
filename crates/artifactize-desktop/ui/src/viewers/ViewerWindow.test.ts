// An Artifact Viewer window against a mocked command boundary: each built-in viewer reads the
// Artifact through its viewer session and Point appends the place clicked; a plugin's page
// reaches the Artifact only through the bridge, answered for its own frame alone.
import { clearMocks, mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit, listen } from "@tauri-apps/api/event";
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
  petMatch: true,
  ...overrides,
});

type Call = { cmd: string; args: Record<string, unknown> };

/** A viewer session of `entry` over `files` (path → text; a path ending in `/` a folder). */
function backend(entry: ViewerEntry, files: Record<string, string | (() => Promise<string>)>, shown: ViewerContext | ((session: number) => ViewerContext) = context(), declarations = "{}") {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    const given = args as Record<string, unknown>;
    calls.push({ cmd, args: given });
    switch (cmd) {
      case "viewer_entry":
        return entry;
      case "viewer_context":
        return typeof shown === "function" ? shown(Number(given.session)) : shown;
      case "viewer_files": {
        const under = given.path ? `${String(given.path)}/` : "";
        return Object.keys(files)
          .filter((path) => path.startsWith(under) && path !== under && !path.slice(under.length).replace(/\/$/, "").includes("/"))
          .map((path) => ({ name: path.slice(under.length).replace(/\/$/, ""), path: path.replace(/\/$/, ""), kind: path.endsWith("/") ? "directory" : "file" }));
      }
      case "viewer_read_text": {
        const read = files[String(given.path)];
        return typeof read === "function" ? read() : read;
      }
      case "viewer_declarations":
        return declarations;
      case "viewer_point":
        return null;
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
    // It goes to the review pointing was started for.
    expect(calls.find((call) => call.cmd === "viewer_point")?.args.review).toBe("run-a-2");
    expect(screen.getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("stops pointing, adding nothing, when the active review changes while it points", async () => {
    const shown = context();
    const calls = backend({ kind: "builtin", viewer: "files" }, { "a.txt": "one\ntwo" }, shown);
    render(ViewerWindow, { session: 8 });
    await fireEvent.click(await screen.findByRole("button", { name: "a.txt" }));
    const line = await screen.findByText("two");
    await fireEvent.click(await screen.findByRole("button", { name: "Point" }));
    // Armed for run-a-2; run-b-3 becomes the active review before the click.
    shown.review = "run-b-3";
    await emit("active-review-changed", { request: "run-b-3" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false"));
    expect(screen.getByRole("status").textContent).toBe("The review changed while pointing; nothing was added.");
    await fireEvent.click(line);
    expect(pointed(calls)).toEqual([]);
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

  it("arms from the pet only in the viewer matched to the review's original request, not a same-named Artifact elsewhere", async () => {
    // Session 1 shows `checkout` in another worktree; session 2 in the review's own.
    backend({ kind: "builtin", viewer: "files" }, { "a.txt": "a" }, (session) => context({ petMatch: session === 2 }));
    mockWindows("viewer-1", "viewer-2");
    const present: unknown[] = [];
    await listen("viewer-presence", (event) => present.push(event.payload));
    const elsewhere = render(ViewerWindow, { session: 1 }).container;
    const original = render(ViewerWindow, { session: 2 }).container;
    const strip = (container: HTMLElement) => within(container.querySelector("header.strip") as HTMLElement);
    await strip(elsewhere).findByRole("button", { name: "Point" });
    await strip(original).findByRole("button", { name: "Point" });
    // Each says which session it is, whether it matches, and for which review.
    await waitFor(() =>
      expect(present).toEqual(
        expect.arrayContaining([
          { session: 1, artifact: "checkout", review: "run-a-2", open: true, pointable: true, pointing: false, petMatch: false },
          { session: 2, artifact: "checkout", review: "run-a-2", open: true, pointable: true, pointing: false, petMatch: true },
        ]),
      ),
    );
    await emit("point-arm", { session: 2 });
    await waitFor(() => expect(strip(original).getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("true"));
    expect(strip(elsewhere).getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false");
    await emit("point-arm", { session: 2 });
    await waitFor(() => expect(strip(original).getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false"));
    // Even asked by its own session, a viewer the library does not match stays unarmed.
    await emit("point-arm", { session: 1 });
    expect(strip(elsewhere).getByRole("button", { name: "Point" }).getAttribute("aria-pressed")).toBe("false");
  });
});

it("keeps a single file in the File Explorer tree", async () => {
  backend({ kind: "builtin", viewer: "files" }, { "": "const total = 1;" }, context({ target: "source/total.ts", targetKind: "file" }));
  render(ViewerWindow, { session: 2 });
  const tree = await screen.findByRole("list", { name: "Files" });
  await waitFor(() => expect(within(tree).getByRole("button", { name: "total.ts" }).getAttribute("aria-current")).toBe("true"));
  expect(await within(screen.getByRole("region", { name: "Document" })).findByText("const total = 1;")).toBeTruthy();
});

describe("Markdown", () => {
  it("keeps the last readable document and its Point path if a new selection fails", async () => {
    let fail!: (reason: unknown) => void;
    const calls = backend({ kind: "builtin", viewer: "markdown" }, {
      "a.md": "# First document", "b.md": () => new Promise((_ok, reject) => { fail = reject; }), "c.md": "# Third document",
    }, context({ type: "markdown", title: "Markdown" }));
    render(ViewerWindow, { session: 2 });
    const first = await screen.findByRole("heading", { name: "First document" });
    const a = screen.getByRole("button", { name: "a.md" });
    await fireEvent.click(screen.getByRole("button", { name: "b.md" }));
    expect(a.getAttribute("aria-current")).toBe("true");
    fail({ kind: "read", message: "Second document unavailable." });
    await screen.findByText("Second document unavailable.");
    expect(a.getAttribute("aria-current")).toBe("true");
    await fireEvent.click(screen.getByRole("button", { name: "Point" }));
    await fireEvent.click(first);
    expect(pointed(calls)).toEqual(["a.md L1:C1"]);
    await fireEvent.click(screen.getByRole("button", { name: "c.md" }));
    await screen.findByRole("heading", { name: "Third document" });
    expect(screen.getByRole("button", { name: "c.md" }).getAttribute("aria-current")).toBe("true");
    expect(screen.queryByText("Second document unavailable.")).toBeNull();
  });
  it("opens web links through IPC, but points at the source line when armed", async () => {
    const text = "# Install\n\nRun the `installer`.\nSee [the docs](https://example.test/docs).\n\n```sh\nfirst command\nsecond command\n```\n\n| Field | Value |\n|---|---|\n| status | ready |";
    const calls = backend({ kind: "builtin", viewer: "markdown" }, { "": text }, context({ targetKind: "file", type: "markdown", title: "Markdown" }));
    render(ViewerWindow, { session: 2 });
    expect((await screen.findByRole("heading", { name: "Install" })).tagName).toBe("H1");
    const docs = screen.getByRole("link", { name: "the docs" });
    await fireEvent.click(docs);
    expect(calls.filter((each) => each.cmd === "chat_open_link").map((each) => each.args)).toEqual([{ url: "https://example.test/docs" }]);
    for (const target of [docs, screen.getByText("second command"), screen.getByRole("cell", { name: "ready" })]) {
      await fireEvent.click(screen.getByRole("button", { name: "Point" }));
      await fireEvent.click(target);
    }
    expect(pointed(calls)).toEqual(["L4:C1", "L8:C1", "L13:C1"]);
    expect(calls.filter((each) => each.cmd === "chat_open_link")).toHaveLength(1);
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
    const strip = screen.getByRole("toolbar", { name: "Graph tools" });
    expect(within(strip).getByRole("button", { name: "Zoom out" }).hasAttribute("disabled")).toBe(false);
    expect(within(strip).getByRole("button", { name: "Zoom in" }).hasAttribute("disabled")).toBe(false);
    expect(within(strip).getByRole("button", { name: "Fit" }).hasAttribute("disabled")).toBe(false);
    expect(within(strip).queryByRole("button", { name: /^Find/ })).toBeNull();
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
    // Until the page reaches the bridge, the strip says it is loading.
    expect((await screen.findByRole("status")).textContent?.trim()).toBe("Loading HTML Viewer…");
    const reply = vi.spyOn(frame.contentWindow!, "postMessage");
    // The strip's Point tells the page to point; Esc tells it to stop.
    await fireEvent.click(await screen.findByRole("button", { name: "Point" }));
    expect(reply).toHaveBeenLastCalledWith({ channel: "artifactize-viewer/v1", event: "point", armed: true }, "*");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(reply).toHaveBeenLastCalledWith({ channel: "artifactize-viewer/v1", event: "point", armed: false }, "*");
    reply.mockClear();
    const ask = { channel: "artifactize-viewer/v1", id: 1, method: "readText", args: { path: "index.html" } };
    // Another window's message is not answered.
    window.dispatchEvent(new MessageEvent("message", { data: ask, source: window }));
    window.dispatchEvent(new MessageEvent("message", { data: ask, source: frame.contentWindow }));
    await waitFor(() => expect(reply).toHaveBeenCalledWith({ channel: "artifactize-viewer/v1", id: 1, result: "<h1>Hi</h1>" }, "*"));
    expect(reply).toHaveBeenCalledTimes(1);
    expect(calls.filter((call) => call.cmd === "viewer_read_text")).toHaveLength(1);
    // Connected, the strip says nothing more of the page until the page reports its status,
    // each line replacing the last.
    expect(screen.queryByRole("status")).toBeNull();
    for (const [id, text] of [[3, "Loading checkout/index.html"], [4, "checkout/index.html · Checkout"]] as const) {
      window.dispatchEvent(new MessageEvent("message", { data: { channel: "artifactize-viewer/v1", id, method: "status", args: { text } }, source: frame.contentWindow }));
      await waitFor(() => expect(screen.getByRole("status").textContent?.trim()).toBe(text));
      expect(reply).toHaveBeenCalledWith({ channel: "artifactize-viewer/v1", id, result: null }, "*");
    }
    reply.mockClear();
    window.dispatchEvent(new MessageEvent("message", { data: { ...ask, id: 2, method: "invoke" }, source: frame.contentWindow }));
    await waitFor(() => expect(reply).toHaveBeenCalledWith({ channel: "artifactize-viewer/v1", id: 2, error: "The viewer bridge has no invoke." }, "*"));
  });

  it("says the page did not connect when it finished loading without reaching the bridge, until it does", async () => {
    mockConvertFileSrc("linux");
    backend({ kind: "web", html: "<p>page</p>" }, {}, context({ type: "html", title: "HTML Viewer" }));
    const { container } = render(ViewerWindow, { session: 5 });
    const frame = await waitFor(() => {
      const found = container.querySelector("iframe");
      expect(found).not.toBeNull();
      return found!;
    });
    frame.dispatchEvent(new Event("load"));
    await waitFor(() => expect(screen.getByRole("status").textContent?.trim()).toBe("HTML Viewer did not connect: its page never asked the viewer for its context."));
    window.dispatchEvent(new MessageEvent("message", { data: { channel: "artifactize-viewer/v1", id: 1, method: "context" }, source: frame.contentWindow }));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });
});
