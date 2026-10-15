// Build inputs and executable candidates belong to one suite, not Cargo's shared binary names.
// This Linux WebKitGTK native helper uses the same POSIX group/lease boundary as Owned.
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { type ChildProcess, spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { groupAlive, Owned } from "./owned.ts";
import { removeFixture } from "./runs.ts";

export const workspace = resolve(import.meta.dirname, "../../../..");
const ui = resolve(import.meta.dirname, "..");
/** A cold GTK/Rust build can take minutes on CI; this bounds work, not a UI transition. */
const BUILD_TIMEOUT = 15 * 60 * 1000;
/** Watchdogs get time to end nested driver groups before their executable files are removed. */
const CLEANUP_TIMEOUT = 20000;

export type Candidate = { root: string; cli: string; app: string; manifest: string };
const leases = new Map<string, ChildProcess>();

/** A pipe, unlike an exit hook, closes on SIGKILL too. Child watchdogs stop their groups;
 * this lease then settles durable Runs and removes the candidate after all groups are gone. */
function lease(root: string) {
  const child = spawn("sh", ["-c", 'read _; "$0" "$1" cleanup "$2"', process.execPath,
    fileURLToPath(new URL("./build.ts", import.meta.url)), root], { detached: true, stdio: ["pipe", "ignore", "ignore"] });
  child.unref();
  (child.stdin as unknown as { unref?: () => void } | null)?.unref?.();
  leases.set(root, child);
}

/** Transfer a prepared candidate's lifetime explicitly to the verification builder. */
export function retain(root: string) {
  leases.get(root)?.kill("SIGKILL");
  leases.delete(root);
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
  await execute(root, process.execPath, ["--test", "--test-concurrency=1", ...tests], env, "suite");
}

/** Each build operation owns its watchdog; stdout can be Cargo's artifact stream. */
async function command(root: string, program: string, args: string[], env: NodeJS.ProcessEnv, capture = false): Promise<string> {
  return execute(root, program, args, env, "build", capture);
}

/** Both lifetimes retain the same group ownership, signal handling and shutdown join. */
async function execute(root: string, program: string, args: string[], env: NodeJS.ProcessEnv,
  lifetime: "build" | "suite", capture = false): Promise<string> {
  const owned = Owned.spawn(program, args, {
    cwd: workspace, env: { ...env, TMPDIR: join(root, "tmp"), DESKTOP_TEST_RESOURCES: root },
    stdio: ["ignore", capture ? "pipe" : 2, "inherit"],
  });
  let output = "";
  owned.child.stdout?.on("data", (chunk) => { output += chunk; });
  let timedOut = false;
  const stop = () => { void owned.stop().catch((error: unknown) => console.error(error)); };
  const timeout = lifetime === "build" ? setTimeout(() => { timedOut = true; stop(); }, BUILD_TIMEOUT) : undefined;
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.once(signal, stop);
  try {
    const result = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((done, fail) => {
      owned.child.once("error", fail);
      owned.child.once("close", (code, signal) => done({ code, signal }));
    });
    if (timedOut || result.code !== 0) throw new Error(`${program} ${args.join(" ")} ${timedOut ? "timed out" : `exited ${result.code ?? result.signal}`}`);
    return output;
  } finally {
    clearTimeout(timeout);
    for (const signal of ["SIGINT", "SIGTERM"] as const) process.off(signal, stop);
    await owned.stop();
  }
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
    await cleanup(root);
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
export function snapshotCandidate(directory: string): Candidate {
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
    rmSync(root, { recursive: true, force: true });
    retain(root);
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
    for (const name of readdirSync(fixtures)) {
      const fixture = join(fixtures, name);
      // A harness nested below this fixture has its own lease and durable Runs.
      await snapshotsGone(join(fixture, "session"));
      removeFixture(candidate.cli, fixture);
    }
  }
  rmSync(root, { recursive: true, force: true });
  retain(root);
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
  } finally { await cleanup(root); }
}

// The owner's pipe watchdog uses the same cleanup boundary after an uncatchable SIGKILL.
if (import.meta.main && process.argv[2] === "cleanup") await cleanup(process.argv[3]);
