// The README screenshots, captured from the real desktop app.
//
//   node assets/capture/screenshots.mts   capture scenes A–C into assets/screenshots/
//
// Linux only: the app runs on its own 1920×1080 virtual display. Capturing builds the app and
// its CLI from this checkout, declares a small project through that CLI, verifies it for real
// and drives the app the way a person would. Nothing is mocked and no model is called.
//
// The scenes (assets/screenshots/):
//   A. monitor.png — Verify: the example's branch expanded to its Runs in the Pane, the Run's
//      graph with mixed verdicts (GREEN, RED, blocked, waiting for a person), and the waiting
//      review count on the bottom bar.
//   B. detail.png  — an Artifact's Detail docked over the graph on its Artifact tab: its evals
//      with their verdicts, and the Artifact between its neighbours, unfolded.
//   C. review.png  — switch to Review in the bottom bar, open the waiting sign-off and claim it,
//      the filter widened to type:human so the Pane keeps it listed: its conversation with the
//      instruction and tools, the notes tool's run line unfolded into its lines, an issue as a
//      bubble, the issues box, and the decision bar with RED chosen.
//
// Modes:
//   (none)    capture into assets/screenshots/.
//   session   the start of the screenshots' review session: capture into the session's own
//             folder, outside the repository, then print one line and stay until stopped.
//   stop      the session's stop: nothing to do; artifactize removes the folder.
//   fresh     the session's tool: answer one just-captured screenshot as an image.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { captureAbort, launchCapture, onDisplay, watchCaptureStop} from "./lifecycle.mts";
import { adopt, cleanup, prepare} from "../../crates/artifactize-desktop/ui/e2e/build.ts";
import { own } from "../../crates/artifactize-desktop/ui/e2e/fixtures/owned.ts";
import { nativePath, asNativePath } from "../../crates/artifactize-desktop/ui/e2e/portable.ts";
import { environment, git, newFixture } from "../../crates/artifactize-desktop/ui/e2e/fixtures/repo.ts";
import { afterRender, fill } from "../../crates/artifactize-desktop/ui/e2e/keys.ts";
import { Graph } from "../../crates/artifactize-desktop/ui/e2e/views/graph.ts";

const here = dirname(fileURLToPath(import.meta.url));
const repository = join(here, "..", "..");
const output = join(repository, "assets", "screenshots");
/** The scenes, in the order they are captured. */
const SCENES = ["monitor.png", "detail.png", "review.png"];
const WIDTH = 1920;
const HEIGHT = 1080;
const WAIT = 60000;

const wrapped = (mode: string[], quiet = false) => onDisplay(new URL(import.meta.url), mode, WIDTH, HEIGHT, quiet);

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
 * waits for a person after its package check passed. Each Artifact is bound in
 * `.artifactize/{name}.artf` and targets its own folder. */
function project(repo: string) {
  mkdirSync(join(repo, ".artifactize"), { recursive: true });
  /** Artifact `name` in its own folder, composing `tools`, with the statements `body`, and
   * `beside` before it in its file. */
  const artifact = (name: string, body: string, files: Record<string, string>, tools = "", beside = "") => {
    mkdirSync(join(repo, name), { recursive: true });
    for (const [file, content] of Object.entries(files)) writeFileSync(join(repo, name, file), content);
    writeFileSync(join(repo, ".artifactize", `${name}.artf`), `${beside}artf ${name}${tools ? ` : ${tools}` : ""} = {\n  path = "${name}/"\n${body}}\n`);
  };
  /** A runtime eval that runs Node itself with `args`, after the comment `note`; `deps` are the
   * Artifacts it reads besides its own. */
  const check = (id: string, title: string, note: string, args: string[], deps: string[] = []) => `
  // ${note}
  eval ${id} : runtime(command = ${JSON.stringify(process.execPath)}, args = ${JSON.stringify(args)}) = {
    title = ${JSON.stringify(title)}${deps.length ? `\n    deps = [${deps.join(", ")}]` : ""}
  }
`;
  artifact("core", check("tests", "The core tests pass", "Run the core tests.", ["--test", "@{path self/sum.test.mjs}"])
    + check("lint", "The core code parses", "Check the syntax of the core code.", ["--check", "@{path self/sum.mjs}"]), {
    "sum.mjs": "export function sum(numbers) {\n  return numbers.reduce((total, each) => total + each, 0);\n}\n",
    "sum.test.mjs": "import assert from \"node:assert/strict\";\nimport test from \"node:test\";\nimport { sum } from \"./sum.mjs\";\n\ntest(\"adds every number\", () => assert.equal(sum([2, 3, 4]), 9));\n",
  });
  artifact("cli", check("tests", "The command line tests pass against core", "Run the command line tests with core.", ["--test", "@{path self/main.test.mjs}"], ["core"])
    + check("lint", "The command line code parses", "Check the syntax of the command line code.", ["--check", "@{path self/main.mjs}"]), {
    "main.mjs": "import { sum } from \"../core/sum.mjs\";\n\n// The parser rewrite fixes this: the first argument is dropped.\nexport function total(args) {\n  return sum(args.slice(1).map(Number));\n}\n",
    "main.test.mjs": "import assert from \"node:assert/strict\";\nimport test from \"node:test\";\nimport { total } from \"./main.mjs\";\n\ntest(\"totals every argument\", () => assert.equal(total([\"2\", \"3\", \"4\"]), 9));\n",
  });
  artifact("docs", check("examples", "Every documented command runs", "Run each command the docs show against the command line.", ["@{path self/examples.mjs}"], ["cli"]), {
    "examples.mjs": "import assert from \"node:assert/strict\";\nimport { total } from \"../cli/main.mjs\";\n\n// The README's example: `total 2 3 4` prints 9.\nassert.equal(total([\"2\", \"3\", \"4\"]), 9);\n",
  });
  artifact("release", check("package", "The release package builds from core", "Build the release package from core.", ["@{path self/package.mjs}"], ["core"]) + `
  eval signoff : human = {
    title = "A person approves the release notes"
    instruction = "Read the release notes and approve them for publication."
  }
`, {
    "package.mjs": "import assert from \"node:assert/strict\";\nimport * as core from \"../core/sum.mjs\";\n\n// The package exports exactly core's public functions.\nassert.deepEqual(Object.keys(core), [\"sum\"]);\n",
    "notes.md": "# Release notes\n\n- Faster startup.\n- Clearer error messages.\n",
  }, "notes", `tool notes = {\n  human read = builtin(op = "read", args = ["@{path self/notes.md}"], description = "Print the release notes.")\n}\n\n`);
}

/** Capture every scene into `output`, building in `work`. */
async function capture(output: string, work: string): Promise<void> {
  const unwatch = watchCaptureStop();
  let candidate: Awaited<ReturnType<typeof prepare>> | undefined;
  try {
    mkdirSync(output, { recursive: true });
    candidate = adopt(await prepare(asNativePath(work)));
    const scope = own(nativePath(candidate.root, "fixtures"));
    const fixture = newFixture(scope);
    let stop = () => scope.close();
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

      const session = await launchCapture(scope, candidate.app, ["--repo", fixture.repo, "--state-dir", fixture.state, "--reviewer", "alice"], environment(fixture), WIDTH, HEIGHT, WAIT, (close) => { stop = close; });
      const app = session.browser;
      stop = session.stop;
      const shot = async (name: string) => {
        captureAbort.signal.throwIfAborted();
        await afterRender(app);
        const png = Buffer.from(await app.takeScreenshot(), "base64");
        if (png.length < 24 || png.readUInt32BE(16) !== WIDTH || png.readUInt32BE(20) !== HEIGHT) {
          throw new Error(`${name} was not captured at ${WIDTH}×${HEIGHT} pixels`);
        }
        writeFileSync(join(output, name), png);
      };

      // A. Verify shows the branch's Run from its timeline. The bar, not Verify's Pane,
      // shows the number of reviews waiting for a person.
      const VERIFY = 'main[aria-label="Verify"]:not([hidden])';
      const MAIN = `${VERIFY} section.stage-area[aria-label="Main area"]`;
      const MODES = '[aria-label="Bottom bar"] [aria-label="Modes"]';
      const waitingReview = `${MODES} button[aria-label="Review, 1 waiting"]`;
      const runChip = app.$(`${VERIFY} [aria-label="Branches"] [role="treeitem"] button[data-chip]:not([data-chip="current"])`);
      await runChip.waitForDisplayed({ timeout: WAIT });
      const runId = await runChip.getAttribute("data-chip");
      await runChip.click();
      // The Pane's shown entry names the Run the Main area shows (J-STAGE).
      await app.waitUntil(() => app.execute((verify, id) => document.querySelector(`${verify} [aria-label="Pane entries"] button[data-chip][aria-pressed="true"]`)?.getAttribute("data-chip") === id
        && !!document.querySelector(`${verify} section.stage-area[aria-label="Main area"] .run`), VERIFY, runId),
        { timeout: WAIT, timeoutMsg: "Verify shows the selected Run" });
      const graph = new Graph(app);
      await graph.node("a:release");
      await app.$(waitingReview).waitForDisplayed({ timeout: WAIT, timeoutMsg: "Review counts the waiting sign-off in the bottom bar" });
      // core → cli, core → release and cli → docs, with the fixture's four different verdicts.
      await app.waitUntil(() => app.execute((main) => {
        const graph = document.querySelector(`${main} .run`);
        // Human sign-off is an active node; its waiting reason is in the Run's Detail and the bar.
        const lights = { core: "green", cli: "red", docs: "blocked", release: "active" };
        return graph?.querySelectorAll(".svelte-flow__edge").length === 3
          && Object.entries(lights).every(([name, light]) =>
            graph.querySelector(`.svelte-flow__node[data-id="a:${name}"][data-light="${light}"]`));
      }, MAIN), { timeout: WAIT, timeoutMsg: "the Run graph draws its three edges and mixed verdicts" });
      // The first fit can happen while the graph area is still taking its size, and the Run view
      // keeps its camera after that; the stage header's Fit frames the drawn graph the same way
      // on every capture. The keyboard then leaves Fit, so the scene shows no focus ring.
      await app.$('[aria-label="Graph tools"]').$("button=Fit").click();
      await app.execute(() => (document.activeElement as HTMLElement | null)?.blur());
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
        const neighbours = document.querySelector('[aria-label="Neighbours"] .svelte-flow');
        const frame = neighbours?.getBoundingClientRect();
        const evals = [...document.querySelectorAll('[aria-label="Evals of cli"] [data-eval]')];
        const marks = evals.some((row) => row.getAttribute("data-eval") === "e:cli/lint" && /GREEN/.test(row.textContent ?? ""))
          && evals.some((row) => row.getAttribute("data-eval") === "e:cli/tests" && /RED/.test(row.textContent ?? ""));
        const drawn = ["a:core", "a:cli", "a:docs"].every((id) => {
          const box = neighbours?.querySelector(`.svelte-flow__node[data-id="${id}"]`)?.getBoundingClientRect();
          return !!box && !!frame && box.width > 0 && box.height > 0 && box.left >= frame.left - 1
            && box.right <= frame.right + 1 && box.top >= frame.top - 1 && box.bottom <= frame.bottom + 1;
        });
        return marks && drawn && (neighbours?.querySelectorAll(".svelte-flow__edge").length ?? 0) >= 2;
      }), { timeout: WAIT, timeoutMsg: "cli's Neighbours show core, cli and docs and their edges" });
      // The scene shows the Detail as read, not the row the click left the keyboard on.
      await app.execute(() => (document.activeElement as HTMLElement | null)?.blur());
      await shot("detail.png");

      // C. Review's own Pane lists the sign-off Verify left. Claim it, run its notes tool,
      // add an issue and choose RED without submitting a result.
      await app.$(waitingReview).click();
      const REVIEW = 'main[aria-label="Review"]:not([hidden])';
      const options = `${REVIEW} [role="listbox"][aria-label="Human reviews"] [role="option"]`;
      await app.waitUntil(() => app.execute((options) => [...document.querySelectorAll(options)].some((row) => row.textContent?.includes("release/signoff")), options),
        { timeout: WAIT, timeoutMsg: "Review lists the release sign-off" });
      const rows = await app.$$(options).getElements();
      for (const row of rows) {
        if ((await row.getText()).includes("release/signoff")) {
          await row.click();
          break;
        }
      }
      const review = `${REVIEW} [aria-label="Review's main area"] [aria-label="Human review"]`;
      const claim = app.$(review).$("button=Claim to review");
      await claim.waitForEnabled({ timeout: WAIT });
      await claim.click();
      // `waiting` lists unclaimed reviews only: widen the filter to `type:human`, so the Pane
      // keeps the claimed sign-off listed and selected (J-REVIEW-FILTER).
      await fill(app, `${REVIEW} input[aria-label="Filter the reviews"]`, "type:human");
      await app.waitUntil(() => app.execute((options) => [...document.querySelectorAll(options)]
        .some((row) => row.getAttribute("aria-selected") === "true" && row.textContent?.includes("release/signoff")), options),
      { timeout: WAIT, timeoutMsg: "the widened filter lists the claimed sign-off, selected" });
      const notes = app.$(review).$("button=notes.read");
      await notes.waitForEnabled({ timeout: WAIT, timeoutMsg: "the sign-off is claimed and its notes tool is usable" });
      await notes.click();
      // The run's own line in the conversation, unfolded into its lines.
      const runLine = app.$(`${review} .run button.fold`);
      await runLine.waitForDisplayed({ timeout: WAIT });
      if ((await runLine.getAttribute("aria-expanded")) !== "true") await runLine.click();
      // The built-in read prints the notes and ends its run with `■ done`.
      await app.waitUntil(() => app.execute((review) => {
        const lines = [...document.querySelectorAll(`${review} [role="log"][aria-label="Tool log"] p:not(.muted)`)]
          .map((row) => row.textContent?.trim());
        return lines.includes("- Faster startup.") && lines.at(-1) === "■ done";
      }, review), { timeout: WAIT, timeoutMsg: "the notes tool printed the notes and finished" });
      // The log may arrive before the tool command returns. RED ignores clicks while that
      // job is still busy, so wait for the tool button to become usable again.
      await notes.waitForEnabled({ timeout: WAIT, timeoutMsg: "the finished notes tool is usable again" });
      // An issue, added from the issues box, stands as the reviewer's bubble.
      await fill(app, `${review} [data-issues]`, "The notes leave out the new config commands.");
      await app.$(review).$("button=Add issue").click();
      await app.$(`${review} [aria-label="Issues"] .message`).waitForDisplayed({ timeout: WAIT, timeoutMsg: "the issue stands as a bubble" });
      await app.$(`${review} [role="radiogroup"][aria-label="Verdict"]`).$("button*=RED").click();
      await app.$(`${review} [role="radio"][aria-checked="true"].red`).waitForDisplayed({ timeout: WAIT });
      await app.$(`${review} [aria-label="Decision"] button.submit:not([disabled])`).waitForDisplayed({ timeout: WAIT, timeoutMsg: "Submit RED can submit" });
      // The shot shows the review as read, not the keyboard's place on the last button.
      await app.execute(() => (document.activeElement as HTMLElement | null)?.blur());
      await shot("review.png");
    } finally { if (existsSync(scope.root)) await stop(); }
  } finally {
    unwatch();
    // A retained fixture means shutdown could not be proved: cleanup refuses, and so does the
    // outer scratch removal. Build/setup failures with no live fixture still remove scratch.
    if (candidate) cleanup(candidate.root);
    rmSync(work, { recursive: true, force: true });
  }
}

const mode = process.argv[2];
if (mode === "check") console.log(JSON.stringify({ frames: SCENES, width: WIDTH, height: HEIGHT, recording: false }));
else if (mode === "stop") process.exit(0);
else if (mode === "fresh") fresh();
else if (process.platform !== "linux") throw new Error("Capturing runs on Linux only.");
else if (!process.env.CAPTURE_DISPLAY_OWNED) process.exit(await (mode === "session" ? session() : wrapped([])));
else if (mode === "session") await capture(join(sessionDirectory(), "fresh"), join(sessionDirectory(), "work"));
else await capture(output, mkdtempSync(join(tmpdir(), "artifactize-screenshots-")));
