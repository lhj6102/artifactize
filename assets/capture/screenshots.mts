// The README screenshots, captured from the real desktop app.
//
//   node assets/capture/screenshots.mts   capture scenes A–C into assets/screenshots/
//
// Linux only: the app runs on its own 1920×1080 virtual display. Capturing builds the app and
// its CLI from this checkout, declares a small project through that CLI, verifies it for real
// and drives the app the way a person would. Nothing is mocked and no model is called.
//
// The scenes (assets/screenshots/):
//   A. monitor.png — the app's one screen: the Pane with the example's branch expanded to its Runs
//      and the waiting sign-off, and the Run's graph in the Main area with mixed verdicts
//      (GREEN, RED, blocked, waiting for a person).
//   B. detail.png  — an Artifact's Detail docked over the graph on its Artifact tab: its evals
//      with their verdicts, and the Artifact between its neighbours, unfolded.
//   C. review.png  — a claimed Human review opened from the Pane's waiting reviews: its
//      instruction, Tools, a Tool log run and the GREEN form.
//
// Modes:
//   (none)    capture into assets/screenshots/.
//   session   the start of the screenshots' review session: capture into the session's own
//             folder, outside the repository, then print one line and stay until stopped.
//   stop      the session's stop: nothing to do; artifactize removes the folder.
//   fresh     the session's tool: answer one just-captured screenshot as an image.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
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

/** A virtual display of its own: Xvfb on the first free display number from a random start.
 * A number another server holds makes Xvfb exit at once, so captures running at the same
 * time never share one; -displayfd says when the display is ready. */
async function virtualDisplay(): Promise<{ number: number; server: ChildProcess }> {
  const start = 100 + Math.floor(Math.random() * 800);
  for (let number = start; number < start + 64; number++) {
    const server = spawn("Xvfb", [`:${number}`, "-displayfd", "3", "-screen", "0", `${WIDTH}x${HEIGHT}x24`, "-nolisten", "tcp"],
      { stdio: ["ignore", "ignore", "ignore", "pipe"] });
    const ready = await new Promise<boolean>((done) => {
      server.stdio[3]!.once("data", () => done(true));
      server.once("error", () => done(false));
      server.once("exit", () => done(false));
    });
    if (ready) return { number, server };
  }
  throw new Error("Xvfb found no free display");
}

/** Re-run this script in `mode` on its own virtual display and session bus, which end with
 * it. The child's output goes to stderr when `quiet`, so a session's stdout carries only its
 * ready line. */
async function wrapped(mode: string[], quiet = false): Promise<number> {
  const display = await virtualDisplay();
  try {
    const env = { ...process.env, DISPLAY: `:${display.number}`, CAPTURE_DISPLAY_OWNED: "1", GDK_BACKEND: "x11", GTK_USE_PORTAL: "0", GIO_USE_VFS: "local", NO_AT_BRIDGE: "1" };
    delete env.WAYLAND_DISPLAY;
    const child = spawn("dbus-run-session", ["--", process.execPath, fileURLToPath(import.meta.url), ...mode],
      { env, stdio: ["ignore", quiet ? process.stderr : "inherit", "inherit"] });
    return await new Promise((done) => child.once("close", (code) => done(code ?? 1)));
  } finally {
    display.server.kill();
  }
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

/** The example project: core feeds cli and release; cli feeds docs. Every check is real: Node's
 * test runner, syntax check and module loader run on the project's own code. cli's test fails
 * on a real bug (its parser drops the first argument), so docs is blocked; release's sign-off
 * waits for a person after its package check passed. Each Artifact is defined in
 * `.artifactize/{name}.artf` and targets its own folder; cli carries a comment. */
function project(repo: string) {
  mkdirSync(join(repo, ".artifactize"), { recursive: true });
  const artifact = (name: string, text: string, files: Record<string, string>) => {
    mkdirSync(join(repo, name), { recursive: true });
    for (const [file, content] of Object.entries(files)) writeFileSync(join(repo, name, file), content);
    writeFileSync(join(repo, ".artifactize", `${name}.artf`), `path = "${name}/"\n${text}`);
  };
  /** A runtime eval's profile that runs Node itself with `args`. */
  const node = (...args: string[]) => `{ kind = "runtime", command = ${JSON.stringify(process.execPath)}, args = ${JSON.stringify(args)} }`;
  artifact("core", `
[evals.tests]
title = "The core tests pass"
profile = ${node("--test", "{core}/sum.test.mjs")}
payload.instruction = "Run the core tests."

[evals.lint]
title = "The core code parses"
profile = ${node("--check", "{core}/sum.mjs")}
payload.instruction = "Check the syntax of the core code."
`, {
    "sum.mjs": "export function sum(numbers) {\n  return numbers.reduce((total, each) => total + each, 0);\n}\n",
    "sum.test.mjs": "import assert from \"node:assert/strict\";\nimport test from \"node:test\";\nimport { sum } from \"./sum.mjs\";\n\ntest(\"adds every number\", () => assert.equal(sum([2, 3, 4]), 9));\n",
  });
  artifact("cli", `
[annotations.comment]
text = "The parser rewrite lands next."

[evals.tests]
title = "The command line tests pass against core"
profile = ${node("--test", "{cli}/main.test.mjs")}
payload.instruction = "Run the command line tests with {core}."

[evals.lint]
title = "The command line code parses"
profile = ${node("--check", "{cli}/main.mjs")}
payload.instruction = "Check the syntax of the command line code."
`, {
    "main.mjs": "import { sum } from \"../core/sum.mjs\";\n\n// The parser rewrite fixes this: the first argument is dropped.\nexport function total(args) {\n  return sum(args.slice(1).map(Number));\n}\n",
    "main.test.mjs": "import assert from \"node:assert/strict\";\nimport test from \"node:test\";\nimport { total } from \"./main.mjs\";\n\ntest(\"totals every argument\", () => assert.equal(total([\"2\", \"3\", \"4\"]), 9));\n",
  });
  artifact("docs", `
[evals.examples]
title = "Every documented command runs"
profile = ${node("{docs}/examples.mjs")}
payload.instruction = "Run each command the docs show against {cli}."
`, {
    "examples.mjs": "import assert from \"node:assert/strict\";\nimport { total } from \"../cli/main.mjs\";\n\n// The README's example: `total 2 3 4` prints 9.\nassert.equal(total([\"2\", \"3\", \"4\"]), 9);\n",
  });
  artifact("release", `
[views.human_tools]
notes = { description = "Print the release notes.", builtin = "read", args = ["{artifactPath}/notes.md"] }

[evals.package]
title = "The release package builds from core"
profile = ${node("{release}/package.mjs")}
payload.instruction = "Build the release package from {core}."

[evals.signoff]
title = "A person approves the release notes"
profile = { kind = "human" }
payload.instruction = "Read the release notes and approve them for publication."
`, {
    "package.mjs": "import assert from \"node:assert/strict\";\nimport * as core from \"../core/sum.mjs\";\n\n// The package exports exactly core's public functions.\nassert.deepEqual(Object.keys(core), [\"sum\"]);\n",
    "notes.md": "# Release notes\n\n- Faster startup.\n- Clearer error messages.\n",
  });
}

/** Capture every scene into `output`, building in `work`. */
async function capture(output: string, work: string): Promise<void> {
  const e2e = "../../crates/artifactize-desktop/ui/e2e";
  const { adopt, cleanup, prepare } = await import(`${e2e}/build.ts`);
  const { environment, git } = await import(`${e2e}/fixtures/repo.ts`);
  const { Graph } = await import(`${e2e}/views/graph.ts`);
  const { Review } = await import(`${e2e}/views/review.ts`);
  const { embeddedPorts } = await import(`${e2e}/portable.ts`);
  const service = createRequire(join(repository, "crates/artifactize-desktop/ui/package.json"))("@wdio/tauri-service");
  mkdirSync(output, { recursive: true });
  const candidate = adopt(await prepare(work));
  // The build's cleanup settles the Runs of fixture roots named and laid out like this one.
  const root = join(candidate.root, "fixtures", "fixture-screenshots");
  const fixture = { root, repo: join(root, "repo"), state: join(root, "state") };
  let browser: WebdriverIO.Browser | undefined;
  try {
    mkdirSync(fixture.repo, { recursive: true });
    project(fixture.repo);
    git(fixture, fixture.repo, "init", "-q", "--initial-branch=main", fixture.repo);
    // No idle grace: the Run ends as soon as only its unclaimed sign-off is left.
    const grace = spawnSync(candidate.cli, ["--state-dir", fixture.state, "limits", "--signoff-idle-grace", "0"],
      { env: environment(fixture), encoding: "utf8" });
    if (grace.status !== 0) throw new Error(`limits ended ${grace.status}: ${grace.stderr}`);
    const verify = spawnSync(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--wait"],
      { env: environment(fixture), encoding: "utf8" });
    // RED is the expected outcome: cli's tests fail on purpose.
    if (verify.status !== 1) throw new Error(`verify ended ${verify.status}: ${verify.stderr}`);

    const env = environment(fixture);
    for (const name of Object.keys(process.env)) if (env[name] === undefined) delete process.env[name];
    Object.assign(process.env, env);
    const capabilities = service.createTauriCapabilities(candidate.app, {
      // Started on the example's branch: the Pane selects it.
      appArgs: ["--repo", fixture.repo, "--state-dir", fixture.state, "--reviewer", "alice"],
      driverProvider: "embedded",
      startTimeout: WAIT,
      commandTimeout: WAIT,
      logLevel: "error",
    });
    capabilities["wdio:enforceWebDriverClassic"] = true;
    // A free WebDriver port of its own, so a capture running beside this one drives its own app.
    const [embeddedPort] = await embeddedPorts(1);
    capabilities["wdio:tauriServiceOptions"] = { ...capabilities["wdio:tauriServiceOptions"], embeddedPort };
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

    // A. The branch's Run shown from its timeline, its graph drawn, and the sign-off in the
    // Waiting stack. The launched branch opens expanded: its Current first, since the Run ended,
    // then its one Run.
    const MAIN = 'section.panel[aria-label="Main area"]';
    const runChip = '[aria-label="Branches"] [role="treeitem"] button[data-chip]:not([data-chip="current"])';
    await app.$(runChip).waitForExist({ timeout: WAIT });
    await app.$(runChip).click();
    await app.waitUntil(() => app.execute((main) => /^run-/.test(document.querySelector(`${main} > header h2`)?.textContent?.trim() ?? ""), MAIN),
      { timeout: WAIT, timeoutMsg: "the Main area shows the Run" });
    const graph = new Graph(app);
    const review = new Review(app, "main");
    await graph.node("a:release");
    await app.$('[aria-label="Waiting reviews"] [role="option"]').waitForExist({ timeout: WAIT, timeoutMsg: "the sign-off waits in the Pane" });
    // core → cli, core → release and cli → docs.
    await app.waitUntil(() => app.execute(() => document.querySelectorAll(".svelte-flow__edge").length === 3),
      { timeout: WAIT, timeoutMsg: "the Run graph draws its three edges" });
    // The first fit can happen while the graph area is still taking its size, and the Run view
    // keeps its camera after that; the tool strip's Fit frames the drawn graph the same way on
    // every capture.
    await app.$('[aria-label="Graph tools"]').$("button=Fit").click();
    let viewport = "";
    await app.waitUntil(async () => {
      const now = await app.execute(() => document.querySelector<HTMLElement>(".svelte-flow__viewport")?.style.transform ?? "");
      const still = now !== "" && now === viewport;
      viewport = now;
      return still;
    }, { timeout: WAIT, interval: 250, timeoutMsg: "the fitted camera holds still" });
    await shot("monitor.png");

    // B. cli's Detail on its Artifact tab: its evals with their verdicts, and, unfolded, cli
    // between the core it uses and the docs that use it.
    await graph.open("a:cli", "Artifact cli");
    const neighboursRow = app.$('[aria-label="Detail"]').$("button=Neighbours");
    await neighboursRow.waitForExist({ timeout: WAIT });
    if ((await neighboursRow.getAttribute("aria-expanded")) !== "true") await neighboursRow.click();
    await app.waitUntil(() => app.execute(() => {
      const neighbours = document.querySelector('[aria-label="Neighbours"]');
      const drawn = ["a:core", "a:cli", "a:docs"].every((id) => {
        const box = neighbours?.querySelector(`.svelte-flow__node[data-id="${id}"]`)?.getBoundingClientRect();
        return !!box && box.width > 0 && box.height > 0;
      });
      return drawn && (neighbours?.querySelectorAll(".svelte-flow__edge").length ?? 0) >= 2;
    }), { timeout: WAIT, timeoutMsg: "cli's Neighbours show core, cli and docs and their edges" });
    // The scene shows the Detail as read, not the row the click left the keyboard on.
    await app.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await shot("detail.png");

    // C. The release sign-off opened from the Pane's waiting reviews, which list the sign-off
    // that verify left: claimed, its notes tool run, the GREEN form open.
    await app.$('[aria-label="Waiting reviews"] [role="option"]').click();
    await app.waitUntil(() => app.execute((main) => document.querySelector(`${main} > header h2`)?.textContent?.trim() === "Review", MAIN),
      { timeout: WAIT, timeoutMsg: "the Main area shows the review" });
    await review.until((shown) => shown.stage === "CLAIM" && !shown.running, "the sign-off can be claimed", WAIT);
    await app.$("button=Claim to review").click();
    await review.until((shown) => shown.stage === "REVIEW (yours)" && !shown.running, "the sign-off is claimed", WAIT);
    await app.$('[aria-label="Human review"]').$("button=notes").click();
    // The built-in read prints the notes and ends its run with `■ done`.
    await review.runUntil("notes", (rows) => rows.includes("- Faster startup.") && rows.at(-1) === "■ done", "the notes tool printed the notes and finished");
    await review.until((shown) => shown.stage === "REVIEW (yours)" && !shown.running, "the notes run settled", WAIT);
    await app.$('[aria-label="Human review"]').$("button=GREEN").click();
    await app.$('[aria-label="Human review"] .form').waitForExist({ timeout: WAIT });
    await shot("review.png");
  } finally {
    try {
      if (browser) await service.cleanupWdioSession(browser);
      await cleanup(candidate.root);
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  }
}

const mode = process.argv[2];
if (mode === "stop") process.exit(0);
else if (mode === "fresh") fresh();
else if (process.platform !== "linux") throw new Error("Capturing runs on Linux only.");
else if (!process.env.CAPTURE_DISPLAY_OWNED) process.exit(await (mode === "session" ? session() : wrapped([])));
else if (mode === "session") await capture(join(sessionDirectory(), "fresh"), join(sessionDirectory(), "work"));
else await capture(output, mkdtempSync(join(tmpdir(), "artifactize-screenshots-")));
