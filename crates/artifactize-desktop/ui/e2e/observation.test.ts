import assert from "node:assert/strict";
import { createServer } from "node:net";
import { it } from "node:test";
import { observeWindow, sessionEndedByApp, windowClosed } from "./observation.ts";

async function endpoint() {
  const server = createServer((socket) => socket.end());
  await new Promise<void>((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  let handles = ["main"];
  const browser = {
    options: { hostname: "127.0.0.1", port: address.port },
    sessionId: "the-live-session",
    getWindowHandles: async () => [...handles],
    status: async () => ({ ready: true }),
    waitUntil: async (check: () => Promise<boolean>) => {
      if (!(await check())) throw new Error("the app is still running");
    },
  } as unknown as WebdriverIO.Browser;
  const close = async () => {
    if (server.listening) await new Promise<void>((done, fail) => server.close((error) => error ? fail(error) : done()));
  };
  return { browser, close, hideWindow: () => { handles = []; } };
}

it("retires only the client session after the established in-app endpoint really ends", async () => {
  const { browser, close } = await endpoint();
  try {
    await observeWindow(browser);
    assert.equal(await windowClosed(browser), false, "the same probe sees the live endpoint");
    await sessionEndedByApp(browser, async () => {
      assert.equal(browser.sessionId, "the-live-session", "the action does not retire a live session");
      await close();
      throw new Error("the quit response ended with the app");
    });
    assert.equal(browser.sessionId, undefined);
    assert.equal(await windowClosed(browser), true);
  } finally {
    await close();
  }
});

it("does not mistake a vanished window for the app ending while its server stays alive", async () => {
  const { browser, close, hideWindow } = await endpoint();
  try {
    await observeWindow(browser);
    await assert.rejects(sessionEndedByApp(browser, async () => { hideWindow(); }), /the app is still running/);
    assert.deepEqual(await browser.getWindowHandles(), []);
    assert.equal(browser.sessionId, "the-live-session");
    assert.equal(await windowClosed(browser), false);
  } finally {
    await close();
  }
});

it("preserves an action failure and never retires a session whose window remains open", async () => {
  const { browser, close } = await endpoint();
  try {
    await observeWindow(browser);
    await assert.rejects(sessionEndedByApp(browser, async () => { throw new Error("quit failed"); }), (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      assert.deepEqual(error.errors.map(String), ["Error: quit failed", "Error: the app is still running"]);
      return true;
    });
    assert.equal(browser.sessionId, "the-live-session");
    assert.equal(await windowClosed(browser), false);
  } finally {
    await close();
  }
});
