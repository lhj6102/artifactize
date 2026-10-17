import { renderHosted as render } from "../test-host";
// An Agent transcript opened before its request has a session: a later state change resolves
// the session and the transcript reads it, as the terminal monitor re-resolves evidence.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
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
          return { rows: [{ text: "Reviewing the input…", group: false, thinking: false }], indicator: "1/1 bottom following", top: 0, total: 1, status: null, following: true, loading: false };
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
              return { rows: [{ text: "Rows of the old session", group: false, thinking: false }], indicator: "1/1 bottom following", top: 0, total: 1, status: null, following: true, loading: false };
            }
            return new Promise((_, reject) => pending.push({ reject }));
          }
          return { rows: [{ text: "Fresh rows of the new session", group: false, thinking: false }], indicator: "1/1 bottom following", top: 0, total: 1, status: null, following: true, loading: false };
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

  it("lays a very long unbroken agent-log line out within the pane's content box", async () => {
    // A 400px pane with 16px padding on each side and a 7.2px monospace cell: 368px of room.
    const [width, padding, cell] = [400, 16, 7.2];
    const rect = (w: number, h: number) => ({ width: w, height: h, top: 0, left: 0, right: w, bottom: h, x: 0, y: 0, toJSON: () => ({}) });
    const boxes = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      return this.classList.contains("probe") ? rect(cell * 10, 18) : rect(width, 360);
    });
    // The pane's padding, as the app's style gives it.
    const sheet = document.head.appendChild(document.createElement("style"));
    sheet.textContent = `.window { padding-left: ${padding}px; padding-right: ${padding}px; }`;
    const line = "x".repeat(600);
    let asked = 0;
    mockIPC(
      (cmd, args) => {
        if (cmd === "transcript_open") return { kind: "local", handle: 9, session: "s-9" };
        if (cmd === "transcript_window") {
          // The backend lays the line out in the columns asked for, as a terminal does.
          asked = (args as { columns: number }).columns;
          const rows = line.match(new RegExp(`.{1,${asked}}`, "g"))!.map((text) => ({ text, group: false, thinking: false }));
          return { rows, indicator: "1/1 bottom following", top: 0, total: 1, status: null, following: true, loading: false };
        }
        return null;
      },
      { shouldMockEvents: true },
    );
    try {
      render(Transcript, { id: "run-a-4" });
      const log = await screen.findByRole("log", { name: "Agent transcript" });
      await waitFor(() => expect(log.querySelectorAll(".row").length).toBeGreaterThan(1));
      const rows = [...log.querySelectorAll(".row")].map((row) => row.textContent ?? "");
      expect(rows.join("")).toBe(line);
      for (const row of rows) expect(row.length * cell).toBeLessThanOrEqual(width - 2 * padding);
    } finally {
      boxes.mockRestore();
      sheet.remove();
    }
  });

  it("draws a scrollbar for the rows it holds out of all laid-out rows, and none when they all fit", async () => {
    let total = 300;
    mockIPC(
      (cmd) => {
        if (cmd === "transcript_open") return { kind: "local", handle: 11, session: "s-11" };
        if (cmd === "transcript_window") {
          // The pane holds three rows here (jsdom lays nothing out); the log has `total`.
          const rows = ["a", "b", "c"].map((text) => ({ text, group: false, thinking: false }));
          return { rows, indicator: "", top: 150, total, status: null, following: false, loading: false };
        }
        return null;
      },
      { shouldMockEvents: true },
    );
    const { unmount } = render(Transcript, { id: "run-a-5" });
    const log = await screen.findByRole("log", { name: "Agent transcript" });
    await waitFor(() => expect(log.querySelector(".thumb")).not.toBeNull());
    const thumb = log.querySelector<HTMLElement>(".thumb")!;
    // A short share keeps a thumb that can be grabbed; it sits halfway down.
    expect(thumb.style.height).toBe("5%");
    expect(parseFloat(thumb.style.top)).toBeCloseTo(47.98, 1);
    unmount();
    total = 3;
    render(Transcript, { id: "run-a-5" });
    const fitting = await screen.findByRole("log", { name: "Agent transcript" });
    await waitFor(() => expect(fitting.querySelectorAll(".row").length).toBe(3));
    expect(fitting.querySelector(".track")).toBeNull();
  });
});
