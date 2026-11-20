// Chat against a mocked command boundary: the sessions grouped by project, a sent message and
// the turn the library streams (text, tool lines, a Run reference that opens the Run in Verify),
// Stop while a turn runs, and the model menu opening upward with the providers' models.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it } from "vitest";
import type { ChatSession, ChatTurn, ChatView } from "../ipc/client";
import { renderHosted } from "../test-host";
import ChatMode from "./ChatMode.svelte";
import { branch, browseView, runChip } from "../monitor/fixtures";

beforeAll(() => {
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
});

const session: ChatSession = {
  id: "chat-1",
  title: "Fix checkout rounding",
  worktree: "/work/app",
  selection: { backend: "claude-agent", model: "claude-opus-5-5" },
  createdAt: "2026-11-21T00:00:00Z",
  updatedAt: "2026-11-21T00:00:00Z",
};

/** The library of one session: `turns` is what it holds, `running` whether a turn runs. */
function backend() {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  const state = { turns: [] as ChatTurn[], running: false };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    switch (cmd) {
      case "chat_list":
        return [session];
      case "chat_open":
        return { session, running: state.running, turns: state.turns } satisfies ChatView;
      case "chat_send":
        state.running = true;
        state.turns = [...state.turns, { user: (args as { text: string }).text, blocks: [], state: { kind: "running" } }];
        return null;
      case "chat_stop":
        state.running = false;
        state.turns = state.turns.map((turn) => ({ ...turn, state: { kind: "stopped" } }));
        return true;
      case "providers_list":
        return { builtin: ["claude-agent", "gemini"], keys: [], custom: [] };
      case "models_list":
        if ((args as { provider: string }).provider === "gemini") throw { kind: "read", message: "No key is set." };
        return [{ slug: "claude-opus-5-5", displayName: "Opus 5.5" }, { slug: "claude-sonnet-5-5", displayName: "Sonnet 5.5" }];
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
  return hosted;
}

it("lists sessions by project, sends a message and shows the turn as the library streams it", async () => {
  const { calls, state } = backend();
  const { app } = show();
  await screen.findByText("app");
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  const composer = await screen.findByLabelText<HTMLTextAreaElement>("Message");
  await fireEvent.input(composer, { target: { value: "Fix it and verify." } });
  await fireEvent.keyDown(composer, { key: "Enter" });
  await waitFor(() => expect(calls.find((call) => call.cmd === "chat_send")?.args).toMatchObject({ id: "chat-1", text: "Fix it and verify.", selection: null }));
  expect(composer.value).toBe("");
  // The turn runs: Send is Stop.
  await screen.findByRole("button", { name: "Stop (Esc)" });
  state.turns = [{
    user: "Fix it and verify.",
    blocks: [
      { kind: "text", text: "Looking at the total." },
      { kind: "tool", name: "edit", arguments: JSON.stringify({ path: "checkout/total.ts", old_text: "a", new_text: "b\nc" }), output: "ok", failed: false },
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
  expect(app.monitor.shown).toMatchObject({ kind: "run", run: "run-9" });
});

it("stops a running turn, which ends marked as stopped", async () => {
  const { state } = backend();
  state.running = true;
  state.turns = [{ user: "Go", blocks: [{ kind: "tool", name: "read", arguments: '{"path":"a.ts"}', output: null, failed: null }], state: { kind: "running" } }];
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  // A tool still running reads in the present tense.
  await screen.findByText("Reading");
  await fireEvent.click(screen.getByRole("button", { name: "Stop (Esc)" }));
  await screen.findByText("Stopped");
  expect(screen.getByRole("button", { name: "Send (Enter)" })).toBeTruthy();
});

it("opens the model menu upward with each provider's models, and a model chosen goes with the next message", async () => {
  const { calls } = backend();
  show();
  await fireEvent.click(await screen.findByRole("button", { name: /Fix checkout rounding/ }));
  await fireEvent.click(await screen.findByRole("button", { name: /claude-opus-5-5/ }));
  const menu = await screen.findByRole("menu", { name: "Models" });
  await within(menu).findByText("Sonnet 5.5");
  // A provider that lists nothing says why.
  await within(menu).findByText("No key is set.");
  expect(within(menu).getByRole("menuitemradio", { name: "Opus 5.5" }).getAttribute("aria-checked")).toBe("true");
  await fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Sonnet 5.5" }));
  expect(screen.queryByRole("menu")).toBeNull();
  const composer = screen.getByLabelText<HTMLTextAreaElement>("Message");
  await fireEvent.input(composer, { target: { value: "Again" } });
  await fireEvent.click(screen.getByRole("button", { name: "Send (Enter)" }));
  await waitFor(() =>
    expect(calls.find((call) => call.cmd === "chat_send")?.args.selection).toEqual({ backend: "claude-agent", model: "claude-sonnet-5-5" }),
  );
});
