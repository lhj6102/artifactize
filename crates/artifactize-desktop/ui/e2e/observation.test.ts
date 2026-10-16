import assert from "node:assert/strict";
import { createServer } from "node:net";
import { it } from "node:test";
import { sessionEndedByApp } from "./observation.ts";
import type { Identity } from "./runs.ts";

const owner: Identity = { pid: 123, start_time: "9007199254740993" };

function app() {
  let running = true;
  const browser = {
    sessionId: "the-live-session",
    execute: async () => ({ ...owner }),
    // Expiry is controlled explicitly: the fake process never changes unless the
    // action changes it, rather than waiting for elapsed wall-clock time.
    waitUntil: async (check: () => Promise<boolean>) => {
      if (!(await check())) throw new Error("the native app is still running");
    },
  } as unknown as WebdriverIO.Browser;
  const alive = (identity: Identity) => {
    assert.deepEqual(identity, owner, "every read refers to the same exact birth identity");
    return running;
  };
  return { browser, alive, exit: () => { running = false; } };
}

it("retires only the client session after the previously live native process exits", async () => {
  const { browser, alive, exit } = app();
  assert.equal(alive(owner), true, "the same observer sees the native process alive");
  await sessionEndedByApp(browser, async () => {
    assert.equal(browser.sessionId, "the-live-session", "the action does not retire a live session");
    exit();
    throw new Error("the quit response ended with the app");
  }, alive);
  assert.equal(browser.sessionId, undefined);
  assert.equal(alive(owner), false);
});

it("fails without retiring when the listener closes but native cleanup remains alive", async () => {
  const server = createServer((socket) => socket.end());
  await new Promise<void>((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const close = async () => {
    if (server.listening) await new Promise<void>((done, fail) => server.close((error) => error ? fail(error) : done()));
  };
  const { browser, alive } = app();
  try {
    assert.equal(server.listening, true);
    await assert.rejects(sessionEndedByApp(browser, close, alive), /the native app is still running/);
    assert.equal(server.listening, false, "the embedded endpoint is already gone");
    assert.equal(alive(owner), true, "native cleanup has not ended with its listener");
    assert.equal(browser.sessionId, "the-live-session", "service teardown cannot turn this failure into a passing quit");
  } finally {
    await close();
  }
});

it("preserves the action failure while the native process remains alive", async () => {
  const { browser, alive } = app();
  await assert.rejects(sessionEndedByApp(browser, async () => { throw new Error("quit failed"); }, alive), (error: unknown) => {
    assert.ok(error instanceof AggregateError);
    assert.deepEqual(error.errors.map(String), ["Error: quit failed", "Error: the native app is still running"]);
    return true;
  });
  assert.equal(browser.sessionId, "the-live-session");
});

it("refuses an app already exited before the action", async () => {
  const { browser, alive, exit } = app();
  exit();
  let acted = false;
  await assert.rejects(sessionEndedByApp(browser, async () => { acted = true; }, alive), /already exited before its quit action/);
  assert.equal(acted, false);
  assert.equal(browser.sessionId, "the-live-session");
});

it("fails when test-only native metadata or its observation is unavailable", async () => {
  const { browser, alive } = app();
  await assert.rejects(sessionEndedByApp(browser, async () => {}, () => { throw new Error("native observation failed"); }), /native observation failed/);
  browser.execute = (async () => undefined) as typeof browser.execute;
  await assert.rejects(sessionEndedByApp(browser, async () => {}, alive), /did not expose its native birth identity/);
  assert.equal(browser.sessionId, "the-live-session");
});
