// The standalone review against a mocked command boundary: a result saved locally whose
// publication failed stays open with its failure, and a replaced state drops the open review.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import ReviewScreen from "./ReviewScreen.svelte";
import { greenForm, humanView } from "./fixtures";
import type { LaunchView } from "../ipc/client";

const launch: LaunchView = { mode: "review", state: "/state", reviewer: "alice", repository: "/repo", request: null };

const row = {
  id: "run-a-2",
  eval: "release/signoff",
  title: "A person approves the release",
  claim: "alice (you)",
  mine: true,
  waiting: "3s",
  repository: "/repo",
};

function backend(options: { generation: () => number; rows: () => (typeof row)[] }) {
  mockIPC(
    (cmd) => {
      switch (cmd) {
        case "human_waiting":
          return { generation: options.generation(), rows: options.rows() };
        case "human_open":
          return humanView("review");
        case "human_form":
          return greenForm;
        case "human_submit":
          return {
            settled: "savedLocally",
            notice: { text: "The Human result is saved locally, but publishing it failed: 403", error: true },
            view: humanView("completed", { result: "GREEN\n{}", meta: "GREEN" }),
          };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}

beforeAll(() => {
  // jsdom lays nothing out; rows only ask to be scrolled into view.
  Element.prototype.scrollIntoView ??= () => {};
});

afterEach(() => {
  cleanup();
  clearMocks();
});

async function openReview() {
  render(ReviewScreen, { launch, active: true });
  await screen.findByText("release/signoff");
  await fireEvent.dblClick(await screen.findByText("release/signoff"));
  return screen.findByLabelText("Human review");
}

describe("ReviewScreen", () => {
  it("keeps a result saved locally open with its publication failure", async () => {
    let rows = [row];
    backend({ generation: () => 0, rows: () => rows });
    const region = await openReview();
    await screen.findByText("REVIEW (yours)");
    await fireEvent.keyDown(region, { key: "g" });
    const comment = await screen.findByLabelText(/comment/);
    rows = [];
    await fireEvent.keyDown(comment, { key: "s", ctrlKey: true });
    await screen.findByText(/publishing it failed/);
    await screen.findByText(/completed result/i);
    expect(screen.queryByLabelText("Human review")).not.toBeNull();
  });

  it("closes the open review when the state is replaced", async () => {
    let generation = 0;
    backend({ generation: () => generation, rows: () => [row] });
    await openReview();
    generation = 1;
    await emit("state-changed", { resync: false });
    await waitFor(() => expect(screen.queryByLabelText("Human review")).toBeNull());
  });
});
