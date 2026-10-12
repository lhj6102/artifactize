// An Agent transcript opened before its request has a session: a later state change resolves
// the session and the transcript reads it, as the terminal monitor re-resolves evidence.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
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
            ? { handle: 7, session: "s-1", text: null }
            : { handle: null, session: null, text: "No saved Agent conversation yet." };
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
    const pending: { reject: (error: unknown) => void }[] = [];
    mockIPC(
      (cmd, args) => {
        if (cmd === "transcript_open") return { handle, session: `s-${handle}`, text: null };
        if (cmd === "transcript_window") {
          if ((args as { handle: number }).handle === 7) {
            return new Promise((_, reject) => pending.push({ reject }));
          }
          return { rows: [{ text: "Fresh rows of the new session", group: false, thinking: false }], indicator: "1/1 bottom following", status: null, following: true, loading: false };
        }
        return null;
      },
      { shouldMockEvents: true },
    );
    render(Transcript, { id: "run-a-3" });
    await waitFor(() => expect(pending.length).toBe(1));
    handle = 8;
    await emit("state-changed", { resync: false });
    await new Promise((done) => setTimeout(done, 0));
    pending[0].reject({ kind: "missing", message: "The transcript was closed." });
    await screen.findByText("Fresh rows of the new session");
    expect(screen.queryByText("The transcript was closed.")).toBeNull();
  });
});
