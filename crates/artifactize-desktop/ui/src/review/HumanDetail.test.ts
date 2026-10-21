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
import { greenForm, humanView, redForm } from "./fixtures";
import type { FormView, ToolView } from "../ipc/client";

type Call = { cmd: string; args: Record<string, unknown> };

/** Answer commands like the backend would for one waiting request, recording every call. */
function backend(stage: "claim" | "review", submit?: (args: Record<string, unknown>) => unknown) {
  const calls: Call[] = [];
  let current = stage;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    switch (cmd) {
      case "human_open":
        return humanView(current);
      case "human_claim":
        current = "review";
        return { notice: { text: "Claimed. Fill GREEN or RED fields, then Submit.", error: false }, view: humanView("review") };
      case "human_form": {
        // The library hands a kept draft back when the schema still takes it.
        const asked = args as { verdict: string; kept: object | null };
        const form = asked.verdict === "RED" ? redForm : greenForm;
        return asked.kept ? { ...form, draft: asked.kept } : form;
      }
      case "human_submit":
        return submit?.(args as Record<string, unknown>);
      case "human_edit_field": {
        // The library's form takes the value as typed.
        const edit = args as { draft: { fields: [string, unknown][] }; name: string; value: unknown };
        return { ...edit.draft, fields: edit.draft.fields.map(([name, value]) => [name, name === edit.name ? edit.value : value]) };
      }
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return calls;
}

type Edited = { draft: { fields: [string, { kind: string; value: unknown[] }][] }; name: string; item?: number };

/** The library's answer to adding or removing an item of a list: `empty` is the list's new
 * item, and one is always left to type in. */
function listed(cmd: string, args: unknown, empty: unknown): unknown {
  const edit = args as Edited;
  const items = (value: unknown[]) => {
    const next = cmd === "human_add_item" ? [...value, empty] : value.toSpliced(edit.item!, 1);
    return next.length > 0 ? next : [empty];
  };
  return {
    ...edit.draft,
    fields: edit.draft.fields.map(([name, value]) => [name, name === edit.name ? { ...value, value: items(value.value) } : value]),
  };
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("HumanDetail", () => {
  it("claims only on an explicit c, never on a key that is part of a composition", async () => {
    const calls = backend("claim");
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByRole("button", { name: "Claim to review" });

    await fireEvent.keyDown(region, { key: "c", isComposing: true });
    await fireEvent.keyDown(region, { key: "c", keyCode: 229 });
    expect(calls.some((call) => call.cmd === "human_claim")).toBe(false);

    await fireEvent.keyDown(region, { key: "c" });
    await waitFor(() => expect(calls.some((call) => call.cmd === "human_claim")).toBe(true));
    await screen.findByText("REVIEW (yours)");
  });

  it("keeps GREEN and RED drafts apart and shows a schema error without losing the draft", async () => {
    const calls = backend("review", () => {
      throw { kind: "invalid", message: "Result does not match the fail schema: /reason: \"no\" is shorter than 3 characters" };
    });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("REVIEW (yours)");

    await fireEvent.keyDown(region, { key: "r" });
    const reason = await screen.findByLabelText(/reason/);
    await user.type(reason, "no");
    // Letters typed into a field are text, not shortcuts.
    expect(calls.filter((call) => call.cmd === "human_form").length).toBe(1);

    await fireEvent.keyDown(reason, { key: "g", ctrlKey: true });
    const comment = await screen.findByLabelText(/comment/);
    await user.type(comment, "확인");
    await fireEvent.keyDown(comment, { key: "r", ctrlKey: true });
    expect(((await screen.findByLabelText(/reason/)) as HTMLTextAreaElement).value).toBe("no");

    await fireEvent.keyDown(await screen.findByLabelText(/reason/), { key: "s", ctrlKey: true });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("/reason");
    const submitted = calls.find((call) => call.cmd === "human_submit");
    expect(submitted?.args.draft).toEqual({ verdict: "RED", fields: [["reason", { kind: "text", value: "no" }]] });
    expect(((await screen.findByLabelText(/reason/)) as HTMLTextAreaElement).value).toBe("no");
  });

  it("leaves an input method its Escape: a composing Escape neither stops editing nor leaves", async () => {
    backend("review");
    let left = 0;
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => (left += 1) });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("REVIEW (yours)");
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
    await screen.findByText("REVIEW (yours)");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    const writes = new Set(["human_form", "human_claim", "human_release", "human_run", "human_submit", "human_cancel"]);
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
    await waitFor(() => expect(calls.find(({ cmd }) => cmd === "human_edit_field")?.args.value).toEqual({ kind: "text", value: "안녕" }));
    await fireEvent.keyDown(comment, { key: "s", ctrlKey: true });
    await waitFor(() => expect(calls.some(({ cmd }) => cmd === "human_submit")).toBe(true));
    expect(calls.find(({ cmd }) => cmd === "human_submit")?.args.draft).toEqual({
      verdict: "GREEN", fields: [["approved", { kind: "fixed" }], ["comment", { kind: "text", value: "안녕" }]],
    });
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
    await screen.findByText("REVIEW (yours)");
    await user.keyboard("g");
    const comment = await screen.findByLabelText(/comment/);
    await waitFor(() => expect(document.activeElement).toBe(comment));
    await user.type(comment, "checked");
    if (from === "Submit button") await user.click(screen.getByRole("button", { name: /Submit/ }));
    else await user.keyboard("{Control>}s{/Control}");

    await screen.findByText("Completed result");
    expect(region.isConnected).toBe(true);
    expect(screen.queryByLabelText(/comment/)).toBeNull();
    expect(left).toBe(0);
    // Use the real focus target, not a key dispatched directly to the Detail.
    await user.keyboard("{Escape}");
    await waitFor(() => expect(left).toBe(1));
  });

  it("does not steal focus moved outside the Detail while submission is running", async () => {
    let finish!: (value: unknown) => void;
    backend("review", () => new Promise((resolve) => (finish = resolve)));
    const user = userEvent.setup();
    let submitted!: () => void;
    const settled = new Promise<void>((resolve) => (submitted = resolve));
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {}, onsubmitted: () => submitted() });
    await screen.findByText("REVIEW (yours)");
    await user.keyboard("g");
    await screen.findByLabelText(/comment/);
    await user.keyboard("{Control>}s{/Control}");
    await screen.findByText(/Submitting GREEN/);
    const elsewhere = document.createElement("button");
    elsewhere.textContent = "Another screen";
    document.body.append(elsewhere);
    try {
      await user.click(elsewhere);
      finish({ notice: null, view: humanView("completed"), settled: "published" });
      await screen.findByText("Completed result");
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
    await screen.findByText("REVIEW (yours)");
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
    const calls = backend("review");
    mockIPC((cmd, args) => {
      calls.push({ cmd, args: args as Record<string, unknown> });
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_form") return list;
      if (cmd === "human_add_item" || cmd === "human_remove_item") return listed(cmd, args, "");
      if (cmd === "human_edit_field") {
        const edit = args as { draft: { fields: [string, unknown][] }; name: string; value: unknown };
        return { ...edit.draft, fields: edit.draft.fields.map(([name, value]) => [name, name === edit.name ? edit.value : value]) };
      }
      return null;
    }, { shouldMockEvents: true });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("REVIEW (yours)");
    await user.click(screen.getByRole("button", { name: "RED" }));
    const first = await screen.findByLabelText("change 1");
    // The constant is the form's own: nothing to enter, so nothing shown.
    expect(screen.queryByText(/approved/)).toBeNull();
    await user.type(first, "Shorter title");
    await user.click(screen.getByRole("button", { name: "+ Add" }));
    await user.type(await screen.findByLabelText("change 2"), "Fix the date");
    await user.click(screen.getByRole("button", { name: "Remove change 1" }));
    await waitFor(() => expect((screen.getByLabelText("change 1") as HTMLTextAreaElement).value).toBe("Fix the date"));
    expect(screen.queryByLabelText("change 2")).toBeNull();
    // The library adds and removes the items; the form asked it each time.
    expect(calls.filter(({ cmd }) => cmd === "human_add_item" || cmd === "human_remove_item").map(({ cmd, args }) => [cmd, args.name, args.item])).toEqual([
      ["human_add_item", "change", undefined],
      ["human_remove_item", "change", 0],
    ]);
    expect(screen.getByRole("button", { name: "Submit RED" })).toBeTruthy();
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
    await screen.findByText("REVIEW (yours)");
    const groups = screen.getAllByRole("group", { name: /^Tools of / });
    expect(groups.map((group) => group.getAttribute("aria-label"))).toEqual(["Tools of release", "Tools of brand"]);
    await user.click(screen.getByRole("button", { name: "▶ preview" }));
    await waitFor(() => expect(ran).toEqual(["preview_brand"]));
    // The launched program keeps printing after its button returned, under its own entry.
    review.logged([
      { run: "1", at: 0, operation: "preview", artifact: "brand", end: null,
        lines: [{ text: "clicked (412, 1)", error: false }, { text: "clicked (412, 2)", error: true }] },
      { run: "2", at: 0, operation: "notes", artifact: "release", lines: [], end: { text: "■ exit 0", error: false } },
    ]);
    const log = screen.getByRole("log", { name: "Tool log" });
    await waitFor(() => expect(log.textContent).toContain("clicked (412, 2)"));
    const rows = [...log.querySelectorAll("p")].map((row) => row.textContent?.replace(/\d\d:\d\d:\d\d/, "HH:MM:SS"));
    expect(rows).toEqual(["▶ HH:MM:SS · preview · brand", "clicked (412, 1)", "clicked (412, 2)", "▶ HH:MM:SS · notes · release", "■ exit 0"]);
    expect(screen.getByText("clicked (412, 2)").classList.contains("error")).toBe(true);
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
    mockIPC((cmd, args) => {
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_form") return findings;
      if (cmd === "human_add_item" || cmd === "human_remove_item") {
        return listed(cmd, args, [["kind", { kind: "fixed" }], ["problem", { kind: "text", value: "" }], ["severity", { kind: "choice", value: null }]]);
      }
      if (cmd === "human_edit_field") {
        const edit = args as { draft: { fields: [string, unknown][] }; name: string; value: unknown };
        return { ...edit.draft, fields: edit.draft.fields.map(([name, value]) => [name, name === edit.name ? edit.value : value]) };
      }
      if (cmd === "human_submit") submitted.push((args as { draft: unknown }).draft);
      return null;
    }, { shouldMockEvents: true });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("REVIEW (yours)");
    await user.click(screen.getByRole("button", { name: "RED" }));
    const first = await screen.findByRole("group", { name: "findings 1" });
    // The group's constant is filled in, not shown.
    expect(within(first).queryByText(/kind/)).toBeNull();
    await user.type(within(first).getByLabelText(/problem/), "typo");
    await user.click(screen.getByRole("button", { name: "+ Add" }));
    const second = await screen.findByRole("group", { name: "findings 2" });
    await user.type(within(second).getByLabelText(/problem/), "date");
    await user.selectOptions(within(second).getByLabelText(/severity/), '"high"');
    await user.click(screen.getByRole("button", { name: "Remove findings 1" }));
    await waitFor(() => expect(screen.queryByRole("group", { name: "findings 2" })).toBeNull());
    await user.click(screen.getByRole("button", { name: "Submit RED" }));
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
    mockIPC((cmd) => (cmd === "human_open" ? humanView("review") : cmd === "human_form" ? optional : null), { shouldMockEvents: true });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await screen.findByText("REVIEW (yours)");
    await user.click(screen.getByRole("button", { name: "GREEN" }));
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
    const tools = await screen.findByRole("region", { name: "Tools" });
    await waitFor(() => expect(document.activeElement).toBe(tools));
  });
});
