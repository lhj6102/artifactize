import { renderHosted as render } from "../test-host";
// An Agent transcript opened before its request has a session: a later state change resolves
// the session and the transcript reads it, as the terminal monitor re-resolves evidence.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import Transcript from "./Transcript.svelte";

beforeAll(() => {
  // jsdom lays nothing out; the transcript only needs to observe its size.
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

describe("Transcript", () => {
  it("finds the session a queued request gains once it runs", async () => {
    let running = false;
    const windows: unknown[] = [];
    mockIPC(
      (cmd, args) => {
        if (cmd === "transcript_open") {
          return running
            ? { kind: "local", handle: 7, session: "s-1" }
            : { kind: "unavailable", text: "No saved Agent conversation yet." };
        }
        if (cmd === "transcript_window") {
          windows.push(args);
          return { rows: [{ text: "Reviewing the input…", group: false, thinking: false }], indicator: "1/1 bottom following", status: null, following: true, loading: false };
        }
        return null;
      },
      { shouldMockEvents: true },
    );
    render(Transcript, { id: "run-a-3" });
    await screen.findByText("No saved Agent conversation yet.");
    running = true;
    await emit("state-changed", { resync: false });
    await screen.findByText("Reviewing the input…");
    expect((windows[0] as { handle: number }).handle).toBe(7);
  });

  it("shows the new reader's rows when the replaced reader's pending window fails", async () => {
    let handle = 7;
    let oldWindows = 0;
    const pending: { reject: (error: unknown) => void }[] = [];
    mockIPC(
      (cmd, args) => {
        if (cmd === "transcript_open") return { kind: "local", handle, session: `s-${handle}` };
        if (cmd === "transcript_window") {
          if ((args as { handle: number }).handle === 7) {
            oldWindows += 1;
            if (oldWindows === 1) {
              return { rows: [{ text: "Rows of the old session", group: false, thinking: false }], indicator: "1/1 bottom following", status: null, following: true, loading: false };
            }
            return new Promise((_, reject) => pending.push({ reject }));
          }
          return { rows: [{ text: "Fresh rows of the new session", group: false, thinking: false }], indicator: "1/1 bottom following", status: null, following: true, loading: false };
        }
        return null;
      },
      { shouldMockEvents: true },
    );
    render(Transcript, { id: "run-a-3" });
    await screen.findByText("Rows of the old session");
    await emit("session-changed", { session: "s-7" });
    await waitFor(() => expect(pending.length).toBe(1));
    handle = 8;
    await emit("state-changed", { resync: false });
    // The replacement clears the old rows before its window can run: observe that
    // rendered state, not an IPC response that precedes the owner's continuation.
    await screen.findByText("Loading session…");
    expect(screen.queryByText("Rows of the old session")).toBeNull();
    pending[0].reject({ kind: "missing", message: "The transcript was closed." });
    await screen.findByText("Fresh rows of the new session");
    expect(screen.queryByText("The transcript was closed.")).toBeNull();
  });
});
