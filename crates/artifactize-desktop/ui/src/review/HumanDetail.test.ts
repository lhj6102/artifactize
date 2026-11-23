// The shared Human Detail against a mocked command boundary: explicit claim, shortcuts that
// leave input methods alone, verdict drafts and correctable schema errors.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { tick } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import HumanDetail from "./HumanDetail.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "./review.svelte";
import { draftLibrary, greenForm, humanView } from "./fixtures";
import type { FormView, ReviewPoint, ToolView } from "../ipc/client";

type Call = { cmd: string; args: Record<string, unknown> };

// The review measures its own width to choose one column or two; jsdom has no layout.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

/** The library's draft of the request the newest backend answers for. */
let library = draftLibrary();

/** Answer commands like the backend would for one waiting request, recording every call; its
 * draft is the library's (`library`). `submit` answers a submission of the saved draft. */
function backend(stage: "claim" | "review", submit?: (args: Record<string, unknown>) => unknown, kept = draftLibrary()) {
  const calls: Call[] = [];
  let current = stage;
  library = kept;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    const drafted = kept.answer(cmd, args);
    if (drafted) return drafted;
    switch (cmd) {
      case "human_open":
        return humanView(current);
      case "human_claim":
        current = "review";
        return { notice: { text: "Claimed. Fill GREEN or RED fields, then Submit.", error: false }, view: humanView("review") };
      case "human_draft_submit":
        return submit?.(args as Record<string, unknown>);
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return calls;
}

/** The saved draft's fields of `verdict`, as the library would submit them. */
const saved = (verdict: "GREEN" | "RED") => (verdict === "GREEN" ? library.draft.green : library.draft.red).draft.fields;

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("HumanDetail", () => {
  it("claims only on an explicit c, never on a key that is part of a composition", async () => {
    const calls = backend("claim");
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByRole("button", { name: "Claim to review (c)" });

    await fireEvent.keyDown(region, { key: "c", isComposing: true });
    await fireEvent.keyDown(region, { key: "c", keyCode: 229 });
    expect(calls.some((call) => call.cmd === "human_claim")).toBe(false);

    await fireEvent.keyDown(region, { key: "c" });
    await waitFor(() => expect(calls.some((call) => call.cmd === "human_claim")).toBe(true));
    await screen.findByText("Yours");
  });

  it("says why a submission settled here was not published, under its completed result", async () => {
    const failure = "Saved locally; publishing to the review store failed: connection refused. Push results from Settings.";
    backend("review", () => ({
      settled: "savedLocally",
      notice: { text: failure, error: true },
      view: humanView("completed", { claim: null, result: '{ "verdict": "GREEN" }', meta: "GREEN · alice" }),
    }));
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("Yours");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    await userEvent.setup().type(comment, "checked");
    await fireEvent.keyDown(comment, { key: "s", ctrlKey: true });
    const result = await screen.findByRole("region", { name: "Verdict" });
    await waitFor(() => expect(within(result).getByRole("status").textContent).toBe(failure));
  });

  it("keeps GREEN and RED drafts apart and shows a schema error without losing the draft", async () => {
    const calls = backend("review", () => {
      throw { kind: "invalid", message: "Result does not match the fail schema: /reason: \"no\" is shorter than 3 characters" };
    });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("Yours");

    await fireEvent.keyDown(region, { key: "r" });
    const reason = await screen.findByLabelText(/reason/);
    await user.type(reason, "no");
    // Letters typed into a field are text, not shortcuts.
    expect(calls.filter((call) => call.cmd === "human_draft_edit" && (call.args.change as { kind: string }).kind === "verdict").length).toBe(1);

    await user.click(screen.getByRole("radio", { name: "GREEN" }));
    const comment = await screen.findByLabelText(/comment/);
    await user.type(comment, "확인");
    await user.click(screen.getByRole("radio", { name: "RED" }));
    expect(((await screen.findByLabelText(/reason/)) as HTMLTextAreaElement).value).toBe("no");

    await fireEvent.keyDown(await screen.findByLabelText(/reason/), { key: "s", ctrlKey: true });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("/reason");
    expect(calls.some((call) => call.cmd === "human_draft_submit")).toBe(true);
    expect(library.draft.verdict).toBe("RED");
    expect(saved("RED")).toEqual([["reason", { kind: "text", value: "no" }]]);
    expect(((await screen.findByLabelText(/reason/)) as HTMLTextAreaElement).value).toBe("no");
  });

  it("leaves an input method its Escape: a composing Escape neither stops editing nor leaves", async () => {
    backend("review");
    let left = 0;
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => (left += 1) });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("Yours");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    await fireEvent.keyDown(comment, { key: "Escape", isComposing: true });
    await fireEvent.keyDown(comment, { key: "Escape", keyCode: 229 });
    await fireEvent.keyDown(region, { key: "Escape", keyCode: 229 });
    expect(screen.queryByLabelText(/comment/)).not.toBeNull();
    expect(left).toBe(0);
  });

  it("leaves composition letters and chords alone, then commits and submits Korean text", async () => {
    const calls = backend("review", () => ({
      notice: { text: "Submitted GREEN", error: false },
      view: humanView("completed", { result: '{"verdict":"GREEN","approved":true,"comment":"안녕"}' }),
      settled: "published",
    }));
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("Yours");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    const writes = new Set(["human_draft_edit", "human_claim", "human_release", "human_run", "human_draft_submit", "human_cancel"]);
    const before = calls.filter(({ cmd }) => writes.has(cmd)).length;
    await fireEvent.compositionStart(comment);
    for (const key of ["g", "r", "q", "u", "c", "Escape"]) {
      await fireEvent.keyDown(comment, { key, isComposing: true });
    }
    for (const key of ["g", "r", "u", "s", "e"]) {
      await fireEvent.keyDown(comment, { key, ctrlKey: true, isComposing: true });
      await fireEvent.keyDown(comment, { key, metaKey: true, isComposing: true });
    }
    expect(calls.filter(({ cmd }) => writes.has(cmd))).toHaveLength(before);
    expect(screen.queryByLabelText(/comment/)).not.toBeNull();
    await fireEvent.input(comment, { target: { value: "안녕" }, inputType: "insertCompositionText", data: "안녕" });
    await fireEvent.compositionEnd(comment, { data: "안녕" });
    await waitFor(() => expect(saved("GREEN")).toEqual([["approved", { kind: "fixed" }], ["comment", { kind: "text", value: "안녕" }]]));
    await fireEvent.keyDown(comment, { key: "s", ctrlKey: true });
    await waitFor(() => expect(calls.some(({ cmd }) => cmd === "human_draft_submit")).toBe(true));
    expect(library.draft.verdict).toBe("GREEN");
  });

  it.each(["typed field", "Submit button"])("keeps the completed result keyboard-accessible after submitting from the %s", async (from) => {
    backend("review", () => ({
      notice: { text: "Submitted GREEN for release/signoff", error: false },
      view: humanView("completed", { result: '{"verdict":"GREEN","approved":true,"comment":"checked"}' }),
      settled: "published",
    }));
    const user = userEvent.setup();
    let left = 0;
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => (left += 1) });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("Yours");
    await user.keyboard("g");
    const comment = await screen.findByLabelText(/comment/);
    await waitFor(() => expect(document.activeElement).toBe(comment));
    await user.type(comment, "checked");
    if (from === "Submit button") await user.click(screen.getByRole("button", { name: /Submit/ }));
    else await user.keyboard("{Control>}s{/Control}");

    await completedResult();
    expect(region.isConnected).toBe(true);
    expect(screen.queryByLabelText(/comment/)).toBeNull();
    expect(left).toBe(0);
    // Use the real focus target, not a key dispatched directly to the Detail.
    await user.keyboard("{Escape}");
    await waitFor(() => expect(left).toBe(1));
  });

  it("submits while a tool runs, cancelling that tool first", async () => {
    const calls: Call[] = [];
    let stop!: () => void;
    const kept = draftLibrary();
    mockIPC((cmd, args) => {
      calls.push({ cmd, args: args as Record<string, unknown> });
      const drafted = kept.answer(cmd, args);
      if (drafted) return drafted;
      switch (cmd) {
        case "human_open":
          return humanView("review");
        case "human_run":
          // The tool runs until it is cancelled.
          return new Promise((resolve) => (stop = () => resolve({ text: "■ error: cancelled", error: true })));
        case "human_cancel":
          stop();
          return null;
        case "human_draft_submit":
          return { notice: { text: "Submitted GREEN", error: false }, view: humanView("completed"), settled: "published" };
        default:
          return null;
      }
    }, { shouldMockEvents: true });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("Yours");
    await user.click(screen.getByRole("button", { name: "GREEN (g)" }));
    await screen.findByLabelText(/comment/);
    await user.click(screen.getByRole("button", { name: "notes" }));
    await screen.findByRole("img", { name: "Running notes" });
    // Release waits for the running work; Submit does not.
    expect((screen.getByRole("button", { name: "Release claim (u)" }) as HTMLButtonElement).disabled).toBe(true);
    const submit = screen.getByRole("button", { name: "Submit GREEN (Ctrl+S)" }) as HTMLButtonElement;
    expect(submit.disabled).toBe(false);
    await user.click(submit);
    await completedResult();
    expect(calls.map(({ cmd }) => cmd).filter((cmd) => ["human_run", "human_cancel", "human_draft_submit"].includes(cmd))).toEqual([
      "human_run", "human_cancel", "human_draft_submit",
    ]);
  });

  it("does not steal focus moved outside the Detail while submission is running", async () => {
    let finish!: (value: unknown) => void;
    backend("review", () => new Promise((resolve) => (finish = resolve)));
    const user = userEvent.setup();
    let submitted!: () => void;
    const settled = new Promise<void>((resolve) => (submitted = resolve));
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {}, onsubmitted: () => submitted() });
    await screen.findByText("Yours");
    await user.keyboard("g");
    await screen.findByLabelText(/comment/);
    await user.keyboard("{Control>}s{/Control}");
    await screen.findByRole("img", { name: /Submitting GREEN/ });
    const elsewhere = document.createElement("button");
    elsewhere.textContent = "Another screen";
    document.body.append(elsewhere);
    try {
      await user.click(elsewhere);
      finish({ notice: null, view: humanView("completed"), settled: "published" });
      await completedResult();
      // The submission's focus recovery runs on the tick after it settled; wait for that step.
      await settled;
      await tick();
      expect(document.activeElement).toBe(elsewhere);
    } finally {
      elsewhere.remove();
    }
  });

  it("Esc stops editing first, then leaves", async () => {
    backend("review");
    let left = 0;
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => (left += 1) });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("Yours");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    await fireEvent.keyDown(comment, { key: "Escape" });
    await waitFor(() => expect(screen.queryByLabelText(/comment/)).toBeNull());
    expect(left).toBe(0);
    // The removed field's focus returns to the Detail, where the next Esc lands.
    await waitFor(() => expect(document.activeElement).toBe(region));
    await fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(left).toBe(1);
  });

  it("hides constants and enters a list one item at a time, added and removed", async () => {
    const list: FormView = {
      verdict: "RED",
      fields: [
        { name: "approved", required: true, hint: "", input: { kind: "fixed", value: "false" } },
        { name: "change", required: true, hint: "array", input: { kind: "list" } },
      ],
      draft: { verdict: "RED", fields: [["approved", { kind: "fixed" }], ["change", { kind: "list", value: [""] }]] },
    };
    const calls = backend("review", undefined, draftLibrary(greenForm, list, { change: "" }));
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("Yours");
    await user.click(screen.getByRole("button", { name: "RED (r)" }));
    const first = await screen.findByLabelText("change 1");
    // The constant is the form's own: nothing to enter, so nothing shown.
    expect(screen.queryByText(/approved/)).toBeNull();
    await user.type(first, "Shorter title");
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.type(await screen.findByLabelText("change 2"), "Fix the date");
    await user.click(screen.getByRole("button", { name: "Remove change 1" }));
    await waitFor(() => expect((screen.getByLabelText("change 1") as HTMLTextAreaElement).value).toBe("Fix the date"));
    expect(screen.queryByLabelText("change 2")).toBeNull();
    // The library adds and removes the items; the form asked it each time.
    const items = calls.flatMap(({ cmd, args }) => {
      const change = args.change as { kind: string; name?: string; item?: number } | undefined;
      return cmd === "human_draft_edit" && (change?.kind === "addItem" || change?.kind === "removeItem") ? [[change.kind, change.name, change.item]] : [];
    });
    expect(items).toEqual([
      ["addItem", "change", undefined],
      ["removeItem", "change", 0],
    ]);
    expect(screen.getByRole("button", { name: "Submit RED (Ctrl+S)" })).toBeTruthy();
  });

  it("groups the tools by their Artifact, the reviewed one first, and logs each run's lines under it", async () => {
    const tools: ToolView[] = [
      { name: "preview_brand", artifact: "brand", operation: "preview", kind: "launch", description: "Show the icon.", action: "preview", error: null },
      { name: "notes_release", artifact: "release", operation: "notes", kind: "output", description: "Print the notes.", action: "$ cat notes.md", error: null },
    ];
    const ran: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "human_open") return humanView("review", { tools });
      if (cmd === "human_run") {
        ran.push((args as { tool: string }).tool);
        return { text: "Ran", error: false };
      }
      return null;
    }, { shouldMockEvents: true });
    const user = userEvent.setup();
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    await screen.findByText("Yours");
    const groups = screen.getAllByRole("group", { name: /^Tools of / });
    expect(groups.map((group) => group.getAttribute("aria-label"))).toEqual(["Tools of release", "Tools of brand"]);
    await user.click(screen.getByRole("button", { name: "preview" }));
    await waitFor(() => expect(ran).toEqual(["preview_brand"]));
    // The launched program keeps printing after its button returned, under its own entry.
    review.logged([
      { run: "1", at: 0, operation: "preview", artifact: "brand", end: null,
        lines: [{ text: "clicked (412, 1)", error: false }, { text: "clicked (412, 2)", error: true }] },
      { run: "2", at: 0, operation: "notes", artifact: "release", lines: [], end: { text: "■ exit 0", error: false } },
    ]);
    // Folded, the Tool log is one line: the last run's start and end.
    const fold = await screen.findByRole("button", { name: "Tool log" });
    await waitFor(() => expect(fold.textContent).toContain("■ exit 0 · 0 lines"));
    expect(fold.textContent).toMatch(/▶ \d\d:\d\d:\d\d · notes/);
    expect(screen.queryByRole("log", { name: "Tool log" })).toBeNull();
    fold.focus();
    await user.keyboard("{Enter}");
    expect(fold.getAttribute("aria-expanded")).toBe("true");
    const log = screen.getByRole("log", { name: "Tool log" });
    await waitFor(() => expect(log.textContent).toContain("clicked (412, 2)"));
    const rows = [...log.querySelectorAll("p")].map((row) => row.textContent?.replace(/\d\d:\d\d:\d\d/, "HH:MM:SS"));
    expect(rows).toEqual(["▶ HH:MM:SS · preview · brand", "clicked (412, 1)", "clicked (412, 2)", "▶ HH:MM:SS · notes · release", "■ exit 0"]);
    expect(screen.getByText("clicked (412, 2)").classList.contains("stderr")).toBe(true);
  });

  it("summarizes a running tool and folds failures without losing earlier runs", async () => {
    backend("review");
    const user = userEvent.setup();
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    await screen.findByText("Yours");
    expect(screen.queryByRole("log", { name: "Tool log" })).toBeNull();
    const run = { run: "1", at: 0, operation: "scan", artifact: "release", lines: [
      { text: "early", error: false }, { text: "warning: missing title", error: true }, { text: "still checking", error: false },
    ] };
    review.logged([{ ...run, end: null }]);
    // Folded to one line: its start, its last stderr line, its newest line while it runs.
    const header = await screen.findByRole("button", { name: "Tool log" });
    const parts = () => [...header.querySelectorAll("span")].map((part) => part.textContent?.replace(/\d\d:\d\d:\d\d/, "HH:MM:SS")).filter((text) => text);
    await waitFor(() => expect(parts()).toEqual(["▶ HH:MM:SS · scan · release", "warning: missing title", "still checking"]));
    review.logged([{ ...run, end: { text: "■ exit 1", error: true } }]);
    await waitFor(() => expect(parts()).toEqual(["▶ HH:MM:SS · scan · release", "warning: missing title", "■ exit 1 · 3 lines"]));
    expect(header.querySelector(".end")?.classList.contains("failed")).toBe(true);
    expect(header.getAttribute("aria-expanded")).toBe("false");
    // A click unfolds every line in the log box, another folds it to its line again.
    await user.click(header);
    const log = screen.getByRole("log", { name: "Tool log" });
    const rows = [...log.querySelectorAll("p")].map((row) => row.textContent?.replace(/\d\d:\d\d:\d\d/, "HH:MM:SS"));
    expect(rows).toEqual(["▶ HH:MM:SS · scan · release", "early", "warning: missing title", "still checking", "■ exit 1"]);
    await user.click(header);
    expect(screen.queryByRole("log", { name: "Tool log" })).toBeNull();
  });

  it("shows Points below the log, inserts at the text caret and removes by point identity", async () => {
    const old: ReviewPoint = { text: "L2:C9", at: "2026-11-23T00:01:00Z", source: "release · Markdown" };
    const fresh: ReviewPoint = { text: "#checkout .total", at: "2026-11-23T00:02:00Z", source: "checkout · HTML Viewer" };
    const form: FormView = { ...greenForm, fields: [
      { name: "quantity", required: false, hint: "", input: { kind: "number" } },
      ...greenForm.fields,
      { name: "details", required: false, hint: "", input: { kind: "text" } },
    ], draft: { ...greenForm.draft, fields: [
      ["quantity", { kind: "number", value: "2" }], ...greenForm.draft.fields, ["details", { kind: "text", value: "left  right" }],
    ] } };
    const kept = draftLibrary(form);
    const calls = backend("review", undefined, kept);
    const user = userEvent.setup();
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    await screen.findByText("Yours");
    expect(screen.queryByRole("list", { name: "Points" })).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    kept.draft.points = [old, fresh];
    await review.readDraft();
    review.logged([{ run: "1", at: 0, operation: "scan", artifact: "release", lines: [], end: { text: "■ exit 0", error: false } }]);
    const points = await screen.findByRole("list", { name: "Points" });
    const [first, second] = within(points).getAllByRole("listitem");
    expect(first.textContent).toContain(old.text);
    expect(second.textContent).toContain(fresh.text);
    expect(second.textContent).toContain(`${fresh.source} · ${new Date(fresh.at).toTimeString().slice(0, 5)}`);
    expect(second.classList.contains("newest")).toBe(true);
    const regions = [...screen.getByLabelText("Human review").querySelectorAll("section[aria-label]")];
    expect(regions.map((region) => region.getAttribute("aria-label"))).toEqual(["Evidence", "Verdict", "Debug"]);
    const insert = within(second).getByRole<HTMLButtonElement>("button", { name: "Insert" });
    expect(insert.disabled).toBe(true);
    await user.click(screen.getByRole("button", { name: "GREEN (g)" }));
    const quantity = await screen.findByRole<HTMLInputElement>("textbox", { name: "quantity" });
    quantity.focus();
    const comment = screen.getByLabelText<HTMLTextAreaElement>(/^comment/);
    await user.click(insert);
    expect(comment.value).toBe(fresh.text);
    expect(quantity.value).toBe("2");
    const details = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "details" });
    details.focus();
    details.setSelectionRange(5, 5);
    quantity.focus();
    await user.click(insert);
    expect(details.value).toBe(`left ${fresh.text} right`);
    expect(comment.value).toBe(fresh.text);
    expect(saved("GREEN")).toContainEqual(["details", { kind: "text", value: `left ${fresh.text} right` }]);
    await user.click(within(second).getByRole("button", { name: `Remove point: ${fresh.text}` }));
    await waitFor(() => expect(within(points).getAllByRole("listitem")).toHaveLength(1));
    expect(first.classList.contains("newest")).toBe(false);
    expect(calls.filter((call) => call.cmd === "human_draft_remove_point")).toEqual([
      { cmd: "human_draft_remove_point", args: { id: "run-a-2", point: fresh } },
    ]);
    await user.click(within(first).getByRole("button", { name: `Remove point: ${old.text}` }));
    await waitFor(() => expect(screen.queryByRole("list", { name: "Points" })).toBeNull());
  });

  it("follows the saved verdict when another window changes it while the form is open", async () => {
    const library = draftLibrary();
    library.draft.verdict = "GREEN";
    backend("review", undefined, library);
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    await screen.findByRole("button", { name: "Submit GREEN (Ctrl+S)" });
    library.draft.verdict = "RED";
    await review.readDraft();
    await screen.findByRole("button", { name: "Submit RED (Ctrl+S)" });
    expect(screen.queryByRole("button", { name: "Submit GREEN (Ctrl+S)" })).toBeNull();
    expect(screen.getByLabelText(/^reason/)).toBeTruthy();
  });

  it("keeps an in-flight edit and a Point that arrives before the edit's answer", async () => {
    const kept = draftLibrary();
    kept.draft.verdict = "GREEN";
    let finish: (() => void) | undefined;
    let reread = false;
    mockIPC(async (cmd, args) => {
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_draft" && reread) kept.draft.points.push({ text: "L4:C1", source: "release · Markdown", at: "2026-11-23T00:00:02Z" });
      // Delay saving the edit itself so the first Point invalidation returns the old text.
      if (cmd === "human_draft_edit") await new Promise<void>((resolve) => (finish = resolve));
      return kept.answer(cmd, args) ?? null;
    }, { shouldMockEvents: true });
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    const comment = await screen.findByLabelText<HTMLTextAreaElement>(/^comment/);
    await fireEvent.input(comment, { target: { value: "Still typing" } });
    await waitFor(() => expect(finish).toBeDefined());
    kept.draft.points.push({ text: "L2:C9", source: "release · Markdown", at: "2026-11-23T00:00:01Z" });
    await review.readDraft();
    await screen.findByText("L2:C9");
    expect(comment.value).toBe("Still typing");
    reread = true;
    finish?.();
    await screen.findByText("L4:C1");
    expect(screen.getByText("L2:C9")).toBeTruthy();
    expect(comment.value).toBe("Still typing");
  });

  it("keeps typed text while the refresh after its save is superseded", async () => {
    const kept = draftLibrary();
    kept.draft.verdict = "GREEN";
    let save: (() => void) | undefined;
    let deferReads = false;
    const reads: (() => void)[] = [];
    mockIPC(async (cmd, args) => {
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_draft_edit") await new Promise<void>((resolve) => (save = resolve));
      const snapshot = kept.answer(cmd, args) ?? null;
      if (cmd === "human_draft" && deferReads) await new Promise<void>((resolve) => reads.push(resolve));
      return snapshot;
    }, { shouldMockEvents: true });
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    const comment = await screen.findByLabelText<HTMLTextAreaElement>(/^comment/);
    await fireEvent.input(comment, { target: { value: "Checking g" } });
    await waitFor(() => expect(save).toBeDefined());
    await review.readDraft(); // While the write is in flight, the saved snapshot is still old.
    deferReads = true;
    save?.();
    await waitFor(() => expect(reads).toHaveLength(1));
    const newest = review.readDraft(); // Another invalidation supersedes the write's refresh.
    await waitFor(() => expect(reads).toHaveLength(2));
    const ready = review.form!.ready;
    try {
      reads[0]();
      const settled = await Promise.race([ready.then(() => true), new Promise<false>((resolve) => setTimeout(() => resolve(false), 0))]);
      expect(settled).toBe(false);
      expect(comment.value).toBe("Checking g");
    } finally { reads[1](); }
    await newest;
    await ready;
    expect(comment.value).toBe("Checking g");
  });

  it("enters a list of flat objects as groups of fields, added and removed", async () => {
    const findings: FormView = {
      verdict: "RED",
      fields: [
        {
          name: "findings",
          required: true,
          hint: "array",
          input: {
            kind: "groups",
            fields: [
              { name: "kind", required: false, hint: "", input: { kind: "fixed", value: '"review"' } },
              { name: "problem", required: true, hint: "string", input: { kind: "text" } },
              { name: "severity", required: false, hint: "", input: { kind: "choice", options: ['"low"', '"high"'] } },
            ],
          },
        },
      ],
      draft: {
        verdict: "RED",
        fields: [["findings", { kind: "groups", value: [[["kind", { kind: "fixed" }], ["problem", { kind: "text", value: "" }], ["severity", { kind: "choice", value: null }]]] }]],
      },
    };
    const submitted: unknown[] = [];
    const empty = [["kind", { kind: "fixed" }], ["problem", { kind: "text", value: "" }], ["severity", { kind: "choice", value: null }]];
    backend("review", () => {
      submitted.push({ verdict: library.draft.verdict, fields: saved("RED") });
      return null;
    }, draftLibrary(greenForm, findings, { findings: empty }));
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("Yours");
    await user.click(screen.getByRole("button", { name: "RED (r)" }));
    const first = await screen.findByRole("group", { name: "findings 1" });
    // The group's constant is filled in, not shown.
    expect(within(first).queryByText(/kind/)).toBeNull();
    await user.type(within(first).getByLabelText(/problem/), "typo");
    await user.click(screen.getByRole("button", { name: "Add" }));
    const second = await screen.findByRole("group", { name: "findings 2" });
    await user.type(within(second).getByLabelText(/problem/), "date");
    await user.selectOptions(within(second).getByLabelText(/severity/), '"high"');
    await user.click(screen.getByRole("button", { name: "Remove findings 1" }));
    await waitFor(() => expect(screen.queryByRole("group", { name: "findings 2" })).toBeNull());
    await user.click(screen.getByRole("button", { name: "Submit RED (Ctrl+S)" }));
    await waitFor(() =>
      expect(submitted).toEqual([
        {
          verdict: "RED",
          fields: [["findings", { kind: "groups", value: [[["kind", { kind: "fixed" }], ["problem", { kind: "text", value: "date" }], ["severity", { kind: "choice", value: 1 }]]] }]],
        },
      ]),
    );
  });

  it("says in grey that an optional field may stay empty", async () => {
    const optional: FormView = { ...greenForm, fields: greenForm.fields.map((field) => ({ ...field, required: field.name !== "comment" })) };
    backend("review", undefined, draftLibrary(optional));
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("Yours");
    await user.click(screen.getByRole("button", { name: "GREEN (g)" }));
    const comment = await screen.findByLabelText(/comment/);
    expect(comment.getAttribute("placeholder")).toBe("Optional");
  });
});

describe("Tools focused", () => {
  it("gives a review opened with Tools focused the keyboard on its Tools", async () => {
    backend("review");
    const review = new Review("run-a-2", new Jobs());
    review.area = "tools";
    renderHosted(HumanDetail, { review, onback: () => {} });
    const tools = await screen.findByRole("group", { name: "Tools" });
    await waitFor(() => expect(document.activeElement).toBe(tools));
  });
});

/** Wait for the submitted review's completed result in place of its form. */
async function completedResult(): Promise<void> {
  await waitFor(() => expect(screen.getByRole("region", { name: "Verdict" }).querySelector(".completed")).not.toBeNull());
}
