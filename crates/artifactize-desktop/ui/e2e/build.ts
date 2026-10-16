// Immutable executable inputs. App/driver lifetimes belong exclusively to WDIO's Tauri service.
import {
  absolutePath, asNativePath, childDirectories, copyFile, executableName, exists, inheritedEnvironment, isInside,
  makeDirectory, nativePath, newDirectory, packageCommand, portablePath, readJson, readText, removeTree, runStep, scratchDirectory,
  setVariable, suitePath, variable, writeJson, type Environment, type NativePath,
} from "./platform.ts";

export const workspace = absolutePath(suitePath(), "../../../..");
const ui = absolutePath(suitePath(), "..");
export type Candidate = { root: NativePath; cli: NativePath; app: NativePath; observer: NativePath; manifest: NativePath };

function resourceRoot(directory = scratchDirectory()): NativePath {
  const root = newDirectory(directory, "artifactize-desktop-build-");
  makeDirectory(nativePath(root, "tmp"));
  return root;
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

/** The test app's windows keep rendering while another test's window covers them: a suite
 * runs several apps at once, and a covered macOS WebView otherwise stops its frames, so a
 * graph waiting for its layout never appears. Other systems ignore the policy. The merged
 * config replaces the window list, so each declared window is copied with the policy. */
function testWindows(): unknown[] {
  const declared = JSON.parse(readText(nativePath(workspace, "crates", "artifactize-desktop", "tauri.conf.json"))) as {
    app: { windows: Record<string, unknown>[] };
  };
  return declared.app.windows.map((window) => ({ ...window, backgroundThrottling: "disabled" }));
}

/** The e2e frontend is never written into ui/dist or borrowed from a previous build. */
async function frontend(root: NativePath): Promise<Environment> {
  const dist = nativePath(root, "frontend");
  const env = cargoEnvironment(root);
  const frontend = packageCommand(["-C", ui, "build", "--mode", "e2e", "--outDir", dist]);
  await runStep(frontend.program, frontend.args, { env, cwd: workspace });
  const app = { withGlobalTauri: true, windows: testWindows() };
  return { ...env, TAURI_CONFIG: JSON.stringify({ build: { frontendDist: portablePath(dist) }, app }) };
}

function save(root: NativePath, cli: NativePath, app: NativePath, observer: NativePath): Candidate {
  const candidate = { root, cli, app, observer, manifest: nativePath(root, "candidate.json") };
  writeJson(candidate.manifest, candidate);
  return candidate;
}

/** Cargo's artifact stream identifies the executables; no shared target/debug lookup. */
export async function prepare(): Promise<Candidate> {
  const root = resourceRoot();
  try {
    const env = await frontend(root);
    const output = await runStep("cargo", ["build", "--locked", "-p", "artifactize", "-p", "artifactize-desktop",
      "--features", "artifactize-desktop/e2e", "--bins", "--message-format=json-render-diagnostics"], { env, cwd: workspace, capture: true });
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
  } catch (error) {
    removeTree(root);
    throw error;
  }
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

/** A harness owns just its immutable binaries and fixture inputs, with no lifecycle lease. */
export async function snapshotCandidate(directory: NativePath): Promise<Candidate> {
  const source = readCandidate();
  const root = resourceRoot(directory);
  try {
    const cli = nativePath(root, executableName("artifactize"));
    const app = nativePath(root, executableName("artifactize-desktop"));
    const observer = nativePath(root, executableName("artifactize-e2e-observe"));
    for (const [from, to] of [[source.cli, cli], [source.app, app], [source.observer, observer]]) copyFile(from, to);
    const candidate = save(root, cli, app, observer);
    setVariable("DESKTOP_TEST_CANDIDATE", candidate.manifest);
    setVariable("DESKTOP_TEST_RESOURCES", root);
    setVariable("TMPDIR", nativePath(root, "tmp"));
    return candidate;
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
      await releaseFixtures(childDirectories(fixtures));
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
    const env = await frontend(root);
    const args = [mode, "--locked", "-p", "artifactize-desktop", "--features", "e2e"];
    if (mode === "clippy") args.push("--all-targets", "--", "-D", "warnings");
    await runStep("cargo", args, { env, cwd: workspace });
  } finally {
    await cleanup(root);
  }
}
