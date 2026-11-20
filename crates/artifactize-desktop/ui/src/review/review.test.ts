// The Review state against a mocked command boundary: verdict forms that answer out of
// order, drafts per verdict, and the Tool log the backend's lines build.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { Jobs } from "../ipc/jobs.svelte";
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
    if (cmd === "human_edit_field") {
      // The library's form takes the value as typed.
      const edit = payload as { draft: { fields: [string, unknown][] }; name: string; value: unknown };
      return { ...edit.draft, fields: edit.draft.fields.map(([name, value]) => [name, name === edit.name ? edit.value : value]) };
    }
    if (cmd === "human_form") {
      drafts.push(payload.kept);
      const verdict = payload.verdict as string;
      await new Promise<void>((answer) => pending.push({ verdict, answer }));
      // The library hands a kept draft back when the schema still takes it.
      const form = verdict === "RED" ? redForm : greenForm;
      return payload.kept ? { ...form, draft: payload.kept } : form;
    }
    return null;
  });
  return { pending, drafts };
}

afterEach(() => clearMocks());

describe("Review", () => {
  it("shows the verdict chosen last when its form arrives before the earlier one", async () => {
    const { pending } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const green = review.choose("GREEN");
    const red = review.choose("RED");
    await waitFor(() => expect(pending.map((request) => request.verdict)).toEqual(["GREEN", "RED"]));
    pending.find((request) => request.verdict === "RED")!.answer();
    await red;
    pending.find((request) => request.verdict === "GREEN")!.answer();
    await green;
    expect(review.form?.verdict).toBe("RED");
  });

  it("opens no form that arrives after editing stopped", async () => {
    const { pending } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const green = review.choose("GREEN");
    await waitFor(() => expect(pending).toHaveLength(1));
    review.stopEditing();
    pending[0].answer();
    await green;
    expect(review.form).toBeNull();
  });

  it("keeps each verdict's typed draft when switching verdicts", async () => {
    const { pending, drafts } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const green = review.choose("GREEN");
    await waitFor(() => expect(pending).toHaveLength(1));
    pending[0].answer();
    await green;
    await review.form!.set("comment", { kind: "text", value: "x" });
    const red = review.choose("RED");
    await waitFor(() => expect(pending[1]?.verdict).toBe("RED"));
    pending[1].answer();
    await red;
    const again = review.choose("GREEN");
    await waitFor(() => expect(pending[2]?.verdict).toBe("GREEN"));
    pending[2].answer();
    await again;
    // The kept draft goes back to the library, which answers with it.
    expect(drafts[2]).toEqual({
      verdict: "GREEN",
      fields: [
        ["approved", { kind: "fixed" }],
        ["comment", { kind: "text", value: "x" }],
      ],
    });
    expect(review.form?.values.comment).toEqual({ kind: "text", value: "x" });
  });

  it("keeps the claim and the open form when a read from before the claim answers after it", async () => {
    // The first read is held until the test answers it; the claim reads after it.
    let answer!: () => void;
    const held = new Promise<void>((done) => (answer = done));
    let opened = 0;
    mockIPC(async (cmd, args) => {
      if (cmd === "human_open") {
        opened += 1;
        if (opened === 2) await held;
        return humanView("claim", { read: opened });
      }
      if (cmd === "human_claim") return { notice: { text: "Claimed.", error: false }, view: humanView("review", { read: 3 }) };
      if (cmd === "human_form") return greenForm;
      if (cmd === "human_edit_field") return (args as { draft: unknown }).draft;
      return null;
    });
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const late = review.load();
    await waitFor(() => expect(opened).toBe(2));
    await review.claim();
    await review.choose("GREEN");
    expect(review.owned).toBe(true);
    answer();
    await late;
    expect(review.view?.stage).toBe("review");
    expect(review.form?.verdict).toBe("GREEN");
  });

  it("runs a tool for its notice; its lines arrive in the Tool log on their own", async () => {
    mockIPC(async (cmd) => {
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_run") return { text: "notes_release launched.", error: false };
      return null;
    });
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await review.run(0);
    expect(review.notice?.text).toBe("notes_release launched.");
    expect(review.log).toEqual([]);
    review.logged([{ run: "7", at: 0, operation: "notes", artifact: "release", lines: [{ text: "hello", error: false }], end: null }]);
    expect(review.log.map((entry) => entry.lines)).toEqual([[{ text: "hello", error: false }]]);
  });
});
