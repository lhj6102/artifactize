// The screen switch against a mocked command boundary: Review shows the number of Human reviews
// the library counts as waiting, only while any wait (never a 0), and follows a change of the
// state.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { cleanup, screen, waitFor } from "@testing-library/svelte";
import { afterEach, expect, it } from "vitest";
import Screens from "./Screens.svelte";

/** The backend's count, read again by every `review_waiting` call. */
function backend(count: () => number) {
  const reads: string[] = [];
  mockIPC(
    (cmd) => {
      reads.push(cmd);
      if (cmd === "review_waiting") return count();
      return null;
    },
    { shouldMockEvents: true },
  );
  return reads;
}

const review = () => screen.getByRole("tab", { name: /^Review/ });

afterEach(() => {
  cleanup();
  clearMocks();
});

it("shows the number of waiting Human reviews beside Review and nothing beside Monitor", async () => {
  const reads = backend(() => 2);
  renderHosted(Screens);
  await waitFor(() => expect(review().textContent).toBe("Review 2"));
  expect(screen.getByRole("tab", { name: "Monitor" }).textContent).toBe("Monitor");
  expect(reads).toEqual(["review_waiting"]);
});

it("follows the state: Review alone, not 0, once none waits, and the number again when new ones wait", async () => {
  let count = 1;
  backend(() => count);
  renderHosted(Screens);
  await waitFor(() => expect(review().textContent).toBe("Review 1"));
  count = 0;
  await emit("state-changed", { resync: false });
  await waitFor(() => expect(review().textContent).toBe("Review"));
  expect(review().querySelector(".count")).toBeNull();
  count = 3;
  await emit("state-changed", { resync: true });
  await waitFor(() => expect(review().textContent).toBe("Review 3"));
});
