// The README screenshots, captured from the real desktop app.
//
//   node assets/capture/screenshots.mts   capture scenes A–C into assets/screenshots/
//
// Linux only: the app runs on its own 1920×1080 virtual display. Capturing builds the app and
// its CLI from this checkout, declares a small project through that CLI, verifies it for real
// and drives the app the way a person would. Nothing is mocked and no model is called.
//
// The scenes (assets/screenshots/):
//   A. monitor.png — the Monitor: Scope and Runs on the left and a Run graph with mixed
//      verdicts (GREEN, RED, blocked, waiting for a person).
//   B. detail.png  — an Artifact's Detail over the graph on its Artifact tab: the Artifact
//      between its neighbours, and its evals with their verdicts.
//   C. review.png  — a claimed Human review: its instruction, Tools, a Tool log run and the
//      GREEN form.
//
// Modes:
//   (none)    capture into assets/screenshots/.
//   session   the start of the screenshots' review session: capture into the session's own
//             folder, outside the repository, then print one line and stay until stopped.
//   stop      the session's stop: nothing to do; artifactize removes the folder.
//   fresh     the session's tool: answer one just-captured screenshot as an image.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repository = join(here, "..", "..");
const output = join(repository, "assets", "screenshots");
/** The scenes, in the order they are captured. */
const SCENES = ["monitor.png", "detail.png", "review.png"];
const WIDTH = 1920;
const HEIGHT = 1080;
const WAIT = 60000;

/** Re-run this script in `mode` on its own virtual display and session bus, which end with
 * it. Its output goes to stderr when `quiet`, so a session's stdout carries only its ready line. */
function wrapped(mode: string[], quiet = false): Promise<number> {
  const env = { ...process.env, CAPTURE_DISPLAY_OWNED: "1", GDK_BACKEND: "x11", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" };
  delete env.WAYLAND_DISPLAY;
  const child = spawn("dbus-run-session", ["--", "xvfb-run", "-a", "-s", `-screen 0 ${WIDTH}x${HEIGHT}x24`, process.execPath, fileURLToPath(import.meta.url), ...mode],
    { env, stdio: ["ignore", quiet ? process.stderr : "inherit", "inherit"] });
  return new Promise((done) => child.once("close", (code) => done(code ?? 1)));
}

/** The session's folder, which artifactize names and removes. */
function sessionDirectory(): string {
  const directory = process.env.ARTIFACTIZE_SESSION_DIR;
  if (!directory) throw new Error("ARTIFACTIZE_SESSION_DIR is not set: run this mode as a tool session.");
  return directory;
}

/** Capture into the session's folder; once captured, say ready and stay until stopped. */
async function session(): Promise<number> {
  const code = await wrapped(["session"], true);
  if (code !== 0) return code;
  console.log("captured");
  // Stay alive: the session lasts as long as this process.
  setInterval(() => {}, 1 << 30);
  return new Promise(() => {});
}

/** The `fresh` tool: one screenshot the session just captured, as an image. */
function fresh(): void {
  const request = JSON.parse(readFileSync(0, "utf8")) as { args: { name: string } };
  const name = request.args.name;
  const file = join(sessionDirectory(), "fresh", name);
  if (!SCENES.includes(name) || !existsSync(file)) {
    console.log(JSON.stringify({ isError: true, content: [{ type: "text", text: `No screenshot ${name}; choose one of ${SCENES.join(", ")}.` }] }));
    return;
  }
  console.log(JSON.stringify({ content: [
    { type: "text", text: `${name}, captured just now from the current app.` },
    { type: "image", mimeType: "image/png", data: readFileSync(file).toString("base64") },
  ] }));
}

/** The example project: core feeds cli and release; cli feeds docs. cli's tests fail, so docs
 * is blocked; release's sign-off waits for a person after its own check passed. */
function project(repo: string, fixtures: typeof import("../../crates/artifactize-desktop/ui/e2e/native-fixtures.ts")) {
  const { runtimeProfile, commandFields } = fixtures;
  const artifact = (name: string, text: string) => {
    mkdirSync(join(repo, name), { recursive: true });
    writeFileSync(join(repo, name, "index.artf"), `name = "${name}"\n${text}`);
  };
  artifact("core", `
[evals.tests]
title = "The core tests pass"
profile = ${runtimeProfile("pass")}
payload.instruction = "Run the core tests."

[evals.lint]
title = "The core code is formatted and linted"
profile = ${runtimeProfile("pass")}
payload.instruction = "Lint the core code."
`);
  artifact("cli", `
[evals.tests]
title = "The command line tests pass against core"
profile = ${runtimeProfile("fail")}
payload.instruction = "Run the command line tests with {core}."

[evals.lint]
title = "The command line code is formatted and linted"
profile = ${runtimeProfile("pass")}
payload.instruction = "Lint the command line code."
`);
  artifact("docs", `
[evals.examples]
title = "Every documented command runs"
profile = ${runtimeProfile("pass")}
payload.instruction = "Run each command the docs show against {cli}."
`);
  artifact("release", `
[views.human_tools]
notes = { description = "Print the release notes.", kind = "output", ${commandFields("read", join(repo, "release", "notes.md"))} }

[evals.package]
title = "The release package builds from core"
profile = ${runtimeProfile("pass")}
payload.instruction = "Build the release package from {core}."

[evals.signoff]
title = "A person approves the release notes"
profile = { kind = "human" }
payload.instruction = "Read the release notes and approve them for publication."
`);
  writeFileSync(join(repo, "release", "notes.md"), "# Release notes\n\n- Faster startup.\n- Clearer error messages.\n");
}

/** Capture every scene into `output`, building in `work`. */
async function capture(output: string, work: string): Promise<void> {
  const e2e = "../../crates/artifactize-desktop/ui/e2e";
  const { adopt, cleanup, prepare } = await import(`${e2e}/build.ts`);
  const fixtures = await import(`${e2e}/native-fixtures.ts`);
  const { environment, git } = await import(`${e2e}/environment.ts`);
  const { graphShown, openArtifact } = await import(`${e2e}/graph-view.ts`);
  const { claimable, reviewing } = await import(`${e2e}/human-review.ts`);
  const service = createRequire(join(repository, "crates/artifactize-desktop/ui/package.json"))("@wdio/tauri-service");
  mkdirSync(output, { recursive: true });
  const candidate = adopt(await prepare(work));
  // The build cleans up fixture roots named like this one, with every Run it settled.
  const root = join(candidate.root, "fixtures", "fixture-screenshots");
  const fixture = { root, repo: join(root, "example"), state: join(root, "state") };
  let browser: WebdriverIO.Browser | undefined;
  try {
    mkdirSync(fixture.repo, { recursive: true });
    project(fixture.repo, fixtures);
    git(fixture.repo, "init", "-q", "--initial-branch=main", fixture.repo);
    const verify = spawnSync(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--defer-human", "--wait"],
      { env: environment(fixture), encoding: "utf8" });
    // RED is the expected outcome: cli's tests fail on purpose.
    if (verify.status !== 1) throw new Error(`verify ended ${verify.status}: ${verify.stderr}`);

    const env = environment(fixture);
    for (const name of Object.keys(process.env)) if (env[name] === undefined) delete process.env[name];
    Object.assign(process.env, env);
    const capabilities = service.createTauriCapabilities(candidate.app, {
      appArgs: ["--state-dir", fixture.state, "--reviewer", "alice"],
      driverProvider: "embedded",
      startTimeout: WAIT,
      commandTimeout: WAIT,
      logLevel: "error",
    });
    capabilities["wdio:enforceWebDriverClassic"] = true;
    browser = await service.startWdioSession(capabilities) as WebdriverIO.Browser;
    const app = browser;
    await app.setWindowSize(WIDTH, HEIGHT);
    await app.waitUntil(() => app.execute(() => [innerWidth, innerHeight].join("x")).then((size) => size === `${WIDTH}x${HEIGHT}`),
      { timeout: WAIT, timeoutMsg: `the window is ${WIDTH}x${HEIGHT}` });
    const shot = async (name: string) => {
      // Let transitions and the graph's layout settle before the frame is taken.
      await app.pause(800);
      writeFileSync(join(output, name), Buffer.from(await app.takeScreenshot(), "base64"));
    };

    // A. The Monitor with the Run selected and its graph drawn.
    await app.$('[aria-label="Runs"] [role="option"]').waitForExist({ timeout: WAIT });
    await app.$('[aria-label="Runs"] [role="option"]').click();
    await graphShown(app, "a:release", WAIT);
    await shot("monitor.png");

    // B. cli's Detail on its Artifact tab: cli between the core it uses and the docs that use
    // it, its evals with their verdicts below.
    await openArtifact(app, "a:cli", "Artifact cli");
    await shot("detail.png");

    // C. The release sign-off in Review: claimed, its notes tool run, the GREEN form open.
    await app.keys(["Control", "2"]);
    await app.$('[aria-label="Requests"] [role="option"]').waitForExist({ timeout: WAIT });
    await app.$('[aria-label="Requests"] [role="option"]').click();
    await app.keys("Enter");
    await app.waitUntil(() => claimable(app), { timeout: WAIT, timeoutMsg: "the sign-off can be claimed" });
    await app.$("button=Claim to review").click();
    await app.waitUntil(() => reviewing(app), { timeout: WAIT, timeoutMsg: "the sign-off is claimed" });
    await app.$('[aria-label="Human review"]').$("button=▶ notes").click();
    await app.waitUntil(() => app.execute(() => document.body.innerText.includes("■ exit 0")),
      { timeout: WAIT, timeoutMsg: "the notes tool finished" });
    await app.waitUntil(() => reviewing(app), { timeout: WAIT, timeoutMsg: "the notes run settled" });
    await app.$('[aria-label="Human review"]').$("button=GREEN").click();
    await app.$('[aria-label="Human review"] .form').waitForExist({ timeout: WAIT });
    await shot("review.png");
  } finally {
    if (browser) await service.cleanupWdioSession(browser);
    await cleanup(candidate.root);
    rmSync(work, { recursive: true, force: true });
  }
}

const mode = process.argv[2];
if (mode === "stop") process.exit(0);
else if (mode === "fresh") fresh();
else if (process.platform !== "linux") throw new Error("Capturing runs on Linux only.");
else if (!process.env.CAPTURE_DISPLAY_OWNED) process.exit(await (mode === "session" ? session() : wrapped([])));
else if (mode === "session") await capture(join(sessionDirectory(), "fresh"), join(sessionDirectory(), "work"));
else await capture(output, mkdtempSync(join(tmpdir(), "artifactize-screenshots-")));
