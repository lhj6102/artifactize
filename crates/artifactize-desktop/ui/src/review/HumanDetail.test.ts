// The shared Human Detail against a mocked command boundary: explicit claim, shortcuts that
// leave input methods alone, verdict drafts and correctable schema errors.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import HumanDetail from "./HumanDetail.svelte";
import { Review } from "./review.svelte";
import { greenForm, humanView, redForm } from "./fixtures";

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
      case "human_form":
        return (args as { verdict: string }).verdict === "RED" ? redForm : greenForm;
      case "human_submit":
        return submit?.(args as Record<string, unknown>);
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return calls;
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("HumanDetail", () => {
  it("claims only on an explicit c, never on a key that is part of a composition", async () => {
    const calls = backend("claim");
    render(HumanDetail, { review: new Review("run-a-2", false), onback: () => {} });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText(/Unclaimed\. c claims it for alice/);

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
    render(HumanDetail, { review: new Review("run-a-2", false), onback: () => {} });
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
    expect(submitted?.args.draft).toEqual({ verdict: "RED", fields: [["reason", { kind: "text", value: "no" }]], json: null });
    expect(((await screen.findByLabelText(/reason/)) as HTMLTextAreaElement).value).toBe("no");
  });

  it("leaves an input method its Escape: a composing Escape neither stops editing nor leaves", async () => {
    backend("review");
    let left = 0;
    render(HumanDetail, { review: new Review("run-a-2", false), onback: () => (left += 1) });
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

  it("Esc stops editing first, then leaves", async () => {
    backend("review");
    let left = 0;
    render(HumanDetail, { review: new Review("run-a-2", false), onback: () => (left += 1) });
    const region = await screen.findByLabelText("Human review");
    await screen.findByText("REVIEW (yours)");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    await fireEvent.keyDown(comment, { key: "Escape" });
    await waitFor(() => expect(screen.queryByLabelText(/comment/)).toBeNull());
    expect(left).toBe(0);
    await fireEvent.keyDown(region, { key: "Escape" });
    expect(left).toBe(1);
  });
});
