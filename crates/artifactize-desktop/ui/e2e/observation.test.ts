import assert from "node:assert/strict";
import { it } from "node:test";
import { serveLoopback } from "./platform.ts";
import { sessionEndedByApp } from "./observation.ts";
import type { Identity } from "./runs.ts";

const owner: Identity = { pid: 123, start_time: "9007199254740993" };

function app() {
  let running = true;
  let graceful = false;
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
  return {
    browser, alive, orderlyExit: () => graceful,
    exit: (orderly = true) => { running = false; graceful = orderly; },
  };
}

const lostWebview = "runtime error: failed to send message to the webview";
const actionError = (message: string, command: string) =>
  new Error(`WebDriverError: ${message} when running "${command}" with method "POST"`);

for (const [message, command] of [["Channel closed", "actions"], [lostWebview, "actions"], [lostWebview, "execute/async"]]) {
  it(`retires the client session for ${message} on ${command} only after a native exit with a receipt`, async () => {
    const { browser, alive, exit, orderlyExit } = app();
    assert.equal(alive(owner), true, "the same observer sees the native process alive");
    await sessionEndedByApp(browser, async () => {
      assert.equal(browser.sessionId, "the-live-session", "the action does not retire a live session");
      exit();
      throw actionError(message, command);
    }, alive, orderlyExit);
    assert.equal(browser.sessionId, undefined);
  });
}

it("rejects losing the webview if the app is still alive or exited without its receipt", async () => {
  for (const stop of [false, true]) {
    const { browser, alive, exit, orderlyExit } = app();
    const actionFailure = actionError(lostWebview, "actions");
    await assert.rejects(sessionEndedByApp(browser, async () => {
      if (stop) exit(false);
      throw actionFailure;
    }, alive, orderlyExit), (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors[0], actionFailure);
      assert.match(String(error.errors[1]), stop ? /without a successful, orderly shutdown/ : /still running/);
      return true;
    });
    assert.equal(browser.sessionId, "the-live-session");
  }
});

it("does not turn a transport loss caused by a crash into an orderly quit", async () => {
  const { browser, alive, exit, orderlyExit } = app();
  await assert.rejects(sessionEndedByApp(browser, async () => {
    exit(false);
    throw new Error("ECONNRESET");
  }, alive, orderlyExit), /quit action and native app-exit observation failed/);
  assert.equal(browser.sessionId, "the-live-session");
});

it("fails without retiring when the listener closes but native cleanup remains alive", async () => {
  const server = await serveLoopback();
  const close = () => server.close();
  const { browser, alive } = app();
  try {
    assert.equal(server.listening(), true);
    await assert.rejects(sessionEndedByApp(browser, close, alive), /the native app is still running/);
    assert.equal(server.listening(), false, "the embedded endpoint is already gone");
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
