// The Review state against a mocked command boundary: verdict forms that answer out of
// order, drafts per verdict, and the Tool log the backend's lines build.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "./review.svelte";
import { logged } from "./tool-log";
import type { LogLine } from "../ipc/client";
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
    review.logged([
      { request: "run-a-2", run: "7", line: { kind: "start", at: 0, operation: "notes", artifact: "release" } },
      { request: "run-a-2", run: "7", line: { kind: "stdout", text: "hello" } },
    ]);
    expect(review.log.map((entry) => entry.lines)).toEqual([[{ text: "hello", error: false }]]);
  });
});

describe("logged", () => {
  const line = (run: string, line: LogLine["line"]): LogLine => ({ request: "run-a-2", run, line });

  it("groups lines under their run, also after a later run started, and ends each once", () => {
    const log = logged([], [
      line("1", { kind: "start", at: 0, operation: "launch", artifact: "app" }),
      line("2", { kind: "start", at: 0, operation: "check", artifact: "lib" }),
      line("2", { kind: "stdout", text: "checked" }),
      line("2", { kind: "end", text: "■ exit 0", error: false }),
      line("1", { kind: "stderr", text: "app says" }),
    ]);
    const later = logged(log, [line("1", { kind: "end", text: "■ exit 1", error: true }), line("9", { kind: "stdout", text: "never started here" })]);
    expect(later.map(({ operation, lines, end }) => ({ operation, lines, end }))).toEqual([
      { operation: "launch", lines: [{ text: "app says", error: true }], end: { text: "■ exit 1", error: true } },
      { operation: "check", lines: [{ text: "checked", error: false }], end: { text: "■ exit 0", error: false } },
    ]);
    // Taking lines in never changes the log it was given.
    expect(log[0].end).toBeNull();
    expect(later[0].at).toMatch(/^\d\d:\d\d:\d\d$/);
  });
});
