// Linux-native scenarios shared by the ownership seam tests. Tests retain their
// independent oracles; this boundary owns programs, paths, environment and signals.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup } from "./build.ts";
import { holdCleanupOutput, moduleURL, NativeProcess, nativeFixture, observe } from "./native-fixtures.ts";
import { groupAlive, registeredGroups, type Group, withCleanup } from "./owned.ts";

async function groupsGone(groups: Group[]): Promise<void> {
  await observe(() => groups.every(({ group, start_time }) => !groupAlive(group, start_time)),
    () => "owned acquisition groups did not exit; preserve inputs");
}

export async function gitBoundaryFixture() {
  return nativeFixture((scratch) => {
  const foreign = scratch.path("foreign");
  const hooks = scratch.path("foreign-hooks");
  const marker = scratch.path("foreign-hook-ran");
  const index = scratch.path("foreign-index");
  mkdirSync(foreign);
  mkdirSync(hooks);
  writeFileSync(join(foreign, "sentinel"), "unchanged");
  writeFileSync(join(hooks, "pre-commit"), `#!/bin/sh\ntouch '${marker}'\nexit 1\n`, { mode: 0o700 });
  const config = scratch.path("foreign-config");
  writeFileSync(config, `[core]\nhooksPath = ${JSON.stringify(hooks)}\n[commit]\ngpgSign = true\n`);
  return {
    exercise() {
      const source = `import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import assert from "node:assert/strict";
import { git } from ${JSON.stringify(moduleURL("./environment.ts"))};
const repo = ${JSON.stringify(scratch.path("repo"))};
mkdirSync(repo);
writeFileSync(repo + "/input", "owned");
git(repo, "init", "-q", "--initial-branch=master", repo);
git(repo, "-C", repo, "add", ".");
git(repo, "-C", repo, "commit", "-qm", "Owned fixture");
git(repo, "-C", repo, "worktree", "add", "-q", "-b", "other", ${JSON.stringify(scratch.path("other"))});
assert.ok(existsSync(repo + "/.git/index"));
assert.ok(existsSync(${JSON.stringify(scratch.path("other/.git"))}));`;
      execFileSync(process.execPath, ["--input-type=module", "-e", source], {
        env: scratch.env({ GIT_DIR: foreign, GIT_WORK_TREE: foreign, GIT_INDEX_FILE: index,
          GIT_CONFIG_GLOBAL: config, GIT_CONFIG_SYSTEM: config, GIT_CONFIG_NOSYSTEM: "0",
          GIT_CONFIG_COUNT: "1", GIT_CONFIG_KEY_0: "core.hooksPath", GIT_CONFIG_VALUE_0: hooks }), encoding: "utf8",
      });
    },
    foreignEntries: () => readdirSync(foreign), indexPresent: () => existsSync(index), hookRan: () => existsSync(marker),
    stop: () => scratch.dispose(),
  };
  });
}

export async function staleGroupFixture() {
  return nativeFixture((scratch) => {
  const owner = scratch.node('console.log("ready"); setInterval(() => {}, 1000);');
  const root = scratch.childRoot();
  const current = () => registeredGroups(scratch.owner)[0];
  return { owner,
    foreignActive: () => { const id = current(); return groupAlive(id.group, id.start_time); },
    async cleanupStale() {
      const id = current();
      writeFileSync(join(root, "groups"), JSON.stringify({ group: id.group, start_time: id.start_time - 1 }) + "\n");
      await cleanup(root);
    },
    inputsPresent: () => existsSync(root),
    async stop() { await owner.stop(); await cleanup(root); await scratch.dispose(); },
  };
  });
}

export async function registrationFailureFixture() {
  return nativeFixture((scratch) => {
  const registry = scratch.path("bad-registry");
  mkdirSync(join(registry, "groups"), { recursive: true });
  const admitted = scratch.path("native-work-admitted");
  const owner = scratch.node(`require("node:fs").writeFileSync(${JSON.stringify(admitted)}, "admitted"); setInterval(() => {}, 1000);`,
    { DESKTOP_TEST_RESOURCES: registry });
  const acquisition = new Promise<Error>((done) => owner.owned.child.once("error", done));
  return { acquisition,
    active: () => groupAlive(owner.owned.group), admitted: () => existsSync(admitted), stopOwner: () => owner.stop(),
    async stop(error: Error) {
      try { await owner.stop(); } catch (failure) { if (failure !== error) throw failure; }
      if (groupAlive(owner.owned.group)) throw new Error("preserve inputs of an acquired group that has not stopped");
      // The intentionally broken registry is not the scratch's true owner registry.
      await cleanup(scratch.owner);
    },
  };
  });
}

export async function pendingAcquisitionFixture() {
  return nativeFixture((scratch) => {
  const admitted = scratch.path("native-work-admitted");
  const program = `require("node:fs").writeFileSync(${JSON.stringify(admitted)}, "admitted"); setInterval(() => {}, 1000);`;
  const owner = scratch.node(`import { writeFileSync } from "node:fs";
import { Owned, groupIdentity } from ${JSON.stringify(moduleURL("./owned.ts"))};
const child = Owned.spawn(process.execPath, ["-e", ${JSON.stringify(program)}], {
  env: process.env, stdio: ["ignore", "ignore", "ignore"]
});
// Remain in the acquisition turn: an asynchronous armed ACK cannot admit work.
writeFileSync(1, JSON.stringify(groupIdentity(child.group)) + "\\n");
process.kill(process.pid, "SIGKILL");`);
  return { owner,
    async joinAcquired() { await groupsGone([JSON.parse(owner.output.trim()) as Group]); },
    admitted: () => existsSync(admitted),
    async stop() { await owner.stop(); await groupsGone(registeredGroups(scratch.owner)); await scratch.dispose(); },
  };
  });
}

export async function execContractFixture() {
  return nativeFixture((scratch) => {
  const owner = scratch.node(`require("node:fs").writeFileSync(3, String(process.pid) + "\\n");
process.stdin.once("data", (data) => {
  process.stdout.write(data);
  process.stderr.write("actual target diagnostic\\n");
  process.stdin.pause();
  process.exitCode = 7;
});`, { PATH: scratch.root }, [], true);
  let readiness = "";
  owner.owned.child.stdio[3]!.on("data", (chunk: Buffer) => { readiness += chunk; });
  return { owner,
    execIdentityRetained: () => Number(readiness.trim()) === owner.owned.child.pid,
    groupIdentityRetained: () => owner.owned.group === owner.owned.child.pid,
    stop: () => scratch.dispose(),
  };
  });
}

export async function unavailableTargetFixture() {
  return nativeFixture((scratch) => {
  const target = scratch.path("missing-command");
  const owner = new NativeProcess("", scratch.env(), [], false, { command: target, args: [] });
  scratch.processes.push(owner);
  return { owner, namesTarget: () => owner.errors.includes(target), stop: () => scratch.dispose() };
  });
}

export async function admittedOrphanFixture(holdCleanup = false) {
  return nativeFixture((scratch) => {
  const lock = scratch.path("native-resource.lock");
  const release = scratch.path("teardown-release");
  const held = holdCleanup ? holdCleanupOutput(scratch) : undefined;
  const program = `const fs = require("node:fs");
function release() {
  fs.rmSync(${JSON.stringify(lock)}, { force: true });
  fs.writeSync(2, "orphan native resource released\\n");
  process.exit(0);
}
process.once("SIGTERM", release);
fs.writeFileSync(${JSON.stringify(lock)}, "owned native resource");
process.stdout.write("ready\\n");
// This path is admitted only by final teardown or the controlled enclosing-join case.
setInterval(() => { if (fs.existsSync(${JSON.stringify(release)})) release(); }, 10);`;
  const owner = scratch.node(`import { existsSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { resourceRoot } from ${JSON.stringify(moduleURL("./build.ts"))};
import { Owned, groupIdentity } from ${JSON.stringify(moduleURL("./owned.ts"))};
const root = resourceRoot();
const child = Owned.spawn(process.execPath, ["-e", ${JSON.stringify(program)}], {
  env: { ...process.env, DESKTOP_TEST_RESOURCES: root }, stdio: ["ignore", "pipe", "inherit"]
});
child.child.once("error", (error) => { throw error; });
child.child.once("close", () => { throw new Error("native fixture exited before owner interruption"); });
const lines = createInterface({ input: child.child.stdout });
lines.once("line", (line) => {
  if (line !== "ready") throw new Error("native fixture did not acquire its resource");
  writeFileSync(1, JSON.stringify({ ...groupIdentity(child.group), resourcesPresent: existsSync(${JSON.stringify(lock)}) }) + "\\n");
  lines.close();
});
process.stdin.once("data", () => process.kill(process.pid, "SIGKILL"));`, held?.env);
  let group: Group | undefined;
  let closing: Promise<void> | undefined;
  let stoppingOwner: Promise<void> | undefined;
  return { owner,
    async acquired() {
      const value = JSON.parse(await owner.line()) as Group & { resourcesPresent: boolean };
      group = value;
      return { resourcesPresent: value.resourcesPresent };
    },
    lockPresent: () => existsSync(lock), inputsPresent: () => existsSync(scratch.root),
    interrupt() { owner.endInput("interrupt"); },
    joinNative: () => groupsGone([group!]),
    nativeExited: () => !!group && !groupAlive(group.group, group.start_time),
    async cleanupReported() {
      if (!held) throw new Error("this scenario did not acquire a held cleanup");
      await held.reported();
    },
    beginEnclosingCleanup() {
      stoppingOwner = owner.stop();
      void stoppingOwner.catch(() => {});
      closing = cleanup(scratch.owner);
      void closing.catch(() => {});
      return closing;
    },
    release() { if (held) held.release(); else writeFileSync(release, "release declared fixture"); },
    async stop() {
      // When enclosing cleanup already completed, its controlled release was sent
      // before removal. Do not recreate a deleted scratch merely to repeat teardown.
      if (existsSync(scratch.root)) {
        writeFileSync(release, "release declared fixture");
        held?.release();
      }
      await withCleanup(() => stoppingOwner ?? owner.stop(), async () => {
        await groupsGone(registeredGroupsIfPresent(scratch.owner));
        if (closing) await closing;
      });
      await scratch.dispose();
    },
  };
  });
}

function registeredGroupsIfPresent(root: string): Group[] {
  return existsSync(join(root, "groups")) ? registeredGroups(root) : [];
}
