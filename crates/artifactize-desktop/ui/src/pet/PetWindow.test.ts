// The pet overlay against a mocked command boundary: it holds the active review's library
// draft (Points, reasons, the verdict and its fields), points in an open viewer of the review's Artifact
// only, submits as Review does, and leads back to the review in the main window.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit, listen } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it } from "vitest";
import { draftLibrary, humanView } from "../review/fixtures";
import PetWindow from "./PetWindow.svelte";

afterEach(() => {
  cleanup();
  clearMocks();
});

type Call = { cmd: string; args: Record<string, unknown> };

function backend(submitError?: string) {
  const calls: Call[] = [];
  const library = draftLibrary();
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    const drafted = library.answer(cmd, args);
    if (drafted) return drafted;
    switch (cmd) {
      case "human_open":
        return humanView("review");
      case "human_draft_submit":
        if (submitError) throw { kind: "validation", message: submitError };
        return { notice: { text: "Submitted.", error: false }, view: humanView("completed"), settled: "published" };
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return { calls, library };
}

it("writes reasons and the verdict to the shared draft, split as Review's is, and submits as Review does", async () => {
  const { calls, library } = backend();
  // Saved before (another window, an earlier run of the app): GREEN was chosen.
  library.draft.verdict = "GREEN";
  const user = userEvent.setup();
  render(PetWindow, { request: "run-a-2" });
  await screen.findByRole("button", { name: "Show release/signoff in Review" });
  const submit = await screen.findByRole<HTMLButtonElement>("button", { name: /^Submit GREEN/ });
  expect(screen.getAllByRole("radio").map((button) => button.textContent?.trim())).toEqual(["GREEN", "RED"]);
  // No claim in the overlay: no Claim to review, no Release claim.
  expect(screen.queryByRole("button", { name: /claim/i })).toBeNull();
  const field = screen.getByRole<HTMLInputElement>("textbox", { name: "Reason" });
  const add = screen.getByRole<HTMLButtonElement>("button", { name: "Add reason (Enter)" });
  expect(add.disabled).toBe(true);
  await user.type(field, "Looks right");
  expect(submit.disabled).toBe(true);
  expect(screen.getByText("Add the typed reason first")).toBeTruthy();
  await user.keyboard("{Enter}");
  await waitFor(() => expect(library.draft.reasons).toEqual(["Looks right"]));
  expect(field.value).toBe("");
  const reasons = screen.getByRole("list", { name: "Reasons" });
  expect(within(reasons).getByText("Looks right")).toBeTruthy();
  // Each reason has Remove alone; none is edited in the overlay.
  expect(within(reasons).queryByRole("button", { name: /Edit/ })).toBeNull();
  await user.click(screen.getByRole("radio", { name: "RED (r)" }));
  await screen.findByRole("button", { name: /^Submit RED/ });
  expect(library.draft.verdict).toBe("RED");
  await user.click(screen.getByRole("button", { name: /^Submit RED/ }));
  await waitFor(() => expect(calls.some((call) => call.cmd === "human_draft_submit")).toBe(true));
});

it("says a submission refusal once, above the decision row, the draft kept", async () => {
  const error = "A comment is required.";
  const { library } = backend(error);
  library.draft.verdict = "GREEN";
  library.draft.reasons = ["Fine"];
  render(PetWindow, { request: "run-a-2" });
  await fireEvent.click(await screen.findByRole("button", { name: /^Submit GREEN/ }));
  await waitFor(() => expect(screen.getAllByText(error)).toHaveLength(1));
  expect(screen.getByText("Fine")).toBeTruthy();
});

it("points only in the open viewer matched to the review's original request, saying why otherwise", async () => {
  backend();
  const armed: unknown[] = [];
  await listen("point-arm", (event) => armed.push(event.payload));
  render(PetWindow, { request: "run-a-2" });
  const point = await screen.findByRole("button", { name: "Point" });
  expect(point.hasAttribute("disabled")).toBe(true);
  screen.getByText("Open release in a viewer to point.");
  await emit("viewer-presence", { session: 3, artifact: "other", review: "run-a-2", open: true, pointable: true, pointing: false, petMatch: false });
  // The same-named Artifact in another worktree: the library does not match it.
  await emit("viewer-presence", { session: 5, artifact: "release", review: "run-a-2", open: true, pointable: true, pointing: false, petMatch: false });
  // Matched, but for a review that is no longer the active one.
  await emit("viewer-presence", { session: 6, artifact: "release", review: "run-b-3", open: true, pointable: true, pointing: false, petMatch: true });
  expect(point.hasAttribute("disabled")).toBe(true);
  await emit("viewer-presence", { session: 4, artifact: "release", review: "run-a-2", open: true, pointable: true, pointing: false, petMatch: true });
  await waitFor(() => expect(point.hasAttribute("disabled")).toBe(false));
  await fireEvent.click(point);
  // Point arms that viewer's session alone.
  await waitFor(() => expect(armed).toEqual([{ session: 4 }]));
  // The viewer closed: Point is disabled again.
  await emit("viewer-presence", { session: 4, artifact: "release", review: "run-a-2", open: false, pointable: true, pointing: false, petMatch: true });
  await waitFor(() => expect(point.hasAttribute("disabled")).toBe(true));
});

it("leads back to the review in the main window", async () => {
  const { calls } = backend();
  const asked: unknown[] = [];
  await listen("pet-open-review", (event) => asked.push(event.payload));
  render(PetWindow, { request: "run-a-2" });
  await fireEvent.click(await screen.findByRole("button", { name: "Show release/signoff in Review" }));
  await waitFor(() => expect(asked).toEqual(["run-a-2"]));
  expect(calls.some((call) => call.cmd === "main_focus")).toBe(true);
});

it("shows a saved draft's verdict and reasons, sending nothing; a fresh draft chooses none and says so", async () => {
  const fresh = backend();
  render(PetWindow, { request: "run-a-2" });
  await screen.findByRole("button", { name: "Show release/signoff in Review" });
  await screen.findByText("Choose GREEN or RED");
  expect(screen.getAllByRole("radio").every((tab) => tab.getAttribute("aria-checked") === "false")).toBe(true);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: /^Submit/ }).disabled).toBe(true);
  expect(fresh.calls.some((call) => call.cmd === "human_draft_edit")).toBe(false);
  cleanup();
  clearMocks();
  // Saved with RED and a reason, as before a restart.
  const saved = backend();
  saved.library.draft.verdict = "RED";
  saved.library.draft.reasons = ["The total is wrong."];
  render(PetWindow, { request: "run-a-2" });
  const chosen = await screen.findByRole("radio", { name: "RED (r)" });
  await waitFor(() => expect(chosen.getAttribute("aria-checked")).toBe("true"));
  expect(await screen.findByText("The total is wrong.")).toBeTruthy();
  expect(saved.calls.some((call) => call.cmd === "human_draft_edit")).toBe(false);
});

it("reads compact Points from the shared draft; Insert puts one into the overlay's reasons field", async () => {
  const { library } = backend();
  const user = userEvent.setup();
  render(PetWindow, { request: "run-a-2" });
  const point = await screen.findByRole<HTMLButtonElement>("button", { name: "Point" });
  expect(point.disabled).toBe(true);
  expect(screen.queryByRole("list", { name: "Points" })).toBeNull();
  const text = "release.md L2:C4";
  library.draft.points = [{ text, source: "release · File Explorer", at: "2026-11-23T00:00:00Z" }];
  await emit("human-draft-changed", { id: "run-a-2" });
  const points = await screen.findByRole("list", { name: "Points" });
  expect(within(points).getByText(text)).toBeTruthy();
  expect(points.textContent).not.toContain("File Explorer");
  expect(within(points).queryByRole("button", { name: /remove/i })).toBeNull();
  const field = screen.getByRole<HTMLInputElement>("textbox", { name: "Reason" });
  await user.type(field, "Here: ");
  await user.click(within(points).getByRole("button", { name: "Insert" }));
  expect(field.value).toBe(`Here: ${text}`);
  expect(library.draft.reasons).toEqual([]);
  expect(library.draft.points).toHaveLength(1);
});
