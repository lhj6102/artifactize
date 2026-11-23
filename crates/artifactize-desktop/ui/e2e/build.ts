// The builds the suite and the desktop's own tools run: the E2E test app with the CLI (a
// candidate), the production app for a person's launch, and the Rust gates. Each build has
// its own folder, Cargo output included, removed when its owner is done with it.
import { readdirSync } from "node:fs";
import { executableName, packageCommand, pathNotUrl, setVariable, startTree, stopTree, tempEnvironment, testWindowOptions, variable, type Environment } from "./platform.ts";
import { absolutePath, asNativePath, copyFile, exists, makeDirectory, nativePath, newDirectory, readJson, removeTree, runProgram, scratchDirectory, suitePath, writeJson, type NativePath } from "./portable.ts";

export const workspace = absolutePath(suitePath(), "../../../..");
const ui = absolutePath(suitePath(), "..");

/** A built E2E app and CLI, described by the manifest file in their folder. */
export type Candidate = { root: NativePath; cli: NativePath; app: NativePath; manifest: NativePath };

/** Run a build step with its progress on standard error; with `capture`, its standard output
 * is returned. An interrupt of this process ends its whole process tree. */
export async function runStep(program: string, args: readonly string[], options: { env: Environment; cwd: NativePath; capture?: boolean }): Promise<string> {
  const child = startTree(program, args, { cwd: options.cwd, env: options.env, capture: options.capture ?? false });
  let interrupted = false;
  let stopFailure: unknown;
  const interrupt = () => {
    interrupted = true;
    try { stopTree(child); } catch (error) { stopFailure = error; }
  };
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  let stdout = "";
  child.stdout?.setEncoding("utf8").on("data", (chunk: string) => { stdout += chunk; });
  try {
    const code = await new Promise<number | null>((done, fail) => {
      child.once("error", fail);
      child.once("close", (status) => done(status));
    });
    if (interrupted) throw new Error(`${program} ${args.join(" ")} was interrupted`, { cause: stopFailure });
    if (code !== 0) throw new Error(`${program} ${args.join(" ")} exited ${code}`);
    return stdout;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
  }
}

/** A new build folder; with Cargo's final and intermediate output inside it. */
function buildFolder(parent = scratchDirectory()): { root: NativePath; env: Environment } {
  const root = newDirectory(parent, "artifactize-desktop-build-");
  return { root, env: {
    ...process.env,
    CARGO_TARGET_DIR: nativePath(root, "target"), CARGO_BUILD_BUILD_DIR: nativePath(root, "cargo-build"),
    CARGO_PROFILE_DEV_DEBUG: "0", CARGO_PROFILE_DEV_INCREMENTAL: "false", ...tempEnvironment(makeDirectory(nativePath(root, "tmp"))),
  } };
}

/** Build the frontend for `kind` into the build folder; the variables that make Cargo embed it. */
async function frontend(root: NativePath, env: Environment, kind: "production" | "e2e"): Promise<Environment> {
  if (!exists(nativePath(ui, "node_modules"))) {
    throw new Error("UI dependencies are missing; run: pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile");
  }
  const dist = nativePath(root, `frontend-${kind}`);
  const command = packageCommand(["-C", ui, "build", "--mode", kind, "--outDir", dist, "--manifest"]);
  await runStep(command.program, command.args, { env, cwd: workspace });
  // Only the test build may carry the WebDriver plugin (E2E-TEST-BUILD-ONLY).
  const manifest = readJson(nativePath(dist, ".vite", "manifest.json")) as Record<string, unknown>;
  const plugin = Object.keys(manifest).some((key) => key.includes("/@wdio/tauri-plugin/"));
  if (plugin !== (kind === "e2e")) throw new Error(`the ${kind} frontend has the wrong WebDriver plugin inputs`);
  const declared = readJson(nativePath(workspace, "crates", "artifactize-desktop", "tauri.conf.json")) as { app: { windows: Record<string, unknown>[] } };
  const app = kind === "e2e" ? { withGlobalTauri: true, ...testWindowOptions(declared.app.windows) } : {};
  return { ...env, TAURI_CONFIG: JSON.stringify({ build: { frontendDist: pathNotUrl(dist) }, app }) };
}

/** Run `build` in a new build folder, removing the folder when it fails. */
async function building<T>(parent: NativePath | undefined, build: (root: NativePath, env: Environment) => Promise<T>): Promise<T> {
  const { root, env } = buildFolder(parent);
  try { return await build(root, env); }
  catch (error) {
    removeTree(root);
    throw error;
  }
}

const verifiedCandidates = new Map<string, Candidate>();
function save(root: NativePath, cli: NativePath, app: NativePath): Candidate {
  const candidate = { root, cli, app, manifest: nativePath(root, "candidate.json") };
  writeJson(candidate.manifest, candidate);
  return candidate;
}

/** Build the E2E test app and the CLI into a new folder under `parent`. */
export function prepare(parent?: NativePath): Promise<Candidate> {
  return building(parent, async (root, base) => {
    const env = await frontend(root, base, "e2e");
    // `json-render-diagnostics` keeps only Cargo's own reports on the captured standard output:
    // Cargo renders every compiler diagnostic as text on standard error, which runStep leaves
    // on this process's standard error, so a failed build prints why it failed before it throws.
    const output = await runStep("cargo", ["build", "--locked", "-p", "artifactize", "-p", "artifactize-desktop",
      "--features", "artifactize-desktop/e2e", "--bins", "--message-format=json-render-diagnostics"], { env, cwd: workspace, capture: true });
    // Cargo's own report names the executables it built.
    const built = new Map<string, NativePath>();
    for (const line of output.split("\n").filter(Boolean)) {
      const item = JSON.parse(line) as { reason?: string; target?: { name?: string; kind?: string[] }; executable?: string };
      if (item.reason === "compiler-artifact" && item.executable && item.target?.kind?.includes("bin") && item.target.name) {
        built.set(item.target.name, asNativePath(item.executable));
      }
    }
    const cli = built.get("artifactize");
    const app = built.get("artifactize-desktop");
    if (!cli || !app) throw new Error("Cargo did not build the CLI and the desktop app");
    return save(root, cli, app);
  });
}

/** The candidate the manifest `DESKTOP_TEST_CANDIDATE` names. */
export function readCandidate(manifest = variable("DESKTOP_TEST_CANDIDATE")): Candidate {
  if (!manifest) throw new Error("DESKTOP_TEST_CANDIDATE is not set; prepare a candidate first");
  const cached = verifiedCandidates.get(manifest);
  if (cached) return cached;
  const candidate = readJson(asNativePath(manifest)) as Candidate;
  for (const path of [candidate.cli, candidate.app]) {
    if (typeof path !== "string" || !exists(asNativePath(path))) throw new Error(`the candidate's ${path} is missing`);
  }
  checkE2eApp(candidate.app, candidate.root);
  verifiedCandidates.set(manifest, candidate);
  return candidate;
}

/** Check the actual executable that will be handed to the service, without opening a window. */
export function checkE2eApp(app: NativePath, cwd: NativePath, env?: Environment): void {
  const checked = runProgram(app, ["--check-e2e-build"], { cwd, env, timeout: 15000 });
  // This is not a manifest field or a caller-provided stamp. The E2E-only binary's
  // `check_build` (desktop src/e2e_metadata.rs) reads the compiled Tauri context and its
  // embedded asset manifest, before the window builder runs.
  const facts = checked.status === 0 ? JSON.parse(checked.stdout) as { embeddedDriver?: boolean; globalTauri?: boolean; testFrontend?: boolean; missingWindowAccess?: string[] } : null;
  if (!facts || facts.embeddedDriver !== true || facts.globalTauri !== true || facts.testFrontend !== true || facts.missingWindowAccess?.length !== 0) {
    throw new Error(`not an E2E test app: ${app}: ${checked.stderr.trim() || checked.stdout.trim() || `metadata probe exited ${checked.status}`}`);
  }
}

/** Makes `candidate` the one this process's fixtures belong to. */
export function adopt(candidate: Candidate): Candidate {
  setVariable("DESKTOP_TEST_CANDIDATE", candidate.manifest);
  return candidate;
}

/** A copy of the prepared candidate's executables in a new folder under `parent`, owned by
 * the caller alone; the prepared one is left as it is. */
export function snapshotCandidate(parent: NativePath): Promise<Candidate> {
  const source = readCandidate();
  return building(parent, async (root) => {
    const cli = nativePath(root, executableName("artifactize"));
    const app = nativePath(root, executableName("artifactize-desktop"));
    copyFile(source.cli, cli);
    copyFile(source.app, app);
    return adopt(save(root, cli, app));
  });
}

/** Remove a candidate's folder once nothing runs from it any more. */
export function cleanup(root: NativePath): void {
  const fixtures = nativePath(root, "fixtures");
  if (exists(fixtures) && readdirSync(fixtures).length) {
    throw new Error(`fixtures not released; kept the candidate and its inputs at ${root}`);
  }
  removeTree(root);
}

/** The production app with the CLI and tools beside it, as an install has them, in a new
 * folder the caller removes once the app has exited. */
export function prepareApp(): Promise<{ root: NativePath; app: NativePath }> {
  return building(undefined, async (root, base) => {
    console.log(`Building the production desktop app in ${root}`);
    const env = await frontend(root, base, "production");
    await runStep("cargo", ["build", "--locked", "-p", "artifactize-desktop", "-p", "artifactize", "-p", "artifactize-tools", "--bins"], { env, cwd: workspace });
    return { root, app: nativePath(root, "target", "debug", executableName("artifactize-desktop")) };
  });
}

/** Clippy or the crate's tests, on the E2E frontend, in a build folder of their own. */
export async function cargoGate(mode: "clippy" | "test"): Promise<void> {
  const root = await building(undefined, async (root, base) => {
    const env = await frontend(root, base, "e2e");
    const args = [mode, "--locked", "-p", "artifactize-desktop", "--features", "e2e"];
    if (mode === "clippy") args.push("--all-targets", "--", "-D", "warnings");
    await runStep("cargo", args, { env, cwd: workspace });
    return root;
  });
  removeTree(root);
}
