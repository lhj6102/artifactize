// The shared IPC boundary reports dispatched and completed commands without exposing
// credentials, mutating generated bindings or confusing superseded search generations.
import { describe, expect, it } from "vitest";
import { Activity, observeCommands, refreshed } from "./activity.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("command activity", () => {
  it("observes dispatch before native completion and preserves results and failures", async () => {
    const activity = new Activity();
    const native = deferred<string>();
    const source = { claim: (_id: string) => native.promise };
    const commands = observeCommands(source, activity);
    expect(activity.snapshot().commands.claim).toEqual({ started: 0, completed: 0, pending: 0 });
    expect(activity.snapshot().commands.unregistered).toBeUndefined();
    const pending = commands.claim("private request");
    const dispatched = activity.snapshot();
    expect(dispatched.commands.claim).toEqual({ started: 1, completed: 0, pending: 1 });
    expect(dispatched.pending).toBe(1);
    expect(source.claim("other")).toBe(native.promise);
    native.resolve("claimed");
    expect(await pending).toBe("claimed");
    expect(activity.snapshot().commands.claim).toEqual({ started: 1, completed: 1, pending: 0 });
    // A reader gets detached immutable values, not access to the activity owner.
    expect(dispatched.commands.claim.pending).toBe(1);
    expect(Object.isFrozen(dispatched.commands.claim)).toBe(true);
    const failure = new Error("native failure");
    const failed = observeCommands({ fail: () => Promise.reject(failure) }, activity).fail();
    await expect(failed).rejects.toBe(failure);
    expect(activity.snapshot().commands.fail).toEqual({ started: 1, completed: 1, pending: 0 });
  });

  it("retains only the latest search generation through A → B → A and reversed completion", async () => {
    // A browse without a query (a navigation, a refresh) is no search.
    const activity = new Activity();
    const first = deferred<string>();
    const middle = deferred<string>();
    const last = deferred<string>();
    const answers = [first, middle, last];
    const commands = observeCommands({ browse: (_navigate: { query: string | null }) => answers.shift()!.promise }, activity);
    const a = commands.browse({ query: "A" });
    const b = commands.browse({ query: "B" });
    const again = commands.browse({ query: "A" });
    first.resolve("old A");
    await a;
    expect(activity.snapshot().search).toEqual({ query: "A", sequence: 3, completed: false });
    last.resolve("new A");
    await again;
    middle.resolve("old B");
    await b;
    expect(activity.snapshot().search).toEqual({ query: "A", sequence: 3, completed: true });
    expect(activity.snapshot().pending).toBe(0);
    const failed = activity.run("browse", () => Promise.reject(new Error("search failed")), "invalid:");
    await expect(failed).rejects.toThrow("search failed");
    expect(activity.snapshot().search).toEqual({ query: "invalid:", sequence: 4, completed: true });
  });

  it("settles synchronous dispatch failures and overlapping command names exactly once", async () => {
    const activity = new Activity();
    const native = deferred<string>();
    const failure = new Error("dispatch failed");
    const commands = observeCommands({
      waiting: () => native.promise,
      thrown: (): Promise<never> => { throw failure; },
    }, activity);
    const waiting = commands.waiting();
    await expect(commands.thrown()).rejects.toBe(failure);
    expect(activity.snapshot().commands.thrown).toEqual({ started: 1, completed: 1, pending: 0 });
    expect(activity.snapshot().pending).toBe(1);
    native.resolve("finished");
    await expect(waiting).resolves.toBe("finished");
    expect(activity.snapshot().commands.waiting).toEqual({ started: 1, completed: 1, pending: 0 });
    expect(activity.snapshot().pending).toBe(0);
    expect(activity.snapshot().revision).toBe(4);
  });

  it("projects pending activity across overlapping commands and reverse completion", async () => {
    const activity = new Activity();
    const first = deferred<string>();
    const second = deferred<string>();
    const other = deferred<string>();
    const answers = [first, second];
    const commands = observeCommands({
      search: () => answers.shift()!.promise,
      browse: () => other.promise,
    }, activity);
    const a = commands.search();
    const b = commands.search();
    const c = commands.browse();
    const dispatched = activity.snapshot();
    expect(dispatched.pending).toBe(3);
    expect(dispatched.commands.search).toEqual({ started: 2, completed: 0, pending: 2 });
    expect(dispatched.commands.browse).toEqual({ started: 1, completed: 0, pending: 1 });

    second.resolve("latest search");
    await b;
    expect(activity.snapshot().pending).toBe(2);
    expect(activity.snapshot().commands.search).toEqual({ started: 2, completed: 1, pending: 1 });
    other.resolve("browse done");
    await c;
    expect(activity.snapshot().pending).toBe(1);
    expect(activity.snapshot().commands.browse).toEqual({ started: 1, completed: 1, pending: 0 });
    first.resolve("earlier search");
    await a;
    expect(activity.snapshot().pending).toBe(0);
    expect(activity.snapshot().commands.search).toEqual({ started: 2, completed: 2, pending: 0 });
    expect(dispatched.pending).toBe(3);
    expect(dispatched.commands.search.pending).toBe(2);
  });

  it("does not retain command arguments or results such as login credentials", async () => {
    const activity = new Activity();
    const commands = observeCommands({ login: async (_secret: string) => "private result" }, activity);
    expect(await commands.login("private token")).toBe("private result");
    expect(JSON.stringify(activity.snapshot())).not.toMatch(/private|token/);
    expect(activity.snapshot().search).toBeNull();
  });

  it("has no manual-refresh witness outside E2E builds", () => {
    expect(import.meta.env.MODE).not.toBe("e2e");
    const activity = new Activity();
    refreshed(activity);
    expect(Object.keys(activity.snapshot())).toEqual(["revision", "pending", "commands", "search"]);
  });
});
