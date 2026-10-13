// Closing the window with the window's claims: every close request is held until
// the prompt is answered, however often the window asks.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import { Leaving } from "./leaving.svelte";

afterEach(() => clearMocks());

function close() {
  let prevented = false;
  return { event: { preventDefault: () => (prevented = true) }, prevented: () => prevented };
}

describe("Leaving", () => {
  it("prevents repeated close requests while claims are unresolved", async () => {
    mockIPC((cmd) => (cmd === "human_taken" ? ["run-a-2"] : null));
    const leaving = new Leaving();
    const [first, second, third] = [close(), close(), close()];
    await Promise.all([leaving.closeRequested(first.event), leaving.closeRequested(second.event)]);
    await leaving.closeRequested(third.event);
    expect([first.prevented(), second.prevented(), third.prevented()]).toEqual([true, true, true]);
    expect(leaving.taken).toEqual(["run-a-2"]);
  });

  it("lets the window close without claims", async () => {
    mockIPC((cmd) => (cmd === "human_taken" ? [] : null));
    const leaving = new Leaving();
    const request = close();
    await leaving.closeRequested(request.event);
    expect(request.prevented()).toBe(false);
  });
});
