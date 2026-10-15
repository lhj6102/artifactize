// Observable fixture boundaries: owned window lifetime, complete failure reporting,
// isolated Git writes, and ownership even when native registration fails. No app build.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { it } from "node:test";
import { windowClosed } from "./driver.ts";
import { cleanup, resourceRoot } from "./build.ts";
import { groupAlive, type Group, Owned, registeredGroups, withCleanup } from "./owned.ts";

/** Only a bound on joining actual native lifetimes, not an assertion delay. */
const GROUP_EXIT_TIMEOUT = 10000;
async function groupsGone(groups: Group[]): Promise<void> {
  const deadline = Date.now() + GROUP_EXIT_TIMEOUT;
  while (groups.some(({ group, start_time }) => groupAlive(group, start_time))) {
    if (Date.now() >= deadline) throw new Error("owned acquisition groups did not exit; preserve inputs");
    await new Promise((done) => setTimeout(done, 10));
  }
}

it("requires an owned app lifetime rather than treating a session response or error as closure", async () => {
  await assert.rejects(windowClosed({ getWindowHandles: async () => [] }), /requires an owned native app session/);
  await assert.rejects(windowClosed({ getWindowHandles: async () => { throw new Error("transport failed"); } }), /requires an owned native app session/);
});

it("retains the primary error and attempts every failed owned teardown", async () => {
  const primary = new Error("the test failed");
  const native = new Error("the native stop failed");
  const lease = new Error("the lease stop failed");
  let drained = false;
  await assert.rejects(withCleanup(() => { throw primary; }, () => { throw native; }, () => {
    drained = true;
    throw lease;
  }), (error) => error instanceof AggregateError && error.errors[0] === primary && error.errors[1] === native && error.errors[2] === lease);
  assert.equal(drained, true, "an earlier teardown failure does not bypass later owners");
});

it("keeps Git repository/index/hooks/config routing inside its fixture", () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-git-boundary-"));
  const foreign = join(root, "foreign");
  const hooks = join(root, "foreign-hooks");
  const marker = join(root, "foreign-hook-ran");
  const index = join(root, "foreign-index");
  mkdirSync(foreign);
  mkdirSync(hooks);
  writeFileSync(join(foreign, "sentinel"), "unchanged");
  writeFileSync(join(hooks, "pre-commit"), `#!/bin/sh\ntouch '${marker}'\nexit 1\n`, { mode: 0o700 });
  const config = join(root, "foreign-config");
  writeFileSync(config, `[core]\nhooksPath = ${JSON.stringify(hooks)}\n[commit]\ngpgSign = true\n`);
  const source = `import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import assert from "node:assert/strict";
import { git } from ${JSON.stringify(new URL("./environment.ts", import.meta.url).href)};
const repo = ${JSON.stringify(join(root, "repo"))};
mkdirSync(repo);
writeFileSync(repo + "/input", "owned");
git(repo, "init", "-q", "--initial-branch=master", repo);
git(repo, "-C", repo, "add", ".");
git(repo, "-C", repo, "commit", "-qm", "Owned fixture");
git(repo, "-C", repo, "worktree", "add", "-q", "-b", "other", ${JSON.stringify(join(root, "other"))});
assert.ok(existsSync(repo + "/.git/index"));
assert.ok(existsSync(${JSON.stringify(join(root, "other", ".git"))}));`;
  try {
    execFileSync(process.execPath, ["--input-type=module", "-e", source], {
      env: { ...process.env, GIT_DIR: foreign, GIT_WORK_TREE: foreign, GIT_INDEX_FILE: index,
        GIT_CONFIG_GLOBAL: config, GIT_CONFIG_SYSTEM: config, GIT_CONFIG_NOSYSTEM: "0",
        GIT_CONFIG_COUNT: "1", GIT_CONFIG_KEY_0: "core.hooksPath", GIT_CONFIG_VALUE_0: hooks },
      encoding: "utf8",
    });
    assert.deepEqual(readdirSync(foreign), ["sentinel"], "the inherited repository was not initialized or modified");
    assert.equal(existsSync(index), false, "staging did not use the inherited index");
    assert.equal(existsSync(marker), false, "commit did not invoke inherited hooks or signing");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

it("does not retain a candidate for an unrelated group's reuse of a registered number", async () => {
  const foreign = mkdtempSync(join(tmpdir(), "artifactize-desktop-foreign-group-"));
  writeFileSync(join(foreign, "groups"), "");
  const owner = Owned.spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    env: { ...process.env, DESKTOP_TEST_RESOURCES: foreign }, stdio: ["ignore", "ignore", "ignore"],
  });
  const root = resourceRoot();
  await withCleanup(async () => {
    const [current] = registeredGroups(foreign);
    assert.ok(groupAlive(current.group, current.start_time), "the independent owner is actually alive");
    // Model a stale registry lifetime without requiring the OS to recycle a PID.
    writeFileSync(join(root, "groups"), JSON.stringify({ group: current.group, start_time: current.start_time - 1 }) + "\n");
    await cleanup(root);
    assert.equal(existsSync(root), false, "a different birth is not an owned user of these inputs");
    assert.ok(groupAlive(current.group, current.start_time), "cleanup leaves the unrelated evaluation untouched");
  }, () => cleanup(root), async () => {
    await owner.stop();
    rmSync(foreign, { recursive: true, force: true });
  });
});

it("joins an acquired process even when registering its group fails", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-registration-"));
  mkdirSync(join(root, "groups")); // appendFile must fail after the process was acquired.
  const admitted = join(root, "native-work-admitted");
  const owner = Owned.spawn(process.execPath, ["-e", `require("node:fs").writeFileSync(${JSON.stringify(admitted)}, "admitted"); setInterval(() => {}, 1000);`], {
    env: { ...process.env, DESKTOP_TEST_RESOURCES: root }, stdio: ["ignore", "ignore", "ignore"],
  });
  const acquisition = await new Promise<Error>((done) => owner.child.once("error", done));
  await withCleanup(async () => {
    assert.equal((acquisition as NodeJS.ErrnoException).code, "EISDIR");
    await assert.rejects(owner.stop(), (error) => error === acquisition);
    assert.equal(groupAlive(owner.group), false, "failed registration still joins the actual acquired group");
    assert.equal(existsSync(admitted), false, "registration failure never admits native work, including a late watchdog ACK");
  }, async () => {
    try { await owner.stop(); } catch (error) { if (error !== acquisition) throw error; }
    if (groupAlive(owner.group)) throw new Error("preserve inputs of an acquired group that has not stopped");
    rmSync(root, { recursive: true, force: true });
  });
});

it("never admits native resources when the owner dies during pending acquisition", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-acquisition-death-"));
  writeFileSync(join(root, "groups"), "");
  const admitted = join(root, "native-work-admitted");
  const program = `require("node:fs").writeFileSync(${JSON.stringify(admitted)}, "admitted"); setInterval(() => {}, 1000);`;
  const source = `import { writeFileSync } from "node:fs";
import { Owned, groupIdentity } from ${JSON.stringify(new URL("./owned.ts", import.meta.url).href)};
const child = Owned.spawn(process.execPath, ["-e", ${JSON.stringify(program)}], {
  env: { ...process.env, DESKTOP_TEST_RESOURCES: ${JSON.stringify(root)} }, stdio: ["ignore", "ignore", "ignore"]
});
// Stay in the acquisition turn: no asynchronous armed ACK can admit native work yet.
writeFileSync(1, JSON.stringify(groupIdentity(child.group)) + "\\n");
process.kill(process.pid, "SIGKILL");`;
  const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e", source], { env: process.env, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  let errors = "";
  owner.child.stdout!.on("data", (chunk) => { output += chunk; });
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  await withCleanup(async () => {
    await closed;
    assert.equal(owner.child.signalCode, "SIGKILL", errors);
    const acquired = JSON.parse(output.trim()) as Group;
    await groupsGone([acquired]);
    assert.equal(existsSync(admitted), false, "owner death before armed admission cannot acquire a native display or lock");
  }, () => owner.stop(), async () => {
    await groupsGone(registeredGroups(root));
    rmSync(root, { recursive: true, force: true });
  });
});

it("keeps the actual program PID, stdin, stdout, stderr and an existing readiness descriptor", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-exec-contract-"));
  writeFileSync(join(root, "groups"), "");
  const source = `require("node:fs").writeFileSync(3, String(process.pid) + "\\n");
process.stdin.once("data", (data) => {
  process.stdout.write(data);
  process.stderr.write("actual target diagnostic\\n");
  process.stdin.pause();
  process.exitCode = 7;
});`;
  const owner = Owned.spawn(process.execPath, ["-e", source], {
    env: { ...process.env, PATH: root, DESKTOP_TEST_RESOURCES: root }, stdio: ["pipe", "pipe", "pipe", "pipe"],
  });
  let output = "";
  let errors = "";
  let readiness = "";
  owner.child.stdout!.on("data", (chunk) => { output += chunk; });
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  owner.child.stdio[3]!.on("data", (chunk) => { readiness += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  await withCleanup(async () => {
    owner.child.stdin!.end("actual stdin payload");
    await closed;
    assert.equal(Number(readiness.trim()), owner.child.pid, "exec retains the acquired process identity");
    assert.equal(owner.group, owner.child.pid);
    assert.equal(output, "actual stdin payload");
    assert.match(errors, /actual target diagnostic/);
    assert.equal(owner.child.exitCode, 7, "the target's real failure status remains observable");
  }, async () => {
    await owner.stop();
    rmSync(root, { recursive: true, force: true });
  });
});

it("reports an unavailable target immediately through its actual exit and diagnostics", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-unavailable-target-"));
  writeFileSync(join(root, "groups"), "");
  const target = join(root, "missing-command");
  const owner = Owned.spawn(target, [], { env: { ...process.env, DESKTOP_TEST_RESOURCES: root }, stdio: ["ignore", "ignore", "pipe"] });
  let errors = "";
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  await withCleanup(async () => {
    await closed;
    assert.equal(owner.child.exitCode, 127);
    assert.ok(errors.includes(target), errors);
  }, async () => {
    await owner.stop();
    rmSync(root, { recursive: true, force: true });
  });
});

it("releases admitted native resources and inherited stderr after its owner is killed", async () => {
  const root = mkdtempSync(join(tmpdir(), "artifactize-desktop-admitted-orphan-"));
  writeFileSync(join(root, "groups"), "");
  const lock = join(root, "native-resource.lock");
  const release = join(root, "teardown-release");
  const program = `const fs = require("node:fs");
function release() {
  fs.rmSync(${JSON.stringify(lock)}, { force: true });
  fs.writeSync(2, "orphan native resource released\\n");
  process.exit(0);
}
process.once("SIGTERM", release);
fs.writeFileSync(${JSON.stringify(lock)}, "owned native resource");
process.stdout.write("ready\\n");
// Teardown can release this declared fixture even if the watchdog regresses. It
// cannot satisfy the assertions, which finish before teardown admits this path.
setInterval(() => { if (fs.existsSync(${JSON.stringify(release)})) release(); }, 10);`;
  const source = `import { existsSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { Owned, groupIdentity } from ${JSON.stringify(new URL("./owned.ts", import.meta.url).href)};
const child = Owned.spawn(process.execPath, ["-e", ${JSON.stringify(program)}], {
  env: { ...process.env, DESKTOP_TEST_RESOURCES: ${JSON.stringify(root)} }, stdio: ["ignore", "pipe", "inherit"]
});
child.child.once("error", (error) => { throw error; });
child.child.once("close", () => { throw new Error("native fixture exited before owner interruption"); });
const lines = createInterface({ input: child.child.stdout });
lines.once("line", (line) => {
  if (line !== "ready") throw new Error("native fixture did not acquire its resource");
  writeFileSync(1, JSON.stringify({ ...groupIdentity(child.group), resourcesPresent: existsSync(${JSON.stringify(lock)}) }) + "\\n");
  lines.close();
});
process.stdin.once("data", () => process.kill(process.pid, "SIGKILL"));`;
  const owner = Owned.spawn(process.execPath, ["--input-type=module", "-e", source], { env: process.env, stdio: ["pipe", "pipe", "pipe"] });
  let errors = "";
  owner.child.stderr!.on("data", (chunk) => { errors += chunk; });
  const exited = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("exit", () => done());
  });
  const closed = new Promise<void>((done, fail) => {
    owner.child.once("error", fail);
    owner.child.once("close", () => done());
  });
  // Both observations are retained immediately; owner exit is only the intermediate
  // event. The inherited stderr writers must really close before release completes.
  void exited.catch(() => {});
  void closed.catch(() => {});
  const lines = createInterface({ input: owner.child.stdout! });
  const acquired = new Promise<Group & { resourcesPresent: boolean }>((done, fail) => {
    lines.once("line", (line) => {
      try { done(JSON.parse(line) as Group & { resourcesPresent: boolean }); }
      catch (error) { fail(error); }
    });
    void exited.then(() => fail(new Error(`owner exited before native readiness: ${errors}`)), fail);
  });
  await withCleanup(async () => {
    const group = await acquired;
    assert.equal(group.resourcesPresent, true);
    assert.ok(existsSync(lock), "the real target acquired a mutable resource before interruption");
    owner.child.stdin!.end("interrupt");
    await exited;
    assert.equal(owner.child.signalCode, "SIGKILL");
    await groupsGone([group]);
    assert.equal(existsSync(lock), false, "the orphan watchdog delivered TERM and released the native resource");
    await closed;
    assert.match(errors, /orphan native resource released/, "final inherited stderr was drained before actual close");
  }, async () => {
    writeFileSync(release, "release declared fixture");
    await owner.stop();
    await groupsGone(registeredGroups(root));
    lines.close();
    rmSync(root, { recursive: true, force: true });
  });
});
