// Cargo stages a local registry under build-dir/package. Reusing that path at
// the same development version can reuse yesterday's extracted dependencies.
import { spawn } from "node:child_process";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { join } from "node:path";

// This private entry point needs a caller that owns tree termination and temporary
// space. It is not a standalone process supervisor or a user-configurable command.
const owner = process.env.ARTIFACTIZE_TMP_DIR ?? process.env.RUNNER_TEMP;
if (!owner) {
  console.error("publish.mjs requires Artifactize runtime or CI-owned temporary space (ARTIFACTIZE_TMP_DIR or RUNNER_TEMP)");
  process.exit(1);
}
const staging = await mkdtemp(join(await realpath(owner), "artifactize-publish-"));
let status;
try {
  status = await new Promise((resolve, reject) => {
    // Inherit the invoking runtime's process group/job and private TMP directory.
    // Its shared platform layer owns cancellation, tree reaping and scratch cleanup;
    // a Windows hard termination cannot run a JavaScript signal handler/finally.
    const child = spawn("cargo", [
      "publish", "--dry-run", "--locked", "--allow-dirty",
      "--target-dir", staging,
      "--config", `build.build-dir=${JSON.stringify(staging)}`,
      "-p", "artifactize-core", "-p", "artifactize-tools", "-p", "artifactize-tui", "-p", "artifactize-mcp", "-p", "artifactize",
    ], { stdio: "inherit" });
    child.once("error", reject);
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
  process.exitCode = status.code ?? 1;
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  // Ordinary Cargo exits have finished their builds. Abnormal termination leaves
  // cleanup to the invoking runtime/CI runner after its process tree is reaped.
  if (!status?.signal) await rm(staging, { recursive: true, force: true });
}
if (status?.signal) process.kill(process.pid, status.signal);
