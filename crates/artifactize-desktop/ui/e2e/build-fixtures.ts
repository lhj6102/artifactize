// Linux fixture scenarios for candidate, runner and detached-cleanup seams. Native
// executable stand-ins, environment lookup and interruption belong here, not cases.
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { cleanup } from "./build.ts";
import { joinLease } from "./lease-receipt.ts";
import { holdCleanupOutput, moduleURL, nativeFixture, observe } from "./native-fixtures.ts";
import { withCleanup } from "./owned.ts";

function diagnostics(errors: string, kind = "Desktop E2E cleanup diagnostics for .+"): string {
  const directory = new RegExp(`${kind}: (.+)`).exec(errors)?.[1];
  if (!directory) throw new Error(`fixture did not report its diagnostics: ${errors}`);
  return directory;
}

async function report(errors: string) {
  const directory = diagnostics(errors);
  let outcome: { code: number | null; signal: string | null; disarmed: boolean } | undefined;
  await observe(() => {
    try { outcome = JSON.parse(readFileSync(join(directory, "outcome.json"), "utf8")); return true; }
    catch (error) {
      if (error instanceof SyntaxError || (error as NodeJS.ErrnoException).code === "ENOENT") return false;
      throw error;
    }
  }, () => `cleanup did not report: ${directory}`);
  return { outcome: outcome!, stdout: readFileSync(join(directory, "stdout.log"), "utf8"),
    stderr: readFileSync(join(directory, "stderr.log"), "utf8"), present: existsSync(join(directory, "outcome.json")) };
}

export async function runnerFixture(fails: boolean) {
  return nativeFixture((scratch) => {
  const marker = scratch.path("completed");
  const test = scratch.path("child.test.ts");
  writeFileSync(test, `import { it } from "node:test";
import { writeFileSync } from "node:fs";
it("actual owned work", async () => {
  await new Promise((done) => setTimeout(done, 100));
  writeFileSync(${JSON.stringify(marker)}, "work completed");
  ${fails ? 'process.stderr.write("NATIVE_NOISE_START\\n" + "n".repeat(160000) + "\\nNATIVE_NOISE_END\\n"); throw new Error("actual suite failure");' : ""}
});\n`);
  const owner = scratch.node(`import { runSuite } from ${JSON.stringify(moduleURL("./build.ts"))};
await runSuite(${JSON.stringify(scratch.owner)}, [${JSON.stringify(test)}], process.env);
console.log("suite joined");`, { ARTIFACTIZE_OUTPUT_DIR: scratch.path("results") });
  return { owner, completed: () => scratch.read("completed"),
    completeLog: () => readFileSync(join(diagnostics(owner.errors, "Full desktop E2E diagnostics"), "complete.log"), "utf8"),
    stop: () => scratch.dispose() };
  });
}

export async function handoffFixture() {
  return nativeFixture((scratch) => {
  const owner = scratch.node(`import { resourceRoot, offerCandidate } from ${JSON.stringify(moduleURL("./build.ts"))};
import { writeFileSync } from "node:fs";
import { join } from "node:path";
const root = resourceRoot();
const candidate = { root, cli: join(root, "cli"), app: join(root, "app"), manifest: join(root, "candidate.json") };
writeFileSync(candidate.cli, "immutable CLI input");
writeFileSync(candidate.app, "immutable app input");
writeFileSync(candidate.manifest, JSON.stringify(candidate));
await offerCandidate(candidate);`);
  let root: string | undefined;
  let offered: string | undefined;
  return {
    owner,
    async offer() { offered = await owner.line(); root = dirname(offered); return offered; },
    present: () => !!offered && existsSync(offered),
    inputsPresent: () => !!root && existsSync(root),
    acceptedManifest: () => JSON.parse(readFileSync(join(root!, "receiving-owner.json"), "utf8")).manifest as string,
    respond(response: "accept" | "wrong" | "EOF" | "terminate") {
      if (response === "accept") {
        writeFileSync(join(root!, "receiving-owner.json"), JSON.stringify({ manifest: offered, cleanup: "after actual owner exit" }));
        owner.endInput(`accept ${offered}\n`);
      } else if (response === "wrong") owner.endInput("accept a-different-manifest\n");
      else if (response === "EOF") owner.endInput();
      else owner.interrupt("terminate");
    },
    async stop() {
      owner.endInput();
      await owner.closed;
      await owner.stop();
      if (root) await cleanup(root);
      await scratch.dispose();
    },
  };
  });
}

export async function failedCleanupFixture() {
  return nativeFixture((scratch) => {
  const owner = scratch.node(`import { resourceRoot } from ${JSON.stringify(moduleURL("./build.ts"))};
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const root = resourceRoot();
rmSync(join(root, "tmp"), { recursive: true });
writeFileSync(join(root, "tmp"), "not a directory");
process.stdout.write(root + "\\n");
process.stdin.once("data", () => process.kill(process.pid, "SIGKILL"));`);
  let root: string | undefined;
  const repair = () => {
    if (!root || !existsSync(root)) return;
    rmSync(join(root, "tmp"), { recursive: true, force: true });
    mkdirSync(join(root, "tmp"));
  };
  return { owner,
    async ready() { root = await owner.line(); },
    interrupt() { owner.endInput("interrupt"); },
    report: () => report(owner.errors),
    inputsPresent: () => !!root && existsSync(root),
    join: () => joinLease(root!),
    async repairAndCleanup() { repair(); await cleanup(root!); },
    async stop() {
      owner.endInput("interrupt");
      await owner.closed;
      await owner.stop();
      repair();
      if (root) await cleanup(root);
      await scratch.dispose();
    },
  };
  });
}

export async function failedLeaseFixture() {
  return nativeFixture((scratch) => {
  writeFileSync(scratch.path("sh"), `#!${process.execPath}\nprocess.stdout.write("lease stdout retained\\n");\nprocess.stderr.write("lease stderr retained\\n");\nprocess.exit(23);\n`, { mode: 0o700 });
  const owner = scratch.node(`import { resourceRoot, offerCandidate } from ${JSON.stringify(moduleURL("./build.ts"))};
const root = resourceRoot();
console.log(JSON.stringify({ root }));
await offerCandidate({ root, cli: root + "/cli", app: root + "/app", manifest: root + "/candidate.json" });`, { PATH: scratch.root });
  let root: string | undefined;
  return { owner,
    async finished() { await owner.closed; root = (JSON.parse(owner.output.split("\n")[0]) as { root: string }).root; },
    report: () => report(owner.errors), inputsPresent: () => !!root && existsSync(root), join: () => joinLease(root!),
    async stop() {
      owner.endInput();
      await owner.stop();
      if (root) await cleanup(root);
      await scratch.dispose();
    },
  };
  });
}

export async function cancelJoinFixture() {
  return nativeFixture((scratch) => {
  const acknowledged = scratch.path("cancel-observed");
  const requested = scratch.path("shutdown-started");
  const ready = scratch.path("descendant-ready");
  const owner = scratch.node(`import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { resourceRoot, runSuite, cleanup } from ${JSON.stringify(moduleURL("./build.ts"))};
import { withCleanup } from ${JSON.stringify(moduleURL("./owned.ts"))};
const root = resourceRoot();
const descendant = \`const { writeFileSync, existsSync } = require("node:fs");
process.on("SIGTERM", () => { writeFileSync(${JSON.stringify(requested)}, "joining"); process.kill(\${process.pid}, "SIGTERM"); });
writeFileSync(${JSON.stringify(ready)}, "ready");
setInterval(() => { if (existsSync(${JSON.stringify(acknowledged)})) process.exit(0); }, 5);\`;
const test = join(root, "child.test.ts");
writeFileSync(test, \`import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { it } from "node:test";
it("successful test with an owned descendant", async () => {
  const child = spawn(process.execPath, ["-e", \${JSON.stringify(descendant)}], { stdio: "ignore" });
  child.unref();
  while (!existsSync(${JSON.stringify(ready)})) await new Promise((done) => setTimeout(done, 5));
});\`);
process.once("SIGTERM", () => writeFileSync(${JSON.stringify(acknowledged)}, "cancelled"));
await withCleanup(() => runSuite(root, [test], process.env), () => cleanup(root));
console.log("suite accepted");`);
  return { owner, cancellationObserved: () => existsSync(requested) && existsSync(acknowledged), stop: () => scratch.dispose() };
  });
}

export async function nestedCleanupFixture() {
  return nativeFixture((scratch) => {
  const held = holdCleanupOutput(scratch);
  const root = scratch.childRoot();
  const env = scratch.env({ ...held.env, TMPDIR: join(root, "tmp"), DESKTOP_TEST_RESOURCES: root, ARTIFACTIZE_OUTPUT_DIR: undefined });
  const owner = scratch.node(`import { resourceRoot } from ${JSON.stringify(moduleURL("./build.ts"))};
const root = resourceRoot();
process.stdout.write(root + "\\n", () => process.kill(process.pid, "SIGKILL"));`, env);
  let closing: Promise<void> | undefined;
  return { owner,
    reported: held.reported,
    nestedInputsPresent: () => existsSync(owner.output.trim()),
    diagnosticsOutside: () => relative(root, diagnostics(owner.errors)).startsWith(".."),
    inputsPresent: () => existsSync(root),
    beginCleanup() { closing = cleanup(root); void closing.catch(() => {}); return closing; },
    release: held.release,
    report: () => report(owner.errors),
    async stop() {
      await withCleanup(held.release, () => owner.stop(), () => closing ?? cleanup(root));
      await scratch.dispose();
    },
  };
  });
}
