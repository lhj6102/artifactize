import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, expect, it } from "vitest";
import type { ReviewMatch, ReviewPage } from "../ipc/client";
import { renderHosted } from "../test-host";
import ReviewMode from "./ReviewMode.svelte";
import { humanView } from "./fixtures";

Element.prototype.scrollIntoView ??= () => {};
globalThis.ResizeObserver ??= class {
  observe() {}
  disconnect() {}
  unobserve() {}
} as unknown as typeof ResizeObserver;

afterEach(() => { cleanup(); clearMocks(); });

const row = (id: string, project = "app"): ReviewMatch => ({
  id, target: { run: "run-a", node: `e:${id}/signoff` }, title: `${id}/signoff`, eval: `${id}/signoff`,
  worktree: `/${project}`, project, projectKey: `/${project}`, branch: "main", age: "now",
  mark: { status: "WAITING_HUMAN", glyph: "▶" }, claimant: null, mine: false,
});

it("leads back to Verify only from a review followed from Verify; Open in Verify is always there", async () => {
  mockIPC((command) => {
    if (command === "human_search") return { reviews: [row("release")], next: null };
    if (command === "human_open_reference") return { view: humanView("claim", { id: "release" }), outsideFilter: false, outsideIssue: null };
    if (command === "human_open") return humanView("claim", { id: "release" });
    return null;
  }, { shouldMockEvents: true });
  const { app } = renderHosted(ReviewMode);
  const area = await screen.findByRole("region", { name: "Review's main area" });
  await app.showReview("release", true);
  expect(await within(area).findByRole("button", { name: "Verify · run-a › release" })).toBeTruthy();
  expect(within(area).getByRole("button", { name: "Open in Verify" })).toBeTruthy();
  await app.showReview("release");
  await waitFor(() => expect(within(area).queryByRole("button", { name: /^Verify · / })).toBeNull());
  expect(within(area).getByRole("button", { name: "Open in Verify" })).toBeTruthy();
});

it("reads three cursor pages once each without replacing the opened review or its list position", async () => {
  const reads: (number | null)[] = [];
  let pageTwo!: (page: ReviewPage) => void;
  mockIPC((command, args) => {
    if (command === "human_search") {
      const start = (args as { start: number | null }).start;
      reads.push(start);
      if (start === null) return { reviews: [row("release")], next: 17 };
      if (start === 17) return new Promise<ReviewPage>((resolve) => { pageTwo = resolve; });
      if (start === 42) return { reviews: [row("third")], next: null };
      throw new Error(`unexpected cursor ${start}`);
    }
    if (command === "human_open_reference" || command === "human_open") {
      const view = humanView("claim", { id: "release" });
      return command === "human_open" ? view : { view, outsideFilter: false, outsideIssue: null };
    }
    return null;
  }, { shouldMockEvents: true });
  const { app } = renderHosted(ReviewMode);
  app.mode = "review";
  const list = await screen.findByRole("listbox", { name: "Human reviews" });
  await fireEvent.click(await within(list).findByRole("option", { name: /release\/signoff/ }));
  await within(list).findByRole("option", { selected: true });
  const opened = screen.getByRole("region", { name: "Review's main area" });
  list.scrollTop = 60;
  const more = within(list).getByRole("button", { name: "More reviews" });
  await fireEvent.click(more);
  await fireEvent.click(more);
  expect(reads).toEqual([null, 17]);
  pageTwo({ reviews: [row("release"), row("second", "other")], next: 42 });
  await within(list).findByRole("option", { name: /second\/signoff/ });
  await waitFor(() => expect(more.getAttribute("aria-disabled")).toBe("false"));
  await fireEvent.click(within(list).getByRole("button", { name: "More reviews" }));
  await within(list).findByRole("option", { name: /third\/signoff/ });
  await waitFor(() => expect(within(list).queryByRole("button", { name: "More reviews" })).toBeNull());
  expect(reads).toEqual([null, 17, 42]);
  expect(within(list).getAllByRole("option").map((option) => option.dataset.key)).toEqual(["release", "second", "third"]);
  expect(within(list).getByRole("option", { selected: true }).textContent).toContain("release/signoff");
  expect(screen.getByRole("region", { name: "Review's main area" })).toBe(opened);
  expect(list.scrollTop).toBe(60);
});
