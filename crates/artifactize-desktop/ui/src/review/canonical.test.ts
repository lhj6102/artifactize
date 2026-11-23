// Canonical request identity at the IPC boundary owns both screens' forms and drafts.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it } from "vitest";
import { waitFor } from "@testing-library/svelte";
import { AppState } from "../shell/app.svelte";
import { draftLibrary, greenForm, humanView, scoredRed } from "./fixtures";

afterEach(() => clearMocks());

function backend() {
  const library = draftLibrary(greenForm, scoredRed);
  mockIPC(async (command, args) => {
    if (command === "human_open") return humanView("review");
    const drafted = library.answer(command, args);
    if (drafted) return drafted;
    throw new Error(`Unexpected command: ${command}`);
  });
}

for (const [first, second] of [["run-a-2", "run-follower-2"], ["run-follower-2", "run-a-2"]]) {
  it(`keeps one canonical owner when ${first} opens before ${second}`, async () => {
    backend();
    const app = new AppState();
    const opened = app.review(first);
    await opened.load();
    await opened.choose("RED");
    await opened.form!.set("note", { kind: "text", value: "first draft" });
    await opened.addReason("first reason");
    const originalForm = opened.form;

    // The screen may already hold this handle when its read discovers the alias.
    const held = app.review(second);
    await held.load();
    expect(app.reviews["run-a-2"]).toBe(opened);
    expect(app.reviews["run-follower-2"]).toBe(opened);
    expect(held.form).toBe(originalForm);
    await held.form!.set("note", { kind: "text", value: "through the alias" });
    expect(opened.form!.values.note).toEqual({ kind: "text", value: "through the alias" });
    await held.addReason("second reason");
    expect(opened.reasons).toEqual(["first reason", "second reason"]);
    await held.choose("GREEN");
    expect(opened.form).toBe(held.form);
    await opened.choose("RED");
    expect(held.form!.values.note).toEqual({ kind: "text", value: "through the alias" });
    expect(held.verdict).toBe("RED");
  });
}

it("converges when original and follower handles both exist before either read finishes", async () => {
  const answers: Record<string, () => void> = {};
  const library = draftLibrary();
  mockIPC(async (command, args) => {
    if (command === "human_draft") return library.answer(command, args);
    if (command !== "human_open") throw new Error(`Unexpected command: ${command}`);
    const id = (args as { id: string }).id;
    await new Promise<void>((answer) => (answers[id] = answer));
    return humanView("review");
  });
  const app = new AppState();
  const original = app.review("run-a-2");
  const follower = app.review("run-follower-2");
  const originalRead = original.load();
  const followerRead = follower.load();
  await waitFor(() => expect(Object.keys(answers)).toEqual(["run-a-2", "run-follower-2"]));
  answers["run-follower-2"]();
  await followerRead;
  answers["run-a-2"]();
  await originalRead;
  original.area = "tools";
  expect(follower.area).toBe("tools");
  const point = { text: "install.md L2:C1", source: "release · File Explorer", at: "2026-11-23T00:00:00Z" };
  library.draft.points.push(point);
  await original.readDraft();
  expect(follower.draft?.points).toEqual([point]);
  expect(app.reviews["run-a-2"]).toBe(original);
  expect(app.reviews["run-follower-2"]).toBe(original);
});
