// The Review state against a mocked command boundary: verdict forms that answer out of
// order, and JSON drafts per verdict.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import { Review } from "./review.svelte";
import { greenForm, humanView, redForm } from "./fixtures";

/** A form request the test answers when it chooses. */
type Pending = { verdict: string; answer: () => void };

function backend() {
  const pending: Pending[] = [];
  const drafts: unknown[] = [];
  mockIPC(async (cmd, args) => {
    const payload = args as Record<string, unknown>;
    if (cmd === "human_open") return humanView("review");
    if (cmd === "human_json") {
      drafts.push(payload.draft);
      return '{\n  "approved": true,\n  "comment": ""\n}';
    }
    if (cmd === "human_form") {
      const verdict = payload.verdict as string;
      await new Promise<void>((answer) => pending.push({ verdict, answer }));
      return verdict === "RED" ? redForm : greenForm;
    }
    return null;
  });
  return { pending, drafts };
}

const settle = () => new Promise((done) => setTimeout(done, 0));

afterEach(() => clearMocks());

describe("Review", () => {
  it("shows the verdict chosen last when its form arrives before the earlier one", async () => {
    const { pending } = backend();
    const review = new Review("run-a-2", false);
    await review.load();
    const green = review.choose("GREEN");
    const red = review.choose("RED");
    await settle();
    pending.find((request) => request.verdict === "RED")?.answer();
    await red;
    pending.find((request) => request.verdict === "GREEN")?.answer();
    await green;
    expect(review.form?.verdict).toBe("RED");
  });

  it("opens no form that arrives after editing stopped", async () => {
    const { pending } = backend();
    const review = new Review("run-a-2", false);
    await review.load();
    const green = review.choose("GREEN");
    await settle();
    review.stopEditing();
    pending[0].answer();
    await green;
    expect(review.form).toBeNull();
  });

  it("edits a typed form as JSON and keeps that JSON as the verdict's draft", async () => {
    const { pending, drafts } = backend();
    const review = new Review("run-a-2", false);
    await review.load();
    const green = review.choose("GREEN");
    await settle();
    pending[0].answer();
    await green;
    review.form!.values.comment = { kind: "text", value: "x" };
    await review.editJson();
    expect(drafts[0]).toEqual({
      verdict: "GREEN",
      fields: [
        ["approved", { kind: "fixed" }],
        ["comment", { kind: "text", value: "x" }],
      ],
      json: null,
    });
    expect(review.form?.json).toContain('"approved": true');
    const red = review.choose("RED");
    await settle();
    pending[1].answer();
    await red;
    expect(review.form?.json).toBeNull();
    await review.choose("GREEN");
    expect(review.form?.json).toContain('"approved": true');
  });

  it("converts newer values typed while a JSON conversion was on its way", async () => {
    const pending: { draft: { fields: [string, { value?: string }][] }; answer: () => void }[] = [];
    mockIPC(async (cmd, args) => {
      const payload = args as Record<string, unknown>;
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_form") return greenForm;
      if (cmd === "human_json") {
        const draft = payload.draft as { fields: [string, { value?: string }][] };
        await new Promise<void>((answer) => pending.push({ draft, answer }));
        const comment = draft.fields.find(([name]) => name === "comment")?.[1].value;
        return JSON.stringify({ approved: true, comment });
      }
      return null;
    });
    const review = new Review("run-a-2", false);
    await review.load();
    await review.choose("GREEN");
    review.form!.values.comment = { kind: "text", value: "before" };
    const converting = review.editJson();
    await settle();
    review.form!.values.comment = { kind: "text", value: "after" };
    pending[0].answer();
    await settle();
    pending[1].answer();
    await converting;
    expect(JSON.parse(review.form!.json!)).toEqual({ approved: true, comment: "after" });
  });
});
