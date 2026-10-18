// An Artifact's Detail against a mocked command boundary: its tabs (the Artifact icon and name, a `/`, then one
// per eval with its verdict's glyph and colour), the Artifact tab's eval list that switches to an
// eval's tab, each tab's own backend Detail, and Close and Esc.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { DetailView, TreeNode } from "../ipc/client";
import DetailPane from "./DetailPane.svelte";

beforeAll(() => {
  // jsdom lays nothing out; the neighbours' small graph only observes sizes and reads transforms.
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

const row = (id: string, kind: TreeNode["kind"], name: string, glyph: string, tone: TreeNode["tone"], children: TreeNode[] = []): TreeNode => ({
  id, kind, name, glyph, tone, weight: "normal", marks: "", text: [{ text: tone === "red" ? "RED" : "GREEN", tone }],
  right: "", changed: false, done: false, attention: false, urgent: false, upstream: [], children,
});

// `app`'s evals use `core`; `docs` uses `app`.
const tree: TreeNode[] = [
  row("a:core", "artifact", "core", "✓", "green"),
  {
    ...row("a:app", "artifact", "app", "✗", "red", [row("e:app/lint", "eval", "lint", "✓", "green"), row("e:app/test", "eval", "test", "✗", "red")]),
    upstream: [{ node: "a:core", complete: true }],
  },
  { ...row("a:docs", "artifact", "docs", "✓", "green"), upstream: [{ node: "a:app", complete: false }] },
];

const view = (node: string, title: string): DetailView => ({
  run: "run-a", node, target: node.startsWith("e:") ? "eval" : "artifact", title, summary: `${title} summary`, request: null, profile: node.startsWith("e:") ? "runtime" : null,
  mark: null, sections: [], evidence: null, running: false, card: null,
});

function setup(tab: string | null = null) {
  const read: string[] = [];
  mockIPC((command, args) => {
    if (command === "run_graph") {
      return {
        run: "run-a",
        nodes: [
          { id: "a:core", name: "core", light: "green" },
          { id: "a:app", name: "app", light: "red" },
          { id: "a:docs", name: "docs", light: "green" },
          { id: "a:far", name: "far", light: "green" },
        ],
        edges: [
          { source: "a:core", target: "a:app", light: "green" },
          { source: "a:app", target: "a:docs", light: "red" },
          { source: "a:docs", target: "a:far", light: "green" },
        ],
      };
    }
    if (command !== "detail") return null;
    const { node } = args as { node: string };
    read.push(node);
    return view(node, node === "a:app" ? "Artifact app" : `app/${node.slice("e:app/".length)} · Check`);
  });
  const ontab = vi.fn();
  const onopen = vi.fn();
  const onback = vi.fn();
  const shown = renderHosted(DetailPane, {
    run: "run-a", node: "a:app", tab, tree, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab, onopen, onback,
  });
  return { ...shown, read, ontab, onopen, onback };
}

it("shows the Artifact tab first with its evals' verdicts, and switches to an eval's tab from the list or the tabs", async () => {
  const { read, ontab, rerender } = setup();
  await screen.findByRole("heading", { name: "Artifact app" });
  const tabs = within(screen.getByRole("tablist", { name: "Tabs of app" })).getAllByRole("tab");
  // It reads as a path: the Artifact's chip, a `/`, then the evals' chips.
  expect(tabs.map((tab) => tab.textContent?.trim())).toEqual(["app", "✓lint", "✗test"]);
  expect(screen.getByRole("tablist").textContent?.replace(/\s+/g, " ").trim()).toBe("app / ✓lint✗test");
  // The Artifact's chip shows the Artifact icon before its name and is read as "Artifact app".
  expect(tabs[0].getAttribute("aria-label")).toBe("Artifact app");
  expect(tabs[0].querySelector("img")?.getAttribute("src")).toMatch(/artifactize-icon-mark/);
  expect(tabs[0].getAttribute("aria-selected")).toBe("true");
  // Each eval's tab carries its verdict's glyph in its verdict's colour.
  expect(tabs[2].querySelector(".tone-red")?.textContent).toBe("✗");
  expect(tabs[1].querySelector(".tone-green")?.textContent).toBe("✓");
  const evals = within(screen.getByRole("list", { name: "Evals of app" })).getAllByRole("button");
  expect(evals.map((item) => item.querySelector(".name")?.textContent)).toEqual(["lint", "test"]);
  expect(evals[1].querySelector(".status .tone-red")?.textContent).toBe("RED");
  expect(read).toEqual(["a:app"]);

  await fireEvent.click(evals[1]);
  expect(ontab).toHaveBeenLastCalledWith("e:app/test");
  await fireEvent.click(tabs[1]);
  expect(ontab).toHaveBeenLastCalledWith("e:app/lint");

  // The shown tab reads that eval's own Detail, and the list belongs to the Artifact tab only.
  await rerender({ tab: "e:app/test" });
  await screen.findByRole("heading", { name: "app/test · Check" });
  expect(read).toEqual(["a:app", "e:app/test"]);
  expect(screen.queryByRole("list", { name: "Evals of app" })).toBeNull();
  expect(within(screen.getByRole("tablist")).getByRole("tab", { selected: true }).textContent?.trim()).toBe("✗test");
});

it("closes with Close and with Esc", async () => {
  const { onback } = setup("e:app/lint");
  await screen.findByRole("heading", { name: "app/lint · Check" });
  await fireEvent.click(screen.getByRole("button", { name: /Close/ }));
  expect(onback).toHaveBeenCalledOnce();
  await fireEvent.keyDown(screen.getByRole("heading", { name: "app/lint · Check" }), { key: "Escape" });
  await waitFor(() => expect(onback).toHaveBeenCalledTimes(2));
});

it("shows an Agent eval's Summary first and its Agent log on the switch below the tabs; Close stays alone", async () => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
  const opened: string[] = [];
  mockIPC((command, args) => {
    if (command === "detail") {
      return {
        ...view("e:app/test", "app/test · Review"), profile: "agent", request: "run-a-7",
        sections: [{ name: "outcome", title: "Outcome", fields: [{ key: "Status", value: "RED" }], waits: [] }],
      };
    }
    if (command === "transcript_open") {
      opened.push((args as { id: string }).id);
      return { kind: "unavailable", text: "No conversation was saved." };
    }
    return null;
  }, { shouldMockEvents: true });
  renderHosted(DetailPane, {
    run: "run-a", node: "a:app", tab: "e:app/test", tree, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(),
  });
  const views = within(await screen.findByRole("tablist", { name: "Views" })).getAllByRole("tab");
  expect(views.map((item) => [item.textContent, item.getAttribute("aria-selected")])).toEqual([["Summary", "true"], ["Agent log", "false"]]);
  await screen.findByText("Outcome");
  expect(screen.queryByRole("log", { name: "Agent transcript" })).toBeNull();
  // The window-level control is Close alone: the views are a level below the tabs.
  const header = screen.getByRole("heading", { name: "app/test · Review" }).parentElement!;
  expect(within(header).getAllByRole("button").map((button) => button.textContent?.trim())).toEqual(["Close Esc"]);
  await fireEvent.click(views[1]);
  await screen.findByRole("log", { name: "Agent transcript" });
  expect(screen.queryByText("Outcome")).toBeNull();
  await waitFor(() => expect(opened).toEqual(["run-a-7"]));
});

it("shows only the Artifact's chip, without a `/`, for an Artifact without evals", async () => {
  mockIPC((command) => (command === "detail" ? view("a:lone", "Artifact lone") : null));
  renderHosted(DetailPane, {
    run: "run-a", node: "a:lone", tab: null, tree: [row("a:lone", "artifact", "lone", "✓", "green")], focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(),
  });
  await screen.findByRole("heading", { name: "Artifact lone" });
  const strip = screen.getByRole("tablist", { name: "Tabs of lone" });
  expect(within(strip).getAllByRole("tab").map((tab) => tab.getAttribute("aria-label"))).toEqual(["Artifact lone"]);
  expect(strip.textContent?.trim()).toBe("lone");
});

it("draws the Artifact between the Artifacts its evals use and those that use it, and opens a neighbour on a click", async () => {
  const { onopen } = setup();
  const group = await screen.findByRole("group", { name: "Neighbours" });
  const node = (id: string) => group.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);
  await waitFor(() => expect(node("a:app")).not.toBeNull());
  // Only the Artifact's own neighbours, with an edge into it and one out of it.
  expect(node("a:core")).not.toBeNull();
  expect(node("a:docs")).not.toBeNull();
  expect(node("a:far")).toBeNull();
  await fireEvent.click(node("a:docs")!);
  expect(onopen).toHaveBeenCalledWith("a:docs");
  await fireEvent.click(node("a:app")!);
  expect(onopen).toHaveBeenCalledTimes(1);
});

it("folds an eval Summary's technical facts under Debug, which t shows and hides", async () => {
  mockIPC((command) =>
    command === "detail"
      ? {
          ...view("e:app/lint", "app/lint · Check"),
          sections: [{ name: "request", title: "Request", fields: [{ key: "Status", value: "GREEN" }, { key: "Request", value: "run-a-3" }], waits: [] }],
          card: { instruction: "Check lint.", result: null, error: null, exitCode: 0, reused: false, took: "2s", tokens: null, model: null },
        }
      : null,
  );
  renderHosted(DetailPane, {
    run: "run-a", node: "a:app", tab: "e:app/lint", tree, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(),
  });
  const heading = await screen.findByRole("heading", { name: "app/lint · Check" });
  const debug = await screen.findByRole("button", { name: /Debug/ });
  expect(debug.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText("run-a-3")).toBeNull();
  await fireEvent.keyDown(heading, { key: "t" });
  await screen.findByText("run-a-3");
  expect(screen.getByRole("button", { name: /Debug/ }).getAttribute("aria-expanded")).toBe("true");
  await fireEvent.keyDown(heading, { key: "t" });
  await waitFor(() => expect(screen.queryByText("run-a-3")).toBeNull());
});
