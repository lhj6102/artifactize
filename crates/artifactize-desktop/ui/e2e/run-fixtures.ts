// Linux durable-worker fixture scenarios. The tests observe saved/actual lifetime
// distinctions without constructing native programs, identities, paths or signals.
import { existsSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup } from "./build.ts";
import { nativeFixture } from "./native-fixtures.ts";
import { groupAlive, registerGroup } from "./owned.ts";
import { exited, finishFixture, removeFixture, running, workers, type Identity } from "./runs.ts";

export async function workerExitFixture() {
  return nativeFixture((scratch) => {
  const child = scratch.node('process.stdin.resume(); process.stdin.on("end", () => process.exit(0)); console.log("ready");', {},
    ["--", "--state-dir", scratch.root, "__execute", "run-exit-probe"]);
  let observed: Identity[] = [];
  return { child,
    discover() { observed = workers(scratch.root); return observed.length; },
    matchesWorker: () => observed[0].pid === child.owned.child.pid,
    active: () => running(observed[0]),
    anotherBirthActive: () => running({ ...observed[0], start_time: observed[0].start_time + 1 }),
    requireExited() { exited(observed, 0); },
    release() { child.endInput(); },
    stop: () => scratch.dispose(),
  };
  });
}

export async function fixtureLifecycle() {
  return nativeFixture((scratch) => {
  scratch.write("input", "owned input");
  return {
    inputsPresent: () => scratch.exists("input"), rootPresent: () => scratch.exists(),
    finish: (stop: () => Promise<void>) => finishFixture(process.execPath, scratch.root, stop),
    stop: () => scratch.dispose(),
  };
  });
}

export async function fixtureGroup() {
  return nativeFixture((scratch) => {
  const owner = scratch.node('process.stdin.resume(); console.log("ready");');
  return { owner,
    register() { scratch.write("input", "owned profile"); registerGroup(scratch.root, owner.owned.group); },
    anotherBirthActive: () => groupAlive(owner.owned.group, -1),
    remove: () => removeFixture(process.execPath, scratch.root),
    inputsPresent: () => scratch.exists("input"), rootPresent: () => scratch.exists(),
    stop: () => scratch.dispose(),
  };
  });
}

export async function fixtureSnapshot() {
  return nativeFixture((scratch) => {
  const snapshot = scratch.path("session/candidate-owned");
  mkdirSync(snapshot, { recursive: true });
  return { remove: () => removeFixture(process.execPath, scratch.root),
    snapshotPresent: () => existsSync(snapshot), stop: () => scratch.dispose() };
  });
}

export async function failedRunCleanupFixture() {
  return nativeFixture((scratch) => {
  const root = scratch.root;
  mkdirSync(join(root, "tmp"));
  writeFileSync(join(root, "groups"), "");
  const cli = scratch.path("cli");
  // A declared executable refuses the CLI arguments. It does not fabricate saved
  // terminal JSON, and is separate from the true scratch owner/registry/receipt.
  symlinkSync(process.execPath, cli);
  writeFileSync(join(root, "candidate.json"), JSON.stringify({ root, cli, app: cli }));
  const fixture = scratch.path("fixtures/fixture-failure");
  const state = join(fixture, "graph-state");
  mkdirSync(state, { recursive: true });
  mkdirSync(join(fixture, "repo"));
  writeFileSync(join(state, "state.sqlite"), "owned state");
  return {
    cleanup: () => cleanup(root),
    finish: (stop: () => Promise<void>) => finishFixture(cli, fixture, stop),
    statePresent: () => existsSync(join(state, "state.sqlite")), executablePresent: () => existsSync(cli),
    stop: () => scratch.dispose(),
  };
  });
}
