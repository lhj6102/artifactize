// A Human review as a conversation against a mocked command boundary: an explicit claim, the
// issues written one at a time as bubbles, the decision bar's claim, verdict and Submit with why
// Submit waits, a verdict's own fields when the eval asks more, each tool run as its own line,
// Points inserted into the issues field, and keys that leave input methods alone.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import HumanDetail from "./HumanDetail.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "./review.svelte";
import { draftLibrary, humanView, scoredRed, greenForm } from "./fixtures";
import type { DraftChange, ReviewPoint } from "../ipc/client";

type Call = { cmd: string; args: Record<string, unknown> };

globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

/** Answer commands like the backend would for one waiting request, recording every call; its
 * draft is `kept`'s. `submit` answers a submission of the saved draft. */
function backend(stage: "claim" | "review", submit?: (args: Record<string, unknown>) => unknown, kept = draftLibrary(), more?: (cmd: string) => unknown) {
  const calls: Call[] = [];
  let current = stage;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    const drafted = kept.answer(cmd, args);
    if (drafted) return drafted;
    const other = more?.(cmd);
    if (other !== undefined) return other;
    switch (cmd) {
      case "human_open":
        return humanView(current);
      case "human_claim":
        current = "review";
        return { notice: { text: "Claimed.", error: false }, view: humanView("review") };
      case "human_draft_submit":
        return submit?.(args as Record<string, unknown>);
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return calls;
}

const changes = (calls: Call[]) => calls.filter((call) => call.cmd === "human_draft_edit").map((call) => call.args.change as DraftChange);
const bar = () => screen.getByRole("group", { name: "Decision" });
const submitButton = () => within(bar()).getByRole("button", { name: /^Submit/ }) as HTMLButtonElement;
const why = () => within(bar()).queryByRole("status")?.textContent ?? "";
const issues = () => within(screen.queryByRole("list", { name: "Issues" }) ?? document.createElement("ol")).queryAllByRole("listitem").map((item) => item.querySelector("p")?.textContent);
const field = () => screen.getByRole("textbox", { name: "Issue" }) as HTMLTextAreaElement;
const submitted = { notice: { text: "Submitted", error: false }, view: humanView("completed", { claim: null, completed: [{ kind: "text", key: "verdict", text: "GREEN" }] }), settled: "published" };

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("HumanDetail", () => {
  it("claims only on an explicit c, before which the bar holds Claim to review and one quiet line stands in the issues box's place", async () => {
    const calls = backend("claim");
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByRole("button", { name: "Claim to review (c)" });
    expect(screen.getByText("Claim it to write issues and a verdict")).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: "Issue" })).toBeNull();
    expect(within(bar()).getByText("Waiting")).toBeTruthy();
    expect(within(bar()).getAllByRole("radio").every((tab) => (tab as HTMLButtonElement).disabled)).toBe(true);
    expect(submitButton().disabled).toBe(true);

    await fireEvent.keyDown(region, { key: "c", isComposing: true });
    await fireEvent.keyDown(region, { key: "c", keyCode: 229 });
    expect(calls.some((call) => call.cmd === "human_claim")).toBe(false);
    await fireEvent.keyDown(region, { key: "c" });
    await waitFor(() => expect(calls.some((call) => call.cmd === "human_claim")).toBe(true));
    await within(bar()).findByText("Yours");
    expect(within(bar()).getByRole("button", { name: "Release claim (u)" })).toBeTruthy();
    expect(screen.queryByText("Claim it to write issues and a verdict")).toBeNull();
    await screen.findByRole("textbox", { name: "Issue" });
  });

  it("adds issues one at a time as bubbles with Enter or Add issue, edits one in place and removes another", async () => {
    const calls = backend("review");
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    const add = () => screen.getByRole("button", { name: "Add issue" }) as HTMLButtonElement;
    expect(add().disabled).toBe(true);
    await user.type(field(), "The notes name 0.21{Shift>}{Enter}{/Shift}twice");
    expect(field().value).toBe("The notes name 0.21\ntwice");
    expect(add().disabled).toBe(false);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(issues()).toEqual(["The notes name 0.21\ntwice"]));
    expect(field().value).toBe("");
    await user.type(field(), "Windows steps are missing");
    await user.click(add());
    await waitFor(() => expect(issues()).toEqual(["The notes name 0.21\ntwice", "Windows steps are missing"]));
    // Edit turns the bubble into a field in its place: Esc leaves it as it was, Enter keeps.
    await user.click(screen.getByRole("button", { name: "Edit issue 2" }));
    const editing = screen.getByRole("textbox", { name: "Edit issue 2" }) as HTMLTextAreaElement;
    expect(document.activeElement).toBe(editing);
    await user.type(editing, " on Windows 11");
    await user.keyboard("{Escape}");
    expect(issues()).toEqual(["The notes name 0.21\ntwice", "Windows steps are missing"]);
    await user.click(screen.getByRole("button", { name: "Edit issue 2" }));
    await user.type(screen.getByRole("textbox", { name: "Edit issue 2" }), " for uninstall{Enter}");
    await waitFor(() => expect(issues()).toEqual(["The notes name 0.21\ntwice", "Windows steps are missing for uninstall"]));
    await user.click(screen.getByRole("button", { name: "Remove issue 1" }));
    await waitFor(() => expect(issues()).toEqual(["Windows steps are missing for uninstall"]));
    expect(changes(calls)).toEqual([
      { kind: "addIssue", text: "The notes name 0.21\ntwice" },
      { kind: "addIssue", text: "Windows steps are missing" },
      { kind: "editIssue", index: 1, previous: "Windows steps are missing", text: "Windows steps are missing for uninstall" },
      { kind: "removeIssue", index: 0, previous: "The notes name 0.21\ntwice" },
    ]);
  });

  it("says why Submit waits: no verdict, a typed issue not added, then the library's readiness; switching keeps the issues", async () => {
    const calls = backend("review", () => submitted);
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    expect(submitButton().textContent).toBe("Submit");
    expect(submitButton().disabled).toBe(true);
    expect(why()).toBe("Choose GREEN or RED");
    await user.click(within(bar()).getByRole("radio", { name: "RED (r)" }));
    await waitFor(() => expect(submitButton().textContent).toBe("Submit RED"));
    expect(why()).toBe("RED needs a issue");
    await user.type(field(), "The notes are wrong");
    expect(why()).toBe("Add the typed issue first");
    expect(submitButton().disabled).toBe(true);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(submitButton().disabled).toBe(false));
    expect(why()).toBe("");
    await user.click(within(bar()).getByRole("radio", { name: "GREEN (g)" }));
    await waitFor(() => expect(submitButton().textContent).toBe("Submit GREEN"));
    expect(issues()).toEqual(["The notes are wrong"]);
    expect(within(bar()).getByRole("radio", { name: "GREEN (g)" }).getAttribute("aria-checked")).toBe("true");
    await user.click(submitButton());
    const result = await screen.findByRole("group", { name: "Result" });
    expect(result.textContent).toContain("GREEN");
    expect(screen.queryByRole("group", { name: "Decision" })).toBeNull();
    expect(screen.queryByRole("textbox", { name: "Issue" })).toBeNull();
    expect(calls.filter((call) => call.cmd === "human_draft_submit")).toHaveLength(1);
  });

  it("shows a verdict's own fields only while that verdict is chosen and the eval asks more than issues", async () => {
    const kept = draftLibrary(greenForm, scoredRed);
    backend("review", undefined, kept);
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    await user.click(within(bar()).getByRole("radio", { name: "GREEN (g)" }));
    await waitFor(() => expect(submitButton().textContent).toBe("Submit GREEN"));
    expect(screen.queryByText(/the eval also asks/)).toBeNull();
    await user.click(within(bar()).getByRole("radio", { name: "RED (r)" }));
    const fields = await screen.findByRole("group", { name: "RED's fields" });
    expect(within(fields).getByText("With RED, the eval also asks")).toBeTruthy();
    expect(within(fields).getByLabelText("note").getAttribute("placeholder")).toBe("Optional");
    await user.type(field(), "Off by one{Enter}");
    await waitFor(() => expect(why()).toBe("score is required"));
    await user.type(within(fields).getByLabelText(/score/), "2");
    await waitFor(() => expect(submitButton().disabled).toBe(false));
    await user.click(within(bar()).getByRole("radio", { name: "GREEN (g)" }));
    await waitFor(() => expect(screen.queryByRole("group", { name: "RED's fields" })).toBeNull());
    await user.click(within(bar()).getByRole("radio", { name: "RED (r)" }));
    expect((within(await screen.findByRole("group", { name: "RED's fields" })).getByLabelText(/score/) as HTMLInputElement).value).toBe("2");
  });

  it("says a library's refusal where why stands, the draft kept", async () => {
    backend("review", () => { throw { kind: "invalid", message: "issue: too short" }; });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    await user.click(within(bar()).getByRole("radio", { name: "RED (r)" }));
    await user.type(field(), "no{Enter}");
    await waitFor(() => expect(submitButton().disabled).toBe(false));
    await user.click(submitButton());
    await waitFor(() => expect(why()).toBe("issue: too short"));
    expect(within(bar()).getByRole("status").classList.contains("failed")).toBe(true);
    expect(issues()).toEqual(["no"]);
  });

  it("shows a review another reviewer holds with only who holds it in the bar", async () => {
    mockIPC((cmd) => (cmd === "human_open" ? humanView("claim", { claim: { reviewer: "bob", mine: false } }) : null), { shouldMockEvents: true });
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await waitFor(() => expect(bar().textContent).toBe("Taken by bob"));
    expect(within(bar()).queryAllByRole("button")).toEqual([]);
    expect(screen.queryByRole("textbox", { name: "Issue" })).toBeNull();
    expect(screen.queryByText("Claim it to write issues and a verdict")).toBeNull();
  });

  it("says why a submission settled here was not published, under its result bubble", async () => {
    const failure = "Saved locally; publishing to the review store failed: connection refused. Push results from Settings.";
    backend("review", () => ({
      settled: "savedLocally",
      notice: { text: failure, error: true },
      view: humanView("completed", { claim: null, result: '{ "verdict": "GREEN" }', meta: "GREEN · alice" }),
    }));
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    await fireEvent.keyDown(region, { key: "g" });
    await waitFor(() => expect(submitButton().disabled).toBe(false));
    await fireEvent.keyDown(field(), { key: "s", ctrlKey: true });
    await screen.findByRole("group", { name: "Result" });
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(failure));
  });

  it("leaves composition keys alone: letters in the issues field are text, a composing Enter adds nothing, Ctrl+S submits", async () => {
    const calls = backend("review", () => submitted);
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    await user.click(field());
    await user.keyboard("gru");
    expect(field().value).toBe("gru");
    expect(changes(calls)).toEqual([]);
    await fireEvent.keyDown(field(), { key: "Enter", isComposing: true });
    await fireEvent.keyDown(field(), { key: "Escape", isComposing: true });
    expect(changes(calls)).toEqual([]);
    expect(document.activeElement).toBe(field());
    await user.clear(field());
    await user.type(field(), "확인{Enter}");
    await waitFor(() => expect(issues()).toEqual(["확인"]));
    await fireEvent.keyDown(screen.getByLabelText("Human review"), { key: "g" });
    await waitFor(() => expect(submitButton().disabled).toBe(false));
    await fireEvent.keyDown(field(), { key: "s", ctrlKey: true });
    await screen.findByRole("group", { name: "Result" });
  });

  it("Esc leaves the issues field first, then cancels running work, then leaves the review", async () => {
    let stop!: () => void;
    backend("review", undefined, draftLibrary(), (cmd) => {
      if (cmd === "human_run") return new Promise((resolve) => (stop = () => resolve({ text: "■ error: cancelled", error: true })));
      if (cmd === "human_cancel") { stop(); return null; }
      return undefined;
    });
    let left = 0;
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => left++ });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    await user.click(screen.getByRole("button", { name: "notes" }));
    await within(bar()).findByRole("button", { name: "Release claim (u)" });
    await waitFor(() => expect((within(bar()).getByRole("button", { name: "Release claim (u)" }) as HTMLButtonElement).disabled).toBe(true));
    await user.click(field());
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(screen.getByLabelText("Human review"));
    expect(left).toBe(0);
    await user.keyboard("{Escape}");
    await waitFor(() => expect((within(bar()).getByRole("button", { name: "Release claim (u)" }) as HTMLButtonElement).disabled).toBe(false));
    expect(left).toBe(0);
    await user.keyboard("{Escape}");
    expect(left).toBe(1);
  });

  it("submits while a tool runs, cancelling that tool first", async () => {
    let stop!: () => void;
    const calls = backend("review", () => submitted, draftLibrary(), (cmd) => {
      if (cmd === "human_run") return new Promise((resolve) => (stop = () => resolve({ text: "■ error: cancelled", error: true })));
      if (cmd === "human_cancel") { stop(); return null; }
      return undefined;
    });
    const user = userEvent.setup();
    renderHosted(HumanDetail, { review: new Review("run-a-2", new Jobs()), onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    await user.click(within(bar()).getByRole("radio", { name: "GREEN (g)" }));
    await user.click(screen.getByRole("button", { name: "notes" }));
    await waitFor(() => expect((within(bar()).getByRole("button", { name: "Release claim (u)" }) as HTMLButtonElement).disabled).toBe(true));
    expect(submitButton().disabled).toBe(false);
    await user.click(submitButton());
    await screen.findByRole("group", { name: "Result" });
    expect(calls.map(({ cmd }) => cmd).filter((cmd) => ["human_run", "human_cancel", "human_draft_submit"].includes(cmd))).toEqual([
      "human_run", "human_cancel", "human_draft_submit",
    ]);
  });

  it("draws each tool run as its own line: running with its newest line and Cancel, ended with its stderr and its end, unfolded into its lines", async () => {
    backend("review");
    const user = userEvent.setup();
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    expect(screen.queryByRole("log", { name: "Tool log" })).toBeNull();
    const first = { run: "1", at: 0, operation: "notes", artifact: "release", lines: [{ text: "# Notes", error: false }], end: { text: "■ exit 0", error: false } };
    const second = { run: "2", at: Date.now(), operation: "scan", artifact: "release", lines: [
      { text: "early", error: false }, { text: "warning: missing title", error: true }, { text: "still checking", error: false },
    ] };
    review.logged([first, { ...second, end: null }]);
    const running = await screen.findByRole("button", { name: "Running scan · release" });
    expect(running.textContent).toContain("still checking");
    const ran = screen.getByRole("button", { name: "Ran notes · release" });
    expect(ran.querySelector(".end")?.textContent).toBe("exit 0 · 1 line");
    review.logged([first, { ...second, end: { text: "■ exit 1", error: true } }]);
    const ended = await screen.findByRole("button", { name: "Ran scan · release" });
    expect(ended.querySelector(".stderr")?.textContent).toBe("warning: missing title");
    expect(ended.querySelector(".end")?.textContent).toBe("exit 1 · 3 lines");
    expect(ended.querySelector(".end")?.classList.contains("failed")).toBe(true);
    expect(ended.getAttribute("aria-expanded")).toBe("false");
    await user.click(ended);
    const log = screen.getByRole("log", { name: "Tool log" });
    expect([...log.querySelectorAll("p")].map((row) => row.textContent?.replace(/\d\d:\d\d:\d\d/, "HH:MM:SS"))).toEqual(
      ["▶ HH:MM:SS · scan · release", "early", "warning: missing title", "still checking", "■ exit 1"]);
    await user.click(ended);
    expect(screen.queryByRole("log", { name: "Tool log" })).toBeNull();
  });

  it("lists Points after the runs; Insert puts a point into the issues field at its caret, else at its end; remove goes by point identity", async () => {
    const old: ReviewPoint = { text: "L2:C9", at: "2026-11-23T00:01:00Z", source: "release · Markdown" };
    const fresh: ReviewPoint = { text: "#checkout .total", at: "2026-11-23T00:02:00Z", source: "checkout · HTML Viewer" };
    const kept = draftLibrary();
    const calls = backend("review", undefined, kept);
    const user = userEvent.setup();
    const review = new Review("run-a-2", new Jobs());
    renderHosted(HumanDetail, { review, onback: () => {} });
    await within(await screen.findByRole("group", { name: "Decision" })).findByText("Yours");
    expect(screen.queryByRole("list", { name: "Points" })).toBeNull();
    kept.draft.points = [old, fresh];
    await review.readDraft();
    const points = await screen.findByRole("list", { name: "Points" });
    const rows = within(points).getAllByRole("listitem");
    expect(rows.map((row) => row.querySelector(".text")?.textContent)).toEqual(["L2:C9", "#checkout .total"]);
    expect(rows[1].classList.contains("newest")).toBe(true);
    // Never placed, the caret is the field's end.
    await user.type(field(), "See ");
    field().blur();
    await user.click(within(rows[0]).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(field().value).toBe("See L2:C9"));
    field().setSelectionRange(4, 4);
    await user.click(within(rows[1]).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(field().value).toBe("See #checkout .totalL2:C9"));
    expect(kept.draft.issues).toEqual([]);
    await user.click(within(points).getByRole("button", { name: "Remove point: L2:C9" }));
    await waitFor(() => expect(within(points).getAllByRole("listitem")).toHaveLength(1));
    expect(calls.find((call) => call.cmd === "human_draft_remove_point")?.args.point).toEqual(old);
  });

  it("gives a review opened with Tools focused the keyboard on its Tools", async () => {
    backend("review");
    const review = new Review("run-a-2", new Jobs());
    review.area = "tools";
    renderHosted(HumanDetail, { review, onback: () => {} });
    const tools = await screen.findByRole("group", { name: "Tools" });
    await waitFor(() => expect(document.activeElement).toBe(tools));
  });
});
