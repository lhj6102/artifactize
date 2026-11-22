// The pet overlay against a mocked command boundary: it holds the active review's library
// draft (Points, the verdict and its form), points in an open viewer of the review's Artifact
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

it("writes the review's form and opens the verdict the draft holds", async () => {
  const { calls, library } = backend();
  // Saved before (another window, an earlier run of the app): GREEN was chosen.
  library.draft.verdict = "GREEN";
  render(PetWindow, { request: "run-a-2" });
  await screen.findByRole("button", { name: "Show release/signoff in Review" });
  // The draft holds GREEN: its form is open, Submit names it.
  await screen.findByRole("button", { name: "Submit GREEN" });
  expect(screen.getAllByRole("radio").map((button) => button.textContent?.trim())).toEqual(["GREEN", "RED"]);
  expect(screen.queryByRole("button", { name: /^(GREEN|RED)$/ })).toBeNull();
  await fireEvent.input(screen.getByLabelText(/comment/i), { target: { value: "Looks right." } });
  await waitFor(() => expect(library.draft.green.draft.fields[1][1]).toEqual({ kind: "text", value: "Looks right." }));
  await fireEvent.click(screen.getByRole("radio", { name: "RED" }));
  await screen.findByRole("button", { name: "Submit RED" });
  expect(library.draft.verdict).toBe("RED");
  await fireEvent.click(screen.getByRole("button", { name: "Submit RED" }));
  await waitFor(() => expect(calls.some((call) => call.cmd === "human_draft_submit")).toBe(true));
});

it("shows a submission error once and keeps the verdict form open", async () => {
  const error = "A comment is required.";
  const { library } = backend(error);
  library.draft.verdict = "GREEN";
  render(PetWindow, { request: "run-a-2" });
  await fireEvent.click(await screen.findByRole("button", { name: "Submit GREEN" }));
  expect((await screen.findByRole("alert")).textContent).toBe(error);
  expect(screen.getAllByText(error)).toHaveLength(1);
  expect(screen.getByLabelText(/comment/i)).toBeTruthy();
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

it("shows a saved verdict's form with what was entered, sending nothing; a fresh draft opens none", async () => {
  const fresh = backend();
  render(PetWindow, { request: "run-a-2" });
  await screen.findByRole("button", { name: "Show release/signoff in Review" });
  // No verdict chosen yet: the choice waits, no form is open.
  await screen.findByRole("button", { name: "GREEN" });
  expect(screen.queryByRole("radiogroup", { name: "Verdict" })).toBeNull();
  expect(fresh.calls.some((call) => call.cmd === "human_draft_edit")).toBe(false);
  cleanup();
  clearMocks();
  // Saved with RED and a reason, as before a restart.
  const saved = backend();
  saved.library.draft.verdict = "RED";
  saved.library.draft.red.draft.fields = [["reason", { kind: "text", value: "The total is wrong." }]];
  render(PetWindow, { request: "run-a-2" });
  const chosen = await screen.findByRole("radio", { name: "RED" });
  expect(chosen.getAttribute("aria-checked")).toBe("true");
  expect((await screen.findByDisplayValue("The total is wrong.")).tagName).toMatch(/INPUT|TEXTAREA/);
  expect(saved.calls.some((call) => call.cmd === "human_draft_edit")).toBe(false);
});

it("reads compact Points from the shared draft and inserts only when a form is open", async () => {
  const { library } = backend();
  const user = userEvent.setup();
  render(PetWindow, { request: "run-a-2" });
  const point = await screen.findByRole<HTMLButtonElement>("button", { name: "Point" });
  expect(point.disabled).toBe(true);
  expect(screen.queryByRole("list", { name: "Points" })).toBeNull();
  expect(screen.queryByRole("textbox")).toBeNull();
  const text = "release.md L2:C4";
  library.draft.points = [{ text, source: "release · File Explorer", at: "2026-11-23T00:00:00Z" }];
  await emit("human-draft-changed", { id: "run-a-2" });
  const points = await screen.findByRole("list", { name: "Points" });
  expect(within(points).getByText(text)).toBeTruthy();
  expect(points.textContent).not.toContain("File Explorer");
  const insert = within(points).getByRole<HTMLButtonElement>("button", { name: "Insert" });
  expect(insert.disabled).toBe(true);
  expect(within(points).queryByRole("button", { name: /remove/i })).toBeNull();
  await user.click(screen.getByRole("button", { name: "GREEN" }));
  const comment = await screen.findByLabelText<HTMLTextAreaElement>(/^comment/);
  await user.type(comment, "Here: ");
  await user.click(insert);
  expect(comment.value).toBe(`Here: ${text}`);
  await waitFor(() => expect(library.draft.green.draft.fields).toContainEqual(["comment", { kind: "text", value: `Here: ${text}` }]));
  expect(library.draft.points).toHaveLength(1);
});
