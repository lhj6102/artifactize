// An Artifact's Detail against a mocked command boundary: its tabs (`Artf:{name}`, a `/`, then one
// per eval with its verdict's glyph and colour), the Artifact tab's eval list that switches to an
// eval's tab, each tab's own backend Detail, and Close and Esc.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, expect, it, vi } from "vitest";
import type { DetailView, TreeNode } from "../ipc/client";
import DetailPane from "./DetailPane.svelte";

afterEach(() => {
  cleanup();
  clearMocks();
});

const row = (id: string, kind: TreeNode["kind"], name: string, glyph: string, tone: TreeNode["tone"], children: TreeNode[] = []): TreeNode => ({
  id, kind, name, glyph, tone, weight: "normal", marks: "", text: [{ text: tone === "red" ? "RED" : "GREEN", tone }],
  right: "", changed: false, done: false, attention: false, urgent: false, upstream: [], children,
});

const tree: TreeNode[] = [
  row("a:app", "artifact", "app", "✗", "red", [row("e:app/lint", "eval", "lint", "✓", "green"), row("e:app/test", "eval", "test", "✗", "red")]),
];

const view = (node: string, title: string): DetailView => ({
  run: "run-a", node, title, summary: `${title} summary`, request: null, profile: node.startsWith("e:") ? "runtime" : null,
  mark: null, sections: [], evidence: null, running: false,
});

function setup(tab: string | null = null) {
  const read: string[] = [];
  mockIPC((command, args) => {
    if (command !== "detail") return null;
    const { node } = args as { node: string };
    read.push(node);
    return view(node, node === "a:app" ? "Artifact app" : `app/${node.slice("e:app/".length)} · Check`);
  });
  const ontab = vi.fn();
  const onback = vi.fn();
  const shown = renderHosted(DetailPane, {
    run: "run-a", node: "a:app", tab, tree, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab, onback,
  });
  return { ...shown, read, ontab, onback };
}

it("shows the Artifact tab first with its evals' verdicts, and switches to an eval's tab from the list or the tabs", async () => {
  const { read, ontab, rerender } = setup();
  await screen.findByRole("heading", { name: "Artifact app" });
  const tabs = within(screen.getByRole("tablist", { name: "Tabs of app" })).getAllByRole("tab");
  // It reads as a path: the Artifact's chip, a `/`, then the evals' chips.
  expect(tabs.map((tab) => tab.textContent?.trim())).toEqual(["Artf:app", "✓lint", "✗test"]);
  expect(screen.getByRole("tablist").textContent?.replace(/\s+/g, " ").trim()).toBe("Artf:app / ✓lint✗test");
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
  expect(within(strip).getAllByRole("tab").map((tab) => tab.textContent?.trim())).toEqual(["Artf:lone"]);
  expect(strip.textContent?.trim()).toBe("Artf:lone");
});
