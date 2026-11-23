// Review's list and the bar's count follow the state while Review stays shown: a request created,
// claimed, released or submitted in any process reaches both through `state-changed`, even while
// the open review's own reads have not answered (they never hold the list's reader).
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import App from "../App.svelte";
import { browseView } from "../monitor/fixtures";
import type { LaunchView, ReviewMatch } from "../ipc/client";
import { humanView } from "./fixtures";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, destroy: async () => {}, setTitle: async () => {} }),
}));

const launch: LaunchView = { mode: "review", state: "/state", reviewer: "alice", request: null, graphBranch: null };

const row = (id: string, claimant: string | null = null): ReviewMatch => ({
  id, target: { run: `run-${id}`, node: "e:release/signoff" }, title: "release/signoff", eval: `${id}/signoff`,
  worktree: `/${id}`, project: id, projectKey: `/${id}`, branch: "main", age: "now",
  mark: { status: "WAITING_HUMAN", glyph: "▶" }, claimant, mine: false,
});

beforeAll(() => {
  Element.prototype.scrollIntoView ??= () => {};
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => { cleanup(); clearMocks(); });

it("lists a review created while Review shows an open review, and the bar counts it, without leaving Review", async () => {
  /** The waiting reviews the state holds; other processes change them. */
  let waiting = [row("first")];
  let searches = 0;
  let opened = 0;
  mockIPC((command) => {
    switch (command) {
      case "launch": return launch;
      case "browse": case "redraw": return browseView(1);
      case "chat_list": return [];
      case "human_search": searches++; return { reviews: waiting, next: null };
      case "human_waiting_count": return waiting.length;
      // The review opens; its later reads never answer, and the list must not wait for them.
      case "human_open_reference":
        return opened++ === 0 ? { view: humanView("claim", { id: "first" }), outsideFilter: false, outsideReason: null } : new Promise(() => {});
      case "human_open": case "human_draft": return new Promise(() => {});
      default: return null;
    }
  }, { shouldMockEvents: true });
  render(App);
  const list = await screen.findByRole("listbox", { name: "Human reviews" });
  await within(list).findByRole("option", { name: /first\/signoff/ });
  const bar = () => screen.getByRole("button", { name: /^Review/ }).getAttribute("aria-label");
  await waitFor(() => expect(bar()).toBe("Review, 1 waiting"));
  await within(list).findByRole("option", { selected: true });

  // Created in another process.
  waiting = [row("second"), row("first")];
  await emit("state-changed", { resync: false });
  await within(list).findByRole("option", { name: /second\/signoff/ });
  await waitFor(() => expect(bar()).toBe("Review, 2 waiting"));
  expect(screen.getByRole("main", { name: "Review" }).hidden).toBe(false);

  // Claimed elsewhere, then released, then submitted: the list follows each, one read at a time.
  waiting = [row("first")];
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(within(list).queryByRole("option", { name: /second\/signoff/ })).toBeNull());
  await waitFor(() => expect(bar()).toBe("Review, 1 waiting"));
  waiting = [row("second"), row("first")];
  const before = searches;
  await Promise.all([emit("state-changed", { resync: false }), emit("state-changed", { resync: false }), emit("state-changed", { resync: false })]);
  await within(list).findByRole("option", { name: /second\/signoff/ });
  await waitFor(() => expect(bar()).toBe("Review, 2 waiting"));
  expect(searches - before).toBeLessThanOrEqual(2);
  waiting = [row("first")];
  await emit("state-changed", { resync: true });
  await waitFor(() => expect(within(list).queryByRole("option", { name: /second\/signoff/ })).toBeNull());
  await waitFor(() => expect(bar()).toBe("Review, 1 waiting"));
});
