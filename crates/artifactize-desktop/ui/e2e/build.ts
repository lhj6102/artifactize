// Immutable executable inputs. App/driver lifetimes belong exclusively to WDIO's Tauri service.
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { executableSuffix, packageCommand } from "./platform.ts";

export const workspace = resolve(import.meta.dirname, "../../../..");
const ui = resolve(import.meta.dirname, "..");
export type Candidate = { root: string; cli: string; app: string; observer: string; manifest: string };

function resourceRoot(directory = process.env.ARTIFACTIZE_TMP_DIR ?? tmpdir()): string {
  mkdirSync(directory, { recursive: true });
  const root = realpathSync(mkdtempSync(join(directory, "artifactize-desktop-build-")));
  mkdirSync(join(root, "tmp"));
  return root;
}

async function command(program: string, args: string[], env: NodeJS.ProcessEnv, capture = false): Promise<string> {
  const child = spawn(program, args, { cwd: workspace, env, stdio: ["ignore", capture ? "pipe" : 2, "inherit"] });
  let output = "";
  child.stdout?.on("data", (chunk: Buffer) => { output += chunk; });
  const code = await new Promise<number | null>((done, fail) => {
    child.once("error", fail);
    child.once("close", done);
  });
  if (code !== 0) throw new Error(`${program} ${args.join(" ")} exited ${code}`);
  return output;
}

/** Both Cargo's final artifacts and its intermediate build directory are invocation-local. */
function cargoEnvironment(root: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    CARGO_TARGET_DIR: join(root, "target"), CARGO_BUILD_BUILD_DIR: join(root, "cargo-build"),
    CARGO_PROFILE_DEV_DEBUG: "0", CARGO_PROFILE_DEV_INCREMENTAL: "false",
    TMPDIR: join(root, "tmp"),
  };
}

/** The e2e frontend is never written into ui/dist or borrowed from a previous build. */
async function frontend(root: string): Promise<NodeJS.ProcessEnv> {
  const dist = join(root, "frontend");
  const env = cargoEnvironment(root);
  const frontend = packageCommand(["-C", ui, "build", "--mode", "e2e", "--outDir", dist]);
  await command(frontend.program, frontend.args, env);
  return { ...env, TAURI_CONFIG: JSON.stringify({ build: { frontendDist: dist }, app: { withGlobalTauri: true } }) };
}

function save(root: string, cli: string, app: string, observer: string): Candidate {
  const candidate = { root, cli, app, observer, manifest: join(root, "candidate.json") };
  writeFileSync(candidate.manifest, JSON.stringify(candidate));
  return candidate;
}

/** Cargo's artifact stream identifies the executables; no shared target/debug lookup. */
export async function prepare(): Promise<Candidate> {
  const root = resourceRoot();
  try {
    const env = await frontend(root);
    const output = await command("cargo", ["build", "--locked", "-p", "artifactize", "-p", "artifactize-desktop",
      "--features", "artifactize-desktop/e2e", "--bins", "--message-format=json-render-diagnostics"], env, true);
    const executables = new Map<string, string>();
    for (const line of output.split("\n").filter(Boolean)) {
      const item = JSON.parse(line) as { reason?: string; target?: { name?: string; kind?: string[] }; executable?: string };
      if (item.reason === "compiler-artifact" && item.executable && item.target?.kind?.includes("bin") && item.target.name) {
        executables.set(item.target.name, item.executable);
      }
    }
    const cli = executables.get("artifactize");
    const app = executables.get("artifactize-desktop");
    const observer = executables.get("artifactize-e2e-observe");
    if (!cli || !app || !observer) throw new Error("Cargo did not emit the CLI, desktop and native observation executables");
    return save(root, cli, app, observer);
  } catch (error) {
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

/** Only the owner's explicitly supplied manifest can select a candidate. */
export function readCandidate(manifest = process.env.DESKTOP_TEST_CANDIDATE): Candidate {
  if (!manifest) throw new Error("DESKTOP_TEST_CANDIDATE is not set; prepare a candidate before starting desktop E2E");
  const item = JSON.parse(readFileSync(manifest, "utf8")) as Partial<Candidate> | null;
  if (!item || typeof item.root !== "string" || !isAbsolute(item.root) || item.manifest !== manifest) {
    throw new Error("invalid desktop candidate manifest");
  }
  for (const path of [item.cli, item.app, item.observer, manifest]) {
    if (typeof path !== "string" || !isAbsolute(path)) throw new Error("invalid desktop candidate executable path");
    const within = relative(item.root, path);
    if (within === ".." || within.startsWith(`..${sep}`) || isAbsolute(within) || !existsSync(path)) {
      throw new Error(`candidate input is missing or outside its root: ${path}`);
    }
  }
  return item as Candidate;
}

/** A harness owns just its immutable binaries and fixture inputs, with no lifecycle lease. */
export async function snapshotCandidate(directory: string): Promise<Candidate> {
  const source = readCandidate();
  const root = resourceRoot(directory);
  try {
    const cli = join(root, `artifactize${executableSuffix}`);
    const app = join(root, `artifactize-desktop${executableSuffix}`);
    const observer = join(root, `artifactize-e2e-observe${executableSuffix}`);
    for (const [from, to] of [[source.cli, cli], [source.app, app], [source.observer, observer]]) copyFileSync(from, to);
    const candidate = save(root, cli, app, observer);
    process.env.DESKTOP_TEST_CANDIDATE = candidate.manifest;
    process.env.DESKTOP_TEST_RESOURCES = root;
    process.env.TMPDIR = join(root, "tmp");
    return candidate;
  } catch (error) {
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

/** Called only after the service's Launcher.run has completed. Durable domain Runs
 * are settled by fixture teardown before their executable inputs are removed. */
export async function cleanup(root: string): Promise<void> {
  const fixtures = join(root, "fixtures");
  if (existsSync(fixtures)) {
    const previous = process.env.DESKTOP_TEST_CANDIDATE;
    process.env.DESKTOP_TEST_CANDIDATE = join(root, "candidate.json");
    try {
      const { releaseFixtures } = await import("./fixture.ts");
      await releaseFixtures(readdirSync(fixtures).map((name) => join(fixtures, name)));
    } finally {
      if (previous === undefined) delete process.env.DESKTOP_TEST_CANDIDATE;
      else process.env.DESKTOP_TEST_CANDIDATE = previous;
    }
  }
  rmSync(root, { recursive: true, force: true });
}

/** Clippy and native tests receive the same owned e2e frontend and Cargo isolation. */
export async function cargoGate(mode: "clippy" | "test"): Promise<void> {
  const root = resourceRoot();
  try {
    const env = await frontend(root);
    const args = [mode, "--locked", "-p", "artifactize-desktop", "--features", "e2e"];
    if (mode === "clippy") args.push("--all-targets", "--", "-D", "warnings");
    await command("cargo", args, env);
  } finally {
    await cleanup(root);
  }
}
