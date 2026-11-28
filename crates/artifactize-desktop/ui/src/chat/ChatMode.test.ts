// Chat against a mocked command boundary: the sessions grouped by project, a sent message and
// the turn the library streams (text, tool lines, a Run reference that opens the Run in Verify),
// Stop while a turn runs, and the model menu opening upward with the providers' models.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import type {
  ChatAgentAvailability,
  ChatAgentCatalogEntry,
  ChatBlock,
  ChatSendResult,
  ChatSession,
  ChatSessionView,
  ChatTarget,
  ChatTurn,
  ChatView,
} from "../ipc/client";
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
  extraAnswers = null;
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

const DEFAULT_TARGET: ChatTarget = {
  engine: "direct",
  backend: { kind: "builtin", provider: "codex" },
  model: "gpt-5-codex",
  effort: null,
};

const session: ChatSession = {
  id: "chat-1",
  title: "Fix checkout rounding",
  worktree: "/work/app",
  project: "app",
  branch: "fix/rounding",
  branchKey: branchKey("fix/rounding"),
  view: "conversation",
  unavailable: null,
  target: DEFAULT_TARGET,
  createdAt: "2026-11-21T00:00:00Z",
  updatedAt: "2026-11-21T00:00:00Z",
  activity: "idle",
  legacyMigrationNotice: false,
  outcome: null,
  unseen: false,
};

const AGY_ENTRY: ChatAgentCatalogEntry = {
  id: "antigravity",
  name: "Antigravity",
  tier: "preset",
  command: "agy",
  args: ["acp"],
  env: {},
  secretEnvKeys: [],
  mcpStdio: true,
  canDisableProjectConfig: true,
  nativeSandboxNote: "Runs natively in the worktree.",
  loginAction: "artifactize login antigravity",
  installHint: "Install agy.",
  registry: null,
};

function chatViewOf(s: ChatSession, running: boolean, turns: ChatTurn[]): ChatView {
  return {
    session: s,
    activity: !running ? "idle" : s.activity === "waitingPermission" ? "waitingPermission" : "running",
    turns,
    liveConfigOptions: [],
    verifyBridgeState: "notConnected",
  };
}

/** The library of one session: `turns` is what it holds, `running` whether a turn runs. */
/** Answers a test adds before the backend's own; undefined falls through to them. */
let extraAnswers: ((cmd: string, args: Record<string, unknown>) => unknown) | null = null;

function backendWith(answer: (cmd: string, args: Record<string, unknown>) => unknown) {
  extraAnswers = answer;
  return backend();
}

function backend() {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  const state = {
    turns: [] as ChatTurn[],
    running: false,
    session,
    other: null as ChatSession | null,
    send: null as ((text: string) => Promise<ChatSendResult | void>) | null,
    stop: null as (() => Promise<boolean>) | null,
    antigravitySignedIn: false,
  };
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args: args as Record<string, unknown> });
      const extra = extraAnswers?.(cmd, args as Record<string, unknown>);
      if (extra !== undefined) return extra;
      switch (cmd) {
        case "chat_list":
          return (args as { query: string | null }).query === "no such chat"
            ? []
            : [state.session, ...(state.other ? [state.other] : [])];
        case "chat_open":
          if (state.other && (args as { id: string }).id === state.other.id) {
            return chatViewOf(state.other, false, []);
          }
          return chatViewOf(state.session, state.running, state.turns);
        case "chat_mark_seen":
          state.session = { ...state.session, unseen: false };
          return null;
        case "chat_set_target": {
          const chosen = (args as { selection: ChatTarget }).selection;
          if (chosen.engine === "direct") {
            state.session = {
              ...state.session,
              legacyMigrationNotice: false,
              target: { ...chosen, effort: chosen.effort ?? "high" },
            };
          } else {
            state.session = {
              ...state.session,
              legacyMigrationNotice: chosen.engine === "legacyClaudeAgent",
              target: chosen,
            };
          }
          return state.session;
        }
        case "chat_set_view":
          state.session = { ...state.session, view: (args as { view: ChatSessionView }).view };
          return state.session;
        case "current_scan":
          return {
            branch: session.branchKey,
            scannedAt: 1,
            fingerprint: "",
            revision: 1,
            scan: { state: "scanned" },
            checking: 0,
            artifacts: 0,
          };
        case "graph":
          return {
            source: (args as { source: unknown }).source,
            shape: 1,
            update: { kind: "shape", nodes: [], edges: [] },
          };
        case "chat_send":
          if (state.send) {
            return state.send((args as { text: string }).text).then((res) => res ?? ({ kind: "accepted" } satisfies ChatSendResult));
          }
          state.running = true;
          state.turns = [
            ...state.turns,
            { user: (args as { text: string }).text, blocks: [], state: { kind: "running" } },
          ];
          return { kind: "accepted" } satisfies ChatSendResult;
        case "chat_permission_respond":
          return true;
        case "chat_trust_grant":
          return {
            agentId: (args as { agent: string }).agent,
            scope: { kind: "directory", canonicalPath: (args as { worktree: string }).worktree },
            scopeLabel: (args as { worktree: string }).worktree,
            fingerprint: { resolvedExecutable: "/usr/bin/agy", executableSha256: "abc123", argsHash: "def456" },
            trustedAt: "2026-11-21T00:00:00Z",
          };
        case "chat_stop":
          if (state.stop) return state.stop();
          state.running = false;
          state.turns = state.turns.map((turn) => ({ ...turn, state: { kind: "stopped" } }));
          return true;
        case "browse":
          return browseView(2, {
            run: {
              kind: "read",
              id: "run-9",
              origin: null,
              headline: null,
              node: "r:run-9",
              mark: { status: "RED", glyph: "✗" },
              needs: [],
              branch: branchKey("fix/rounding"),
              chatBranch: branchKey("fix/rounding"),
            },
          });
        case "chat_agents_list":
          return [AGY_ENTRY];
        case "chat_agent_probe": {
          if (!state.antigravitySignedIn) {
            return {
              kind: "notSetUp",
              executable: "/usr/bin/agy",
              version: "1.5.14",
              reason: "Not signed in.",
              loginAction: "artifactize login antigravity",
            } satisfies ChatAgentAvailability;
          }
          return {
            kind: "ready",
            executable: "/usr/bin/agy",
            version: "1.5.14",
            models: [{ slug: "gemini-demo", displayName: "Gemini Demo", efforts: [], defaultEffort: null }],
            configOptions: [
              {
                id: "model",
                name: "Model",
                category: "model",
                currentValue: "gemini-demo",
                choices: [{ value: "gemini-demo", name: "Gemini Demo", description: null, bypassWarning: false }],
              },
            ],
          } satisfies ChatAgentAvailability;
        }
        case "providers_list":
          return {
            builtin: ["codex", "claude-agent", "gemini", "anthropic"],
            aliasTargets: ["codex", "openai", "anthropic", "gemini"],
            keys: [
              { provider: "gemini", variable: "GEMINI_API_KEY", source: "absent", saved: false },
              { provider: "anthropic", variable: "ANTHROPIC_API_KEY", source: "saved", saved: true },
            ],
            custom: [],
          };
        case "models_list": {
          const { provider } = args as { provider: string };
          if (provider === "gemini") return { kind: "notSetUp", reason: "No key is set." };
          if (provider === "anthropic") return { kind: "failed", error: "Model listing timed out." };
          return {
            kind: "models",
            models: [
              { slug: "gpt-5-codex", displayName: "GPT-5 Codex", efforts: ["high", "low", "xhigh"], defaultEffort: "high" },
              { slug: "gpt-5-mini", displayName: "GPT-5 Mini", efforts: ["high", "low"], defaultEffort: "high" },
              { slug: "quiet-id", displayName: "Quiet", efforts: [], defaultEffort: null },
            ],
          };
        }
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
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
  // Grouped by project: an open folder row over its sessions, each with its branch after it.
  const project = await screen.findByRole("button", { name: "app" });
  expect(project.getAttribute("aria-expanded")).toBe("true");
  const row = await screen.findByRole("button", { name: /Fix checkout rounding/ });
  expect(within(row).getByText("fix/rounding")).toBeTruthy();
  await fireEvent.click(row);
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
      tool({ name: "edit", arguments: JSON.stringify({ path: "checkout/total.ts", old_text: "a", new_text: "b\nc" }), output: "ok", failed: false, toolKind: "edit", summary: "edit checkout/total.ts +2 −1" }),
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
  state.turns = [{ user: "Go", blocks: [tool({ name: "read", arguments: '{"path":"a.ts"}', output: null, failed: null, status: "running", toolKind: "read", summary: "read a.ts" })], state: { kind: "running" } }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  // A tool still running reads in the present tense.
  await screen.findByText("Reading");
  await fireEvent.click(screen.getByRole("button", { name: "Stop (Esc)" }));
  await screen.findByText("Stopped");
  expect(screen.getByRole("button", { name: "Send (Enter)" })).toBeTruthy();
});

it("filters the upward target menu and saves a model and effort immediately, even while streaming", async () => {
  const { calls, state } = backend();
  state.running = true;
  state.session = {
    ...session,
    target: { engine: "direct", backend: { kind: "builtin", provider: "codex" }, model: "gpt-5-codex", effort: "high" },
  };
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const trigger = await screen.findByRole("button", { name: /gpt-5-codex.*high/ });
  await fireEvent.click(trigger);
  const menu = await screen.findByRole("menu", { name: "Chat agents and models" });
  const filter = within(menu).getByRole("searchbox", { name: "Filter agents and models" });
  expect(document.activeElement).toBe(filter);
  await within(menu).findByText("GPT-5 Mini");
  await within(menu).findByText("Model listing timed out.");
  await within(menu).findByText("Providers not set up: Gemini");
  await within(menu).findByText("Agents not set up: Antigravity");
  expect(within(menu).getByRole("menuitemradio", { name: "GPT-5 Codex · high" }).getAttribute("aria-checked")).toBe("true");
  await fireEvent.input(filter, { target: { value: "missing" } });
  expect(within(menu).getByText("No agent or model matches.")).toBeTruthy();
  await fireEvent.input(filter, { target: { value: "quiet-id" } });
  expect(within(menu).getByRole("menuitemradio", { name: "Quiet" })).toBeTruthy();
  expect(within(menu).queryByRole("menuitemradio", { name: "GPT-5 Mini" })).toBeNull();
  await fireEvent.input(filter, { target: { value: "cOdEx" } });
  await fireEvent.click(within(menu).getByRole("menuitemradio", { name: "GPT-5 Mini · low" }));
  await screen.findByRole("button", { name: /gpt-5-mini.*low/ });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(calls.find((call) => call.cmd === "chat_set_target")?.args).toEqual({
    id: session.id,
    selection: { engine: "direct", backend: { kind: "builtin", provider: "codex" }, model: "gpt-5-mini", effort: "low" },
  });
  await fireEvent.click(trigger);
  const reopened = await screen.findByRole("menu");
  await fireEvent.keyDown(within(reopened).getByRole("searchbox"), { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(trigger);
  expect(calls.some(({ cmd }) => cmd === "chat_stop" || cmd === "chat_send")).toBe(false);
  await fireEvent.click(trigger);
  await fireEvent.click(await screen.findByRole("menuitemradio", { name: "GPT-5 Codex" }));
  expect(calls.filter(({ cmd }) => cmd === "chat_set_target").at(-1)?.args).toEqual({
    id: session.id,
    selection: { engine: "direct", backend: { kind: "builtin", provider: "codex" }, model: "gpt-5-codex", effort: null },
  });
  await screen.findByRole("button", { name: /gpt-5-codex.*high/ });
  await fireEvent.click(trigger);
  await fireEvent.pointerDown(screen.getByRole("textbox", { name: "Message" }));
  expect(screen.queryByRole("menu")).toBeNull();
});

it("probes Antigravity ACP agent after sign-in on reopening the Chat target menu", async () => {
  const { calls, state } = backend();
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const trigger = await screen.findByRole("button", { name: /gpt-5-codex/ });
  await fireEvent.click(trigger);
  const menu = screen.getByRole("menu", { name: "Chat agents and models" });
  await within(menu).findByText("Agents not set up: Antigravity");
  expect(calls.some(({ cmd, args }) => cmd === "chat_agent_probe" && args.agent === "antigravity")).toBe(true);
  await fireEvent.keyDown(within(menu).getByRole("searchbox"), { key: "Escape" });
  state.antigravitySignedIn = true;

  await fireEvent.click(trigger);
  const reopened = screen.getByRole("menu", { name: "Chat agents and models" });
  const agent = await within(reopened).findByRole("menuitemradio", { name: "Antigravity" });
  await fireEvent.click(agent);
  expect(calls.find(({ cmd }) => cmd === "chat_set_target")?.args).toEqual({
    id: session.id,
    selection: {
      engine: "acp",
      agent: "antigravity",
      config: [{ id: "model", value: "gemini-demo", category: "model" }],
    },
  });
});

/** A library with no session yet whose providers answer once `answer` is called, the custom
 * provider `lab` listing `models`. */
function unlisted(models: { slug: string; displayName: string }[]) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  let answer: () => void = () => {};
  const answered = new Promise<void>((done) => (answer = done));
  const starting: ChatTarget | null = models[0]
    ? { engine: "direct", backend: { kind: "custom", name: "lab" }, model: models[0].slug, effort: "medium" }
    : null;
  let created: ChatSession | null = null;
  mockIPC(
    async (cmd, args) => {
      calls.push({ cmd, args: args as Record<string, unknown> });
      switch (cmd) {
        case "chat_list":
          return created ? [created] : [];
        case "chat_branches":
          return [{ key: session.branchKey, project: session.project, name: session.branch, workspace: session.worktree }];
        case "chat_default_target":
          await answered;
          return starting;
        case "chat_create": {
          const { worktree, selection } = args as { worktree: string; selection: ChatTarget | null };
          created = { ...session, id: "chat-2", title: "New chat", worktree, target: selection };
          return created;
        }
        case "chat_open":
          return chatViewOf(created!, false, []);
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return { calls, answer };
}

it("waits for the target check and creates with the target displayed in New chat", async () => {
  const { calls, answer } = unlisted([{ slug: "lab-large", displayName: "Lab large" }]);
  show();
  await fireEvent.click(await screen.findByRole("button", { name: "New chat" }));
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  const create = within(dialog).getByRole("button", { name: "Create" });
  await waitFor(() => expect(within(dialog).getByRole("status").textContent?.trim()).toBe("Checking available agents and providers…"));
  expect(create.hasAttribute("disabled")).toBe(true);
  answer();
  await waitFor(() => expect(within(dialog).getByRole("status").textContent?.trim()).toBe("lab · lab-large · medium"));
  expect(create.hasAttribute("disabled")).toBe(false);
  await fireEvent.click(create);
  await waitFor(() =>
    expect(calls.find((call) => call.cmd === "chat_create")?.args).toEqual({
      worktree: session.worktree,
      branch: session.branchKey,
      selection: { engine: "direct", backend: { kind: "custom", name: "lab" }, model: "lab-large", effort: "medium" },
    }),
  );
  const composer = await screen.findByRole("textbox", { name: "Message" });
  await waitFor(() => expect(document.activeElement).toBe(composer));
  expect(screen.queryByRole("dialog", { name: "New chat" })).toBeNull();
  expect(screen.getByRole("log", { name: "Conversation" }).textContent?.trim()).toBe("");
});

it("creates a usable empty Chat after every agent and provider answers with none ready", async () => {
  const { calls, answer } = unlisted([]);
  show();
  await fireEvent.click(await screen.findByRole("button", { name: "New chat" }));
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  answer();
  await waitFor(() =>
    expect(within(dialog).getByRole("status").textContent?.trim()).toBe(
      "No agent or provider is ready. The session starts without one; set up a Chat agent or provider in Settings to send.",
    ),
  );
  const create = within(dialog).getByRole("button", { name: "Create" });
  expect(create.hasAttribute("disabled")).toBe(false);
  await fireEvent.click(create);
  await screen.findByRole("button", { name: "Choose a model" });
  expect(calls.find((call) => call.cmd === "chat_create")?.args).toEqual({
    worktree: session.worktree,
    branch: session.branchKey,
    selection: null,
  });
  expect(screen.queryByRole("dialog", { name: "New chat" })).toBeNull();
  expect(screen.getByRole("log", { name: "Conversation" }).textContent?.trim()).toBe("");
  expect(screen.getByRole("tab", { name: "Graph" }).hasAttribute("disabled")).toBe(false);
});

it("keeps the typed draft and enables the first Send only after a target is picked", async () => {
  const { calls, state } = backend();
  state.session = { ...session, target: null };
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const composer = await screen.findByRole<HTMLTextAreaElement>("textbox", { name: "Message" });
  const send = screen.getByRole<HTMLButtonElement>("button", { name: "Send (Enter)" });
  await fireEvent.input(composer, { target: { value: "My first draft" } });
  await fireEvent.keyDown(composer, { key: "Enter" });
  expect(send.disabled).toBe(true);
  expect(screen.getByText("Choose a model to send")).toBeTruthy();
  expect(composer.value).toBe("My first draft");
  expect(calls.some(({ cmd }) => cmd === "chat_send")).toBe(false);
  await fireEvent.click(screen.getByRole("button", { name: "Choose a model" }));
  const model = await screen.findByRole("menuitemradio", { name: "GPT-5 Mini" });
  expect(screen.queryAllByRole("menuitemradio", { checked: true })).toHaveLength(0);
  await fireEvent.click(model);
  await waitFor(() => expect(send.disabled).toBe(false));
  expect(screen.queryByText("Choose a model to send")).toBeNull();
  state.send = async () => {
    expect(state.session.target).toMatchObject({ engine: "direct", model: "gpt-5-mini" });
    return { kind: "accepted" };
  };
  await fireEvent.click(send);
  await waitFor(() =>
    expect(calls.find(({ cmd }) => cmd === "chat_send")?.args).toEqual({
      id: session.id,
      text: "My first draft",
      selection: null,
    }),
  );
});

it("shows the library's no-model refusal if a Send slips past the displayed state", async () => {
  const { state } = backend();
  state.send = () => Promise.reject({ kind: "noModel", message: "Choose a model before sending." });
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const composer = await screen.findByRole<HTMLTextAreaElement>("textbox", { name: "Message" });
  await fireEvent.input(composer, { target: { value: "Keep this draft" } });
  await fireEvent.keyDown(composer, { key: "Enter" });
  await screen.findByText("Choose a model before sending.");
  expect(composer.value).toBe("Keep this draft");
  expect(screen.getByRole("log", { name: "Conversation" }).textContent?.trim()).toBe("");
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
  await fireEvent.click(screen.getByRole("tab", { name: "Graph" }));
  const graph = await screen.findByRole("region", { name: "Current" });
  await within(graph).findByText("This worktree defines no Artifacts.");
  expect(screen.queryByRole("log", { name: "Conversation" })).toBeNull();
  // The composer floats over the graph with the draft as it was typed.
  const floating = within(graph).getByRole("group", { name: "Chat" });
  expect(within(floating).getByRole<HTMLTextAreaElement>("textbox", { name: "Message" }).value).toBe("Trace the graph");
  expect(calls.find(({ cmd }) => cmd === "chat_set_view")?.args).toEqual({ id: session.id, view: "graph" });
  expect(calls.find(({ cmd }) => cmd === "current_scan")?.args).toEqual({ branch: session.branchKey, refresh: false });
  app.mode = "verify";
  await waitFor(() => expect(app.mode).toBe("verify"));
  app.mode = "chat";
  expect((await screen.findByRole("tab", { name: "Graph" })).getAttribute("aria-selected")).toBe("true");
  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));
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
    if (cmd === "chat_open") return chatViewOf(gone, false, []);
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
  state.other = { ...session, id: "chat-2", title: "Another chat", outcome: "running", activity: "running" };
  show();
  await screen.findByRole("button", { name: "Fix checkout rounding fix/rounding finished" });
  expect(screen.getByRole("button", { name: "Another chat fix/rounding running" })).toBeTruthy();
  expect(calls.some(({ cmd }) => cmd === "chat_mark_seen")).toBe(false);
  // Search turns the top row into the field, focused.
  await fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
  const search = await screen.findByRole("searchbox", { name: "Search chats" });
  expect(document.activeElement).toBe(search);
  expect(screen.queryByRole("button", { name: "New chat" })).toBeNull();
  await fireEvent.input(search, { target: { value: "no such chat" } });
  await screen.findByText("No chat matches.");
  expect(calls.filter(({ cmd }) => cmd === "chat_list").at(-1)?.args).toEqual({ worktree: null, query: "no such chat" });
  // Esc lists every session again and gives the top row back.
  await fireEvent.keyDown(search, { key: "Escape" });
  const saved = await screen.findByRole("button", { name: "Fix checkout rounding fix/rounding finished" });
  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "Search chats" })));
  expect(screen.getByRole("button", { name: "New chat" })).toBeTruthy();
  expect(calls.filter(({ cmd }) => cmd === "chat_list").at(-1)?.args).toEqual({ worktree: null, query: null });
  await fireEvent.click(saved);
  await waitFor(() => expect(saved.textContent).not.toContain("finished"));
  expect(calls.find(({ cmd }) => cmd === "chat_mark_seen")?.args).toEqual({ id: session.id });
});

const tool = (overrides: Partial<Extract<ChatBlock, { kind: "tool" }>> = {}): ChatBlock => ({
  kind: "tool",
  callId: null,
  title: null,
  name: "read",
  arguments: '{"path":"one.ts"}',
  summary: "read one.ts",
  output: "ok",
  failed: false,
  toolKind: "read",
  status: "completed",
  exitCode: null,
  omittedLines: 0,
  outputTruncated: false,
  locations: [],
  diff: null,
  ...overrides,
});

it("folds consecutive routine successes, leaves pending and failed tools and Runs visible, and opens kept details", async () => {
  const { state } = backend();
  state.running = true;
  state.turns = [{ user: "Go", state: { kind: "running" }, blocks: [
    tool(), tool({ summary: "read two.ts" }),
    tool({ name: "run_command", toolKind: "command", summary: "ran cargo test", exitCode: 0, output: "first\n…\nlast", omittedLines: 38, outputTruncated: true }),
    tool({ summary: "read now.ts", output: null, failed: null, status: "running" }),
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
  await fireEvent.click(screen.getByRole("tab", { name: "Graph" }));
  await screen.findByRole("region", { name: "Current" });
  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));
  log = await screen.findByRole("log");
  await waitFor(() => expect(log.scrollTop).toBe(125));
  await fireEvent.click(screen.getByRole("button", { name: "Scroll to end" }));
  expect(log.scrollTop).toBe(800);
  expect(screen.queryByRole("button", { name: "Scroll to end" })).toBeNull();
  height = 1200;
  await emit("session-changed", { session: session.id });
  await waitFor(() => expect(log.scrollTop).toBe(1000));
  // Scrolling up before the browser dispatches its async scroll event must not be snapped back
  // to the end when a streaming chunk arrives in the same frame.
  log.scrollTop = 250;
  height = 1400;
  state.turns = [{ user: "Go", blocks: [{ kind: "text", text: "Streamed while scrolling up" }], state: { kind: "running" } }];
  await emit("session-changed", { session: session.id });
  await screen.findByText("Streamed while scrolling up");
  await screen.findByRole("button", { name: "Scroll to end" });
  expect(log.scrollTop).toBe(250);
});

it("folds and unfolds a project from its folder row, and names nothing of the session in the stage header", async () => {
  const { state } = backend();
  state.other = { ...session, id: "chat-2", title: "Another chat", project: "site", branch: "main" };
  show();
  const app = await screen.findByRole("button", { name: "app" });
  const site = screen.getByRole("button", { name: "site" });
  expect(app.querySelector("svg")?.getAttribute("data-icon")).toBe("folder-open");
  // A click anywhere on a project folds it: its sessions leave the Pane, its folder closes.
  await fireEvent.click(app);
  expect(app.getAttribute("aria-expanded")).toBe("false");
  expect(app.querySelector("svg")?.getAttribute("data-icon")).toBe("folder-closed");
  expect(screen.queryByRole("list", { name: "Sessions of app" })).toBeNull();
  expect(screen.getByRole("list", { name: "Sessions of site" })).toBeTruthy();
  await fireEvent.click(app);
  expect(screen.getByRole("list", { name: "Sessions of app" })).toBeTruthy();
  expect(site.getAttribute("aria-expanded")).toBe("true");
  // New chat leads the Pane with its own icon; Search is the find icon alone.
  expect(screen.getByRole("button", { name: "New chat" }).querySelector("svg")?.getAttribute("data-icon")).toBe("new-chat");
  expect(screen.getByRole("button", { name: "Search chats" }).textContent?.trim()).toBe("");
  // A shown session: the header holds the view tabs and the tools, never its title or branch.
  await fireEvent.click(screen.getByRole("button", { name: /^Another chat/ }));
  const main = screen.getByLabelText("Chat's main area");
  await within(main).findByRole("log", { name: "Conversation" });
  const header = main.querySelector("header")!;
  expect(within(header).getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Conversation", "Graph"]);
  expect(header.textContent).not.toMatch(/Another chat|main|site/);
});

it("shows a message sent from Graph and its reply in a card above the composer, gone once Conversation is shown", async () => {
  const { state } = backend();
  state.session = {
    ...session,
    view: "graph",
    target: { engine: "direct", backend: { kind: "builtin", provider: "codex" }, model: "gpt-5-codex", effort: "high" },
  };
  state.turns = [{ user: "Earlier work", blocks: [{ kind: "text", text: "Done before." }], state: { kind: "completed" } }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const graph = await screen.findByRole("region", { name: "Current" });
  const floating = within(graph).getByRole("group", { name: "Chat" });
  // Nothing sent from Graph yet: no card, only the composer.
  expect(within(floating).queryByRole("region", { name: "Reply" })).toBeNull();
  const composer = within(floating).getByRole<HTMLTextAreaElement>("textbox", { name: "Message" });
  await fireEvent.input(composer, { target: { value: "Rename the total" } });
  await fireEvent.keyDown(composer, { key: "Enter" });
  const card = await within(floating).findByRole("region", { name: "Reply" });
  const head = within(card).getByRole("button", { name: /Rename the total/ });
  expect(head.getAttribute("aria-expanded")).toBe("true");
  expect(within(head).getByLabelText("Running")).toBeTruthy();
  // The reply streams into the card, the newest message's alone.
  state.turns = [state.turns[0], { user: "Rename the total", blocks: [{ kind: "text", text: "Renamed it." }], state: { kind: "completed" } }];
  state.running = false;
  await emit("session-changed", { session: session.id });
  await within(card).findByText("Renamed it.");
  expect(within(card).queryByText("Done before.")).toBeNull();
  expect(within(head).queryByLabelText("Running")).toBeNull();
  // A click on its head folds it to the head, and another unfolds it.
  await fireEvent.click(head);
  expect(head.getAttribute("aria-expanded")).toBe("false");
  expect(within(card).queryByText("Renamed it.")).toBeNull();
  await fireEvent.click(head);
  await within(card).findByText("Renamed it.");
  // Conversation holds the whole turn; back in Graph the card is gone.
  await fireEvent.click(screen.getByRole("tab", { name: "Conversation" }));
  const log = await screen.findByRole("log", { name: "Conversation" });
  expect(within(log).getByText("Renamed it.")).toBeTruthy();
  await fireEvent.click(screen.getByRole("tab", { name: "Graph" }));
  const again = await screen.findByRole("region", { name: "Current" });
  expect(within(again).queryByRole("region", { name: "Reply" })).toBeNull();
});

it("finds text in the conversation from the stage header, steps through its places and closes with Esc", async () => {
  const { state } = backend();
  state.turns = [
    { user: "Apply the discount", blocks: [{ kind: "text", text: "The DISCOUNT is applied twice." }], state: { kind: "completed" } },
    { user: "Thanks", blocks: [{ kind: "text", text: "No other discount remains." }], state: { kind: "completed" } },
  ];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const main = screen.getByLabelText("Chat's main area");
  await within(main).findByRole("log", { name: "Conversation" });
  const tools = within(main).getByRole("toolbar", { name: "Tools" });
  // Find is the Conversation's own tool, the find icon alone, then the session's Move.
  expect(within(tools).getAllByRole("button").map((tool) => tool.getAttribute("aria-label"))).toEqual(["Find (Ctrl+F)", "Move to another checkout"]);
  await fireEvent.click(within(tools).getByRole("button", { name: "Find (Ctrl+F)" }));
  const field = within(tools).getByRole<HTMLInputElement>("textbox", { name: "Find in the conversation" });
  expect(document.activeElement).toBe(field);
  // Every place, ignoring case: the first is shown, Enter the next, Shift+Enter the one before.
  await fireEvent.input(field, { target: { value: "discount" } });
  await waitFor(() => expect(within(tools).getByRole("status").textContent).toBe("1 of 3"));
  await fireEvent.keyDown(field, { key: "Enter" });
  await waitFor(() => expect(within(tools).getByRole("status").textContent).toBe("2 of 3"));
  await fireEvent.keyDown(field, { key: "Enter", shiftKey: true });
  await fireEvent.keyDown(field, { key: "Enter", shiftKey: true });
  await waitFor(() => expect(within(tools).getByRole("status").textContent).toBe("3 of 3"));
  await fireEvent.input(field, { target: { value: "coupon" } });
  await waitFor(() => expect(within(tools).getByRole("status").textContent).toBe("No match"));
  // Esc gives the find icon back.
  await fireEvent.keyDown(field, { key: "Escape" });
  expect(within(tools).queryByRole("textbox")).toBeNull();
  expect(within(tools).getByRole("button", { name: "Find (Ctrl+F)" })).toBeTruthy();
});

it("moves a session to another checkout from its Move tool, refusing while a turn runs and saying why the library refused", async () => {
  const { calls, state } = backendWith((cmd, args) => extra(cmd, args));
  const other = { key: branchKey("fix/after-discount"), project: "app", name: "fix/after-discount", workspace: "/work/app-after" };
  const own = { key: session.branchKey, project: "app", name: "fix/rounding", workspace: session.worktree };
  let refuse = true;
  function extra(cmd: string, _args: Record<string, unknown>): unknown {
    if (cmd === "chat_branches") return [other, own];
    if (cmd === "chat_set_worktree") {
      if (refuse) throw { kind: "busy", message: "Stop the running turn first" };
      state.session = { ...state.session, worktree: other.workspace, branch: other.name, branchKey: other.key };
      return state.session;
    }
    return undefined;
  }
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const main = screen.getByLabelText("Chat's main area");
  const move = await within(main).findByRole("button", { name: "Move to another checkout" });
  await fireEvent.click(move);
  const dialog = await screen.findByRole("dialog", { name: "Move chat" });
  // The session's own checkout first, and Move waits for another one.
  const choice = within(dialog).getByRole<HTMLSelectElement>("combobox", { name: "Checkout" });
  await waitFor(() => expect([...choice.options].map((option) => option.value)).toEqual([own.workspace, other.workspace]));
  const submit = within(dialog).getByRole<HTMLButtonElement>("button", { name: "Move" });
  expect(submit.disabled).toBe(true);
  await fireEvent.change(choice, { target: { value: other.workspace } });
  expect(submit.disabled).toBe(false);
  // Refused: why, in the dialog, which stays open.
  await fireEvent.click(submit);
  expect((await within(dialog).findByRole("alert")).textContent).toBe("Stop the running turn first");
  refuse = false;
  await fireEvent.click(submit);
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Move chat" })).toBeNull());
  expect(calls.filter(({ cmd }) => cmd === "chat_set_worktree").at(-1)?.args).toEqual({ id: session.id, worktree: other.workspace });
  await waitFor(() => expect(within(screen.getByRole("button", { name: /Fix checkout rounding/ })).getByText("fix/after-discount")).toBeTruthy());
  // While a turn runs, Move is unavailable and says why.
  state.running = true;
  await emit("session-changed", { session: session.id });
  await waitFor(() => expect((within(main).getByRole("button", { name: "Move to another checkout" }) as HTMLButtonElement).disabled).toBe(true));
  expect(within(main).getByRole("button", { name: "Move to another checkout" }).getAttribute("title")).toBe("Stop the running turn to move the chat");
});

it("starts New chat from a project's row on that project's branch", async () => {
  const { calls } = backendWith((cmd) => (cmd === "chat_branches" ? [
    { key: branchKey("main"), project: "site", name: "main", workspace: "/work/site" },
    { key: session.branchKey, project: "app", name: "fix/rounding", workspace: session.worktree },
  ] : undefined));
  show();
  const projects = await screen.findByText("Projects");
  expect(projects).toBeTruthy();
  await fireEvent.click(await screen.findByRole("button", { name: "New chat in app" }));
  const dialog = await screen.findByRole("dialog", { name: "New chat" });
  await waitFor(() => expect(within(dialog).getByRole<HTMLSelectElement>("combobox", { name: "Branch" }).value).toBe(session.worktree));
  expect(calls.some(({ cmd }) => cmd === "chat_branches")).toBe(true);
});

it("asks for Workspace Trust on trustRequired and retries send automatically after confirmation", async () => {
  const { calls, state } = backend();
  state.session = {
    ...session,
    target: { engine: "acp", agent: "antigravity", config: [] },
  };
  let trusted = false;
  state.send = async () => {
    if (!trusted) {
      return {
        kind: "trustRequired",
        requirement: {
          agentId: "antigravity",
          agentName: "Antigravity",
          scope: { kind: "gitRepo", commonDir: "/work/app/.git" },
          scopeLabel: "/work/app/.git",
          fingerprint: { resolvedExecutable: "/usr/bin/agy", executableSha256: "deadbeef1234", argsHash: "abcd" },
          disclosure: "Trusting this Git repository applies to all worktrees sharing this .git directory.",
          reason: "Antigravity has not been trusted for this repository yet.",
          coversAllWorktrees: true,
        },
      };
    }
    state.running = true;
    state.turns = [{ user: "Run ACP", blocks: [], state: { kind: "running" } }];
    return { kind: "accepted" };
  };
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const composer = await screen.findByRole<HTMLTextAreaElement>("textbox", { name: "Message" });
  await fireEvent.input(composer, { target: { value: "Run ACP" } });
  await fireEvent.keyDown(composer, { key: "Enter" });
  const dialog = await screen.findByRole("dialog", { name: "Trust workspace" });
  expect(within(dialog).getByText(/deadbeef1234/)).toBeTruthy();
  expect(composer.value).toBe("Run ACP");
  trusted = true;
  await fireEvent.click(within(dialog).getByRole("button", { name: "Trust and continue" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Trust workspace" })).toBeNull());
  // Trust is asked for the launcher the dialog showed.
  expect(calls.some(({ cmd, args }) => cmd === "chat_trust_grant" && args.agent === "antigravity"
    && (args.fingerprint as { executableSha256?: string } | undefined)?.executableSha256 === "deadbeef1234")).toBe(true);
  expect(calls.filter(({ cmd }) => cmd === "chat_send")).toHaveLength(2);
});

it("renders rich ACP blocks, interactive permission cards with always-persistence disclosure, and legacy migration banner", async () => {
  const { calls, state } = backend();
  state.running = true;
  state.session = {
    ...session,
    activity: "waitingPermission",
    target: { engine: "acp", agent: "antigravity", config: [] },
  };
  state.turns = [
    {
      user: "Check and patch",
      state: { kind: "waitingPermission" },
      blocks: [
        { kind: "handoff", from: "Direct (codex)", to: "ACP (antigravity)", summary: "Prior context" },
        { kind: "thought", text: "Checking tests first." },
        {
          kind: "plan",
          steps: [
            { content: "Read failing test", status: "completed", priority: "high" },
            { content: "Run cargo test", status: "in_progress", priority: null },
          ],
        },
        {
          kind: "permission",
          permissionId: "perm-1",
          title: "Run cargo test",
          toolKind: "command",
          summary: "cargo test -p app",
          detail: null,
          status: "pending",
          selectedOptionId: null,
          options: [
            { optionId: "once", name: "Allow once", kind: "allow_once", persistsInAgentSettings: false },
            { optionId: "always", name: "Always allow", kind: "allow_always", persistsInAgentSettings: true },
          ],
        },
        { kind: "notice", text: "Saved model fell back to default." },
      ],
    },
  ];
  show();
  const row = await screen.findByRole("button", { name: /Fix checkout rounding.*waiting/ });
  await fireEvent.click(row);
  const log = within(await screen.findByRole("log", { name: "Conversation" }));
  expect(log.getByText("Thought")).toBeTruthy();
  expect(log.getByText("Read failing test")).toBeTruthy();
  expect(log.getByText("Saved model fell back to default.")).toBeTruthy();
  expect(log.getByText("May be saved in agent settings")).toBeTruthy();
  await fireEvent.click(log.getByRole("button", { name: /Allow once/ }));
  expect(calls.find(({ cmd }) => cmd === "chat_permission_respond")?.args).toEqual({
    id: session.id,
    permissionId: "perm-1",
    optionId: "once",
  });

  // Legacy claude-agent session shows migration notice and disables Send until switched.
  state.running = false;
  state.session = {
    ...session,
    activity: "idle",
    legacyMigrationNotice: true,
    target: { engine: "legacyClaudeAgent", model: "sonnet", effort: null },
  };
  await emit("session-changed", { session: session.id });
  await screen.findByText(/Legacy Claude Agent Chat has been retired/);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Send (Enter)" }).disabled).toBe(true);
});
