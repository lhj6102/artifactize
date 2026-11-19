// An Artifact's Detail against a mocked command boundary: its tabs (the Artifact icon and name, a `/`, then one
// per eval with its verdict's glyph and colour), the Artifact tab's eval list that switches to an
// eval's tab, each tab's own backend Detail, and Close and Esc.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, beforeAll, expect, it, vi } from "vitest";
import type { DetailView, Source, TreeNode } from "../ipc/client";
import DetailPane from "./DetailPane.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "../review/review.svelte";
import { humanView } from "../review/fixtures";
import { branch } from "./fixtures";

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

// Each test starts with the sections folded as a first launch has them.
beforeEach(() => localStorage.clear());

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

const RUN: Source = { kind: "run", run: "run-a" };
/** The backend's row of `node` in `rows`, as `detail_row` answers. */
const rowOf = (args: unknown, rows = tree) => rows.find((each) => each.id === (args as { node: string }).node) ?? null;

const view = (node: string, title: string): DetailView => ({
  source: RUN, plan: null, node, target: node.startsWith("e:") ? "eval" : "artifact", title, summary: `${title} summary`, request: null, profile: node.startsWith("e:") ? "runtime" : null,
  mark: null, sections: [], evidence: null, running: false, card: null, outcome: null,
});

function setup(tab: string | null = null, drawn: { wholes: string[]; copies: number } = { wholes: [], copies: 1 }) {
  const read: string[] = [];
  mockIPC((command, args) => {
    if (command === "detail_row") return rowOf(args);
    if (command === "neighbours") {
      // The backend cut two more Artifacts that use `app`.
      return { moreUses: 0, moreUsedBy: 2, ...drawn, shape: {
        nodes: [
          { id: "a:core", name: "core", light: "green", tally: null, note: null, parts: [] },
          { id: "a:app", name: "app", light: "red", tally: null, note: null, parts: [] },
          { id: "a:docs", name: "docs", light: "green", tally: null, note: null, parts: [] },
          { id: "a:far", name: "far", light: "green", tally: null, note: null, parts: [] },
        ],
        edges: [
          { source: "a:core", target: "a:app", light: "green" },
          { source: "a:app", target: "a:docs", light: "red" },
          { source: "a:docs", target: "a:far", light: "green" },
        ],
      } };
    }
    if (command !== "detail") return null;
    const { node } = args as { node: string };
    read.push(node);
    return view(node, node === "a:app" ? "Artifact app" : `app/${node.slice("e:app/".length)} · Check`);
  });
  const ontab = vi.fn();
  const onopen = vi.fn();
  const onback = vi.fn();
  const ongone = vi.fn();
  const shown = renderHosted(DetailPane, {
    source: RUN, revision: 1, branch: null, node: "a:app", tab, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab, onopen, onback, ongone,
  });
  return { ...shown, read, ontab, onopen, onback, ongone };
}

it("shows the Artifact tab first, without chips, with its evals' verdicts, and switches to an eval's tab from the list or the chips", async () => {
  const { read, ontab, rerender } = setup();
  await screen.findByRole("heading", { name: "Artifact app" });
  // The Artifact's own tab has no chips: its Evals rows lead to each eval.
  expect(screen.queryByRole("tablist", { name: "Tabs of app" })).toBeNull();
  const evals = within(screen.getByRole("list", { name: "Evals of app" })).getAllByRole("button");
  expect(evals.map((item) => item.querySelector(".name")?.textContent)).toEqual(["lint", "test"]);
  expect(evals[1].querySelector(".status .tone-red")?.textContent).toBe("RED");
  // Each row leads with its verdict's icon in its verdict's colour.
  expect(evals[1].querySelector(".glyph.tone-red svg")).not.toBeNull();
  expect(read).toEqual(["a:app"]);
  // Sections under header rows: Evals, Tools and Path unfolded, Neighbours and Debug folded.
  expect(screen.getByRole("button", { name: "Evals" }).getAttribute("aria-expanded")).toBe("true");
  expect((await screen.findByRole("button", { name: "Neighbours" })).getAttribute("aria-expanded")).toBe("false");

  await fireEvent.click(evals[1]);
  expect(ontab).toHaveBeenLastCalledWith("e:app/test");

  // An eval's tab reads that eval's own Detail under chips that read as a path.
  await rerender({ tab: "e:app/test" });
  await screen.findByRole("heading", { name: "app/test · Check" });
  expect(read).toEqual(["a:app", "e:app/test"]);
  expect(screen.queryByRole("list", { name: "Evals of app" })).toBeNull();
  const tabs = within(screen.getByRole("tablist", { name: "Tabs of app" })).getAllByRole("tab");
  expect(tabs.map((tab) => tab.textContent?.trim())).toEqual(["app", "lint", "test"]);
  expect(screen.getByRole("tablist").textContent?.replace(/\s+/g, " ").trim()).toBe("app / linttest");
  // The Artifact's chip shows the Artifact icon before its name and is read as "Artifact app".
  expect(tabs[0].getAttribute("aria-label")).toBe("Artifact app");
  expect(tabs[0].querySelector("img")?.getAttribute("src")).toMatch(/artifactize-icon-mark/);
  // Each eval's chip carries its verdict's icon in its verdict's colour.
  expect(tabs[2].querySelector(".tone-red svg")).not.toBeNull();
  expect(tabs[1].querySelector(".tone-green svg")).not.toBeNull();
  expect(within(screen.getByRole("tablist")).getByRole("tab", { selected: true }).textContent?.trim()).toBe("test");
  await fireEvent.click(tabs[1]);
  expect(ontab).toHaveBeenLastCalledWith("e:app/lint");
  await fireEvent.click(tabs[0]);
  expect(ontab).toHaveBeenLastCalledWith(null);
});

it("stars the Artifact with the star beside its title, and takes the star away; a failed star goes back", async () => {
  const shown = branch("main", []);
  let kept = ["docs"];
  let refuse = false;
  const asked: unknown[] = [];
  mockIPC((command, args) => {
    if (command === "detail_row") return rowOf(args);
    if (command === "detail") return view((args as { node: string }).node, "Artifact app");
    if (command === "starred_artifacts") return kept;
    if (command === "star_artifact") {
      asked.push(args);
      if (refuse) throw { kind: "setting", message: "the state directory is read-only" };
      const { artifact, on } = args as { artifact: string; on: boolean };
      kept = on ? [...kept, artifact] : kept.filter((name) => name !== artifact);
      return kept;
    }
    return null;
  });
  renderHosted(DetailPane, {
    source: RUN, revision: 1, branch: shown, node: "a:app", tab: null, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onopen: vi.fn(), onback: vi.fn(), ongone: vi.fn(),
  });
  await screen.findByRole("heading", { name: "Artifact app" });
  const star = await screen.findByRole("button", { name: "Star app" });
  expect(star.getAttribute("aria-pressed")).toBe("false");
  expect(star.classList.contains("on")).toBe(false);
  await fireEvent.click(star);
  await waitFor(() => expect(star.getAttribute("aria-pressed")).toBe("true"));
  expect(star.classList.contains("on")).toBe(true);
  await fireEvent.click(star);
  await waitFor(() => expect(star.getAttribute("aria-pressed")).toBe("false"));
  expect(asked).toEqual([
    { branch: shown.key, artifact: "app", on: true },
    { branch: shown.key, artifact: "app", on: false },
  ]);
  // Not kept: the star goes back, and the Detail says why.
  refuse = true;
  await fireEvent.click(star);
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("the state directory is read-only"));
  expect(star.getAttribute("aria-pressed")).toBe("false");
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
    if (command === "detail_row") return rowOf(args);
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
    source: RUN, revision: 1, branch: null, node: "a:app", tab: "e:app/test", focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(), ongone: vi.fn(),
  });
  const views = within(await screen.findByRole("tablist", { name: "Views" })).getAllByRole("tab");
  expect(views.map((item) => [item.textContent, item.getAttribute("aria-selected")])).toEqual([["Summary", "true"], ["Agent log", "false"]]);
  await screen.findByRole("button", { name: "Outcome" });
  expect(screen.queryByRole("log", { name: "Agent transcript" })).toBeNull();
  // The window-level control is Close alone, without a key cap: the views are a level below
  // the tabs.
  const header = screen.getByRole("heading", { name: "app/test · Review" }).parentElement!;
  expect(within(header).getAllByRole("button").map((button) => button.textContent?.trim())).toEqual(["Close"]);
  await fireEvent.click(views[1]);
  await screen.findByRole("log", { name: "Agent transcript" });
  expect(screen.queryByRole("button", { name: "Outcome" })).toBeNull();
  await waitFor(() => expect(opened).toEqual(["run-a-7"]));
});

it("keeps Close alone in the header on a Human review's tab, the review showing none of its own", async () => {
  mockIPC((command, args) => {
    if (command === "detail_row") return rowOf(args);
    if (command === "detail") return { ...view("e:app/test", "app/test · Sign-off"), profile: "human", request: "run-a-2" };
    return command === "human_open" ? humanView("review") : null;
  }, { shouldMockEvents: true });
  const reviewing = new Review("run-a-2", new Jobs());
  const onback = vi.fn();
  renderHosted(DetailPane, {
    source: RUN, revision: 1, branch: null, node: "a:app", tab: "e:app/test", focused: false, active: true,
    review: () => reviewing, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onopen: vi.fn(), onback, ongone: vi.fn(),
  });
  await screen.findByText("REVIEW (yours)");
  const header = screen.getByRole("heading", { name: "app/test · Sign-off" }).parentElement!;
  expect(within(header).getAllByRole("button").map((button) => button.textContent?.trim())).toEqual(["Close"]);
  expect(screen.getAllByRole("button", { name: /Close/ })).toHaveLength(1);
  await fireEvent.click(within(header).getByRole("button", { name: /Close/ }));
  expect(onback).toHaveBeenCalledOnce();
});

it("shows an Artifact without evals or neighbours with no chips, no Evals and no Neighbours", async () => {
  const neighbours = vi.fn();
  mockIPC((command, args) => {
    if (command === "detail_row") return rowOf(args, [row("a:lone", "artifact", "lone", "✓", "green")]);
    if (command === "neighbours") {
      neighbours();
      // Drawn once, using nothing and used by nothing.
      return { moreUses: 0, moreUsedBy: 0, wholes: [], copies: 1, shape: {
        nodes: [{ id: "a:lone", name: "lone", light: "green", tally: null, note: null, parts: [] }], edges: [],
      } };
    }
    return command === "detail" ? view("a:lone", "Artifact lone") : null;
  });
  renderHosted(DetailPane, {
    source: RUN, revision: 1, branch: null, node: "a:lone", tab: null, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(), ongone: vi.fn(),
  });
  await screen.findByRole("heading", { name: "Artifact lone" });
  expect(screen.queryByRole("tablist")).toBeNull();
  expect(screen.queryByRole("list", { name: "Evals of lone" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Evals" })).toBeNull();
  // A section with nothing to show is not shown, once its neighbours are read.
  await waitFor(() => expect(neighbours).toHaveBeenCalled());
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(screen.queryByRole("button", { name: "Neighbours" })).toBeNull();
});

it("draws the Artifact between the Artifacts its evals use and those that use it once Neighbours is unfolded, and opens a neighbour on a click", async () => {
  const { onopen } = setup();
  await fireEvent.click(await screen.findByRole("button", { name: "Neighbours" }));
  const group = await screen.findByRole("group", { name: "Neighbours" });
  const node = (id: string) => group.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);
  await waitFor(() => expect(node("a:app")).not.toBeNull());
  // Only the Artifact's own neighbours, with an edge into it and one out of it.
  expect(node("a:core")).not.toBeNull();
  expect(node("a:docs")).not.toBeNull();
  expect(node("a:far")).toBeNull();
  expect(screen.getByText("And 2 more that use it, not shown.")).toBeTruthy();
  await fireEvent.click(node("a:docs")!);
  expect(onopen).toHaveBeenCalledWith("a:docs");
  await fireEvent.click(node("a:app")!);
  expect(onopen).toHaveBeenCalledTimes(1);
  // Drawn once, it says nothing of where.
  expect(screen.queryByText(/^Drawn /)).toBeNull();
});

it("folds an eval Summary's technical facts under Debug, which t shows and hides", async () => {
  mockIPC((command, args) =>
    command === "detail_row"
      ? rowOf(args)
      : command === "detail"
      ? {
          ...view("e:app/lint", "app/lint · Check"),
          sections: [{ name: "request", title: "Request", fields: [{ key: "Status", value: "GREEN" }, { key: "Request", value: "run-a-3" }], waits: [] }],
          card: { instruction: "Check lint.", result: null, error: null, exitCode: 0, reused: false, took: "2s", tokens: null, model: null },
        }
      : null,
  );
  renderHosted(DetailPane, {
    source: RUN, revision: 1, branch: null, node: "a:app", tab: "e:app/lint", focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(), ongone: vi.fn(),
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

it("lets go of a Detail whose node the Run no longer has", async () => {
  mockIPC((command, args) => {
    if (command === "detail_row") return rowOf(args, []);
    return command === "detail" ? view("a:app", "Artifact app") : null;
  });
  const ongone = vi.fn();
  renderHosted(DetailPane, {
    source: RUN, revision: 1, branch: null, node: "a:app", tab: null, focused: false, active: true,
    review: () => undefined, openReview: vi.fn(), onfocus: vi.fn(), ontab: vi.fn(), onback: vi.fn(), ongone,
  });
  await waitFor(() => expect(ongone).toHaveBeenCalledOnce());
});

it("names, under Debug, every whole a part of several wholes is drawn in", async () => {
  setup(null, { wholes: ["kit", "site"], copies: 2 });
  await fireEvent.click(await screen.findByRole("button", { name: "Debug" }));
  const drawn = await screen.findByText("Drawn in");
  expect(drawn.nextElementSibling?.textContent).toBe("kit, site");
});
