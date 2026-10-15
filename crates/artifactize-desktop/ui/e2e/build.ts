// Build inputs and executable candidates belong to one suite, not Cargo's shared binary names.
// This Linux WebKitGTK native helper uses the same POSIX group/lease boundary as Owned.
import { appendFileSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { type ChildProcess, spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { acknowledge } from "./handoff.ts";
import { groupAlive, Owned } from "./owned.ts";
import { removeFixture } from "./runs.ts";

export const workspace = resolve(import.meta.dirname, "../../../..");
const ui = resolve(import.meta.dirname, "..");
/** A cold GTK/Rust build can take minutes on CI; this bounds work, not a UI transition. */
const BUILD_TIMEOUT = 15 * 60 * 1000;
/** Watchdogs get time to end nested driver groups before their executable files are removed. */
const CLEANUP_TIMEOUT = 20000;

export type Candidate = { root: string; cli: string; app: string; manifest: string };
const leases = new Map<string, { child: ChildProcess; closed: Promise<void> }>();

/** A pipe, unlike an exit hook, closes on SIGKILL too. Child watchdogs stop their groups;
 * this lease then settles durable Runs and removes the candidate after all groups are gone. */
function lease(root: string) {
  const child = spawn("sh", ["-c", 'read _; "$0" "$1" cleanup "$2"', process.execPath,
    fileURLToPath(new URL("./build.ts", import.meta.url)), root], { detached: true, stdio: ["pipe", "ignore", "ignore"] });
  const closed = new Promise<void>((done, fail) => {
    child.once("error", fail);
    child.once("close", () => done());
  });
  // Install rejection observation immediately, while the creator retains the actual
  // failure for the explicit release boundary below.
  void closed.catch((error: unknown) => { console.error(`candidate lease failed for ${root}`, error); process.exitCode = 1; });
  child.unref();
  (child.stdin as unknown as { unref?: () => void } | null)?.unref?.();
  leases.set(root, { child, closed });
}

/** Disarm only after cleanup completes or the receiving owner actually accepts responsibility. */
async function disarm(root: string): Promise<void> {
  const lease = leases.get(root);
  if (!lease) return;
  lease.child.ref();
  lease.child.kill("SIGKILL");
  await lease.closed;
  lease.child.stdin?.destroy();
  leases.delete(root);
}

/** Offer a candidate while retaining its fallback until the builder explicitly accepts it. */
export async function offerCandidate(candidate: Candidate): Promise<void> {
  const controller = new AbortController();
  const interrupted = () => controller.abort(new Error("candidate handoff was cancelled"));
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.on(signal, interrupted);
  try {
    await acknowledge(candidate.manifest, process.stdin, process.stdout, controller.signal);
    controller.signal.throwIfAborted();
    await disarm(candidate.root);
    controller.signal.throwIfAborted();
  } catch (error) {
    try { await cleanup(candidate.root); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "candidate handoff and cleanup failed"); }
    throw error;
  } finally {
    for (const signal of ["SIGINT", "SIGTERM"] as const) process.off(signal, interrupted);
  }
}

export function resourceRoot(): string {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "artifactize-desktop-build-")));
  mkdirSync(join(root, "tmp"));
  writeFileSync(join(root, "groups"), "");
  lease(root);
  return root;
}

/** A suite ends with its test runner or its owner's cancellation, not a build watchdog. */
export async function runSuite(root: string, tests: string[], env: NodeJS.ProcessEnv): Promise<void> {
  // The runtime's declared output survives candidate cleanup. Standalone invocations
  // get a distinct result directory too, never a log below the soon-removed build.
  const output = env.ARTIFACTIZE_OUTPUT_DIR ?? tmpdir();
  mkdirSync(output, { recursive: true });
  const diagnostics = mkdtempSync(join(output, "desktop-e2e-results-"));
  console.error(`Full desktop E2E diagnostics: ${diagnostics}`);
  await execute(root, process.execPath, ["--test", "--test-concurrency=1",
    "--test-reporter=spec", `--test-reporter-destination=${join(diagnostics, "complete.log")}`,
    `--test-reporter=${fileURLToPath(new URL("./concise.ts", import.meta.url))}`, "--test-reporter-destination=stdout", ...tests], env, "suite", false, diagnostics);
}

/** Each build operation owns its watchdog; stdout can be Cargo's artifact stream. */
async function command(root: string, program: string, args: string[], env: NodeJS.ProcessEnv, capture = false): Promise<string> {
  return execute(root, program, args, env, "build", capture);
}

/** Both lifetimes retain the same group ownership, signal handling and shutdown join. */
async function execute(root: string, program: string, args: string[], env: NodeJS.ProcessEnv,
  lifetime: "build" | "suite", capture = false, diagnostics?: string): Promise<string> {
  const owned = Owned.spawn(program, args, {
    cwd: workspace, env: { ...env, TMPDIR: join(root, "tmp"), DESKTOP_TEST_RESOURCES: root },
    stdio: ["ignore", capture || diagnostics ? "pipe" : 2, diagnostics ? "pipe" : "inherit"],
  });
  let output = "";
  owned.child.stdout?.on("data", (chunk: Buffer) => {
    if (capture) output += chunk;
    if (diagnostics) { appendFileSync(join(diagnostics, "runner-stdout.log"), chunk); process.stderr.write(chunk); }
  });
  owned.child.stderr?.on("data", (chunk: Buffer) => {
    if (diagnostics) { appendFileSync(join(diagnostics, "runner-stderr.log"), chunk); process.stderr.write(chunk); }
  });
  let timedOut = false;
  let cancelled = false;
  const failures: unknown[] = [];
  const stop = () => { cancelled = true; void owned.stop().catch(() => {}); };
  const timeout = lifetime === "build" ? setTimeout(() => { timedOut = true; stop(); }, BUILD_TIMEOUT) : undefined;
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.on(signal, stop);
  try {
    const result = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
      owned.child.once("error", fail);
      owned.child.once("close", (code, signal) => done({ code, signal }));
    });
    if (cancelled || result.code !== 0) throw new Error(`${program} ${args.join(" ")} ${timedOut ? "timed out" : cancelled ? "was cancelled" : `exited ${result.code ?? result.signal}`}`);
  } catch (error) { failures.push(error); }
  clearTimeout(timeout);
  try { await owned.stop(); } catch (error) { failures.push(error); }
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.off(signal, stop);
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AggregateError(failures, "owned execution and cleanup failed");
  return output;
}

/** No task writes ui/dist: the exact frontend input is declared to Tauri for this invocation. */
export async function frontend(root: string): Promise<NodeJS.ProcessEnv> {
  const dist = join(root, "frontend");
  await command(root, "pnpm", ["-C", ui, "build", "--outDir", dist], process.env);
  return { ...process.env, TAURI_CONFIG: JSON.stringify({ build: { frontendDist: dist } }) };
}

function save(root: string, cli: string, app: string): Candidate {
  const candidate = { root, cli, app, manifest: join(root, "candidate.json") };
  writeFileSync(candidate.manifest, JSON.stringify(candidate));
  return candidate;
}

/** Select executable outputs Cargo actually emitted; no target/debug names or mtime inference. */
export async function prepare(): Promise<Candidate> {
  const root = resourceRoot();
  try {
    const env = await frontend(root);
    const output = await command(root, "cargo", ["build", "--locked", "-p", "artifactize", "-p", "artifactize-desktop",
      "--target-dir", join(root, "target"), "--message-format=json-render-diagnostics"], {
      ...env, CARGO_PROFILE_DEV_DEBUG: "0", CARGO_PROFILE_DEV_INCREMENTAL: "false",
    }, true);
    const executables = new Map<string, string>();
    for (const line of output.split("\n").filter(Boolean)) {
      const item = JSON.parse(line) as { reason?: string; target?: { name?: string; kind?: string[] }; executable?: string };
      if (item.reason === "compiler-artifact" && item.executable && item.target?.kind?.includes("bin") && item.target.name) {
        executables.set(item.target.name, item.executable);
      }
    }
    const cli = executables.get("artifactize");
    const app = executables.get("artifactize-desktop");
    if (!cli || !app) throw new Error("Cargo did not emit the CLI and desktop executable candidates");
    return save(root, cli, app);
  } catch (error) {
    try { await cleanup(root); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "candidate build and cleanup failed"); }
    throw error;
  }
}

/** Explicit prepared input only; an Agent session must never find an arbitrary prior build. */
export function readCandidate(manifest = process.env.DESKTOP_TEST_CANDIDATE): Candidate {
  if (!manifest) throw new Error("DESKTOP_TEST_CANDIDATE is not set; run the owned e2e runner or prepare a candidate before the review");
  const value: unknown = JSON.parse(readFileSync(manifest, "utf8"));
  if (!value || typeof value !== "object") throw new Error("invalid desktop candidate manifest");
  const item = value as Partial<Candidate>;
  if (typeof item.root !== "string" || typeof item.cli !== "string" || typeof item.app !== "string" || !isAbsolute(item.root)) {
    throw new Error("invalid desktop candidate paths");
  }
  for (const path of [item.cli, item.app, manifest]) {
    const within = relative(item.root, path);
    if (!isAbsolute(path) || within.startsWith("..") || isAbsolute(within) || !existsSync(path)) throw new Error(`candidate input is missing or outside its root: ${path}`);
  }
  return { root: item.root, cli: item.cli, app: item.app, manifest };
}

/** A tool session owns its executable snapshot until every child and durable Run has stopped. */
export async function snapshotCandidate(directory: string): Promise<Candidate> {
  const source = readCandidate();
  const root = realpathSync(mkdtempSync(join(directory, "candidate-")));
  mkdirSync(join(root, "tmp"));
  writeFileSync(join(root, "groups"), "");
  lease(root);
  try {
    const cli = join(root, "artifactize");
    const app = join(root, "desktop");
    copyFileSync(source.cli, cli);
    copyFileSync(source.app, app);
    const candidate = save(root, cli, app);
    process.env.DESKTOP_TEST_CANDIDATE = candidate.manifest;
    process.env.DESKTOP_TEST_RESOURCES = root;
    process.env.TMPDIR = join(root, "tmp");
    return candidate;
  } catch (error) {
    try { await cleanup(root); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "candidate snapshot and cleanup failed"); }
    throw error;
  }
}

/** Remove owned inputs only after all registered build/test/driver groups have stopped. */
export async function cleanup(root: string): Promise<void> {
  const deadline = Date.now() + CLEANUP_TIMEOUT;
  while (existsSync(join(root, "groups"))) {
    const alive = readFileSync(join(root, "groups"), "utf8").trim().split("\n").filter(Boolean).filter((group) => groupAlive(Number(group)));
    if (alive.length === 0) break;
    if (Date.now() >= deadline) throw new Error(`cannot remove desktop inputs while owned groups run: ${alive.join(", ")}`);
    await new Promise((done) => setTimeout(done, 50));
  }
  await childrenGone(join(root, "tmp"), "artifactize-desktop-build-");
  // Node workers may have been SIGKILLed and skipped their fixture exit hook. Settle
  // durable Runs before removing the executable their independently owned workers use.
  const fixtures = join(root, "fixtures");
  if (existsSync(join(root, "candidate.json")) && existsSync(fixtures)) {
    const candidate = readCandidate(join(root, "candidate.json"));
    const failures: unknown[] = [];
    for (const name of readdirSync(fixtures)) {
      const fixture = join(fixtures, name);
      try {
        // A failed snapshot join forbids removing this fixture, but must not skip
        // settlement of other independently owned fixtures.
        await snapshotsGone(join(fixture, "session"));
        removeFixture(candidate.cli, fixture);
      } catch (error) { failures.push(error); }
    }
    if (failures.length > 0) throw new AggregateError(failures, "candidate fixture cleanup failed; preserve inputs");
  }
  // A local fallback is still a user of these inputs until its real close. If this
  // cleanup belongs to the detached fallback itself, there is no local lease to join.
  await disarm(root);
  rmSync(root, { recursive: true, force: true });
}

async function childrenGone(directory: string, prefix: string): Promise<void> {
  const deadline = Date.now() + CLEANUP_TIMEOUT;
  while (existsSync(directory) && readdirSync(directory).some((name) => name.startsWith(prefix))) {
    if (Date.now() >= deadline) throw new Error(`owned child inputs did not finish cleanup: ${directory}`);
    await new Promise((done) => setTimeout(done, 50));
  }
}

/** A killed harness can finish its lease cleanup after its leader exits. */
export async function snapshotsGone(directory: string): Promise<void> {
  await childrenGone(directory, "candidate-");
}

/** Native runtime gates own their frontend too; Cargo's ordinary cache is not an executable input. */
export async function cargoGate(mode: "clippy" | "test"): Promise<void> {
  const root = resourceRoot();
  try {
    const env = await frontend(root);
    const args = mode === "clippy" ? ["clippy", "--locked", "-p", "artifactize-desktop", "--all-targets", "--", "-D", "warnings"] : ["test", "--locked", "-p", "artifactize-desktop"];
    await command(root, "cargo", args, env);
  } catch (error) {
    try { await cleanup(root); }
    catch (shutdown) { throw new AggregateError([error, shutdown], "native Cargo gate and cleanup failed"); }
    throw error;
  }
  await cleanup(root);
}

// The owner's pipe watchdog uses the same cleanup boundary after an uncatchable SIGKILL.
if (import.meta.main && process.argv[2] === "cleanup") await cleanup(process.argv[3]);
