// Invocation-owned build inputs for the Human app launcher and WDIO test runners.
// Test app/driver lifetimes belong exclusively to WDIO's Tauri service.
import { ProcessTreeNotJoined, executableName, inheritedEnvironment, packageCommand, removeTree, runStep, scratchDirectory, setVariable, suitePath, testWindowOptions, variable, withBuildInterrupts, type Environment, isInside, newDirectory, pathNotUrl, writeJson } from "./platform.ts";
import { absolutePath, asNativePath, childDirectories, copyFile, exists, makeDirectory, nativePath, readJson, type NativePath } from "./portable.ts";

export const workspace = absolutePath(suitePath(), "../../../..");
const ui = absolutePath(suitePath(), "..");
export type Candidate = { root: NativePath; cli: NativePath; app: NativePath; observer: NativePath; manifest: NativePath };

function resourceRoot(directory = scratchDirectory()): NativePath {
  const root = newDirectory(directory, "artifactize-desktop-build-");
  try {
    makeDirectory(nativePath(root, "tmp"));
    return root;
  } catch (error) {
    try { removeTree(root); }
    catch (cleanup) { throw new AggregateError([error, cleanup], "build root initialization and cleanup failed"); }
    throw error;
  }
}

/** Both Cargo's final artifacts and its intermediate build directory are invocation-local. */
function cargoEnvironment(root: NativePath): Environment {
  return {
    ...inheritedEnvironment(),
    CARGO_TARGET_DIR: nativePath(root, "target"), CARGO_BUILD_BUILD_DIR: nativePath(root, "cargo-build"),
    CARGO_PROFILE_DEV_DEBUG: "0", CARGO_PROFILE_DEV_INCREMENTAL: "false",
    TMPDIR: nativePath(root, "tmp"),
  };
}

/** Build only the selected frontend and inspect its actual bundler inputs. */
async function frontend(root: NativePath, build: { kind: "production" | "e2e"; signal: AbortSignal }): Promise<Environment> {
  const env = cargoEnvironment(root);
  const selected = build.kind;
  const dist = nativePath(root, `frontend-${selected}`);
  const frontend = packageCommand(["-C", ui, "build", "--mode", selected, "--outDir", dist, "--manifest"]);
  await runStep(frontend.program, frontend.args, { env, cwd: workspace, signal: build.signal });
  const manifest = readJson(nativePath(dist, ".vite", "manifest.json")) as Record<string, unknown>;
  const plugin = Object.keys(manifest).some((key) => key.includes("/@wdio/tauri-plugin/"));
  if (plugin !== (selected === "e2e")) throw new Error(`${selected} frontend has the wrong WebDriver plugin inputs`);
  const declared = readJson(nativePath(workspace, "crates", "artifactize-desktop", "tauri.conf.json")) as {
    app: { windows: Record<string, unknown>[] };
  };
  const app = selected === "e2e" ? { withGlobalTauri: true, ...testWindowOptions(declared.app.windows) } : {};
  // frontendDist takes a URL or a folder. A URL makes the app browse it instead of embedding
  // and serving the frontend, so the folder is given in a form that cannot be a URL.
  return { ...env, TAURI_CONFIG: JSON.stringify({ build: { frontendDist: pathNotUrl(dist) }, app }) };
}

/** Production app and installed-app sidecars, with no WebDriver feature or fixture state.
 * The caller owns the returned root until the app exits. */
export async function prepareApp(): Promise<{ root: NativePath; app: NativePath }> {
  if (!exists(nativePath(ui, "node_modules"))) {
    throw new Error("UI dependencies are missing; run: pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile");
  }
  const root = resourceRoot();
  try {
    console.log(`Building the production desktop app in ${root}`);
    // The build runs as an owned process tree, so an interrupt or failure removes its folder
    // only once every compiler process in it has ended.
    return await withBuildInterrupts(async (signal) => {
      const env = await frontend(root, { kind: "production", signal });
      // Keep both commands beside the app, just as in an installed distribution. Source
      // builds do not register them on PATH or change the person's command installation.
      await runStep("cargo", ["build", "--locked", "-p", "artifactize-desktop", "-p", "artifactize", "-p", "artifactize-tools", "--bins"],
        { env, cwd: workspace, signal });
      return { root, app: nativePath(root, "target", "debug", executableName("artifactize-desktop")) };
    });
  } catch (error) { failedBuild(root, error); }
}

function save(root: NativePath, cli: NativePath, app: NativePath, observer: NativePath): Candidate {
  const candidate = { root, cli, app, observer, manifest: nativePath(root, "candidate.json") };
  writeJson(candidate.manifest, candidate);
  return candidate;
}

/** Cargo's artifact stream identifies the executables; no shared target/debug lookup. */
export async function prepare(directory?: NativePath): Promise<Candidate> {
  const root = resourceRoot(directory);
  try {
    return await withBuildInterrupts(async (signal) => {
      const env = await frontend(root, { kind: "e2e", signal });
      const output = await runStep("cargo", ["build", "--locked", "-p", "artifactize", "-p", "artifactize-desktop",
        "--features", "artifactize-desktop/e2e", "--bins", "--message-format=json-render-diagnostics"], { env, cwd: workspace, capture: true, signal });
      const executables = new Map<string, NativePath>();
      for (const line of output.split("\n").filter(Boolean)) {
        const item = JSON.parse(line) as { reason?: string; target?: { name?: string; kind?: string[] }; executable?: string };
        if (item.reason === "compiler-artifact" && item.executable && item.target?.kind?.includes("bin") && item.target.name) {
          executables.set(item.target.name, asNativePath(item.executable));
        }
      }
      const cli = executables.get("artifactize");
      const app = executables.get("artifactize-desktop");
      const observer = executables.get("artifactize-e2e-observe");
      if (!cli || !app || !observer) throw new Error("Cargo did not emit the CLI, desktop and native observation executables");
      return save(root, cli, app, observer);
    });
  } catch (error) { failedBuild(root, error); }
}

function failedBuild(root: NativePath, error: unknown): never {
  if (error instanceof ProcessTreeNotJoined) {
    throw new Error(`desktop build shutdown was not confirmed; retained inputs at ${root}`, { cause: error });
  }
  removeTree(root);
  throw error;
}

/** Only the owner's explicitly supplied manifest can select a candidate. */
export function readCandidate(manifest = variable("DESKTOP_TEST_CANDIDATE")): Candidate {
  if (!manifest) throw new Error("DESKTOP_TEST_CANDIDATE is not set; prepare a candidate before starting desktop E2E");
  const item = readJson(asNativePath(manifest)) as Partial<Record<keyof Candidate, unknown>> | null;
  if (!item || typeof item.root !== "string" || item.manifest !== asNativePath(manifest)) throw new Error("invalid desktop candidate manifest");
  const root = asNativePath(item.root);
  for (const path of [item.cli, item.app, item.observer, manifest]) {
    if (typeof path !== "string") throw new Error("invalid desktop candidate executable path");
    const executable = asNativePath(path);
    if (!isInside(root, executable) || !exists(executable)) throw new Error(`candidate input is missing or outside its root: ${path}`);
  }
  return item as Candidate;
}

/** Makes `candidate` the one this process's fixtures and temporary files belong to. */
export function adopt(candidate: Candidate): Candidate {
  setVariable("DESKTOP_TEST_CANDIDATE", candidate.manifest);
  setVariable("DESKTOP_TEST_RESOURCES", candidate.root);
  setVariable("TMPDIR", nativePath(candidate.root, "tmp"));
  return candidate;
}

/** A harness owns just its immutable binaries and fixture inputs, with no lifecycle lease. */
export async function snapshotCandidate(directory: NativePath): Promise<Candidate> {
  const source = readCandidate();
  const root = resourceRoot(directory);
  try {
    const cli = nativePath(root, executableName("artifactize"));
    const app = nativePath(root, executableName("artifactize-desktop"));
    const observer = nativePath(root, executableName("artifactize-e2e-observe"));
    for (const [from, to] of [[source.cli, cli], [source.app, app], [source.observer, observer]]) copyFile(from, to);
    return adopt(save(root, cli, app, observer));
  } catch (error) {
    removeTree(root);
    throw error;
  }
}

/** Called only after the service's Launcher.run has completed. Durable domain Runs
 * are settled by fixture teardown before their executable inputs are removed. */
export async function cleanup(root: NativePath): Promise<void> {
  const fixtures = nativePath(root, "fixtures");
  if (exists(fixtures)) {
    const previous = variable("DESKTOP_TEST_CANDIDATE");
    setVariable("DESKTOP_TEST_CANDIDATE", nativePath(root, "candidate.json"));
    try {
      const { releaseFixtures } = await import("./fixture.ts");
      const kept = await releaseFixtures(childDirectories(fixtures));
      // A fixture kept for diagnosis keeps the candidate that holds it.
      if (kept.length > 0) throw new Error(`fixtures retained for diagnosis: ${kept.join(", ")}`);
    } finally {
      setVariable("DESKTOP_TEST_CANDIDATE", previous);
    }
  }
  removeTree(root);
}

/** Clippy and native tests receive the same owned e2e frontend and Cargo isolation. */
export async function cargoGate(mode: "clippy" | "test"): Promise<void> {
  const root = resourceRoot();
  try {
    await withBuildInterrupts(async (signal) => {
      const env = await frontend(root, { kind: "e2e", signal });
      const args = [mode, "--locked", "-p", "artifactize-desktop", "--features", "e2e"];
      if (mode === "clippy") args.push("--all-targets", "--", "-D", "warnings");
      await runStep("cargo", args, { env, cwd: workspace, signal });
    });
  } catch (error) { failedBuild(root, error); }
  await cleanup(root);
}
