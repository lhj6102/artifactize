// Linux Agent-session fixture boundary. Requires the declared Node harness, procfs,
// POSIX groups and the native driver capabilities in driver.ts. Tests use portable
// calls and lifecycle observations; executable lookup, environment, signals and
// candidate/state paths stay here.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { snapshotsGone } from "./build.ts";
import { checkBuild, LAUNCH_TIMEOUT } from "./driver.ts";
import { cli, createGraph, environment } from "./fixture.ts";
import { groupAlive, Owned, registeredGroups, registerGroup, withCleanup } from "./owned.ts";
import { removeFixture, running, startedWorkers, states } from "./runs.ts";

/** Includes the detached lease's native-group grace and durable-worker join. */
const RELEASE_TIMEOUT = 60000;
type Operation = "snapshot" | "launch" | "cli" | "change-declaration" | "act";
export type AgentWork = {
  scopes: string[];
  inputsPresent: boolean;
  workersActive: boolean;
  nativeActive: boolean;
  workersExited: () => boolean;
  nativeExited: () => boolean;
  inputsRemoved: () => boolean;
  waitReleased: () => Promise<void>;
};
export type AgentSession = {
  call: (operation: Operation, args?: Record<string, unknown>) => string;
  observeWork: () => Promise<AgentWork>;
  interrupt: (kind: "terminate" | "kill") => void;
  stop: () => Promise<void>;
};

async function released(check: () => boolean): Promise<void> {
  const deadline = Date.now() + RELEASE_TIMEOUT;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("Agent session inputs or actual native/durable users did not finish release");
    await new Promise((done) => setTimeout(done, 50));
  }
}

/** Own setup from its first launch request, including failed readiness. A successful
 * return transfers this one private session to the caller's stop boundary. */
export async function startAgent(): Promise<AgentSession> {
  checkBuild();
  if (process.platform !== "linux") throw new Error("Agent session fixture ownership requires Linux procfs and POSIX groups");
  const fixture = createGraph();
  const directory = join(fixture.root, "session");
  mkdirSync(directory);
  const harness = resolve(import.meta.dirname, "../../harness/harness.mts");
  const env = { ...environment(fixture), ARTIFACTIZE_SESSION_DIR: directory };
  const owner = Owned.spawn(process.execPath, [harness, "start"], { env, stdio: ["ignore", "pipe", "inherit"] });
  // Preserve and drain the harness's complete control/diagnostic output after readiness too.
  owner.child.stdout!.on("data", (chunk: Buffer) => process.stderr.write(chunk));
  let ready = false;
  let interrupted = false;
  let ending: Promise<void> | undefined;
  const invoke = (mode: Operation | "stop", args: Record<string, unknown> = {}) => execFileSync(process.execPath, [harness, mode], {
    env, input: JSON.stringify({ args }), encoding: "utf8", timeout: LAUNCH_TIMEOUT,
  });
  const stop = () => ending ??= withCleanup(() => {
    if (ready && !interrupted) invoke("stop");
  }, () => owner.stop(), () => snapshotsGone(directory), () => removeFixture(cli(), fixture.root));
  try {
    if (owner.group) registerGroup(fixture.root, owner.group);
    await new Promise<void>((done, fail) => {
      const lines = createInterface({ input: owner.child.stdout! });
      const deadline = setTimeout(() => finish(new Error("Agent session did not become ready")), LAUNCH_TIMEOUT);
      const exited = (code: number | null) => finish(new Error(`Agent session exited ${code} before ready`));
      const failed = (error: Error) => finish(error);
      function finish(error?: Error) {
        clearTimeout(deadline);
        lines.close();
        owner.child.off("exit", exited);
        owner.child.off("error", failed);
        if (error) fail(error); else done();
      }
      lines.once("line", (line) => line === "ready" ? finish() : finish(new Error(`unexpected Agent readiness: ${line}`)));
      owner.child.once("exit", exited);
      owner.child.once("error", failed);
    });
    ready = true;
    return {
      call(operation, args = {}) {
        if (ending || interrupted) throw new Error("the owned Agent session is stopping");
        const output = invoke(operation, args);
        const reply = JSON.parse(output) as { isError?: boolean; content: { text: string }[] };
        if (reply.isError) throw new Error(`Agent ${operation} failed: ${reply.content.map((part) => part.text).join("\n")}`);
        return reply.content.map((part) => part.text).join("\n");
      },
      async observeWork() {
        const candidates = readdirSync(directory).filter((name) => name.startsWith("candidate-"));
        if (candidates.length !== 1) throw new Error("Agent session has no unambiguous owned executable snapshot");
        const candidate = join(directory, candidates[0]);
        const manifest = JSON.parse(readFileSync(join(candidate, "candidate.json"), "utf8")) as { cli: string; app: string };
        const fixtures = join(candidate, "fixtures");
        const roots = readdirSync(fixtures).map((name) => join(fixtures, name));
        const exposed = roots.flatMap(states);
        const workers = await startedWorkers(exposed);
        const groups = registeredGroups(candidate).filter((group) => groupAlive(group.group, group.start_time));
        const workersExited = () => workers.every((worker) => !running(worker));
        const nativeExited = () => groups.every((group) => !groupAlive(group.group, group.start_time));
        const inputsRemoved = () => !existsSync(candidate);
        return {
          scopes: exposed.map((state) => basename(state)).sort(),
          inputsPresent: existsSync(manifest.cli) && existsSync(manifest.app),
          workersActive: workers.some(running), nativeActive: groups.length > 0,
          workersExited, nativeExited, inputsRemoved,
          waitReleased: () => released(() => {
            if (!inputsRemoved()) return false;
            if (!nativeExited() || !workersExited()) throw new Error("Agent inputs were removed before their actual users exited");
            return true;
          }),
        };
      },
      interrupt(kind) {
        if (ending || interrupted) throw new Error("the owned Agent session is stopping");
        interrupted = true;
        if (!owner.child.kill(kind === "terminate" ? "SIGTERM" : "SIGKILL")) throw new Error("Agent interruption could not be delivered to its owner");
      },
      stop,
    };
  } catch (error) {
    return withCleanup(() => { throw error; }, stop);
  }
}
