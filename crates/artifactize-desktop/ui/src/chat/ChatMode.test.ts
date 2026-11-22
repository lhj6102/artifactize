// Chat against a mocked command boundary: the sessions grouped by project, a sent message and
// the turn the library streams (text, tool lines, a Run reference that opens the Run in Verify),
// Stop while a turn runs, and the model menu opening upward with the providers' models.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type { ChatBlock, ChatModel, ChatSession, ChatSessionView, ChatTurn, ChatView } from "../ipc/client";
import { renderHosted } from "../test-host";
import ChatMode from "./ChatMode.svelte";
import { branch, branchKey, browseView, runChip } from "../monitor/fixtures";

vi.mock("../monitor/layout", () => ({
  NODE: { width: 168, height: 44 },
  layout: async () => new Map(),
}));

beforeAll(() => {
  Element.prototype.scrollIntoView ??= () => {};
  window.matchMedia ??= ((query: string) => ({
    matches: false, media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

const session: ChatSession = {
  id: "chat-1",
  title: "Fix checkout rounding",
  worktree: "/work/app",
  project: "app",
  branch: "fix/rounding",
  branchKey: branchKey("fix/rounding"),
  view: "conversation",
  unavailable: null,
  selection: { backend: "claude-agent", model: "claude-opus-5-5", effort: null },
  createdAt: "2026-11-21T00:00:00Z",
  updatedAt: "2026-11-21T00:00:00Z",
  running: false,
  outcome: null,
  unseen: false,
};

/** The library of one session: `turns` is what it holds, `running` whether a turn runs. */
function backend() {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  const state = {
    turns: [] as ChatTurn[], running: false, session,
    other: null as ChatSession | null,
    send: null as ((text: string) => Promise<void>) | null,
    stop: null as (() => Promise<boolean>) | null,
    antigravitySignedIn: false,
  };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    switch (cmd) {
      case "chat_list":
        return (args as { query: string | null }).query === "no such chat" ? [] : [state.session, ...(state.other ? [state.other] : [])];
      case "chat_open":
        if (state.other && (args as { id: string }).id === state.other.id) return { session: state.other, running: false, turns: [] } satisfies ChatView;
        return { session: state.session, running: state.running, turns: state.turns } satisfies ChatView;
      case "chat_mark_seen":
        state.session = { ...state.session, unseen: false };
        return null;
      case "chat_set_model": {
        const chosen = (args as { selection: ChatModel }).selection;
        state.session = { ...state.session, selection: { ...chosen, effort: chosen.effort ?? "high" } };
        return state.session;
      }
      case "chat_set_view":
        state.session = { ...state.session, view: (args as { view: ChatSessionView }).view };
        return state.session;
      case "current_scan":
        return { branch: session.branchKey, scannedAt: 1, fingerprint: "", revision: 1, scanning: false, checking: 0, artifacts: 0, failure: null };
      case "graph":
        return { source: (args as { source: unknown }).source, shape: 1, update: { kind: "shape", nodes: [], edges: [] } };
      case "chat_send":
        if (state.send) return state.send((args as { text: string }).text);
        state.running = true;
        state.turns = [...state.turns, { user: (args as { text: string }).text, blocks: [], state: { kind: "running" } }];
        return null;
      case "chat_stop":
        if (state.stop) return state.stop();
        state.running = false;
        state.turns = state.turns.map((turn) => ({ ...turn, state: { kind: "stopped" } }));
        return true;
      case "browse":
        // Opened by its ID, run-9 is read with its live branch.
        return browseView(2, { run: { kind: "read", id: "run-9", origin: null, node: "r:run-9", mark: { status: "RED", glyph: "✗" }, needs: [], branch: branchKey("fix/rounding"), chatBranch: branchKey("fix/rounding") } });
      case "providers_list":
        return { builtin: ["claude-agent", "gemini", "anthropic", "antigravity"], keys: [
          { provider: "gemini", source: "absent" }, { provider: "anthropic", source: "saved" },
        ], custom: [] };
      case "claude_agent_status":
        return { state: "signedIn", method: "claude.ai", plan: null };
      case "antigravity_status":
        return { state: state.antigravitySignedIn ? "signedIn" : "signedOut" };
      case "models_list": {
        const { provider } = args as { provider: string };
        if (provider === "antigravity") return state.antigravitySignedIn
          ? { kind: "models", models: [{ slug: "gemini-demo", displayName: "Gemini Demo", efforts: [], defaultEffort: null }] }
          : { kind: "notSetUp", reason: "Not signed in." };
        if (provider === "gemini") return { kind: "notSetUp", reason: "No key is set." };
        if (provider === "anthropic") return { kind: "failed", error: "Model listing timed out." };
        return { kind: "models", models: [
          { slug: "claude-opus-5-5", displayName: "Opus 5.5", efforts: ["high", "low", "xhigh"], defaultEffort: "high" },
          { slug: "claude-sonnet-5-5", displayName: "Sonnet 5.5", efforts: ["high", "low"], defaultEffort: "high" },
          { slug: "quiet-id", displayName: "Quiet", efforts: [], defaultEffort: null },
        ] };
      }
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return { calls, state };
}

/** Render Chat with the live branch `app / fix/rounding` working in the session's worktree. */
function show() {
  const hosted = renderHosted(ChatMode);
  const main = { ...branch("fix/rounding", [runChip("run-9", "GREEN")]), workspace: "/work/app" };
  hosted.app.browse.view = browseView(1, { branches: [main] });
  // Chat is on screen: it follows the library's changes.
  hosted.app.mode = "chat";
  return hosted;
}

it("lists sessions by project, sends a message and shows the turn as the library streams it", async () => {
  const { calls, state } = backend();
  const { app } = show();
  // Grouped by the project and branch the library names.
  await screen.findByText("app / fix/rounding");
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const composer = await screen.findByLabelText<HTMLTextAreaElement>("Message");
  await fireEvent.input(composer, { target: { value: "Fix it and verify." } });
  await fireEvent.keyDown(composer, { key: "Enter" });
  await waitFor(() => expect(calls.find((call) => call.cmd === "chat_send")?.args).toMatchObject({ id: "chat-1", text: "Fix it and verify.", selection: null }));
  // The turn runs: Send is Stop.
  await screen.findByRole("button", { name: "Stop (Esc)" });
  expect(composer.value).toBe("");
  state.turns = [{
    user: "Fix it and verify.",
    blocks: [
      { kind: "text", text: "Looking at the total." },
      { kind: "tool", name: "edit", arguments: JSON.stringify({ path: "checkout/total.ts", old_text: "a", new_text: "b\nc" }), output: "ok", failed: false, toolKind: "edit", summary: "edit checkout/total.ts +2 −1", exitCode: null, omittedLines: 0, outputTruncated: false },
      { kind: "run", card: { id: "run-9", state: "red", failed: ["checkout/unit"], waiting: [] } },
    ],
    state: { kind: "completed" },
  }];
  state.running = false;
  await emit("session-changed", { session: "chat-1" });
  const log = await screen.findByRole("log", { name: "Conversation" });
  await within(log).findByText("Looking at the total.");
  expect(within(log).getByText("Edited").parentElement?.textContent).toContain("checkout/total.ts +2 −1");
  const card = log.querySelector<HTMLElement>('[data-run="run-9"]')!;
  expect(card.textContent).toContain("RED");
  expect(card.textContent).toContain("checkout/unit");
  // Open in Verify shows that Run in Verify.
  await fireEvent.click(within(card).getByRole("button", { name: /Open in Verify/ }));
  expect(app.mode).toBe("verify");
  await waitFor(() => expect(app.monitor.shown).toMatchObject({ kind: "run", run: "run-9" }));
});

it("stops a running turn, which ends marked as stopped", async () => {
  const { state } = backend();
  state.running = true;
  state.turns = [{ user: "Go", blocks: [{ kind: "tool", name: "read", arguments: '{"path":"a.ts"}', output: null, failed: null, toolKind: "read", summary: "read a.ts", exitCode: null, omittedLines: 0, outputTruncated: false }], state: { kind: "running" } }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  // A tool still running reads in the present tense.
  await screen.findByText("Reading");
  await fireEvent.click(screen.getByRole("button", { name: "Stop (Esc)" }));
  await screen.findByText("Stopped");
  expect(screen.getByRole("button", { name: "Send (Enter)" })).toBeTruthy();
});

it("filters the upward model menu and saves a model and effort immediately, even while streaming", async () => {
  const { calls, state } = backend();
  state.running = true;
  state.session = { ...session, selection: { ...session.selection, effort: "high" } };
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const trigger = await screen.findByRole("button", { name: /claude-opus-5-5.*high/ });
  await fireEvent.click(trigger);
  const menu = await screen.findByRole("menu", { name: "Models" });
  const filter = within(menu).getByRole("searchbox", { name: "Filter models" });
  expect(document.activeElement).toBe(filter);
  await within(menu).findByText("Sonnet 5.5");
  await within(menu).findByText("Model listing timed out.");
  await within(menu).findByText("Not set up: Gemini, Antigravity");
  expect(within(menu).getByRole("menuitemradio", { name: "Opus 5.5 · high" }).getAttribute("aria-checked")).toBe("true");
  await fireEvent.input(filter, { target: { value: "missing" } });
  expect(within(menu).getByText("No model matches.")).toBeTruthy();
  await fireEvent.input(filter, { target: { value: "quiet-id" } });
  expect(within(menu).getByRole("menuitemradio", { name: "Quiet" })).toBeTruthy();
  expect(within(menu).queryByRole("menuitemradio", { name: "Sonnet 5.5" })).toBeNull();
  await fireEvent.input(filter, { target: { value: "cLaUdE AgEnT" } });
  await fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Sonnet 5.5 · low" }));
  await screen.findByRole("button", { name: /Sonnet 5.5.*low/ });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(calls.find((call) => call.cmd === "chat_set_model")?.args).toEqual({ id: session.id, selection: { backend: "claude-agent", model: "claude-sonnet-5-5", effort: "low" } });
  await fireEvent.click(trigger);
  const reopened = await screen.findByRole("menu");
  await fireEvent.keyDown(within(reopened).getByRole("searchbox"), { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(trigger);
  expect(calls.some(({ cmd }) => cmd === "chat_stop" || cmd === "chat_send")).toBe(false);
  await fireEvent.click(trigger);
  await fireEvent.click(await screen.findByRole("menuitemradio", { name: "Opus 5.5" }));
  expect(calls.filter(({ cmd }) => cmd === "chat_set_model").at(-1)?.args).toEqual({ id: session.id, selection: { backend: "claude-agent", model: "claude-opus-5-5", effort: null } });
  await screen.findByRole("button", { name: /Opus 5.5.*high/ });
  await fireEvent.click(trigger);
  await fireEvent.pointerDown(screen.getByRole("textbox", { name: "Message" }));
  expect(screen.queryByRole("menu")).toBeNull();
});

it("reads Antigravity's account models after sign-in on reopening the Chat model menu", async () => {
  const { calls, state } = backend();
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const trigger = await screen.findByRole("button", { name: /claude-opus-5-5/ });
  await fireEvent.click(trigger);
  const menu = screen.getByRole("menu", { name: "Models" });
  await within(menu).findByText("Not set up: Gemini, Antigravity");
  expect(calls.some(({ cmd, args }) => cmd === "models_list" && args.provider === "antigravity")).toBe(true);
  expect(calls.some(({ cmd }) => cmd === "antigravity_status")).toBe(false);
  await fireEvent.keyDown(within(menu).getByRole("searchbox"), { key: "Escape" });
  state.antigravitySignedIn = true;

  await fireEvent.click(trigger);
  const reopened = screen.getByRole("menu", { name: "Models" });
  const model = await within(reopened).findByRole("menuitemradio", { name: "Gemini Demo" });
  expect(within(reopened).getByText("Not set up: Gemini")).toBeTruthy();
  await fireEvent.click(model);
  expect(calls.find(({ cmd }) => cmd === "chat_set_model")?.args).toEqual({
    id: session.id, selection: { backend: "antigravity", model: "gemini-demo", effort: null },
  });
});

/** A library with no session yet whose providers answer once `answer` is called, the custom
 * provider `lab` listing `models`. */
function unlisted(models: { slug: string; displayName: string }[]) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  let answer: () => void = () => {};
  const answered = new Promise<void>((done) => (answer = done));
  // The library's default: the custom provider `lab`'s first model, once its providers answered.
  const starting = models[0] ? { backend: "lab", model: models[0].slug, effort: "medium" } : null;
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    switch (cmd) {
      case "chat_list":
        return [];
      case "chat_branches":
        return [{ key: session.branchKey, project: session.project, name: session.branch, workspace: session.worktree }];
      case "chat_default_model":
        await answered;
        return starting;
      case "chat_create":
        return { ...session, id: "chat-2", worktree: (args as { worktree: string }).worktree, selection: starting! };
      case "chat_open":
        return { session: { ...session, id: "chat-2" }, running: false, turns: [] } satisfies ChatView;
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return { calls, answer };
}

it("offers Create in New chat only once the model the session starts with is known", async () => {
  const { calls, answer } = unlisted([{ slug: "lab-large", displayName: "Lab large" }]);
  show();
  await fireEvent.click(await screen.findByRole("button", { name: "New chat" }));
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  const create = within(dialog).getByRole("button", { name: "Create" });
  // The providers are still being read: the dialog says so, and Create waits.
  await waitFor(() => expect(within(dialog).getByRole("status").textContent?.trim()).toBe("Reading the providers' models…"));
  expect(create.hasAttribute("disabled")).toBe(true);
  answer();
  await waitFor(() => expect(within(dialog).getByRole("status").textContent?.trim()).toBe("lab · lab-large · medium"));
  expect(create.hasAttribute("disabled")).toBe(false);
  await fireEvent.click(create);
  await waitFor(() => expect(calls.find((call) => call.cmd === "chat_create")?.args).toEqual({
    worktree: session.worktree, branch: session.branchKey, selection: null,
  }));
  const composer = await screen.findByRole("textbox", { name: "Message" });
  await waitFor(() => expect(document.activeElement).toBe(composer));
  expect(screen.queryByRole("dialog", { name: "New chat" })).toBeNull();
  expect(screen.getByRole("log", { name: "Conversation" }).textContent?.trim()).toBe("");
});

it("says no provider lists a model once every provider has answered with none", async () => {
  const { calls, answer } = unlisted([]);
  show();
  await fireEvent.click(await screen.findByRole("button", { name: "New chat" }));
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  answer();
  await waitFor(() => expect(within(dialog).getByRole("status").textContent?.trim()).toBe("No provider lists a model. Connect one in Settings."));
  expect(within(dialog).getByRole("button", { name: "Create" }).hasAttribute("disabled")).toBe(true);
  expect(calls.some((call) => call.cmd === "chat_create")).toBe(false);
});

it("switches a session to its library-owned Graph with the same composer, and keeps it through mode switches", async () => {
  const { calls } = backend();
  const { app } = show();
  // The branch need not exist in Verify's current snapshot.
  app.browse.view = browseView(1);
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  await screen.findByRole("log", { name: "Conversation" });
  const composer = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message" });
  await fireEvent.input(composer, { target: { value: "Trace the graph" } });
  await fireEvent.click(screen.getByRole("button", { name: "Graph" }));
  const graph = await screen.findByRole("region", { name: "Current" });
  await within(graph).findByText("This worktree defines no Artifacts.");
  expect(screen.queryByRole("log", { name: "Conversation" })).toBeNull();
  expect(screen.getByRole("textbox", { name: "Message" })).toBe(composer);
  expect(composer.value).toBe("Trace the graph");
  expect(calls.find(({ cmd }) => cmd === "chat_set_view")?.args).toEqual({ id: session.id, view: "graph" });
  expect(calls.find(({ cmd }) => cmd === "current_scan")?.args).toEqual({ branch: session.branchKey, refresh: false });
  app.mode = "verify";
  await waitFor(() => expect(app.mode).toBe("verify"));
  app.mode = "chat";
  expect((await screen.findByRole("button", { name: "Graph" })).getAttribute("aria-pressed")).toBe("true");
  await fireEvent.click(screen.getByRole("button", { name: "Conversation" }));
  await screen.findByRole("log", { name: "Conversation" });
  expect(calls.filter(({ cmd }) => cmd === "chat_set_view").map(({ args }) => args)).toEqual([
    { id: session.id, view: "graph" }, { id: session.id, view: "conversation" },
  ]);
});

it("opens a Run origin in Conversation even when the session was saved in Graph", async () => {
  const { calls, state } = backend();
  state.session = { ...session, view: "graph" };
  state.turns = [{
    user: "Verify it", state: { kind: "completed" },
    blocks: [{ kind: "run", card: { id: "run-9", state: "green", failed: [], waiting: [] } }],
  }];
  const { app } = show();
  const revealed = vi.spyOn(Element.prototype, "scrollIntoView");
  try {
    app.showChat(session.id, "run-9");
    const conversation = await screen.findByRole("log", { name: "Conversation" });
    const card = conversation.querySelector('[data-run="run-9"]');
    await waitFor(() => expect(revealed.mock.contexts).toContain(card));
    expect(calls.find(({ cmd }) => cmd === "chat_set_view")?.args).toEqual({ id: session.id, view: "conversation" });
    expect(calls.some(({ cmd }) => cmd === "current_scan")).toBe(false);
    expect(screen.getByRole("button", { name: "Scroll to end" })).toBeTruthy();
  } finally {
    revealed.mockRestore();
  }
});

it("says why a session whose branch cannot be worked on now sends nothing, its composer off", async () => {
  const calls: string[] = [];
  const gone: ChatSession = { ...session, view: "graph", unavailable: "The branch fix/rounding is no longer checked out." };
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "chat_list") return [gone];
    if (cmd === "chat_open") return { session: gone, running: false, turns: [] } satisfies ChatView;
    return null;
  }, { shouldMockEvents: true });
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  expect((await screen.findByRole("status")).textContent).toBe("The branch fix/rounding is no longer checked out.");
  const composer = screen.getByLabelText<HTMLTextAreaElement>("Message");
  expect(composer.disabled).toBe(true);
  expect(screen.getByRole("button", { name: "Send (Enter)" }).hasAttribute("disabled")).toBe(true);
  await fireEvent.keyDown(composer, { key: "Enter" });
  expect(calls).not.toContain("chat_send");
  expect(calls).not.toContain("current_scan");
  expect(screen.queryByRole("region", { name: "Current" })).toBeNull();
  expect(screen.queryByRole("button", { name: /^Refresh/ })).toBeNull();
});


it("delegates search to the library, clears it with Esc, and acknowledges an unseen outcome only when shown", async () => {
  const { calls, state } = backend();
  state.session = { ...session, outcome: "completed", unseen: true };
  state.other = { ...session, id: "chat-2", title: "Another chat", outcome: "running", running: true };
  show();
  await screen.findByRole("button", { name: "Fix checkout rounding finished" });
  expect(screen.getByRole("button", { name: "Another chat running" })).toBeTruthy();
  expect(calls.some(({ cmd }) => cmd === "chat_mark_seen")).toBe(false);
  const search = screen.getByRole("searchbox", { name: "Search chats" });
  await fireEvent.input(search, { target: { value: "no such chat" } });
  await screen.findByText("No chat matches.");
  expect(calls.filter(({ cmd }) => cmd === "chat_list").at(-1)?.args).toEqual({ worktree: null, query: "no such chat" });
  await fireEvent.keyDown(search, { key: "Escape" });
  const saved = await screen.findByRole("button", { name: "Fix checkout rounding finished" });
  expect(calls.filter(({ cmd }) => cmd === "chat_list").at(-1)?.args).toEqual({ worktree: null, query: null });
  await fireEvent.click(saved);
  await waitFor(() => expect(saved.textContent).not.toContain("finished"));
  expect(calls.find(({ cmd }) => cmd === "chat_mark_seen")?.args).toEqual({ id: session.id });
});

const tool = (overrides: Partial<Extract<ChatBlock, { kind: "tool" }>> = {}): ChatBlock => ({
  kind: "tool", name: "read", arguments: '{"path":"one.ts"}', summary: "read one.ts",
  output: "ok", failed: false, toolKind: "read", exitCode: null, omittedLines: 0, outputTruncated: false, ...overrides,
});

it("folds consecutive routine successes, leaves pending and failed tools and Runs visible, and opens kept details", async () => {
  const { state } = backend();
  state.running = true;
  state.turns = [{ user: "Go", state: { kind: "running" }, blocks: [
    tool(), tool({ summary: "read two.ts" }),
    tool({ name: "run_command", toolKind: "command", summary: "ran cargo test", exitCode: 0, output: "first\n…\nlast", omittedLines: 38, outputTruncated: true }),
    tool({ summary: "read now.ts", output: null, failed: null }),
    { kind: "run", card: { id: "run-9", state: "green", failed: [], waiting: [] } },
    tool({ name: "run_command", toolKind: "command", summary: "ran cargo fmt", exitCode: 1, output: "No such file", failed: true }),
  ] }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const log = within(await screen.findByRole("log"));
  const group = log.getByText("Read 2 files · ran 1 command");
  const row = (name: string) => log.getByText(name, { selector: "summary span" }).closest("summary")!;
  expect(group.closest("details")?.open).toBe(false);
  expect(row("now.ts").closest("details")?.parentElement?.closest("details")).toBeNull();
  expect(row("No such file").closest("details")?.parentElement?.closest("details")).toBeNull();
  expect(log.getByRole("button", { name: "Open in Verify" })).toBeTruthy();
  await fireEvent.click(group.closest("summary")!);
  expect(group.closest("details")?.open).toBe(true);
  await fireEvent.click(row("cargo test"));
  const disclosed = row("cargo test").closest("details")!;
  expect(disclosed.open).toBe(true);
  expect(disclosed.textContent).toContain("Asked with");
  expect(within(disclosed).getByText("0")).toBeTruthy();
  expect(disclosed.querySelector("pre")?.textContent).toBe("… 38 earlier lines\nfirst\n…\nlast");
  await fireEvent.click(row("cargo test"));
  expect(disclosed.open).toBe(false);
});

function clipboard() {
  const writeText = vi.fn(async (_text: string) => {});
  const prior = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  return { writeText, restore: () => prior ? Object.defineProperty(navigator, "clipboard", prior) : Reflect.deleteProperty(navigator, "clipboard") };
}

it("renders only the supported Markdown, opens its link through IPC and copies code verbatim", async () => {
  const clip = clipboard();
  try {
    const { calls, state } = backend();
    const literal = "  ## User content [not a link](https://example.org)\n";
    state.turns = [{ user: literal, state: { kind: "completed" }, blocks: [{ kind: "text", text: [
      "A paragraph with `some_code` and [Reference](https://example.org/docs).", "", "- One", "- Two", "", "2. Ordered", "3. Items", "",
      "```ts", "  item()", "    other()", "```", "", "<img src=x onerror=alert(1)>", "", "# Literal heading", "", "[Unsafe](javascript:alert(1))",
    ].join("\n") }] }];
    show();
    await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
    const log = await screen.findByRole("log");
    const contents = within(log);
    expect(log.querySelector(".message")?.textContent).toBe(literal);
    expect(contents.getAllByRole("listitem")).toHaveLength(4);
    expect(log.querySelector("p code")?.textContent).toBe("some_code");
    expect(contents.getByText("<img src=x onerror=alert(1)>")).toBeTruthy();
    expect(log.querySelector("img")).toBeNull();
    expect(contents.getByText("# Literal heading")).toBeTruthy();
    expect(contents.getAllByRole("link")).toHaveLength(1);
    await fireEvent.click(contents.getByRole("link", { name: "Reference" }));
    expect(calls.find(({ cmd }) => cmd === "chat_open_link")?.args).toEqual({ url: "https://example.org/docs" });
    await fireEvent.click(contents.getByRole("button", { name: "Copy" }));
    expect(clip.writeText).toHaveBeenCalledWith("  item()\n    other()");
  } finally {
    clip.restore();
  }
});

it("keeps a rejected send out of the log, copies it and restores only into an empty composer", async () => {
  const clip = clipboard();
  try {
    const { state, calls } = backend();
    let reject!: (error: unknown) => void;
    state.send = () => new Promise((_answer, fail) => { reject = fail; });
    show();
    await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
    const composer = await screen.findByLabelText<HTMLTextAreaElement>("Message");
    const draft = "  Try again.\n";
    await fireEvent.input(composer, { target: { value: draft } });
    await fireEvent.keyDown(composer, { key: "Enter" });
    await fireEvent.input(composer, { target: { value: "New draft" } });
    await fireEvent.keyDown(composer, { key: "Enter" });
    expect(calls.filter(({ cmd }) => cmd === "chat_send").map(({ args }) => args.text)).toEqual([draft]);
    reject({ kind: "action", message: "Turn could not start." });
    await screen.findByText("Turn could not start.");
    expect(screen.getByRole("log").textContent).not.toContain("Try again.");
    const restore = screen.getByRole<HTMLButtonElement>("button", { name: "Restore" });
    expect(restore.disabled).toBe(true);
    expect(composer.value).toBe("New draft");
    await fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    expect(clip.writeText).toHaveBeenCalledWith(draft);
    await fireEvent.input(composer, { target: { value: "" } });
    expect(restore.disabled).toBe(false);
    await fireEvent.click(restore);
    expect(composer.value).toBe(draft);
    expect(calls.filter(({ cmd }) => cmd === "chat_send")).toHaveLength(1);
  } finally {
    clip.restore();
  }
});

it("shows Stopping until the turn settles, including after the stop request has been acknowledged", async () => {
  const { calls, state } = backend();
  state.running = true;
  let stop!: (value: boolean) => void;
  state.stop = () => new Promise((resolve) => { stop = resolve; });
  state.turns = [{ user: "Go", blocks: [], state: { kind: "running" } }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const composer = await screen.findByLabelText<HTMLTextAreaElement>("Message");
  await fireEvent.keyDown(composer, { key: "Escape" });
  await screen.findByText("Stopping…");
  await fireEvent.keyDown(composer, { key: "Escape" });
  expect(calls.filter(({ cmd }) => cmd === "chat_stop")).toHaveLength(1);
  state.turns = [{ ...state.turns[0], blocks: [{ kind: "text", text: "Still winding down." }] }];
  stop(true);
  await screen.findByText("Still winding down.");
  expect(screen.getByText("Stopping…")).toBeTruthy();
  state.running = false;
  state.turns = [{ ...state.turns[0], state: { kind: "stopped" } }];
  await emit("session-changed", { session: session.id });
  await screen.findByText("Stopped");
  expect(screen.queryByText("Stopping…")).toBeNull();
});

it("shows a stop failure and an interrupted turn without resending it", async () => {
  const { state, calls } = backend();
  state.running = true;
  state.stop = async () => { throw { kind: "action", message: "Unable to request a stop." }; };
  state.turns = [{ user: "Go", blocks: [{ kind: "text", text: "Partial answer" }], state: { kind: "running" } }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  await fireEvent.click(await screen.findByRole("button", { name: "Stop (Esc)" }));
  await screen.findByText("Unable to request a stop.");
  expect(screen.queryByText("Stopping…")).toBeNull();
  state.running = false;
  state.turns = [{ ...state.turns[0], state: { kind: "interrupted" } }];
  await emit("session-changed", { session: session.id });
  await screen.findByText(/Interrupted ·/);
  expect(screen.getByText("Partial answer")).toBeTruthy();
  expect(calls.some(({ cmd }) => cmd === "chat_send")).toBe(false);
});

it("keeps where a session is read across streaming, sessions and views, and follows again from Scroll to end", async () => {
  const { state } = backend();
  state.other = { ...session, id: "chat-2", title: "Another chat" };
  const { app } = show();
  let height = 800;
  vi.spyOn(Element.prototype, "scrollHeight", "get").mockImplementation(() => height);
  vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(() => 200);
  const original = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop")?.set;
  vi.spyOn(Element.prototype, "scrollTop", "set").mockImplementation(function (this: Element, top: number) {
    original?.call(this, Math.min(top, height - 200));
  });
  const first = await screen.findByRole("button", { name: /Fix checkout rounding/ });
  await fireEvent.click(first);
  let log = await screen.findByRole("log");
  await waitFor(() => expect(log.scrollTop).toBe(600));
  log.scrollTop = 125;
  await fireEvent.scroll(log);
  await screen.findByRole("button", { name: "Scroll to end" });
  height = 1000;
  state.turns = [{ user: "Go", blocks: [{ kind: "text", text: "A new answer" }], state: { kind: "running" } }];
  await emit("session-changed", { session: session.id });
  await screen.findByText("A new answer");
  expect(log.scrollTop).toBe(125);
  await fireEvent.click(screen.getByRole("button", { name: /Another chat/ }));
  await waitFor(() => expect(screen.getByRole("log").scrollTop).toBe(800));
  await fireEvent.click(first);
  log = await screen.findByRole("log");
  await waitFor(() => expect(log.scrollTop).toBe(125));
  app.mode = "verify";
  await fireEvent.scroll(log, { target: { scrollTop: 0 } });
  app.mode = "chat";
  await waitFor(() => expect(log.scrollTop).toBe(125));
  await fireEvent.click(screen.getByRole("button", { name: "Graph" }));
  await screen.findByRole("region", { name: "Current" });
  await fireEvent.click(screen.getByRole("button", { name: "Conversation" }));
  log = await screen.findByRole("log");
  await waitFor(() => expect(log.scrollTop).toBe(125));
  await fireEvent.click(screen.getByRole("button", { name: "Scroll to end" }));
  expect(log.scrollTop).toBe(800);
  expect(screen.queryByRole("button", { name: "Scroll to end" })).toBeNull();
  height = 1200;
  await emit("session-changed", { session: session.id });
  await waitFor(() => expect(log.scrollTop).toBe(1000));
});
