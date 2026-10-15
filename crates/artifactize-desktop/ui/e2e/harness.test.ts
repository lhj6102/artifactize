// The Agent session's launch and declaration tools drive the real app and real fixture bytes.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { afterEach, beforeEach, describe, it } from "node:test";
import { create, createGraph } from "./fixture.ts";
import { checkBuild, LAUNCH_TIMEOUT, start, type Session } from "./driver.ts";
import { commandStream, snapshot, target } from "../../harness/harness.mts";
import { Owned } from "./owned.ts";
import { snapshotsGone } from "./build.ts";
import { running, startedWorkers, states } from "./runs.ts";
import { press } from "./keys.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

// Pure lifecycle seams: selecting this suite never starts the native app or creates files.
describe("desktop command stream lifecycle", () => {
  for (const fails of [false, true]) {
    it(`drains a ${fails ? "failed" : "successful"} relaunch before releasing its replacement and rejects queued/new work`, async () => {
      const started = deferred<void>();
      const replacement = deferred<void>();
      const events: string[] = [];
      let owner = "original";
      const commands = commandStream(() => { events.push("closed"); }, async () => {
        events.push(`stopped ${owner}`);
        events.push("removed inputs");
      });
      const launch = commands.run(async () => {
        events.push(`stopped ${owner}`);
        started.resolve();
        await replacement.promise;
        owner = "replacement";
        if (fails) throw new Error("replacement did not render");
        return "launched";
      });
      await started.promise;
      const queued = assert.rejects(commands.run(async () => { events.push("queued work"); }), /session is stopping/);
      const ending = commands.end();
      const completed = fails ? assert.rejects(launch, /replacement did not render/) : launch;
      try {
        assert.equal(commands.end(), ending, "signal and socket stop join the same shutdown");
        assert.deepEqual(events, ["stopped original", "closed"], "admission closes without releasing the active launch's inputs");
        await assert.rejects(commands.run(async () => { events.push("new work"); }), /session is stopping/);
      } finally {
        replacement.resolve();
        await Promise.all([completed, queued, ending]);
      }
      assert.deepEqual(events, ["stopped original", "closed", "stopped replacement", "removed inputs"]);
    });
  }

  it("reports a failed owner release without proceeding to input removal or retrying release", async () => {
    const stopped = deferred<void>();
    let inputs = "preserved";
    const commands = commandStream(() => {}, async () => {
      await stopped.promise;
      inputs = "removed";
    });
    const ending = commands.end();
    const failed = assert.rejects(ending, /owner still running/);
    stopped.reject(new Error("owner still running"));
    await failed;
    assert.equal(inputs, "preserved");
    assert.equal(commands.end(), ending, "a repeated signal cannot bypass the failed release");
    await assert.rejects(commands.run(async () => { inputs = "changed"; }), /session is stopping/);
    assert.equal(inputs, "preserved");
  });
});

describe("desktop snapshot controls", () => {
  let session: Session;
  beforeEach(async () => {
    checkBuild();
    session = await start(await create());
    await session.browser.$('[role="treeitem"]').waitForExist();
  });
  afterEach(async () => { await session?.dispose(); });

  it("observes without DOM markers and never retargets a removed control to its identical replacement", async () => {
    const { browser } = session;
    const palette = () => browser.$('[role="dialog"][aria-label="Command palette"]');
    await press(browser, ["Control", "k"], () => palette().isExisting(), "the command palette");
    const shown = await snapshot(browser);
    const control = shown.match(/\[(\d+)\] dialog "Command palette"/);
    assert.ok(control, shown);
    assert.equal(await browser.execute(() => document.querySelectorAll("[data-snap]").length), 0,
      "snapshot adds no control attributes to the page");
    await press(browser, ["Escape"], async () => !(await palette().isExisting()), "the command palette closed");
    await press(browser, ["Control", "k"], () => palette().isExisting(), "the command palette");
    // Do not take another snapshot: its old number still names the removed native
    // element, even though an indistinguishable new control occupies the same place.
    await assert.rejects(target(browser, Number(control[1])), /no element|stale|no such element/);
    assert.equal(await palette().isExisting(), true, "the replacement remains untouched");
  });
});

describe("desktop Agent session", () => {
  const harness = resolve(import.meta.dirname, "../../harness/harness.mts");
  let owner: Owned;
  let env: NodeJS.ProcessEnv;
  let directory: string;
  let stopped = false;
  function call(mode: string, args: Record<string, unknown> = {}): string {
    const output = execFileSync(process.execPath, [harness, mode], {
      env, input: JSON.stringify({ args }), encoding: "utf8", timeout: mode === "launch" ? LAUNCH_TIMEOUT : 30000,
    });
    if (mode === "stop") { stopped = true; return output; }
    const reply = JSON.parse(output) as { isError?: boolean; content: { text: string }[] };
    assert.equal(reply.isError ?? false, false, output);
    return reply.content.map((part) => part.text).join("\n");
  }

  beforeEach(async () => {
    checkBuild();
    stopped = false;
    directory = join(createGraph().root, "session");
    mkdirSync(directory);
    env = { ...process.env, ARTIFACTIZE_SESSION_DIR: directory, ARTIFACTIZE_REMOTE: "off" };
    owner = Owned.spawn(process.execPath, [harness, "start"], { env, stdio: ["ignore", "pipe", "inherit"] });
    await new Promise<void>((done, fail) => {
      const lines = createInterface({ input: owner.child.stdout! });
      const deadline = setTimeout(() => finish(new Error("desktop Agent session did not become ready")), LAUNCH_TIMEOUT);
      const exited = (code: number | null) => finish(new Error(`desktop Agent session exited ${code} before ready`));
      function finish(error?: Error) {
        clearTimeout(deadline);
        lines.close();
        owner.child.off("exit", exited);
        if (error) fail(error);
        else done();
      }
      lines.once("line", (line) => line === "ready" ? finish() : finish(new Error(`unexpected readiness: ${line}`)));
      owner.child.once("exit", exited);
    });
  });
  afterEach(async () => {
    if (!owner) return;
    try { if (!stopped) call("stop"); }
    finally {
      await owner.stop();
      await snapshotsGone(directory);
    }
  });

  it("relaunches each real entry point and shows a changed declaration only after current-graph refresh", () => {
    assert.match(call("snapshot"), /signoff/);
    assert.match(call("launch", { screen: "graph" }), /Current Artifact graph/);
    assert.match(call("cli", { args: ["run", "list", "--json"] }), /--- stdout\n\[\]/, "graph state has no saved Run");
    const changed = call("change-declaration");
    assert.match(changed, /Docs criterion revision 1/);
    assert.doesNotMatch(call("snapshot"), /Docs criterion revision 1/, "no refresh was simulated");
    const refreshed = call("act", { action: "key", key: "Control+r" });
    const control = refreshed.match(/\[(\d+)\] summary "Structure and evaluation criteria/);
    assert.ok(control, refreshed);
    assert.match(call("act", { action: "click", element: Number(control[1]) }), /Docs criterion revision 1/);
    const review = call("launch", { screen: "review" });
    assert.match(review, /Search requests/);
    assert.match(review, /release\/signoff/);
    assert.match(call("launch", { screen: "monitor" }), /Run run-/);
    assert.match(call("snapshot"), /signoff/);
  });

  it("returns the rendered results of a deferred search, including A → B → A", () => {
    let shown = call("launch", { screen: "review" });
    const query = (text: string) => {
      const field = shown.match(/\[(\d+)\] combobox "Search requests"/);
      assert.ok(field, shown);
      call("act", { action: "click", element: Number(field[1]) });
      call("act", { action: "key", key: "Control+a" });
      call("act", { action: "key", key: "Backspace" });
      shown = call("act", { action: "type", text });
      return shown.slice(shown.indexOf("\ntext:\n"));
    };
    assert.match(query("eval:release/signoff"), /release\/signoff/);
    assert.doesNotMatch(query("eval:absent/criterion"), /release\/signoff/, "the reply must not contain the previous query's rows");
    assert.match(query("eval:release/signoff"), /release\/signoff/);
  });

  it("settles workers from every exposed state before deleting the fixture", async () => {
    call("launch", { screen: "graph" });
    assert.match(call("cli", { args: ["verify", "--all", "--json"] }), /"run": "run-/);
    const candidate = join(directory, readdirSync(directory).find((name) => name.startsWith("candidate-"))!);
    const fixtures = join(candidate, "fixtures");
    const root = join(fixtures, readdirSync(fixtures)[0]);
    const exposed = states(root);
    assert.equal(exposed.length, 2, "both Monitor state and graph-state exist");
    const owners = await startedWorkers(exposed);
    assert.ok(owners.some(running), "durable workers actually exist before stop");
    // Relaunching away from the graph must not lose its state ownership.
    call("launch", { screen: "monitor" });
    call("stop");
    assert.ok(owners.every((owner) => !running(owner)), "every observed worker exited");
    assert.equal(existsSync(root), false, "only then are the working files removed");
  });
});
