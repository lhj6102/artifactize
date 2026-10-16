// Cargo's multi-package dry run stages a local registry under target/package. Reusing
// that registry path at the same development version can reuse yesterday's extracted
// dependencies. Own both output directories: Cargo stages packages under build-dir.
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";

let child;
let interrupted;
const forward = (signal) => {
  interrupted ??= signal;
  child?.kill(signal);
};
const signals = ["SIGINT", "SIGTERM"];
const handlers = signals.map((signal) => {
  const handler = () => forward(signal);
  process.on(signal, handler);
  return handler;
});

function cargo(args, capture = false) {
  if (interrupted) return Promise.resolve({ code: null, signal: interrupted });
  return new Promise((resolve, reject) => {
    child = spawn("cargo", args, { stdio: ["inherit", capture ? "pipe" : "inherit", "inherit"] });
    let stdout = "";
    child.stdout?.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
    child.once("error", reject);
    child.once("close", (code, signal) => {
      child = undefined;
      resolve({ code, signal, stdout });
    });
  });
}

async function verify() {
  const metadata = await cargo(["metadata", "--no-deps", "--locked", "--format-version", "1"], true);
  if (metadata.code !== 0) return metadata;
  const { target_directory: target } = JSON.parse(metadata.stdout);
  await mkdir(target, { recursive: true });
  const staging = await mkdtemp(join(target, "publish-"));
  try {
    return await cargo([
      "publish", "--dry-run", "--locked", "--allow-dirty",
      "--target-dir", staging,
      "--config", `build.build-dir=${JSON.stringify(staging)}`,
      "-p", "artifactize-core", "-p", "artifactize-tools", "-p", "artifactize-tui", "-p", "artifactize",
    ]);
  } finally {
    // close (not exit) means Cargo and its inherited output streams have finished.
    await rm(staging, { recursive: true, force: true });
  }
}

try {
  const result = await verify();
  interrupted ??= result.signal;
  process.exitCode = result.code ?? 1;
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  signals.forEach((signal, index) => process.removeListener(signal, handlers[index]));
}
if (interrupted) process.kill(process.pid, interrupted);
