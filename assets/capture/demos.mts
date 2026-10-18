// The README demos, recorded from the real desktop app.
//
//   node assets/capture/demos.mts   record demos A and B into assets/demos/
//
// Linux only: the app runs on its own 1920×1080 virtual display, and ffmpeg turns the frames
// into GIFs. Recording builds the app and its CLI from this checkout; every Run, reuse,
// dependency wait and verdict shown is real. Nothing is mocked and no model is called.
//
// The demos (assets/demos/):
//   A. review-loop.gif — an example shaped like this repository (core, cli, tui, mcp, docs,
//      site, release) goes through four Runs of real `verify`, recorded from the Monitor and
//      played back fast. Each eval is a runtime check that fails while its Artifact's code.txt
//      contains BUG; between Runs this script removes the BUG it just saw, as an AI would fix
//      the code. Try 1 fails mcp and site; try 2 reuses the rest at once and fails docs partway;
//      try 3 fails release near the end; try 4 passes to the end and is SATISFIED. An overlay
//      names the try and the speed.
//   B. graph-growth.gif — this repository's own Artifact graph, release by release from the
//      history of main, in the graph view, with the release in its top-left corner. One blank
//      step says 0.1.0–0.6.0 had no Artifacts yet; the graph starts at 0.7.0 and ends at the
//      current main. examples/ is left out. Declarations the current app no longer reads
//      are converted once to the current format: 0.7–0.8's artifactize.json, and dependency
//      evals, whose relations become references.
//
// Modes:
//   (none)        record into assets/demos/.
//   session       the start of the demos' review session: record into the session's own
//                 folder, outside the repository, then print one line and stay until stopped.
//   stop          the session's stop: nothing to do; artifactize removes the folder.
//   gif stored    a tool: a saved demo's length, or its frame about `at` seconds in.
//   gif fresh     the session's tool: the same for a demo the session just recorded.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repository = join(here, "..", "..");
const output = join(repository, "assets", "demos");
/** The demos, in the order they are recorded. */
const DEMOS = ["review-loop.gif", "graph-growth.gif"];
const WIDTH = 1920;
const HEIGHT = 1080;
const WAIT = 60000;
/** Demo A plays back this many times faster than it ran. */
const SPEED = 8;
/** The GIFs' width, frame rate and palette; one step of demo B stays this long. */
const GIF = { width: 1280, fps: 10, colours: 128, step: 1.6, hold: 1.5 };

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

/** Record into the session's folder; once recorded, say ready and stay until stopped. */
async function session(): Promise<number> {
  const code = await wrapped(["session"], true);
  if (code !== 0) return code;
  console.log("recorded");
  // Stay alive: the session lasts as long as this process.
  setInterval(() => {}, 1 << 30);
  return new Promise(() => {});
}

/** The `gif` tools: without `at`, a demo's length and frame count; with `at`, its frame about
 * `at` seconds in, as a PNG. `stored` reads the demo saved in the Artifact, `fresh` the one
 * the session just recorded. */
function gif(source: string): void {
  const request = JSON.parse(readFileSync(0, "utf8")) as { context: { artifactPath: string }; args: { name: string; at?: number } };
  const { name, at } = request.args;
  const folder = source === "fresh" ? join(sessionDirectory(), "fresh") : request.context.artifactPath;
  const file = join(folder, name);
  const answer = (content: object[], isError = false) => console.log(JSON.stringify(isError ? { isError, content } : { content }));
  if (!DEMOS.includes(name) || !existsSync(file)) {
    answer([{ type: "text", text: `No demo ${name}; choose one of ${DEMOS.join(", ")}.` }], true);
    return;
  }
  const label = source === "fresh" ? `${name}, recorded just now from the current app` : `${name}, as saved`;
  const [width, height, frames, seconds] = run("ffprobe", ["-v", "error", "-count_frames", "-select_streams", "v:0",
    "-show_entries", "stream=width,height,nb_read_frames:format=duration", "-of", "csv=p=0", file]).trim().split(/[,\n]/);
  const length = Number(seconds);
  if (at === undefined) {
    answer([{ type: "text", text: `${label}: ${length.toFixed(1)} s, ${frames} frames, ${width}×${height}. Ask for a frame with at (seconds).` }]);
    return;
  }
  if (at < 0 || at > length) {
    answer([{ type: "text", text: `${name} is ${length.toFixed(1)} s long; choose at from 0 to ${length.toFixed(1)}.` }], true);
    return;
  }
  const frame = spawnSync("ffmpeg", ["-v", "error", "-ss", String(at), "-i", file, "-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"], { maxBuffer: 1 << 26 });
  if (frame.status !== 0 || frame.stdout.length === 0) throw new Error(`ffmpeg could not read ${name} at ${at} s`);
  answer([{ type: "text", text: `${label}, at ${at} s of ${length.toFixed(1)} s.` }, { type: "image", mimeType: "image/png", data: frame.stdout.toString("base64") }]);
}

function run(program: string, args: string[], options: { cwd?: string; env?: NodeJS.ProcessEnv } = {}): string {
  const done = spawnSync(program, args, { ...options, encoding: "utf8", maxBuffer: 1 << 28 });
  if (done.status !== 0) throw new Error(`${program} ${args.join(" ")} failed: ${done.stderr || done.error}`);
  return done.stdout;
}

/** Frames as they were taken: each PNG and the moment it was taken, in seconds. */
type Frame = { file: string; at: number };
/** A stretch of the GIF: its frames, how much faster than recorded they play, the caption
 * for its whole length, and how long its last frame stays. */
type Segment = { frames: Frame[]; speed: number; caption: string; hold: number };

/** Take frames until `done` resolves and the app shows the final state `final` waits for,
 * then one more, so that final state is the recording's last frame. */
async function record(browser: WebdriverIO.Browser, directory: string, done: Promise<unknown>, final: () => Promise<boolean>): Promise<Frame[]> {
  const frames: Frame[] = [];
  let finished = false;
  void done.finally(() => { finished = true; });
  const take = async () => {
    const file = join(directory, `${String(frames.length).padStart(5, "0")}.png`);
    writeFileSync(file, Buffer.from(await browser.takeScreenshot(), "base64"));
    frames.push({ file, at: performance.now() / 1000 });
  };
  while (!finished) await take();
  await done;
  const until = performance.now() + WAIT;
  while (!await final()) {
    if (performance.now() > until) throw new Error("the app did not show the Run's final state");
    await take();
  }
  await take();
  return frames;
}

/** drawtext's text, with its own special characters escaped. */
function drawn(text: string): string {
  return text.replaceAll("\\", "\\\\").replaceAll(":", "\\:").replaceAll("'", "\\'").replaceAll("%", "\\%");
}

/** Encode `segments` into one GIF at `gif`: frames keep their recorded spacing divided by the
 * segment's speed, and each caption stays at `at`, the graph's top-left corner, for its segment. */
function encode(segments: Segment[], gif: string, work: string, at: { x: number; y: number }): void {
  const list: string[] = [];
  const captions: string[] = [];
  let clock = 0;
  for (const segment of segments) {
    const start = clock;
    segment.frames.forEach((frame, index) => {
      const next = segment.frames[index + 1];
      const duration = next ? Math.max((next.at - frame.at) / segment.speed, 0.01) : segment.hold;
      list.push(`file '${frame.file}'`, `duration ${duration.toFixed(3)}`);
      clock += duration;
    });
    captions.push(`drawtext=font=DejaVu Sans:fontsize=40:fontcolor=white:box=1:boxcolor=0x262E3BE6:boxborderw=18:x=${at.x}:y=${at.y}:text='${drawn(segment.caption)}':enable='gte(t,${start.toFixed(3)})*lt(t,${clock.toFixed(3)})'`);
  }
  // The concat demuxer takes the last entry's duration only when its file is listed again.
  list.push(`file '${segments.at(-1)!.frames.at(-1)!.file}'`);
  const concat = join(work, `${basename(gif)}.txt`);
  writeFileSync(concat, `${list.join("\n")}\n`);
  run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
    "-filter_complex", `[0:v]${captions.join(",")},fps=${GIF.fps},scale=${GIF.width}:-1:flags=lanczos,split[a][b];` +
    `[a]palettegen=max_colors=${GIF.colours}:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle`,
    gif]);
}

/** Launch the app on `args` in `env`, at the demo's size. */
async function launch(service: any, app: string, args: string[], env: NodeJS.ProcessEnv): Promise<WebdriverIO.Browser> {
  for (const name of Object.keys(process.env)) if (env[name] === undefined) delete process.env[name];
  Object.assign(process.env, env);
  const capabilities = service.createTauriCapabilities(app, {
    appArgs: args, driverProvider: "embedded", startTimeout: WAIT, commandTimeout: WAIT, logLevel: "error",
  });
  capabilities["wdio:enforceWebDriverClassic"] = true;
  const browser = await service.startWdioSession(capabilities) as WebdriverIO.Browser;
  await browser.setWindowSize(WIDTH, HEIGHT);
  await browser.waitUntil(() => browser.execute(() => [innerWidth, innerHeight].join("x")).then((size) => size === `${WIDTH}x${HEIGHT}`),
    { timeout: WAIT, timeoutMsg: `the window is ${WIDTH}x${HEIGHT}` });
  return browser;
}

/** Demo A's project. Each Artifact's evals run check.mjs on it: the check takes its seconds,
 * then fails while the Artifact's code.txt contains BUG. */
const LOOP: { name: string; uses: string[]; evals: [string, number][]; bug?: boolean }[] = [
  { name: "core", uses: [], evals: [["tests", 3], ["lint", 2]] },
  { name: "cli", uses: ["core"], evals: [["tests", 4]] },
  { name: "tui", uses: ["core"], evals: [["tests", 3]] },
  { name: "mcp", uses: ["core"], evals: [["tests", 4]], bug: true },
  { name: "site", uses: [], evals: [["links", 2]], bug: true },
  { name: "docs", uses: ["cli", "mcp"], evals: [["examples", 4]], bug: true },
  { name: "release", uses: ["cli", "tui", "docs"], evals: [["package", 5]], bug: true },
];
/** What each try fails, and so what the fix after it changes. */
const FAILS = [["mcp", "site"], ["docs"], ["release"], []];

function loopProject(root: string, repo: string): void {
  const checker = join(root, "check.mjs");
  writeFileSync(checker, `import { readFileSync } from "node:fs";
const [artifact, seconds] = process.argv.slice(2);
await new Promise((done) => setTimeout(done, Number(seconds) * 1000));
process.exit(readFileSync(artifact + "/code.txt", "utf8").includes("BUG") ? 1 : 0);
`);
  for (const artifact of LOOP) {
    const folder = join(repo, artifact.name);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, "code.txt"), artifact.bug ? "work in progress: BUG\n" : "done\n");
    const uses = artifact.uses.map((name) => `{${name}}`).join(" and ");
    const evals = artifact.evals.map(([id, seconds]) => `
[evals.${id}]
title = "${artifact.name} ${id} pass"
profile = { kind = "runtime", command = ${JSON.stringify(process.execPath)}, args = [${JSON.stringify(checker)}, "{${artifact.name}}", "${seconds}"] }
payload.instruction = "Run the ${id} of {${artifact.name}}${uses ? ` with ${uses}` : ""}."
`).join("");
    writeFileSync(join(folder, "index.artf"), `name = "${artifact.name}"\n${evals}`);
  }
}

async function reviewLoop(service: any, e2e: Record<string, any>, candidate: any, work: string): Promise<Segment[]> {
  const root = join(candidate.root, "fixtures", "fixture-review-loop");
  // The build's cleanup settles the Runs of fixture roots laid out as root/repo and root/state.
  const fixture = { root, repo: join(root, "repo"), state: join(root, "state") };
  mkdirSync(fixture.repo, { recursive: true });
  loopProject(root, fixture.repo);
  e2e.git(fixture.repo, "init", "-q", "--initial-branch=main", fixture.repo);
  const env = e2e.environment(fixture);
  const browser = await launch(service, candidate.app, ["--state-dir", fixture.state, "--reviewer", "alice"], env);
  const segments: Segment[] = [];
  try {
    const first = '[aria-label="Runs"] [data-index="0"]';
    /** The Run `id`'s panel shows its own graph, not the one drawn before it was opened: a
     * graph of another Run is replaced by a new canvas. */
    const opened = (id: string) => browser.execute((id) => {
      const canvas = document.querySelector(`[aria-label="Run ${id}"] .svelte-flow`);
      const node = canvas?.querySelector('.svelte-flow__node[data-id="a:release"]');
      const box = node?.getBoundingClientRect();
      return !!canvas && canvas !== (window as unknown as { before?: Element }).before && !!box && box.width > 0
        && document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)?.closest(".svelte-flow__node") === node;
    }, id);
    /** The Run `id` ended and its graph is lit as it ended: every Artifact settled, and exactly
     * `fails` red. */
    const ended = (id: string, fails: string[]) => browser.execute((id, fails, count) => {
      const panel = document.querySelector(`[aria-label="Run ${id}"]`);
      const lights = [...(panel?.querySelectorAll<HTMLElement>(".svelte-flow__node") ?? [])].map((node) => [node.dataset.id ?? "", node.dataset.light ?? ""]);
      return (panel?.querySelector(".headline")?.textContent ?? "").includes("at Run end") && lights.length === count
        && lights.every(([, light]) => light !== "waiting" && light !== "active")
        && lights.filter(([, light]) => light === "red").map(([node]) => node.slice(2)).sort().join() === [...fails].sort().join();
    }, id, fails, LOOP.length);
    for (const [index, fails] of FAILS.entries()) {
      const frames = join(work, `loop-${index + 1}`);
      mkdirSync(frames);
      const started = run(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--json"], { env: { ...env } });
      const id = (JSON.parse(started) as { run: string }).run;
      // Open the new Run as soon as the Monitor lists it first, then record it until it ends.
      await browser.waitUntil(() => browser.$(first).getText().then((text) => text.includes(id), () => false),
        { timeout: WAIT, timeoutMsg: `the Monitor lists ${id}` });
      await browser.execute(() => { (window as unknown as { before?: Element | null }).before = document.querySelector(".svelte-flow"); });
      await browser.$(first).click();
      await browser.waitUntil(() => opened(id), { timeout: WAIT, timeoutMsg: `the Monitor draws ${id}'s graph` });
      // `run show --wait` exits with the Run's outcome: 0 when it is satisfied.
      const settled = new Promise<{ code: number | null; text: string }>((done, failed) => {
        const watcher = spawn(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "show", id, "--wait", "--json"], { env });
        let text = "";
        watcher.stdout.on("data", (chunk) => { text += chunk; });
        watcher.once("error", failed);
        watcher.once("close", (code) => done({ code, text }));
      });
      segments.push({ frames: await record(browser, frames, settled, () => ended(id, fails)), speed: SPEED, caption: `try ${index + 1}/${FAILS.length} · ×${SPEED}`, hold: GIF.hold });
      const { code, text } = await settled;
      const view = JSON.parse(text) as { status: string; validation: { satisfied: boolean } | null; requests: { evalId: string; status: string }[] };
      const red = [...new Set(view.requests.filter((request) => request.status === "RED").map((request) => request.evalId.split("/")[0]))].sort();
      const last = fails.length === 0;
      // Every request settled as planned: GREEN, RED where the BUG still is, BLOCKED below it.
      const unexpected = view.requests.filter((request) => !["GREEN", "RED", "BLOCKED"].includes(request.status));
      if (red.join() !== [...fails].sort().join() || unexpected.length > 0 || view.status !== (last ? "GREEN" : "RED")
        || view.validation?.satisfied !== last || (code === 0) !== last) {
        throw new Error(`try ${index + 1} ended ${view.status} (exit ${code}) with RED ${red.join(", ") || "nothing"} and ${unexpected.map((request) => `${request.evalId} ${request.status}`).join(", ") || "no other states"}; planned RED ${fails.join(", ") || "nothing"}`);
      }
      // The fix: remove the BUG from what just failed, as an AI would fix the code.
      for (const name of fails) writeFileSync(join(fixture.repo, name, "code.txt"), "done\n");
    }
  } finally {
    await service.cleanupWdioSession(browser);
  }
  return segments;
}

/** The release tags of `main`, oldest first: vX.Y.Z without a pre-release part. */
function releases(main: string): string[] {
  return run("git", ["tag", "--merged", main, "--sort=v:refname"], { cwd: repository })
    .split("\n").filter((tag) => /^v\d+\.\d+\.\d+$/.test(tag));
}

/** TOML for one 0.7–0.8 `artifactize.json`: its name and evals. A mount's alias in an
 * instruction becomes a reference to the Artifact it mounted. */
function convertJson(text: string): string {
  const old = JSON.parse(text) as {
    name: string; mounts?: Record<string, string>;
    evals?: { id: string; title: string; profile: Record<string, unknown>; payload?: { instruction?: string } }[];
  };
  const lines = [`name = ${JSON.stringify(old.name)}`];
  for (const item of old.evals ?? []) {
    const profile = item.profile;
    const kept = profile.kind === "agent"
      ? { kind: "agent", backend: profile.backend, model: profile.model }
      : profile.kind === "runtime" ? { kind: "runtime", command: profile.command, args: profile.args ?? [] } : { kind: "human" };
    const fields = Object.entries(kept).map(([key, value]) => `${key} = ${JSON.stringify(value)}`).join(", ");
    const instruction = Object.entries(old.mounts ?? {})
      .reduce((text, [alias, artifact]) => text.replaceAll(`{${alias}}`, `{${artifact}}`), item.payload?.instruction ?? item.title);
    lines.push("", `[evals.${item.id}]`, `title = ${JSON.stringify(item.title)}`, `profile = { ${fields} }`,
      `payload.instruction = ${JSON.stringify(instruction)}`);
  }
  return `${lines.join("\n")}\n`;
}

/** Every file under `directory` named like `match`. */
function files(directory: string, match: (name: string) => boolean): string[] {
  return readdirSync(directory, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && match(entry.name))
    .map((entry) => join(entry.parentPath, entry.name));
}

/** Put `ref`'s tree, without examples/, in `tree`, in the format the current app reads. */
function checkout(ref: string, tree: string): void {
  rmSync(tree, { recursive: true, force: true });
  mkdirSync(tree, { recursive: true });
  run("sh", ["-c", `git -C "$1" archive "$2" | tar -x -C "$3" --exclude=examples`, "sh", repository, ref, tree]);
  // 0.7–0.8 named the ignore file differently; the folders it lists are not part of the graph.
  const oldIgnore = join(tree, ".artifactizeignore");
  if (existsSync(oldIgnore)) renameSync(oldIgnore, join(tree, ".artfignore"));
  const ignored = existsSync(join(tree, ".artfignore"))
    ? readFileSync(join(tree, ".artfignore"), "utf8").split("\n").map((line) => line.trim()).filter((line) => line && !line.startsWith("#"))
    : [];
  for (const file of files(tree, (name) => name === "artifactize.json")) {
    const path = relative(tree, file);
    if (ignored.some((folder) => path.startsWith(folder.replace(/\/?$/, "/")))) continue;
    writeFileSync(join(dirname(file), "index.artf"), convertJson(readFileSync(file, "utf8")));
    rmSync(file);
  }
  for (const file of files(tree, (name) => name.endsWith(".artf"))) {
    const text = readFileSync(file, "utf8");
    // A dependency eval only named what it waits for; a reference keeps that relation.
    const converted = text.replace(/profile = \{ kind = "dependency", depends_on = \[([^\]]*)\] \}/g, (_, names: string) => {
      const references = [...names.matchAll(/"([^"]+)"/g)].map(([, name]) => `{${name}}`).join(", ");
      return `profile = { kind = "runtime", command = "true", args = [] }\npayload.instruction = "Ready when ${references} are."`;
    });
    if (converted !== text) writeFileSync(file, converted);
  }
}

async function graphGrowth(service: any, e2e: Record<string, any>, candidate: any, work: string): Promise<Segment[]> {
  const root = join(candidate.root, "fixtures", "fixture-graph-growth");
  const fixture = { root, repo: join(root, "repo"), state: join(root, "state") };
  // The remote's main when this checkout has one: a local main is often behind it.
  const main = spawnSync("git", ["rev-parse", "--verify", "--quiet", "origin/main"], { cwd: repository }).status === 0 ? "origin/main" : "main";
  const tags = releases(main);
  const first = tags.findIndex((tag) => tag === "v0.7.0");
  if (first < 0) throw new Error("main has no v0.7.0 release");
  const steps = [...tags.slice(first).map((tag) => ({ ref: tag, caption: tag.slice(1) })), { ref: main, caption: "main" }];
  const frames = join(work, "growth");
  mkdirSync(frames);
  const segments: Segment[] = [];
  // 0.1.0–0.6.0 had no graph to draw, and the app opens only on a project with an Artifact:
  // their one step is a blank card in the app's background colour, with its caption.
  const blank = join(frames, "before.png");
  run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", `color=c=0xFAFAFA:s=${WIDTH}x${HEIGHT}`, "-frames:v", "1", blank]);
  segments.push({ frames: [{ file: blank, at: 0 }], speed: 1, caption: `${tags[0].slice(1)}–${tags[first - 1].slice(1)} · no Artifacts yet`, hold: GIF.step });
  checkout(steps[0].ref, fixture.repo);
  const browser = await launch(service, candidate.app, ["graph", "--repo", fixture.repo, "--state-dir", fixture.state, "--reviewer", "alice"], e2e.environment(fixture));
  try {
    for (const [index, step] of steps.entries()) {
      if (index > 0) {
        checkout(step.ref, fixture.repo);
        await browser.$("button=Refresh").click();
      }
      await browser.waitUntil(() => browser.execute(() => !document.querySelector<HTMLButtonElement>(".current header button")?.disabled
        && document.querySelectorAll(".current .svelte-flow__node").length > 0), { timeout: WAIT, timeoutMsg: `${step.caption}'s graph is drawn` });
      const failure = await browser.execute(() => document.querySelector(".current .failure")?.textContent ?? "");
      if (failure) throw new Error(`${step.caption} could not be read: ${failure}`);
      // Refresh keeps the camera; fit the grown graph into view the way a person would.
      await browser.$(".current .svelte-flow__controls-fitview").click();
      await browser.waitUntil(() => browser.executeAsync<boolean, []>((done) => {
        const camera = () => document.querySelector(".current .svelte-flow__viewport")?.getAttribute("style");
        const before = camera();
        setTimeout(() => done(before === camera()), 300);
      }), { timeout: WAIT, timeoutMsg: `${step.caption}'s graph is in view` });
      const file = join(frames, `${step.caption}.png`);
      writeFileSync(file, Buffer.from(await browser.takeScreenshot(), "base64"));
      segments.push({ frames: [{ file, at: 0 }], speed: 1, caption: step.caption, hold: GIF.step });
    }
  } finally {
    await service.cleanupWdioSession(browser);
  }
  return segments;
}

/** Record every demo into `output`, building and keeping frames in `work`. */
async function capture(output: string, work: string): Promise<void> {
  const base = "../../crates/artifactize-desktop/ui/e2e";
  const { adopt, cleanup, prepare } = await import(`${base}/build.ts`);
  const e2e = await import(`${base}/environment.ts`);
  const service = createRequire(join(repository, "crates/artifactize-desktop/ui/package.json"))("@wdio/tauri-service");
  mkdirSync(work, { recursive: true });
  const candidate = adopt(await prepare(join(work, "build")));
  try {
    const frames = join(work, "frames");
    mkdirSync(frames);
    const loop = await reviewLoop(service, e2e, candidate, frames);
    const growth = await graphGrowth(service, e2e, candidate, frames);
    mkdirSync(output, { recursive: true });
    // Each caption sits inside the graph's canvas, clear of the app's own header lines.
    const gifs = [[DEMOS[0], loop, { x: 490, y: 190 }], [DEMOS[1], growth, { x: 28, y: 180 }]] as const;
    for (const [name, segments, at] of gifs) {
      // Write beside the old GIF and replace it whole, so a failed encode keeps the last one.
      const gif = join(output, name);
      encode(segments, `${gif}.new.gif`, work, at);
      renameSync(`${gif}.new.gif`, gif);
    }
  } finally {
    try {
      await cleanup(candidate.root);
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  }
  for (const name of DEMOS) console.log(`${name}: ${(statSync(join(output, name)).size / 1e6).toFixed(1)} MB`);
}

const mode = process.argv[2];
if (mode === "stop") process.exit(0);
else if (mode === "gif") gif(process.argv[3]);
else if (process.platform !== "linux") throw new Error("Recording runs on Linux only.");
else if (!process.env.CAPTURE_DISPLAY_OWNED) process.exit(await (mode === "session" ? session() : wrapped([])));
else if (!existsSync(join(repository, ".git"))) throw new Error("Recording reads the release history: run it in a Git checkout.");
else if (mode === "session") await capture(join(sessionDirectory(), "fresh"), join(sessionDirectory(), "work"));
else await capture(output, mkdtempSync(join(tmpdir(), "artifactize-demos-")));
