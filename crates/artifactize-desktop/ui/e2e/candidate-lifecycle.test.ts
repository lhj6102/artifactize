// A session-private candidate must outlive all its children, but not an interrupted owner.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { describe, it } from "node:test";
import { createGraph } from "./fixture.ts";
import { checkBuild, LAUNCH_TIMEOUT } from "./driver.ts";
import { groupAlive, Owned } from "./owned.ts";
import { snapshotsGone } from "./build.ts";
import { running, startedWorkers, states } from "./runs.ts";

async function eventually(check: () => boolean, what: string) {
  // Includes the driver watchdog grace and the resource lease's durable-Run cleanup.
  await new Promise<void>((done, fail) => {
    const deadline = setTimeout(() => { clearInterval(poll); fail(new Error(`timed out: ${what}`)); }, 30000);
    const inspect = () => { if (check()) { clearTimeout(deadline); clearInterval(poll); done(); } };
    const poll = setInterval(inspect, 50);
    inspect();
  });
}

describe("an interrupted candidate owner", { skip: process.platform !== "linux" && "the Linux harness owns process groups and Xvfb" }, () => {
  for (const signal of ["SIGTERM", "SIGKILL"] as const) {
    it(`cleans its executable snapshot only after children stop on ${signal}`, async () => {
      checkBuild();
      const directory = join(createGraph().root, "session");
      mkdirSync(directory);
      const owner = Owned.spawn(process.execPath, [resolve(import.meta.dirname, "../../harness/harness.mts"), "start"], {
        env: { ...process.env, ARTIFACTIZE_SESSION_DIR: directory, ARTIFACTIZE_REMOTE: "off" }, stdio: ["ignore", "pipe", "inherit"],
      });
      let candidate: string | undefined;
      try {
        await new Promise<void>((done, fail) => {
          const lines = createInterface({ input: owner.child.stdout! });
          const deadline = setTimeout(() => finish(new Error("candidate owner did not become ready")), LAUNCH_TIMEOUT);
          const exit = (code: number | null) => finish(new Error(`candidate owner exited ${code}`));
          function finish(error?: Error) {
            clearTimeout(deadline);
            lines.close();
            owner.child.off("exit", exit);
            if (error) fail(error); else done();
          }
          lines.once("line", (line) => line === "ready" ? finish() : finish(new Error(`unexpected readiness: ${line}`)));
          owner.child.once("exit", exit);
        });
        const folders = readdirSync(directory).filter((name) => name.startsWith("candidate-"));
        assert.equal(folders.length, 1, "the session owns exactly one executable snapshot");
        candidate = join(directory, folders[0]);
        const manifest = JSON.parse(readFileSync(join(candidate, "candidate.json"), "utf8")) as { cli: string; app: string };
        assert.ok(existsSync(manifest.cli) && existsSync(manifest.app), "the session's executable inputs exist while its children run");
        const tool = (mode: string, args: object) => {
          const result = execFileSync(process.execPath, [resolve(import.meta.dirname, "../../harness/harness.mts"), mode], {
            env: { ...process.env, ARTIFACTIZE_SESSION_DIR: directory }, input: JSON.stringify({ args }),
            encoding: "utf8", timeout: LAUNCH_TIMEOUT,
          });
          const reply = JSON.parse(result) as { isError?: boolean };
          assert.equal(reply.isError ?? false, false, result);
        };
        tool("launch", { screen: "graph" });
        tool("cli", { args: ["verify", "--all", "--json"] });
        const fixtures = join(candidate, "fixtures");
        const exposed = readdirSync(fixtures).flatMap((name) => states(join(fixtures, name)));
        assert.equal(exposed.length, 2, "interruption owns Monitor state and graph-state");
        const owners = await startedWorkers(exposed);
        assert.ok(owners.some(running), "actual durable workers exist before interruption");
        const groups = readFileSync(join(candidate, "groups"), "utf8").trim().split("\n").map(Number).filter((group) => groupAlive(group));
        assert.ok(groups.length > 0, "the session registers its owned OS process groups");
        owner.child.kill(signal);
        await eventually(() => groups.every((group) => !groupAlive(group)), "the session's children stop");
        await eventually(() => {
          if (existsSync(candidate!)) return false;
          assert.ok(owners.every((identity) => !running(identity)), "candidate removal follows every durable worker's actual exit");
          return true;
        }, "the session's snapshot and durable fixture state are removed");
      } finally {
        await owner.stop();
        await snapshotsGone(directory);
      }
    });
  }
});
