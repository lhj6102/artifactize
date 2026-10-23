// The images rendered from this folder's HTML figures.
//
//   node assets/others/capture.mts         render every figure in every language into this folder
//   node assets/others/capture.mts check   render them again elsewhere and compare with the saved ones
//   node assets/others/capture.mts gif     a tool: a saved GIF's length, or its frame about `at` seconds in
//
// Each figure is an HTML page in this folder that takes ?lang=; graph-example.html in en, ko and
// zh becomes graph-example.en.png, graph-example.ko.png and graph-example.zh.png. roles.html
// moves: in English only, it becomes the looping roles.en.gif. The README and the landing page
// use those images, never the HTML.
//
// Linux only, like the other capture scripts. Rendering uses headless Google Chrome (or Chromium; CHROME names another binary) through the
// DevTools protocol, with no packages to install. A small local server serves this folder, the
// landing page's brand fonts at /fonts/, as the landing page does, and the brand images at /brand/. The page sets
// data-ready once its fonts are loaded; a still is taken then, at twice the figure's size. A
// moving figure draws itself with figure.frame(t) as a pure function of time: it is stopped, one
// picture is taken for each frame of its loop, and ffmpeg turns them into the GIF.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const fonts = join(here, "..", "..", "website", "landing", "fonts");
const brand = join(here, "..", "brand");
/** The figures and the languages each is rendered in; a moving one names its GIF's frame rate
 * and width. */
const FIGURES: { page: string; width: number; height: number; languages: string[]; motion?: { fps: number; width: number } }[] = [
  { page: "graph-example", width: 1600, height: 900, languages: ["en", "ko", "zh"] },
  { page: "roles", width: 1440, height: 600, languages: ["en"], motion: { fps: 20, width: 1200 } },
];
const SCALE = 2;
const WAIT = 30000;
const TYPES: Record<string, string> = { ".html": "text/html", ".css": "text/css", ".woff2": "font/woff2", ".svg": "image/svg+xml" };

/** Serve this folder at /, the brand fonts at /fonts/ and the brand images at /brand/ on a free
 * local port. */
async function serve(): Promise<{ server: Server; origin: string }> {
  const server = createServer((request, response) => {
    const path = normalize(decodeURIComponent(new URL(request.url ?? "/", "http://localhost").pathname));
    const file = path.startsWith("/fonts/") ? join(fonts, path.slice("/fonts/".length))
      : path.startsWith("/brand/") ? join(brand, path.slice("/brand/".length))
      : join(here, path);
    try {
      const body = readFileSync(file);
      response.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
      response.end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("the local server has no port");
  return { server, origin: `http://127.0.0.1:${address.port}` };
}

/** The Chrome binary: CHROME, else the first of the usual names on PATH. */
function chromeBinary(): string {
  if (process.env.CHROME) return process.env.CHROME;
  for (const name of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    if (spawnSync("sh", ["-c", `command -v ${name}`]).status === 0) return name;
  }
  throw new Error("No Chrome found: install Google Chrome or Chromium, or set CHROME to its binary.");
}

/** One DevTools connection to a page: send a command, or wait for an event. */
class Page {
  private next = 1;
  private pending = new Map<number, { done: (result: any) => void; fail: (error: Error) => void }>();
  private waiting = new Map<string, () => void>();
  private socket: WebSocket;
  constructor(socket: WebSocket) {
    this.socket = socket;
    socket.addEventListener("message", (message) => {
      const data = JSON.parse(String(message.data));
      if (data.id && this.pending.has(data.id)) {
        const { done, fail } = this.pending.get(data.id)!;
        this.pending.delete(data.id);
        if (data.error) fail(new Error(`${data.error.message} (${data.error.code})`));
        else done(data.result);
      } else if (data.method && this.waiting.has(data.method)) {
        this.waiting.get(data.method)!();
        this.waiting.delete(data.method);
      }
    });
  }
  send(method: string, params: object = {}): Promise<any> {
    const id = this.next++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((done, fail) => this.pending.set(id, { done, fail }));
  }
  event(method: string): Promise<void> {
    return new Promise((done) => this.waiting.set(method, done));
  }
}

/** Start headless Chrome on a profile of its own and connect to its first page. Chrome puts its
 * singleton socket in TMPDIR, whose path must fit a Unix socket address; a deeper one, such as
 * a verify Run's private temporary folder, makes it exit at once, so it then gets /tmp. */
async function chrome(profile: string): Promise<{ browser: ChildProcess; page: Page; socket: WebSocket }> {
  const browser = spawn(chromeBinary(), [
    "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`,
    "--no-first-run", "--no-default-browser-check", "--disable-gpu", "--hide-scrollbars",
    "--force-color-profile=srgb", "--font-render-hinting=none", "about:blank",
  ], { stdio: ["ignore", "ignore", "pipe"], env: { ...process.env, TMPDIR: tmpdir().length > 60 ? "/tmp" : tmpdir() } });
  // Chrome prints its DevTools address on stderr once it listens.
  const address = await new Promise<string>((done, fail) => {
    let output = "";
    const timer = setTimeout(() => fail(new Error(`Chrome did not start:\n${output}`)), WAIT);
    browser.stderr!.on("data", (chunk) => {
      output += chunk;
      const found = /DevTools listening on (ws:\/\/[^\s]+)/.exec(output);
      if (found) { clearTimeout(timer); done(found[1]); }
    });
    browser.once("exit", () => { clearTimeout(timer); fail(new Error(`Chrome exited:\n${output}`)); });
  });
  const { port } = new URL(address);
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json() as { type: string; webSocketDebuggerUrl: string }[];
  const socket = new WebSocket(targets.find((target) => target.type === "page")!.webSocketDebuggerUrl);
  await new Promise((done, fail) => { socket.onopen = done; socket.onerror = () => fail(new Error("no DevTools connection")); });
  return { browser, page: new Page(socket), socket };
}

/** Run `command`, failing with its output when it fails. */
function run(command: string, args: string[]): void {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error) throw new Error(`${command} could not start: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} failed:\n${result.stderr}`);
}

/** Film the moving figure on `page` into the looping GIF `gif`: stop it, draw and photograph each
 * frame of its loop, then let ffmpeg build one palette for the whole loop and encode with it. */
async function film(page: Page, motion: { fps: number; width: number }, gif: string): Promise<void> {
  const work = mkdtempSync(join(tmpdir(), "artifactize-figure-frames-"));
  try {
    const seconds = await page.send("Runtime.evaluate", { expression: "figure.stop(), figure.seconds", returnByValue: true });
    const frames = Math.round(seconds.result.value * motion.fps);
    mkdirSync(join(work, "frames"));
    for (let index = 0; index < frames; index++) {
      await page.send("Runtime.evaluate", { expression: `figure.frame(${index / motion.fps})` });
      const shot = await page.send("Page.captureScreenshot", { format: "png" });
      writeFileSync(join(work, "frames", `${String(index).padStart(4, "0")}.png`), Buffer.from(shot.data, "base64"));
    }
    const input = ["-framerate", String(motion.fps), "-i", join(work, "frames", "%04d.png")];
    const scaled = `scale=${motion.width}:-1:flags=lanczos`;
    const palette = join(work, "palette.png");
    // One thread each, so the same frames always give the same bytes.
    run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-threads", "1", ...input,
      "-vf", `${scaled},palettegen=max_colors=256:stats_mode=full:reserve_transparent=0`, "-update", "1", palette]);
    run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-threads", "1", ...input, "-i", palette,
      "-filter_complex", `[0:v]${scaled}[v];[v][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle`,
      "-loop", "0", gif]);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

/** Render every figure in every language into `output`; answer the files written. */
async function render(output: string): Promise<string[]> {
  const profile = mkdtempSync(join(tmpdir(), "artifactize-figures-chrome-"));
  const { server, origin } = await serve();
  const { browser, page, socket } = await chrome(profile);
  const written: string[] = [];
  try {
    await page.send("Page.enable");
    for (const figure of FIGURES) {
      // A GIF is scaled down from its frames anyway: they are taken at the figure's own size.
      const scale = figure.motion ? 1 : SCALE;
      await page.send("Emulation.setDeviceMetricsOverride", { width: figure.width, height: figure.height, deviceScaleFactor: scale, mobile: false });
      for (const language of figure.languages) {
        const loaded = page.event("Page.loadEventFired");
        await page.send("Page.navigate", { url: `${origin}/${figure.page}.html?lang=${language}` });
        await loaded;
        const ready = await page.send("Runtime.evaluate", {
          expression: `new Promise((done, fail) => { const start = Date.now(); (function wait() {
            if (document.documentElement.dataset.ready) done(document.documentElement.dataset.ready);
            else if (Date.now() - start > ${WAIT}) fail(new Error("the page never became ready"));
            else setTimeout(wait, 50); })(); })`,
          awaitPromise: true,
          returnByValue: true,
        });
        if (ready.exceptionDetails) throw new Error(`${figure.page}.html?lang=${language}: ${ready.exceptionDetails.exception?.description ?? ready.exceptionDetails.text}`);
        if (figure.motion) {
          const name = `${figure.page}.${language}.gif`;
          await film(page, figure.motion, join(output, name));
          written.push(name);
          continue;
        }
        const shot = await page.send("Page.captureScreenshot", { format: "png" });
        const name = `${figure.page}.${language}.png`;
        writeFileSync(join(output, name), Buffer.from(shot.data, "base64"));
        written.push(name);
      }
    }
  } finally {
    socket.close();
    browser.kill();
    server.close();
    await new Promise((done) => browser.once("exit", done));
    rmSync(profile, { recursive: true, force: true });
  }
  return written;
}

/** Render into a temporary folder and compare each image with the saved one. */
async function check(): Promise<number> {
  const fresh = mkdtempSync(join(tmpdir(), "artifactize-figures-"));
  try {
    const stale = (await render(fresh)).filter((name) => {
      let saved: Buffer;
      try { saved = readFileSync(join(here, name)); } catch { return true; }
      return !saved.equals(readFileSync(join(fresh, name)));
    });
    if (stale.length === 0) {
      console.log("Every saved image matches its HTML.");
      return 0;
    }
    console.error(`These images differ from what their HTML renders now: ${stale.join(", ")}.`);
    console.error("Run node assets/others/capture.mts to render them again.");
    return 1;
  } finally {
    rmSync(fresh, { recursive: true, force: true });
  }
}

/** The `gif` tool: without `at`, a saved GIF's length and frame count; with `at`, its frame about
 * `at` seconds in, as a PNG. */
function gif(): void {
  const request = JSON.parse(readFileSync(0, "utf8")) as { args: { name: string; at?: number } };
  const { name, at } = request.args;
  const gifs = FIGURES.filter((figure) => figure.motion).flatMap((figure) => figure.languages.map((language) => `${figure.page}.${language}.gif`));
  const answer = (content: object[], isError = false) => console.log(JSON.stringify(isError ? { isError, content } : { content }));
  if (!gifs.includes(name)) {
    answer([{ type: "text", text: `No GIF ${name}; choose one of ${gifs.join(", ")}.` }], true);
    return;
  }
  const file = join(here, name);
  const probe = spawnSync("ffprobe", ["-v", "error", "-count_frames", "-select_streams", "v:0",
    "-show_entries", "stream=width,height,nb_read_frames:format=duration", "-of", "csv=p=0", file], { encoding: "utf8" });
  if (probe.status !== 0) throw new Error(`ffprobe could not read ${name}: ${probe.stderr}`);
  const [width, height, frames, seconds] = probe.stdout.trim().split(/[,\n]/);
  const length = Number(seconds);
  if (at === undefined) {
    answer([{ type: "text", text: `${name}: ${length.toFixed(1)} s loop, ${frames} frames, ${width}×${height}. Ask for a frame with at (seconds).` }]);
    return;
  }
  if (at < 0 || at > length) {
    answer([{ type: "text", text: `${name} is ${length.toFixed(1)} s long; choose at from 0 to ${length.toFixed(1)}.` }], true);
    return;
  }
  const frame = spawnSync("ffmpeg", ["-v", "error", "-ss", String(at), "-i", file, "-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"], { maxBuffer: 1 << 26 });
  if (frame.status !== 0 || frame.stdout.length === 0) throw new Error(`ffmpeg could not read ${name} at ${at} s`);
  answer([{ type: "text", text: `${name}, at ${at} s of ${length.toFixed(1)} s.` }, { type: "image", mimeType: "image/png", data: frame.stdout.toString("base64") }]);
}

const mode = process.argv[2];
if (mode === "check") {
  process.exitCode = await check();
} else if (mode === "gif") {
  gif();
} else if (mode === undefined) {
  for (const name of await render(here)) console.log(`rendered assets/others/${name}`);
} else {
  console.error(`Unknown mode ${mode}: run with no mode to render, with check to compare, or with gif as a tool.`);
  process.exitCode = 2;
}
