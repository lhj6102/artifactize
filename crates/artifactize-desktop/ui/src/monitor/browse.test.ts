// The monitor snapshot against a mocked command boundary: clock redraws and reads that answer
// out of order never roll the shown snapshot back, and a newer snapshot always advances the
// revision Detail reads by.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import type { BrowseView } from "../ipc/client";
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

const settle = () => new Promise((done) => setTimeout(done, 0));

afterEach(() => clearMocks());

describe("Browse", () => {
  it("keeps a read's snapshot when a clock redraw of the older one answers after it", async () => {
    const answers = backend();
    const browse = new Browse();
    browse.view = view(1);
    const read = browse.navigate({ full: true });
    const clock = browse.redraw();
    await settle();
    answers.find((pending) => pending.cmd === "browse")?.answer(2);
    await read;
    answers.find((pending) => pending.cmd === "redraw")?.answer(1);
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
    await settle();
    answers.find((pending) => pending.cmd === "redraw")?.answer(2);
    await clock;
    expect(browse.revision).toBe(2);
    answers.find((pending) => pending.cmd === "browse")?.answer(2);
    await read;
    expect(browse.view?.revision).toBe(2);
  });
});
