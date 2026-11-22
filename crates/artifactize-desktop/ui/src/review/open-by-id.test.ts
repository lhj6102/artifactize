// Open by ID uses the Human review layout in every Human stage, as a reference does.
import { waitFor } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import type { HumanView } from "../ipc/client";
import { AppState } from "../shell/app.svelte";
import { draftLibrary, humanView } from "./fixtures";

afterEach(() => clearMocks());

/** The library holding request `id` as the Human review `view`. */
function backend(view: HumanView, draft?: ReturnType<typeof draftLibrary>) {
  mockIPC((command, args) => {
    const id = (args as { id: string }).id;
    const saved = draft?.answer(command, args);
    if (saved) return saved;
    switch (command) {
      case "request_detail":
        return { source: { kind: "run", run: "run-a" }, node: "e:release/signoff", target: "eval", title: "release/signoff", summary: "", request: id, profile: "human", mark: null, plan: null, outcome: null, sections: [], evidence: null, card: null, running: false };
      case "human_open_reference":
        return { view, outsideFilter: true, outsideReason: "claimed" };
      case "human_open":
        return view;
      default:
        return null;
    }
  });
}

describe("openRequest", () => {
  it("opens a claimable review as its review", async () => {
    backend(humanView("claim"));
    const app = new AppState();
    await app.openRequest("run-a-2");
    expect(app.reviewMode.shown.kind).toBe("review");
  });

  it("opens a submitted review, and one another reviewer holds, read-only", async () => {
    for (const view of [humanView("completed"), humanView("claim", { claim: { reviewer: "bob", mine: false } })]) {
      backend(view);
      const app = new AppState();
      await app.openRequest("run-a-2");
      expect(app.reviewMode.shown).toEqual({ kind: "review", id: view.id });
      expect(app.reviews[view.id]?.view).toEqual(view);
      clearMocks();
    }
  });
});

it("routes a shared review's draft and Tool log notifications by their original request", async () => {
  const view = humanView("review", { id: "follower", owner: "owner" });
  const library = draftLibrary();
  backend(view, library);
  const app = new AppState();
  await app.openRequest("follower");
  const review = app.reviews.follower;
  await waitFor(() => expect(review.draft?.points).toEqual([]));
  const point = { text: "L2:C1", at: "2026-11-23T00:00:00Z", source: "release · Markdown" };
  library.draft.points.push(point);
  const entry = { run: "tool-1", at: 0, operation: "show", artifact: "release", lines: [], end: null };
  app.draftChanged("follower");
  app.logged([{ key: { kind: "review", request: "follower" }, entries: [entry] }]);
  expect(review.draft?.points).toEqual([]);
  expect(review.log).toEqual([]);
  app.draftChanged("owner");
  app.logged([{ key: { kind: "review", request: "owner" }, entries: [entry] }]);
  await waitFor(() => expect(review.draft?.points).toEqual([point]));
  expect(review.log).toEqual([entry]);
});
