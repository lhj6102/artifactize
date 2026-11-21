// The pet overlay against a mocked command boundary: it holds the active review's library
// draft (Notes, the verdict and its form), points in an open viewer of the review's Artifact
// only, submits as Review does, and leads back to the review in the main window.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit, listen } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, expect, it } from "vitest";
import { draftLibrary, humanView } from "../review/fixtures";
import PetWindow from "./PetWindow.svelte";

afterEach(() => {
  cleanup();
  clearMocks();
});

type Call = { cmd: string; args: Record<string, unknown> };

function backend() {
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
        return { notice: { text: "Submitted.", error: false }, view: humanView("completed", { notes: library.draft.notes }), settled: "published" };
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  return { calls, library };
}

it("writes the review's Notes and opens the verdict the draft holds", async () => {
  const { calls, library } = backend();
  // Saved before (another window, an earlier run of the app): GREEN was chosen.
  library.draft.verdict = "GREEN";
  render(PetWindow, { request: "run-a-2" });
  await screen.findByRole("button", { name: "Show release/signoff in Review" });
  // The draft holds GREEN: its form is open, Submit names it.
  await screen.findByRole("button", { name: "Submit GREEN" });
  await fireEvent.input(screen.getByLabelText("Notes"), { target: { value: "Looks right." } });
  await waitFor(() => expect(library.draft.notes).toBe("Looks right."));
  await fireEvent.click(screen.getByRole("radio", { name: "RED" }));
  await screen.findByRole("button", { name: "Submit RED" });
  expect(library.draft.verdict).toBe("RED");
  await fireEvent.click(screen.getByRole("button", { name: "Submit RED" }));
  await waitFor(() => expect(calls.some((call) => call.cmd === "human_draft_submit")).toBe(true));
});

it("points only in the open viewer matched to the review's original request, saying why otherwise", async () => {
  backend();
  const armed: unknown[] = [];
  await listen("point-arm", (event) => armed.push(event.payload));
  render(PetWindow, { request: "run-a-2" });
  const point = await screen.findByRole("button", { name: "Point" });
  expect(point.hasAttribute("disabled")).toBe(true);
  screen.getByText("Open release in a viewer to point.");
  await emit("viewer-presence", { session: 3, artifact: "other", review: "run-a-2", open: true, pointable: true, petMatch: false });
  // The same-named Artifact in another worktree: the library does not match it.
  await emit("viewer-presence", { session: 5, artifact: "release", review: "run-a-2", open: true, pointable: true, petMatch: false });
  // Matched, but for a review that is no longer the active one.
  await emit("viewer-presence", { session: 6, artifact: "release", review: "run-b-3", open: true, pointable: true, petMatch: true });
  expect(point.hasAttribute("disabled")).toBe(true);
  await emit("viewer-presence", { session: 4, artifact: "release", review: "run-a-2", open: true, pointable: true, petMatch: true });
  await waitFor(() => expect(point.hasAttribute("disabled")).toBe(false));
  await fireEvent.click(point);
  // Point arms that viewer's session alone.
  await waitFor(() => expect(armed).toEqual([{ session: 4 }]));
  // The viewer closed: Point is disabled again.
  await emit("viewer-presence", { session: 4, artifact: "release", review: "run-a-2", open: false, pointable: true, petMatch: true });
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

it("marks what a viewer's Point appended in the Notes until they are next edited", async () => {
  const { library } = backend();
  library.draft.verdict = "GREEN";
  const { container } = render(PetWindow, { request: "run-a-2" });
  await screen.findByRole("button", { name: "Submit GREEN" });
  // A viewer's Point appended a line through the library, and says so.
  library.draft.notes = "Read it.\nrelease.md L2:C4";
  await emit("notes-pointed", { review: "run-a-2", text: "release.md L2:C4" });
  await emit("human-draft-changed", { id: "run-a-2" });
  await waitFor(() => expect([...container.querySelectorAll(".backdrop mark")].map((mark) => mark.textContent)).toEqual(["release.md L2:C4"]));
  // Edited, the Notes read as one text again.
  await fireEvent.input(screen.getByLabelText("Notes"), { target: { value: "Read it.\nrelease.md L2:C4\nFine." } });
  await waitFor(() => expect(container.querySelector(".backdrop")).toBeNull());
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
