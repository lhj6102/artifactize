// The standalone review against a mocked command boundary: the request search, a result saved
// locally whose publication failed stays open with its failure, a replaced state drops the open
// review, and a request that is not a Human review opens read-only.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import ReviewScreen from "./ReviewScreen.svelte";
import { greenForm, humanView } from "./fixtures";
import type { DetailView, LaunchView, Listed, Opened } from "../ipc/client";

const WAITING = "kind:human status:waiting_human";
const launch: LaunchView = { mode: "review", state: "/state", reviewer: "alice", search: WAITING, request: null };

const row: Listed = {
  id: "run-a-2",
  eval: "release/signoff",
  title: "A person approves the release",
  kind: "human",
  mark: { status: "WAITING_HUMAN", glyph: "?" },
  claim: "alice (you)",
  mine: true,
  age: "3s",
  repository: "/repo",
};

const agentRow: Listed = {
  ...row,
  id: "run-a-3",
  eval: "app/agent",
  title: "An Agent reads the input",
  kind: "agent",
  mark: { status: "GREEN", glyph: "✓" },
  claim: "-",
  mine: false,
};

const agentDetail: DetailView = {
  run: "run-a",
  node: "eval:app/agent",
  target: "eval",
  title: "app/agent · An Agent reads the input",
  summary: "GREEN · the input is complete",
  request: "run-a-3",
  profile: "agent",
  mark: { status: "GREEN", glyph: "✓" },
  running: false,
  sections: [{ name: "outcome", title: "Outcome", fields: [{ key: "Verdict", value: "GREEN" }], waits: [] }],
  evidence: null,
  card: null,
};

function delayedRows() {
  let resolve!: (rows: Listed[]) => void;
  const promise = new Promise<Listed[]>((done) => { resolve = done; });
  return { promise, resolve };
}

function backend(options: {
  generation: () => number;
  rows: (query: string) => Listed[] | Promise<Listed[]>;
  searched?: string[];
}) {
  mockIPC(
    async (cmd, args) => {
      const payload = args as Record<string, unknown>;
      switch (cmd) {
        case "review_search": {
          const query = payload.query as string;
          options.searched?.push(query);
          return {
            generation: options.generation(),
            rows: await options.rows(query),
            next: null,
            error: query.includes("kind:robot") ? "Unknown kind `robot`" : null,
            waiting: query.includes(WAITING),
          };
        }
        case "review_complete":
          return { keep: "", candidates: [] };
        case "review_detail":
          return payload.id === row.id ? { ...agentDetail, profile: "human", request: row.id } : agentDetail;
        case "transcript_open":
          return { kind: "unavailable", text: "No conversation was saved." } satisfies Opened;
        case "human_open":
          return humanView("review");
        case "human_form":
          return greenForm;
        case "human_submit":
          return {
            settled: "savedLocally",
            notice: { text: "The Human result is saved locally, but publishing it failed: 403", error: true },
            view: humanView("completed", { result: "GREEN\n{}", meta: "GREEN" }),
          };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}

beforeAll(() => {
  // jsdom lays nothing out; rows only ask to be scrolled into view, and a transcript only
  // observes its size.
  Element.prototype.scrollIntoView ??= () => {};
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.useRealTimers();
});

async function openReview() {
  renderHosted(ReviewScreen, { launch, active: true });
  await screen.findByText("release/signoff");
  await fireEvent.dblClick(await screen.findByText("release/signoff"));
  return screen.findByLabelText("Human review");
}

describe("ReviewScreen", () => {
  it("keeps a result saved locally open with its publication failure", async () => {
    let rows = [row];
    backend({ generation: () => 0, rows: () => rows });
    const region = await openReview();
    await screen.findByText("REVIEW (yours)");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    rows = [];
    await fireEvent.keyDown(comment, { key: "s", ctrlKey: true });
    await screen.findByText(/publishing it failed/);
    await screen.findByText(/completed result/i);
    expect(screen.queryByLabelText("Human review")).not.toBeNull();
  });

  it("closes the open review when the state is replaced", async () => {
    let generation = 0;
    backend({ generation: () => generation, rows: () => [row] });
    await openReview();
    generation = 1;
    await emit("state-changed", { resync: false });
    await waitFor(() => expect(screen.queryByLabelText("Human review")).toBeNull());
  });

  it("searches with the typed query, says why an invalid one lists nothing, and starts on waiting reviews", async () => {
    const searched: string[] = [];
    backend({
      generation: () => 0,
      rows: (query) => (query.includes("kind:agent") ? [agentRow] : query.includes(WAITING) ? [row] : []),
      searched,
    });
    renderHosted(ReviewScreen, { launch, active: true });
    await screen.findByText("release/signoff");
    expect(searched[0]).toBe(WAITING);
    expect(screen.getByText(/Waiting Human reviews \(1\)/)).not.toBeNull();
    const field = screen.getByLabelText<HTMLInputElement>("Search requests");
    expect(field.value).toBe(WAITING);
    await fireEvent.input(field, { target: { value: "kind:agent" } });
    await screen.findByText("app/agent");
    expect(screen.getByText(/Requests \(1\)/)).not.toBeNull();
    expect(screen.queryByText("release/signoff")).toBeNull();
    await fireEvent.input(field, { target: { value: "kind:robot" } });
    await screen.findByText("Unknown kind `robot`");
  });

  it.each(["Enter", "Escape"])("%s waits for an already dispatched search before focusing its list", async (key) => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    const answer = delayedRows();
    const searched: string[] = [];
    backend({ generation: () => 0, rows: (query) => query === "" ? answer.promise : [], searched });
    renderHosted(ReviewScreen, { launch, active: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.getByText("No Human reviews are waiting.")).not.toBeNull();
    const field = screen.getByLabelText<HTMLInputElement>("Search requests");
    field.focus();
    await fireEvent.keyDown(field, { key: "Escape" });
    expect(field.value).toBe("");
    // Dispatch the debounced query, but hold its native answer until after leaving the field.
    await vi.advanceTimersByTimeAsync(120);
    expect(searched).toEqual([WAITING, ""]);
    await fireEvent.keyDown(field, { key });
    expect(document.activeElement).toBe(field);
    answer.resolve([row]);
    await vi.advanceTimersByTimeAsync(0);
    await tick();
    expect(document.activeElement).toBe(screen.getByRole("listbox", { name: "Requests" }));
  });

  it("waits for a refresh that supersedes the search before focusing the new list", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    const search = delayedRows();
    const refresh = delayedRows();
    const searched: string[] = [];
    let requests = 0;
    backend({
      generation: () => 0,
      rows: (query) => query === "" ? (++requests === 1 ? search.promise : refresh.promise) : [],
      searched,
    });
    renderHosted(ReviewScreen, { launch, active: true });
    await vi.advanceTimersByTimeAsync(0);
    const field = screen.getByLabelText<HTMLInputElement>("Search requests");
    field.focus();
    await fireEvent.keyDown(field, { key: "Escape" });
    await vi.advanceTimersByTimeAsync(120);
    await fireEvent.keyDown(field, { key: "Enter" });
    await emit("state-changed", { resync: false });
    expect(searched).toEqual([WAITING, "", ""]);
    // The superseded search cannot satisfy Enter: only the newer answer will be displayed.
    search.resolve([agentRow]);
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.queryByRole("listbox", { name: "Requests" })).toBeNull();
    expect(document.activeElement).toBe(field);
    refresh.resolve([row]);
    await vi.advanceTimersByTimeAsync(0);
    await tick();
    expect(document.activeElement).toBe(screen.getByRole("listbox", { name: "Requests" }));
    expect(screen.queryByText("app/agent")).toBeNull();
  });

  it("Enter waits for the initial query even before its first answer", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    const answer = delayedRows();
    backend({ generation: () => 0, rows: () => answer.promise });
    renderHosted(ReviewScreen, { launch, active: true });
    await vi.advanceTimersByTimeAsync(0);
    const field = screen.getByLabelText<HTMLInputElement>("Search requests");
    field.focus();
    await fireEvent.keyDown(field, { key: "Enter" });
    answer.resolve([row]);
    await vi.advanceTimersByTimeAsync(0);
    await tick();
    expect(document.activeElement).toBe(screen.getByRole("listbox", { name: "Requests" }));
  });

  it("opens a request that is not a Human review read-only", async () => {
    backend({ generation: () => 0, rows: () => [agentRow] });
    renderHosted(ReviewScreen, { launch: { ...launch, search: "kind:agent" }, active: true });
    await fireEvent.dblClick(await screen.findByText("app/agent"));
    await screen.findByText(/An Agent reads the input · read-only/);
    expect(screen.queryByLabelText("Human review")).toBeNull();
    // Its Summary first; the Agent log is the other choice of the switch below the title.
    const views = within(screen.getByRole("tablist", { name: "Views" })).getAllByRole("tab");
    expect(views.map((view) => [view.textContent, view.getAttribute("aria-selected")])).toEqual([["Summary", "true"], ["Agent log", "false"]]);
    await screen.findByText("Verdict");
    expect(screen.queryByRole("log", { name: "Agent transcript" })).toBeNull();
    await fireEvent.click(views[1]);
    await screen.findByText("No conversation was saved.");
    expect(screen.queryByText("Verdict")).toBeNull();
    await fireEvent.keyDown(screen.getByRole("document"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByText(/· read-only/)).toBeNull());
  });
});
