// Open a request by ID (J-OPEN-ID): a Human review still waiting for this reviewer opens as its
// review; a submitted one, one another reviewer holds, or any other request opens read-only.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import type { HumanView } from "../ipc/client";
import { AppState } from "../shell/app.svelte";
import { humanView } from "./fixtures";

afterEach(() => clearMocks());

/** The library holding request `id` as the Human review `view`. */
function backend(view: HumanView) {
  mockIPC((command, args) => {
    const id = (args as { id: string }).id;
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
      expect(app.reviewMode.shown.kind).toBe("request");
      clearMocks();
    }
  });
});
