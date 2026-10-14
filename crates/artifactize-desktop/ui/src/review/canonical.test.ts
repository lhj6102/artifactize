// Canonical request identity at the IPC boundary owns both screens' forms and drafts.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it } from "vitest";
import { AppState } from "../shell/app.svelte";
import { greenForm, humanView, redForm } from "./fixtures";

afterEach(() => clearMocks());

function backend() {
  mockIPC(async (command, args) => {
    const payload = args as Record<string, unknown>;
    if (command === "human_open") return humanView("review");
    if (command === "human_form") {
      const form = payload.verdict === "RED" ? redForm : greenForm;
      return payload.kept ? { ...form, draft: payload.kept } : form;
    }
    if (command === "human_edit_field") {
      const edit = payload as { draft: { fields: [string, unknown][] }; name: string; value: unknown };
      return { ...edit.draft, fields: edit.draft.fields.map(([name, value]) => [name, name === edit.name ? edit.value : value]) };
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

for (const [first, second] of [["run-a-2", "run-follower-2"], ["run-follower-2", "run-a-2"]]) {
  it(`keeps one canonical owner when ${first} opens before ${second}`, async () => {
    backend();
    const app = new AppState();
    const opened = app.review(first);
    await opened.load();
    await opened.choose("GREEN");
    await opened.form!.set("comment", { kind: "text", value: "first draft" });
    const originalForm = opened.form;

    // The screen may already hold this handle when its read discovers the alias.
    const held = app.review(second);
    await held.load();
    expect(app.review("run-a-2")).toBe(opened);
    expect(app.review("run-follower-2")).toBe(opened);
    expect(held.form).toBe(originalForm);
    await held.form!.set("comment", { kind: "text", value: "through the alias" });
    expect(opened.form!.values.comment).toEqual({ kind: "text", value: "through the alias" });
    await held.choose("RED");
    expect(opened.form).toBe(held.form);
    await opened.choose("GREEN");
    expect(held.form!.values.comment).toEqual({ kind: "text", value: "through the alias" });
    held.stopEditing();
    expect(opened.form).toBeNull();
  });
}

it("converges when original and follower handles both exist before either read finishes", async () => {
  const answers: Record<string, () => void> = {};
  mockIPC(async (command, args) => {
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
  await Promise.resolve();
  answers["run-follower-2"]();
  await followerRead;
  answers["run-a-2"]();
  await originalRead;
  original.technical = true;
  expect(follower.technical).toBe(true);
  expect(app.review("run-a-2")).toBe(original);
  expect(app.review("run-follower-2")).toBe(original);
});
