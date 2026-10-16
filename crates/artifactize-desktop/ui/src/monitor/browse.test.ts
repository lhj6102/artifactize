// The monitor snapshot against a mocked command boundary: clock redraws and reads that answer
// out of order never roll the shown snapshot back, and a newer snapshot always advances the
// revision Detail reads by.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { events, onStateChanged, type BrowseView, type Navigate } from "../ipc/client";
import { Browse } from "./browse.svelte";

function view(revision: number): BrowseView {
  return {
    scope: { kind: "all" },
    scopeLabel: "all repositories",
    scopes: [],
    badge: [],
    runs: [],
    selectedRun: null,
    older: false,
    newer: false,
    run: null,
    error: null,
    refreshed: true,
    replaced: false,
    revision,
  };
}

type Answer = (revision: number) => void;

function backend() {
  const answers: { cmd: string; answer: Answer }[] = [];
  mockIPC(
    (cmd) =>
      new Promise<BrowseView>((resolve) =>
        answers.push({ cmd, answer: (revision) => resolve(view(revision)) }),
      ),
  );
  return answers;
}

afterEach(() => clearMocks());

describe("Browse", () => {
  it("keeps ordinary state hints incremental and coalesces a manual refresh without losing its reset", async () => {
    const pending: { navigate: Navigate; answer: () => void }[] = [];
    mockIPC((cmd, args) => {
      if (cmd !== "browse") throw new Error(`Unexpected command: ${cmd}`);
      return new Promise<BrowseView>((resolve) => pending.push({
        navigate: (args as { navigate: Navigate }).navigate,
        answer: () => resolve(view(pending.length)),
      }));
    }, { shouldMockEvents: true });
    const browse = new Browse();
    const stop = onStateChanged((resync) => browse.refresh(resync));
    try {
      await events.stateChanged.emit({ resync: false });
      await waitFor(() => expect(pending).toHaveLength(1));
      expect(pending[0].navigate).toMatchObject({ full: true, resync: false });

      browse.refresh();
      await events.stateChanged.emit({ resync: false });
      expect(pending).toHaveLength(1);
      pending[0].answer();
      await waitFor(() => expect(pending).toHaveLength(2));
      expect(pending[1].navigate).toMatchObject({ full: true, resync: true });
      pending[1].answer();
      await waitFor(() => expect(browse.revision).toBe(2));

      await events.stateChanged.emit({ resync: false });
      await waitFor(() => expect(pending).toHaveLength(3));
      expect(pending[2].navigate).toMatchObject({ full: true, resync: false });
      pending[2].answer();
      await waitFor(() => expect(browse.revision).toBe(3));

      await events.stateChanged.emit({ resync: true });
      await waitFor(() => expect(pending).toHaveLength(4));
      expect(pending[3].navigate).toMatchObject({ full: true, resync: true });
      pending[3].answer();
      await waitFor(() => expect(browse.revision).toBe(4));
    } finally {
      stop();
      for (const read of pending) read.answer();
    }
  });

  it("keeps a read's snapshot when a clock redraw of the older one answers after it", async () => {
    const answers = backend();
    const browse = new Browse();
    browse.view = view(1);
    const read = browse.navigate({ full: true });
    const clock = browse.redraw();
    await waitFor(() => expect(answers.map((pending) => pending.cmd)).toEqual(["browse", "redraw"]));
    answers.find((pending) => pending.cmd === "browse")!.answer(2);
    await read;
    answers.find((pending) => pending.cmd === "redraw")!.answer(1);
    await clock;
    expect(browse.view?.revision).toBe(2);
    expect(browse.revision).toBe(2);
  });

  it("advances the revision when a clock redraw carries a newer snapshot before its read answers", async () => {
    const answers = backend();
    const browse = new Browse();
    browse.view = view(1);
    const read = browse.navigate({ full: true });
    const clock = browse.redraw();
    await waitFor(() => expect(answers.map((pending) => pending.cmd)).toEqual(["browse", "redraw"]));
    answers.find((pending) => pending.cmd === "redraw")!.answer(2);
    await clock;
    expect(browse.revision).toBe(2);
    answers.find((pending) => pending.cmd === "browse")!.answer(2);
    await read;
    expect(browse.view?.revision).toBe(2);
  });
});
