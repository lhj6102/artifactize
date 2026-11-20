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
//      site, release) goes through four Runs of real `verify`, recorded from the app with its
//      branch expanded in the Pane, each new Run shown from its timeline in the Main area, and played back
//      fast. Each eval is a runtime check that fails while its Artifact's code.txt
//      contains BUG; between Runs this script removes the BUG it just saw, as an AI would fix
//      the code. Try 1 fails mcp and site; try 2 reuses the rest at once and fails docs partway;
//      try 3 fails release near the end; try 4 passes to the end and is SATISFIED. An overlay
//      names the try, the speed and how many of the Run's evals reused their saved result, as
//      the Run's records say; the script checks each try reused exactly the unchanged evals.
//   B. graph-growth.gif — this repository's own Artifact graph, release by release from the
//      history of main (or of origin/main when it is ahead), each committed in turn to a Git
//      checkout, in the app opened by `graph --repo` on that branch's Current, with the
//      release in the graph's top-left corner. One blank
//      step says 0.1.0–0.6.0 had no Artifacts yet; the graph starts at 0.7.0 and ends at the
//      current main. examples/ is left out. Declarations the current app no longer reads
//      are converted once to the current format: 0.7–0.8's artifactize.json, and dependency
//      evals, whose relations become references; then the built CLI's own `config migrate`
//      moves a release declared in its tree into `.artifactize/`, one file per Artifact.
//
// Modes:
//   (none)        record into assets/demos/.
//   session       the start of the demos' review session: record into the session's own
//                 folder, outside the repository, then print one line and stay until stopped.
//   stop          the session's stop: nothing to do; artifactize removes the folder.
//   gif stored    a tool: a saved demo's length, or its frame about `at` seconds in.
//   gif fresh     the session's tool: the same for a demo the session just recorded.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
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
const GIF = { width: 1280, fps: 25, colours: 128, step: 1.6, hold: 1.5 };
/** The progress bar: its height in GIF pixels and its colour, the introduction video's. */
const PROGRESS = { height: 4, colour: "0x3EE08F", track: "0xE3E8EE" };
/** The statuses' colours as the app's theme sets them: GREEN, RED, running and waiting. */
const STATUS_COLOURS = ["--green", "--red", "--running", "--queued"].map((token) => {
  const tokens = readFileSync(join(repository, "crates/artifactize-desktop/ui/src/tokens.css"), "utf8");
  const value = new RegExp(`${token}:\\s*#([0-9a-fA-F]{6})`).exec(tokens)?.[1];
  if (!value) throw new Error(`the theme has no ${token}`);
  return `0x${value}`;
});

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
/** A demo's segments, and where its captions go: the graph's top-left corner, in window pixels. */
type Recording = { segments: Segment[]; at: { x: number; y: number } };

/** Inside the top-left corner of the element `selector` names, as drawn now, clear of the
 * graph's edges: a caption box reaches 18 px past its text. */
async function corner(browser: WebdriverIO.Browser, selector: string): Promise<{ x: number; y: number }> {
  const box = await browser.execute((query) => {
    const rect = document.querySelector(query)?.getBoundingClientRect();
    return rect ? { x: rect.left, y: rect.top } : null;
  }, selector);
  if (!box) throw new Error(`${selector} is not drawn`);
  return { x: Math.round(box.x + 28), y: Math.round(box.y + 28) };
}

/** Take frames until `done` resolves and the app shows the final state `final` waits for,
 * then one more, so that final state is the recording's last frame. */
async function record(browser: WebdriverIO.Browser, directory: string, done: Promise<unknown>, final: () => Promise<boolean>, live: () => Promise<boolean>): Promise<Frame[]> {
  const frames: Frame[] = [];
  let finished = false;
  // Ended either way: a failed Run watch fails the recording below, where it is awaited.
  done.then(() => { finished = true; }, () => { finished = true; });
  const take = async () => {
    const file = join(directory, `${String(frames.length).padStart(5, "0")}.png`);
    writeFileSync(file, Buffer.from(await browser.takeScreenshot(), "base64"));
    frames.push({ file, at: performance.now() / 1000 });
  };
  // Frames that show the Run still in progress: the loop must be seen running, not only ended.
  let running = 0;
  while (!finished) {
    const shown = await live();
    await take();
    if (shown && !finished) running += 1;
  }
  if (running < LIVE_FRAMES) throw new Error(`only ${running} frames showed the Run in progress`);
  await done;
  const until = performance.now() + WAIT;
  while (!await final()) {
    if (performance.now() > until) throw new Error("the app did not show the Run's final state");
    await take();
  }
  // The last frame holds the longest: keep it only if the final state still shows after it.
  await take();
  while (!await final()) {
    if (performance.now() > until) throw new Error("the app did not keep the Run's final state");
    frames.pop();
    await take();
  }
  return frames;
}

/** The fewest frames a try must show of its Run in progress. */
const LIVE_FRAMES = 8;

/** drawtext's text, with its own special characters escaped. */
function drawn(text: string): string {
  return text.replaceAll("\\", "\\\\").replaceAll(":", "\\:").replaceAll("'", "\\'").replaceAll("%", "\\%");
}

/** Encode `segments` into one GIF at `gif`: frames keep their recorded spacing divided by the
 * segment's speed, and each caption stays at `at`, the graph's top-left corner, for its segment.
 * A green bar along the bottom fills as the GIF plays, so it reads as a recording, not a still. */
function encode(segments: Segment[], gif: string, work: string, at: { x: number; y: number }): void {
  const list: string[] = [];
  const captions: string[] = [];
  /** A moment of the GIF to the millisecond, the concat list's resolution. */
  const ms = (seconds: number) => Math.round(seconds * 1000) / 1000;
  let clock = 0;
  for (const segment of segments) {
    const start = clock;
    // Each frame starts where its recorded moment falls at the segment's speed, counted from the
    // segment's first frame, so the whole plays exactly that much faster: rounding to the
    // millisecond never adds up. Frames that fall in the same millisecond keep the later one.
    const first = segment.frames[0].at;
    const placed: { file: string; at: number }[] = [];
    for (const frame of segment.frames) {
      const moment = ms(start + (frame.at - first) / segment.speed);
      if (placed.at(-1)?.at === moment) placed.pop();
      placed.push({ file: frame.file, at: moment });
    }
    const end = ms(placed.at(-1)!.at + segment.hold);
    placed.forEach((frame, index) => {
      const next = placed[index + 1]?.at ?? end;
      list.push(`file '${frame.file}'`, `duration ${(next - frame.at).toFixed(3)}`);
    });
    clock = end;
    captions.push(`drawtext=font=DejaVu Sans:fontsize=40:fontcolor=white:box=1:boxcolor=0x262E3BE6:boxborderw=18:x=${at.x}:y=${at.y}:text='${drawn(segment.caption)}':enable='gte(t,${start.toFixed(3)})*lt(t,${clock.toFixed(3)})'`);
  }
  // The concat demuxer takes the last entry's duration only when its file is listed again.
  list.push(`file '${segments.at(-1)!.frames.at(-1)!.file}'`);
  const concat = join(work, `${basename(gif)}.txt`);
  writeFileSync(concat, `${list.join("\n")}\n`);
  // Drawn after fps and scaling, so it advances on every GIF frame and stays a crisp thin line;
  // laid over in RGB, since a subsampled overlay would wash out a status's thin strips.
  const { height, colour, track } = PROGRESS;
  // The GIF's frames on the frame rate's grid: as many as fit in the planned length.
  const count = Math.max(Math.floor(clock * GIF.fps), 1);
  // The bar grows with the frame's number, so it is full on the last frame whatever its time.
  const progress = `drawbox=x=0:y=ih-${height}:w=iw:h=${height}:color=${track}:t=fill[track];` +
    `color=c=${colour}:s=${GIF.width}x${height}[bar];[track][bar]overlay=x='-w+W*min(n/${Math.max(count - 1, 1)},1)':y=H-h:shortest=1:format=rgb`;
  // The repeated last entry would otherwise play its hold a second time: end at the planned length.
  const frames = `[0:v]${captions.join(",")},fps=${GIF.fps},trim=end_frame=${count},scale=${GIF.width}:-1:flags=lanczos,${progress}`;
  // The bar's few pixels would not earn its colours a palette entry, nor would the statuses'
  // thin strips, icons and words always: generate the palette with entries to spare, then set
  // the last ones to the bar, its track and each status's colour exactly.
  const pinned = [colour, track, ...STATUS_COLOURS];
  const generated = join(work, `${basename(gif)}.palette.png`);
  const palette = join(work, `${basename(gif)}.pinned.png`);
  run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat,
    "-filter_complex", `${frames},palettegen=max_colors=${GIF.colours - pinned.length}:stats_mode=diff:reserve_transparent=0`,
    "-update", "1", generated]);
  // The palette is a 16×16 image read row by row: its last entries are its last row's end.
  run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-i", generated, "-vf",
    pinned.map((pin, at) => `drawbox=x=${16 - pinned.length + at}:y=15:w=1:h=1:color=${pin}:t=fill`).join(","),
    "-update", "1", palette]);
  run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", concat, "-i", palette,
    "-filter_complex", `${frames}[v];[v][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle`,
    gif]);
}

/** Launch the app on `args` in `env`, at the demo's size. */
async function launch(service: any, app: string, args: string[], env: NodeJS.ProcessEnv): Promise<WebdriverIO.Browser> {
  const { embeddedPorts } = await import("../../crates/artifactize-desktop/ui/e2e/portable.ts");
  for (const name of Object.keys(process.env)) if (env[name] === undefined) delete process.env[name];
  Object.assign(process.env, env);
  const capabilities = service.createTauriCapabilities(app, {
    appArgs: args, driverProvider: "embedded", startTimeout: WAIT, commandTimeout: WAIT, logLevel: "error",
  });
  capabilities["wdio:enforceWebDriverClassic"] = true;
  // A free WebDriver port of its own, so a capture running beside this one drives its own app.
  const [embeddedPort] = await embeddedPorts(1);
  capabilities["wdio:tauriServiceOptions"] = { ...capabilities["wdio:tauriServiceOptions"], embeddedPort };
  const browser = await service.startWdioSession(capabilities) as WebdriverIO.Browser;
  try {
    await browser.setWindowSize(WIDTH, HEIGHT);
    await browser.waitUntil(() => browser.execute(() => [innerWidth, innerHeight].join("x")).then((size) => size === `${WIDTH}x${HEIGHT}`),
      { timeout: WAIT, timeoutMsg: `the window is ${WIDTH}x${HEIGHT}` });
  } catch (error) {
    // The app runs already: end it before the caller removes what it runs from.
    await service.cleanupWdioSession(browser);
    throw error;
  }
  return browser;
}

/** How long one try's Run may take: its longest chain of checks takes 16 s. */
const TRY_DEADLINE = 120000;

/** Demo A's input → consumer edges, one per Artifact a declaration uses. */
const EDGES = (): number => LOOP.reduce((count, artifact) => count + artifact.uses.length, 0);

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
    mkdirSync(join(repo, ".artifactize"), { recursive: true });
    writeFileSync(join(repo, ".artifactize", `${artifact.name}.artf`), `path = "${artifact.name}/"\n${evals}`);
  }
}

/** Stop `child` if it still runs, and wait until it has exited. */
async function stopped(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((done) => child.once("close", done));
  child.kill();
  await exited;
}

async function reviewLoop(service: any, e2e: Record<string, any>, candidate: any, work: string): Promise<Recording> {
  const root = join(candidate.root, "fixtures", "fixture-review-loop");
  // Laid out as the suite's fixtures, so its teardown cancels the Runs left open and waits
  // until they have ended before the build is removed.
  const fixture = { root, repo: join(root, "repo"), state: join(root, "state") };
  mkdirSync(fixture.repo, { recursive: true });
  loopProject(root, fixture.repo);
  e2e.git(fixture, fixture.repo, "init", "-q", "--initial-branch=main", fixture.repo);
  const env = e2e.environment(fixture);
  /** The `run show --wait` watchers still running. */
  const watchers = new Set<ChildProcess>();
  // Started on the example's branch: the Pane expands it, and each new Run arrives in its timeline.
  const browser = await launch(service, candidate.app, ["--repo", fixture.repo, "--state-dir", fixture.state, "--reviewer", "alice"], env);
  const segments: Segment[] = [];
  // The graph's top-left corner, below the Main area's header row and the graph's tool strip.
  let at = { x: 0, y: 0 };
  /** The Main area while it shows the Run `id`. */
  const MAIN = 'section.panel[aria-label="Main area"]';
  try {
    const chip = (id: string) => `[aria-label="Branches"] button[data-chip="${id}"]`;
    /** The Main area shows the Run `id`'s own graph, not the one drawn before it was opened: a
     * graph of another Run is replaced by a new canvas, while the Run already shown keeps its. */
    const opened = (id: string) => browser.execute((id, main, edges) => {
      const panel = document.querySelector(main);
      const canvas = panel?.querySelector(":scope > header h2")?.textContent?.trim() === id ? panel?.querySelector(".run .svelte-flow") : null;
      const node = canvas?.querySelector('.svelte-flow__node[data-id="a:release"]');
      const box = node?.getBoundingClientRect();
      const before = (window as unknown as { before?: { canvas: Element | null; run: string | null } }).before;
      return !!canvas && (canvas !== before?.canvas || before.run === id) && !!box && box.width > 0
        && canvas.querySelectorAll(".svelte-flow__edge").length === edges
        && document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)?.closest(".svelte-flow__node") === node;
    }, id, MAIN, EDGES());
    /** The Run `id` ended and its graph is drawn and lit as it ended: every Artifact settled,
     * exactly `fails` red, and every edge drawn. */
    const ended = (id: string, fails: string[]) => browser.execute((id, main, fails, count, edges) => {
      const shown = document.querySelector(main);
      const panel = shown?.querySelector(":scope > header h2")?.textContent?.trim() === id ? shown?.querySelector(".run") : null;
      // Every edge drawn as a visible line, not only present in the page.
      const lines = [...(panel?.querySelectorAll<SVGPathElement>(".svelte-flow__edge path.svelte-flow__edge-path") ?? [])].filter((line) => {
        const box = line.getBoundingClientRect();
        return !/NaN/.test(line.getAttribute("d") ?? "NaN") && (box.width > 0 || box.height > 0) && getComputedStyle(line).visibility !== "hidden";
      });
      if (lines.length !== edges) return false;
      const lights = [...(panel?.querySelectorAll<HTMLElement>(".svelte-flow__node") ?? [])].map((node) => [node.dataset.id ?? "", node.dataset.light ?? ""]);
      // The header row's headline names the Run's verdict once it ended.
      const verdict = shown?.querySelector(":scope > header button.headline .status")?.textContent?.trim() ?? "";
      return /^(GREEN|RED|YELLOW|ERROR|BLOCKED|INCOMPLETE)$/.test(verdict) && lights.length === count
        && lights.every(([, light]) => light !== "waiting" && light !== "active")
        && lights.filter(([, light]) => light === "red").map(([node]) => node.slice(2)).sort().join() === [...fails].sort().join();
    }, id, MAIN, fails, LOOP.length, EDGES());
    /** The evals whose saved GREEN result the next try reuses: those GREEN so far whose
     * Artifact, and every Artifact they use, the fixes left as they were. */
    const reusable = new Set<string>();
    for (const [index, fails] of FAILS.entries()) {
      const frames = join(work, `loop-${index + 1}`);
      mkdirSync(frames);
      const started = run(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "verify", "--all", "--json"], { env: { ...env } });
      const id = (JSON.parse(started) as { run: string }).run;
      // Show the new Run as soon as it starts the branch's timeline, then record it until it ends.
      await browser.$(chip(id)).waitForExist({ timeout: WAIT, timeoutMsg: `the branch's timeline starts with ${id}` });
      await browser.execute((main) => {
        const shown = document.querySelector(main);
        const canvas = shown?.querySelector(".run .svelte-flow") ?? null;
        const run = canvas ? shown?.querySelector(":scope > header h2")?.textContent?.trim() ?? null : null;
        (window as unknown as { before?: { canvas: Element | null; run: string | null } }).before = { canvas, run };
      }, MAIN);
      await browser.$(chip(id)).click();
      await browser.waitUntil(() => opened(id), { timeout: WAIT, timeoutMsg: `the Main area draws ${id}'s graph` });
      // `run show --wait` exits with the Run's outcome: 0 when it is satisfied. A Run that has
      // not ended by the deadline fails the recording; teardown stops a watcher still running.
      const settled = new Promise<{ code: number | null; text: string }>((done, failed) => {
        const watcher = spawn(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "run", "show", id, "--wait", "--json"], { env });
        watchers.add(watcher);
        const deadline = setTimeout(() => {
          failed(new Error(`${id} did not end within ${TRY_DEADLINE / 1000} s`));
          watcher.kill();
        }, TRY_DEADLINE);
        let text = "";
        watcher.stdout.on("data", (chunk) => { text += chunk; });
        watcher.once("error", (error) => {
          clearTimeout(deadline);
          failed(error);
        });
        watcher.once("close", (code) => {
          clearTimeout(deadline);
          watchers.delete(watcher);
          done({ code, text });
        });
      });
      /** The Run `id` is shown in progress: an Artifact of its graph still waits or runs. */
      const live = () => browser.execute((id, main) => {
        const shown = document.querySelector(main);
        const panel = shown?.querySelector(":scope > header h2")?.textContent?.trim() === id ? shown?.querySelector(".run") : null;
        return [...(panel?.querySelectorAll<HTMLElement>(".svelte-flow__node") ?? [])].some((node) => node.dataset.light === "waiting" || node.dataset.light === "active");
      }, id, MAIN);
      const recorded = await record(browser, frames, settled, () => ended(id, fails), live);
      const area = await corner(browser, `${MAIN} .run .canvas`);
      at = { x: Math.max(at.x, area.x), y: Math.max(at.y, area.y) };
      const { code, text } = await settled;
      const view = JSON.parse(text) as { status: string; validation: { satisfied: boolean } | null; requests: { evalId: string; status: string; source: { kind: string } | null }[] };
      // Unchanged work is reused, as the Run's records say: exactly the evals it could reuse
      // took their saved result, and every other one ran or was blocked.
      const reused = view.requests.filter((request) => request.source?.kind === "cache").map((request) => request.evalId).sort();
      if (reused.join() !== [...reusable].sort().join()) {
        throw new Error(`try ${index + 1} reused ${reused.join(", ") || "nothing"}; planned ${[...reusable].sort().join(", ") || "nothing"}`);
      }
      segments.push({ frames: recorded, speed: SPEED, caption: `try ${index + 1}/${FAILS.length} · ×${SPEED} · ${reused.length} of ${view.requests.length} evals reused`, hold: GIF.hold });
      const red = [...new Set(view.requests.filter((request) => request.status === "RED").map((request) => request.evalId.split("/")[0]))].sort();
      const last = fails.length === 0;
      // Every request settled as planned: GREEN, RED where the BUG still is, BLOCKED below it.
      const unexpected = view.requests.filter((request) => !["GREEN", "RED", "BLOCKED"].includes(request.status));
      if (red.join() !== [...fails].sort().join() || unexpected.length > 0 || view.status !== (last ? "GREEN" : "RED")
        || view.validation?.satisfied !== last || (code === 0) !== last) {
        throw new Error(`try ${index + 1} ended ${view.status} (exit ${code}) with RED ${red.join(", ") || "nothing"} and ${unexpected.map((request) => `${request.evalId} ${request.status}`).join(", ") || "no other states"}; planned RED ${fails.join(", ") || "nothing"}`);
      }
      // The fix: remove the BUG from what just failed, as an AI would fix the code. What it
      // changes, and what uses it, is evaluated again; the rest of what passed is reused.
      for (const request of view.requests) if (request.status === "GREEN") reusable.add(request.evalId);
      for (const name of fails) {
        writeFileSync(join(fixture.repo, name, "code.txt"), "done\n");
        for (const artifact of LOOP.filter((each) => each.name === name || each.uses.includes(name))) {
          for (const [check] of artifact.evals) reusable.delete(`${artifact.name}/${check}`);
        }
      }
    }
  } finally {
    try {
      for (const watcher of watchers) await stopped(watcher);
      await service.cleanupWdioSession(browser);
    } finally {
      // The Runs this demo started end before its project, its state and the build go.
      e2e.cancelRuns(fixture);
    }
  }
  return { segments, at };
}

/** The newer of this checkout's main and the remote's: a local main is often behind the
 * remote's, and sometimes ahead of it. Diverged, the remote's is the published history. */
function newestMain(): string {
  const exists = (ref: string) => spawnSync("git", ["rev-parse", "--verify", "--quiet", ref], { cwd: repository }).status === 0;
  if (!exists("origin/main")) return "main";
  if (!exists("main")) return "origin/main";
  const contains = (newer: string, older: string) => spawnSync("git", ["merge-base", "--is-ancestor", older, newer], { cwd: repository }).status === 0;
  return contains("main", "origin/main") ? "main" : "origin/main";
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

/** Put `ref`'s tree, without examples/, in `tree`, a Git checkout, in the format the current
 * app reads: older forms converted here, then a tree that declares its Artifacts where they
 * live migrated by the built CLI `cli`. The checkout's own Git folder stays. */
function checkout(ref: string, tree: string, cli: string): void {
  for (const entry of readdirSync(tree)) if (entry !== ".git") rmSync(join(tree, entry), { recursive: true, force: true });
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
  // 0.17–0.18 allowed `space::name`; a name is one identifier now, `space-name`.
  for (const file of files(tree, (name) => name.endsWith(".artf"))) {
    const text = readFileSync(file, "utf8");
    const spaced = /^(name = ")([^"]*)"/gm;
    const converted = text
      .replace(spaced, (_, start: string, name: string) => `${start}${name.replaceAll("::", "-")}"`)
      .replace(/\{([A-Za-z0-9][A-Za-z0-9_-]*(?:::[A-Za-z0-9][A-Za-z0-9_-]*)+)\}/g, (_, name: string) => `{${name.replaceAll("::", "-")}}`);
    if (converted !== text) writeFileSync(file, converted);
  }
  // A reference reached through a folder Artifact into one nested in it; that file is the
  // nested Artifact's now, so the reference names it.
  const folders = new Map<string, string>();
  for (const file of files(tree, (name) => name === "index.artf")) {
    const name = /^name = "([^"]*)"/m.exec(readFileSync(file, "utf8"))?.[1];
    if (name) folders.set(name, dirname(file));
  }
  const owners = [...folders].sort(([, one], [, other]) => other.length - one.length);
  for (const file of files(tree, (name) => name.endsWith(".artf"))) {
    const text = readFileSync(file, "utf8");
    const converted = text.replace(/\{([A-Za-z0-9][A-Za-z0-9_-]*)\}\/([^"\s}]+)/g, (whole, name: string, path: string) => {
      const folder = folders.get(name);
      if (!folder) return whole;
      const target = join(folder, path);
      const [owner, root] = owners.find(([, root]) => !relative(root, target).startsWith("..")) ?? [name, folder];
      if (owner === name) return whole;
      const rest = relative(root, target);
      return rest ? `{${owner}}/${rest.split("\\").join("/")}` : `{${owner}}`;
    });
    if (converted !== text) writeFileSync(file, converted);
  }
  // A release from before definitions lived in .artifactize/: its own CLI's successor moves them.
  const defined = existsSync(join(tree, ".artifactize")) && files(join(tree, ".artifactize"), (name) => name.endsWith(".artf")).length > 0;
  if (!defined) run(cli, ["--repo", tree, "config", "migrate"]);
}

async function graphGrowth(service: any, e2e: Record<string, any>, candidate: any, work: string): Promise<Recording> {
  const root = join(candidate.root, "fixtures", "fixture-graph-growth");
  const fixture = { root, repo: join(root, "repo"), state: join(root, "state") };
  const main = newestMain();
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
  // Each step is committed to the checkout's main branch, whose Current the app shows.
  mkdirSync(fixture.repo, { recursive: true });
  e2e.git(fixture, fixture.repo, "init", "-q", "--initial-branch=main", fixture.repo);
  const commit = (step: { ref: string; caption: string }) => {
    checkout(step.ref, fixture.repo, candidate.cli);
    e2e.git(fixture, fixture.repo, "add", "-A");
    e2e.git(fixture, fixture.repo, "commit", "-q", "--allow-empty", "-m", step.caption);
  };
  commit(steps[0]);
  const browser = await launch(service, candidate.app, ["graph", "--repo", fixture.repo, "--state-dir", fixture.state, "--reviewer", "alice"], e2e.environment(fixture));
  let at = { x: 0, y: 0 };
  try {
    /** When the shown Current was last scanned, as its header says it; "" while none ended. */
    const scannedAt = () => browser.execute(() => {
      const scan = document.querySelector('section.panel[aria-label="Main area"] > header .scan');
      return scan?.querySelector(".checking") ? "" : (scan?.querySelector("time")?.getAttribute("datetime") ?? "");
    });
    for (const [index, step] of steps.entries()) {
      // A scan that ended after this step's checkout: the one Refresh starts, or the first.
      const before = index > 0 ? await scannedAt() : "";
      if (index > 0) {
        commit(step);
        await browser.$("button=Refresh").click();
      }
      await browser.waitUntil(async () => {
        const now = await scannedAt();
        return now !== "" && now !== before;
      }, { timeout: WAIT, timeoutMsg: `${step.caption}'s scan ended` });
      // Names read from about half size on: every Artifact of a graph this size is then drawn as
      // an element, so its nodes and edges can be compared below; Fit zooms out again after.
      for (let tries = 0; tries < 20; tries += 1) {
        const percent = Number.parseInt(await browser.$('[aria-label="Graph tools"] [aria-label="Zoom"]').getText(), 10);
        if (percent >= 50) break;
        await browser.$('[aria-label="Graph tools"] [aria-label="Zoom in"]').click();
      }
      // This release's own topology, as the CLI reads its declarations: its Artifacts, and an
      // edge for every relation between two of them but a part's, which the Current draws as
      // the part inside its whole. A part of several wholes is drawn once in each, so the
      // drawing is compared by its copies, each keyed by the wholes it is drawn inside, and by the
      // Artifacts its edges join, an edge's ends being copy keys ending in their Artifacts' ids.
      // The graph drawn is this release's only once these are exactly its own, never the
      // previous one's.
      const graph = JSON.parse(run(candidate.cli, ["--repo", fixture.repo, "--state-dir", fixture.state, "config", "graph", "--json"],
        { env: e2e.environment(fixture) })) as { artifacts: Record<string, unknown>; relations: { source: string; target: string; kind: string }[] };
      const names = new Set(Object.keys(graph.artifacts));
      // Where each Artifact is drawn: one copy inside each whole that names it as a part, its key
      // the path of wholes down to it, and once at the top for an Artifact that is no part. A part
      // that would close a cycle is not drawn again inside itself.
      const partsOf = new Map<string, string[]>();
      for (const { source, target, kind } of graph.relations) {
        if (kind === "part" && names.has(source) && names.has(target)) partsOf.set(target, [...(partsOf.get(target) ?? []), source]);
      }
      const isPart = new Set([...partsOf.values()].flat());
      const copies: string[] = [];
      const place = (name: string, above: string | null, around: Set<string>) => {
        const key = above === null ? `a:${name}` : `${above}/a:${name}`;
        copies.push(key);
        for (const part of partsOf.get(name) ?? []) if (!around.has(part)) place(part, key, new Set([...around, part]));
      };
      for (const name of names) if (!isPart.has(name)) place(name, null, new Set([name]));
      const expected = [
        [...new Set(copies)].sort().join(" "),
        [...new Set(graph.relations.filter(({ source, target, kind }) => kind !== "part" && source !== target && names.has(source) && names.has(target))
          .map(({ source, target }) => `a:${source}>a:${target}`))].sort().join(" "),
      ].join("\n");
      await browser.waitUntil(() => browser.execute((expected) => {
        const set = (values: string[]) => [...new Set(values)].sort().join(" ");
        const ends = (id: string) => id.split(">").map((key) => key.split("/").pop()).join(">");
        // Every copy by its key, the wholes it is drawn inside included: a previous release's
        // drawing with the same Artifacts nested otherwise does not pass.
        const artifacts = set([...document.querySelectorAll<HTMLElement>(".current .svelte-flow__node[data-artifact]")].map((node) => node.dataset.id ?? ""));
        const edges = set([...document.querySelectorAll<HTMLElement>(".current .svelte-flow__edge")].map((edge) => ends(edge.dataset.id ?? "")));
        const refresh = document.querySelector<HTMLButtonElement>('section.panel[aria-label="Main area"] > header .actions button');
        // The graph shown was read at the scan that just ended, not kept from the one before.
        const host = document.querySelector<HTMLElement>('section.panel[aria-label="Main area"] .host');
        const current = !!host && host.dataset.read !== undefined && host.dataset.read === host.dataset.drawn;
        return !!refresh && !refresh.disabled && current && `${artifacts}\n${edges}` === expected;
      }, expected), { timeout: WAIT, timeoutMsg: `${step.caption}'s own graph is drawn` });
      const failure = await browser.execute(() => document.querySelector('section.panel[aria-label="Main area"] > header .failure')?.textContent ?? "");
      if (failure) throw new Error(`${step.caption} could not be read: ${failure}`);
      // Complete and still: exactly this release's Artifact nodes and edges, every node drawn
      // inside the canvas, every edge drawn as a visible line, and nothing (nodes, edges, camera) changing
      // over 300 ms.
      const drawn = (inside: boolean) => browser.waitUntil(() => browser.executeAsync<boolean, [boolean, string]>((inside, expected, done) => {
        const shown = () => {
          const canvas = document.querySelector(".current .svelte-flow")?.getBoundingClientRect();
          const nodes = [...document.querySelectorAll<HTMLElement>(".current .svelte-flow__node")];
          const set = (values: string[]) => [...new Set(values)].sort().join(" ");
          const ends = (id: string) => id.split(">").map((key) => key.split("/").pop()).join(">");
          const artifacts = set(nodes.filter((node) => node.dataset.artifact !== undefined).map((node) => node.dataset.id ?? ""));
          // The camera, the revision drawn and every node's light: none may change while it holds.
          const camera = `${document.querySelector(".current .svelte-flow__viewport")?.getAttribute("style")} ${document.querySelector<HTMLElement>('section.panel[aria-label="Main area"] .host')?.dataset.drawn} ${nodes.map((node) => `${node.dataset.id}=${node.dataset.light}`).join(",")}`;
          // Fitted out until names do not read, the graph checked above is painted as blocks.
          if (inside && nodes.length === 0 && document.querySelector(".current canvas.paint")) return `painted ${camera}`;
          if (`${artifacts}\n${set([...document.querySelectorAll<HTMLElement>(".current .svelte-flow__edge")].map((edge) => ends(edge.dataset.id ?? "")))}` !== expected) return null;
          const edges = document.querySelectorAll(".current .svelte-flow__edge").length;
          const lines = [...document.querySelectorAll<SVGPathElement>(".current .svelte-flow__edge path.svelte-flow__edge-path")].filter((line) => {
            const box = line.getBoundingClientRect();
            return !/NaN/.test(line.getAttribute("d") ?? "NaN") && (box.width > 0 || box.height > 0) && getComputedStyle(line).visibility !== "hidden";
          }).length;
          const within = !!canvas && nodes.every((node) => {
            const box = node.getBoundingClientRect();
            return box.width > 0 && (!inside || (box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom));
          });
          return within && nodes.length > 0 && lines === edges ? `${nodes.length} ${edges} ${camera}` : null;
        };
        const before = shown();
        setTimeout(() => done(before !== null && before === shown()), 300);
      }, inside, expected), { timeout: WAIT, timeoutMsg: `${step.caption}'s whole graph is drawn${inside ? " in view" : ""}` });
      // Refresh keeps the camera: once the grown graph is drawn, fit it into view the way a
      // person would.
      await drawn(false);
      await browser.$('[aria-label="Graph tools"]').$("button=Fit").click();
      await drawn(true);
      at = await corner(browser, ".current .canvas");
      const file = join(frames, `${step.caption}.png`);
      writeFileSync(file, Buffer.from(await browser.takeScreenshot(), "base64"));
      segments.push({ frames: [{ file, at: 0 }], speed: 1, caption: step.caption, hold: GIF.step });
    }
  } finally {
    try {
      await service.cleanupWdioSession(browser);
    } finally {
      // Nothing of its state keeps running once its project and the build go.
      e2e.cancelRuns(fixture);
    }
  }
  return { segments, at };
}

/** Record every demo into `output`, building and keeping frames in `work`. */
async function capture(output: string, work: string): Promise<void> {
  const base = "../../crates/artifactize-desktop/ui/e2e";
  let candidate: { root: string } | undefined;
  // The work folder goes whatever fails, the build and its setup included.
  try {
    const { adopt, prepare } = await import(`${base}/build.ts`);
    const e2e = await import(`${base}/fixtures/repo.ts`);
    const service = createRequire(join(repository, "crates/artifactize-desktop/ui/package.json"))("@wdio/tauri-service");
    mkdirSync(work, { recursive: true });
    candidate = adopt(await prepare(join(work, "build")));
    const frames = join(work, "frames");
    mkdirSync(frames);
    const loop = await reviewLoop(service, e2e, candidate, frames);
    const growth = await graphGrowth(service, e2e, candidate, frames);
    mkdirSync(output, { recursive: true });
    // Each caption sits inside the graph's canvas, clear of the Pane and the app's own header lines.
    const gifs = [[DEMOS[0], loop], [DEMOS[1], growth]] as const;
    for (const [name, { segments, at }] of gifs) {
      // Write beside the old GIF and replace it whole, so a failed encode keeps the last one.
      const gif = join(output, name);
      encode(segments, `${gif}.new.gif`, work, at);
      renameSync(`${gif}.new.gif`, gif);
    }
  } finally {
    try {
      if (candidate) {
        const { cleanup } = await import(`${base}/build.ts`);
        await cleanup(candidate.root);
      }
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
