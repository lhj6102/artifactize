#!/usr/bin/env node
// Read-only production observations. The exported boundary also accepts owned stand-ins.
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

const execute = promisify(execFile);
const CRATES = ["artifactize", "artifactize-core", "artifactize-tools", "artifactize-mcp"];
const PAGES = ["", "ko/", "docs/", "docs/ko/", "install.sh", "install.ps1"];
// Leave time for the enclosing 30-second eval to save an address-specific diagnostic.
const OBSERVATION_MS = 25000;
const BODY_BYTES = 8 * 1024 * 1024;
const DEFAULTS = {
  origin: "origin",
  source: "https://raw.githubusercontent.com/lhj6102/artifactize/",
  index: "https://index.crates.io/",
  crates: "https://static.crates.io/crates/",
};
const ACTIONS = ["website-config", "website-behavior", "crates-config", "crates-behavior", "release-config", "release-behavior"];
const base = (value) => value.endsWith("/") ? value : `${value}/`;
const address = (root, path) => new URL(path, base(root)).href;
const failure = (url, expected, actual) => new Error(`${url}: expected ${expected}; ${actual}`);

function version(text) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(text);
  if (!match || [match[4], text.split("+")[1]].some(part => part !== undefined && part.split(".").some(identifier => !identifier))
      || match[4]?.split(".").some(identifier => /^0\d+$/.test(identifier))) {
    throw new Error(`Invalid semantic version ${JSON.stringify(text)}`);
  }
  return { release: match.slice(1, 4).map(BigInt), pre: match[4]?.split(".") };
}
function compare(left, right) {
  const a = version(left), b = version(right);
  for (let index = 0; index < 3; index++) {
    if (a.release[index] !== b.release[index]) return a.release[index] < b.release[index] ? -1 : 1;
  }
  if (!a.pre || !b.pre) return a.pre ? -1 : b.pre ? 1 : 0;
  for (let index = 0; index < Math.max(a.pre.length, b.pre.length); index++) {
    const x = a.pre[index], y = b.pre[index];
    if (x === y) continue;
    if (x === undefined || y === undefined) return x === undefined ? -1 : 1;
    const numericX = /^\d+$/.test(x), numericY = /^\d+$/.test(y);
    if (numericX && numericY) return BigInt(x) < BigInt(y) ? -1 : 1;
    if (numericX !== numericY) return numericX ? -1 : 1;
    return x < y ? -1 : 1;
  }
  return 0;
}
function machineTarget() {
  const arch = ({ x64: "x86_64", arm64: "aarch64" })[process.arch];
  if (!arch) throw new Error(`install.sh has no archive for CPU ${process.arch}`);
  // These are install.sh's actual choices, including musl on every Linux system.
  const os = ({ linux: "unknown-linux-musl", darwin: "apple-darwin" })[process.platform];
  if (!os) throw new Error(`install.sh has no archive for platform ${process.platform}`);
  return `${arch}-${os}`;
}

/** Observe one declared condition; inputs are service addresses and the origin to compare.
 * No worktree version, API token, deployment command or mutation enters this boundary. */
export async function observe(action, supplied) {
  if (!ACTIONS.includes(action)) throw new Error(`Unknown observation ${action}`);
  const inputs = { ...DEFAULTS, ...supplied };
  const signal = AbortSignal.timeout(OBSERVATION_MS);
  async function response(url, consume = true) {
    let response;
    try { response = await fetch(url, { signal, headers: { "user-agent": "release-observer" } }); }
    catch (error) { throw failure(url, "HTTP 200", error.message); }
    if (response.status !== 200) {
      await response.body?.cancel();
      throw failure(url, "HTTP 200", `HTTP ${response.status}`);
    }
    if (!consume) {
      await response.body?.cancel();
      return response;
    }
    const chunks = [];
    let bytes = 0;
    for await (const chunk of response.body) {
      bytes += chunk.length;
      if (bytes > BODY_BYTES) throw failure(url, `at most ${BODY_BYTES} response bytes`, "response is too large");
      chunks.push(chunk);
    }
    return { url: response.url, bytes: Buffer.concat(chunks) };
  }
  async function origin() {
    let stdout;
    try {
      ({ stdout } = await execute("git", ["ls-remote", "--tags", inputs.origin, "refs/tags/*"],
        { signal, maxBuffer: BODY_BYTES, encoding: "utf8" }));
    } catch (error) { throw failure(`git ls-remote ${inputs.origin}`, "origin refs", error.message); }
    const rows = stdout.trim().split(/\r?\n/).map((line) => line.split(/\s+/));
    const tags = rows.map(([, ref]) => /^refs\/tags\/v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))$/.exec(ref)?.[1]).filter(Boolean);
    const newest = tags.sort(compare).at(-1);
    if (!newest) throw failure(inputs.origin, "at least one stable vMAJOR.MINOR.PATCH release tag", "none found");
    return { newest };
  }
  if (action === "website-behavior") {
    for (const page of PAGES) await response(address(inputs.target, page), false);
    return `${inputs.target}: all six website routes answer HTTP 200`;
  }
  if (action === "website-config") {
    let stdout;
    try {
      ({ stdout } = await execute("git", ["ls-remote", inputs.origin, "refs/heads/main"], { signal, maxBuffer: BODY_BYTES, encoding: "utf8" }));
    } catch (error) { throw failure(inputs.origin, "origin/main", error.message); }
    const sha = /^([a-f0-9]{40}|[a-f0-9]{64})\s+refs\/heads\/main\s*$/.exec(stdout.trim())?.[1];
    if (!sha) throw failure(inputs.origin, "origin/main's commit", "no matching ref");
    for (const name of ["install.sh", "install.ps1"]) {
      const served = address(inputs.target, name);
      const expected = address(inputs.source, `${sha}/website/landing/${name}`);
      const live = await response(served);
      const source = await response(expected);
      if (!live.bytes.equals(source.bytes)) throw failure(served, `the exact bytes of ${expected}`, "served installer differs from origin/main");
    }
    return `${inputs.target}: both served installers equal origin/main ${sha}`;
  }
  const newest = action.endsWith("-config") ? (await origin()).newest : undefined;
  if (action.startsWith("crates-")) {
    for (const name of CRATES) {
      const index = address(inputs.index, `${name.slice(0, 2)}/${name.slice(2, 4)}/${name}`);
      const data = (await response(index)).bytes.toString("utf8");
      let releases;
      try {
        releases = data.trim().split(/\r?\n/).map((line) => JSON.parse(line)).map((entry) => {
          if (typeof entry.vers !== "string" || typeof entry.yanked !== "boolean") throw new Error("index entry needs vers and yanked");
          version(entry.vers);
          return entry;
        });
      } catch (error) { throw failure(index, "newline-delimited crate version records", error.message); }
      const latest = releases.filter((entry) => !entry.yanked).map((entry) => entry.vers).sort(compare).at(-1);
      if (!latest) throw failure(index, "a non-yanked crate version", "none found");
      if (action === "crates-config") {
        if (latest !== newest) throw failure(index, `latest non-yanked version ${newest} (origin's latest stable tag)`, `found ${latest}`);
      } else {
        await response(address(inputs.crates, `${name}/${name}-${latest}.crate`), false);
      }
    }
    return `${inputs.target}: ${action === "crates-config" ? `all five versions equal origin v${newest}` : "all five newest non-yanked crate files answer HTTP 200"}`;
  }
  const release = await response(inputs.target, false);
  const tag = decodeURIComponent(new URL(release.url).pathname.split("/").at(-1));
  if (action === "release-config") {
    if (tag !== `v${newest}`) throw failure(inputs.target, `latest release v${newest} (origin's latest stable tag)`, `redirected to ${release.url}`);
    return `${inputs.target}: latest release is origin v${newest}`;
  }
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw failure(inputs.target, "a stable release tag URL", `redirected to ${release.url}`);
  const download = new URL(release.url);
  download.pathname = download.pathname.replace(/\/tag\/[^/]+$/, `/download/${tag}/`);
  const target = inputs.machine ?? machineTarget();
  const archive = `artifactize-${tag}-${target}.tar.gz`;
  for (const file of [archive, `${archive}.sha256`]) await response(address(download.href, file), false);
  return `${inputs.target}: ${archive} and its checksum answer HTTP 200`;
}

// Self-contained public-boundary tests: real HTTP and a fixture-owned origin, no production.
async function selftest() {
  const { default: assert } = await import("node:assert/strict");
  const { createServer } = await import("node:http");
  const { mkdtemp, mkdir, writeFile, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const root = await mkdtemp(join(tmpdir(), "artifactize-observer-"));
  const repo = join(root, "origin");
  let server;
  try {
    await mkdir(repo);
    const git = async (...args) => (await execute("git", ["-C", repo, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", ...args], { timeout: OBSERVATION_MS, encoding: "utf8" })).stdout;
    await git("init", "--initial-branch=main");
    await writeFile(join(repo, "input.txt"), "fixture origin\n");
    await git("add", "input.txt");
    await git("commit", "-qm", "Fixture origin");
    await git("tag", "v1.2.3");
    await git("tag", "v9.0.0-preview");
    const sha = (await git("rev-parse", "HEAD")).trim();
    let fault;
    const names = ["artifactize", "artifactize-core", "artifactize-tools", "artifactize-mcp"];
    server = createServer((request, response) => {
      const path = new URL(request.url, "http://fixture").pathname;
      if (fault?.type === "404" || fault?.type === "mismatch" && fault.action.endsWith("-behavior")) {
        response.writeHead(fault.type === "404" ? 404 : 503).end("unavailable"); return;
      }
      if (path === "/releases/latest") {
        response.writeHead(302, { location: `/releases/tag/${fault?.action === "release-config" ? "v0.0.1" : "v1.2.3"}` }).end(); return;
      }
      if (path.startsWith("/releases/tag/")) { response.end("release"); return; }
      if (path.startsWith("/releases/download/")) { response.end("owned release asset"); return; }
      if (path.startsWith("/index/ar/ti/")) {
        const name = path.split("/").at(-1);
        if (!names.includes(name)) { response.writeHead(404).end(); return; }
        const latest = fault?.action === "crates-config" ? "9.9.9" : "1.2.3";
        response.end([{ vers: "0.9.0", yanked: false }, { vers: latest, yanked: false }, { vers: "1.2.3-dev.1", yanked: false }, { vers: "99.0.0", yanked: true }].map(entry => JSON.stringify(entry)).join("\n")); return;
      }
      if (path.startsWith("/crates/") && path.endsWith("-1.2.3.crate")) { response.end("owned crate"); return; }
      if (path.startsWith(`/raw/${sha}/website/landing/`)) { response.end("installer bytes\n"); return; }
      if (path === "/website/install.sh" || path === "/website/install.ps1") {
        response.end(fault?.action === "website-config" ? "different installer\n" : "installer bytes\n"); return;
      }
      if (["/website/", "/website/ko/", "/website/docs/", "/website/docs/ko/"].includes(path)) { response.end("owned page"); return; }
      response.writeHead(404).end();
    });
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
    const endpoint = `http://127.0.0.1:${server.address().port}`;
    const common = { origin: repo, source: `${endpoint}/raw/`, index: `${endpoint}/index/`, crates: `${endpoint}/crates/`, machine: "x86_64-unknown-linux-musl" };
    const cases = [
      ["website-config", `${endpoint}/website/`], ["website-behavior", `${endpoint}/website/`],
      ["crates-config", `${endpoint}/crates/artifactize`], ["crates-behavior", `${endpoint}/crates/artifactize`],
      ["release-config", `${endpoint}/releases/latest`], ["release-behavior", `${endpoint}/releases/latest`],
    ];
    for (const [action, target] of cases) {
      fault = undefined;
      assert.match(await observe(action, { ...common, target }), /all|both|latest|checksum/);
      for (const type of ["mismatch", "404"]) {
        fault = { action, type };
        await assert.rejects(observe(action, { ...common, target }), error => error.message.includes(endpoint) && error.message.includes("expected"), `${action} names its failed address for ${type}`);
      }
    }
    await new Promise(resolve => { server.close(resolve); server.closeAllConnections(); });
    server = undefined;
    for (const [action, target] of cases) {
      await assert.rejects(observe(action, { ...common, target }), error => error.message.includes(endpoint) && error.message.includes("expected HTTP 200"), `${action} reports an unreachable address`);
    }
    console.log("All six observations passed against matching stand-ins and rejected mismatches, 404s and unreachable hosts.");
  } finally {
    if (server) await new Promise(resolve => { server.close(resolve); server.closeAllConnections(); });
    await rm(root, { recursive: true, force: true });
  }
}

async function main() {
  const [action, target, origin = "origin"] = process.argv.slice(2);
  if (action === "test") { await selftest(); return; }
  if (!target) throw new Error(`Usage: node observe-live.mjs ${ACTIONS.join("|")} TARGET [ORIGIN]`);
  console.log(await observe(action, { target, origin }));
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { await main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
