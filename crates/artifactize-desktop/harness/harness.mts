// The desktop app as an Agent tool session (declared in ../index.artf).
//
//   start     runs the real app headless on its own display, on fixture state made by the real
//             CLI, through the e2e driver (which owns Xvfb, tauri-driver and the app); prints
//             one ready line and stays alive, serving the bound tools on a socket in
//             $ARTIFACTIZE_SESSION_DIR
//   stop      ends the app and removes what start made
//   snapshot  what the window shows, as text
//   act       click an element, type text, or press a key; answers with the new snapshot
//   cli       run the artifactize CLI against the app's repository and state, from another process
//
// Needs the normal app and CLI builds (cargo build -p artifactize -p artifactize-desktop, after
// pnpm build in ui/): a review drives the prepared candidate and never rebuilds it.
import { createConnection, createServer, type Socket } from "node:net";
import { spawnSync } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { cancelRuns, cli, create, type Fixture } from "../ui/e2e/fixture.ts";
import { LAUNCH_TIMEOUT, startBuilt, type Session } from "../ui/e2e/driver.ts";
import { page, press, typeText } from "../ui/e2e/keys.ts";

const SOCKET = "control.sock";
const sessionDir = process.env.ARTIFACTIZE_SESSION_DIR;

type Request =
  | { op: "snapshot" }
  | { op: "click"; element: number }
  | { op: "type"; element?: number; text: string }
  | { op: "key"; key: string }
  | { op: "cli"; args: string[] }
  | { op: "launch"; screen: "monitor" | "review" | "graph" }
  | { op: "change-declaration" }
  | { op: "stop" };

const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** The window as text: its focus, every visible control with a number to act on, then its text. */
async function snapshot(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => {
    for (const old of document.querySelectorAll("[data-snap]")) old.removeAttribute("data-snap");
    const visible = (element: Element) => {
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return box.width > 0 && box.height > 0 && style.visibility !== "hidden" && style.display !== "none";
    };
    const name = (element: Element) =>
      element.getAttribute("aria-label") ??
      (element as HTMLElement).innerText?.replace(/\s+/g, " ").trim().slice(0, 80) ??
      "";
    const lines: string[] = [];
    let count = 0;
    const controls = "a, button, input, textarea, select, summary, [role], [tabindex], [aria-label]";
    for (const element of document.querySelectorAll(controls)) {
      if (!visible(element)) continue;
      const role = element.getAttribute("role") ?? element.tagName.toLowerCase();
      count += 1;
      element.setAttribute("data-snap", String(count));
      const states = [
        ["selected", "aria-selected"],
        ["expanded", "aria-expanded"],
        ["checked", "aria-checked"],
        ["pressed", "aria-pressed"],
        ["current", "aria-current"],
      ]
        .map(([label, attribute]) => (element.hasAttribute(attribute) ? `${label}=${element.getAttribute(attribute)}` : ""))
        .filter(Boolean);
      if ((element as HTMLButtonElement).disabled || element.getAttribute("aria-disabled") === "true") states.push("disabled");
      if (element === document.activeElement) states.push("focused");
      const value = (element as HTMLInputElement).value;
      if (typeof value === "string" && element.matches("input, textarea, select")) states.push(`value=${JSON.stringify(value)}`);
      lines.push(`[${count}] ${role} ${JSON.stringify(name(element))}${states.length ? ` (${states.join(", ")})` : ""}`);
    }
    return `controls:\n${lines.join("\n")}\n\ntext:\n${document.body.innerText}`;
  });
}

/** The snapshot once two reads in a row agree: WebKit renders asynchronously. */
async function settled(browser: WebdriverIO.Browser): Promise<string> {
  let last = await snapshot(browser);
  for (let tries = 0; tries < 20; tries += 1) {
    await pause(100);
    const next = await snapshot(browser);
    if (next === last) return next;
    last = next;
  }
  return last;
}

/** A launch is ready when the fixture's actual entry-point content has rendered. */
async function rendered(browser: WebdriverIO.Browser, screen: "monitor" | "review" | "graph") {
  await browser.waitUntil(() => browser.execute((entry) => {
    if (entry === "graph") return !!document.querySelector('.svelte-flow__node[data-id="a:release"]');
    const selector = entry === "review" ? '[aria-label="Requests"] [role="option"]' : '[role="treeitem"]';
    return [...document.querySelectorAll(selector)].some((row) => row.textContent?.includes("signoff"));
  }, screen), { timeout: LAUNCH_TIMEOUT, timeoutMsg: `${screen} fixture content did not render` });
}

async function target(browser: WebdriverIO.Browser, element: number) {
  const found = await browser.$(`[data-snap="${element}"]`);
  if (!(await found.isExisting())) throw new Error(`no element [${element}] in the last snapshot; take a new snapshot`);
  return found;
}

/** Keys as `Control+k` or `Enter`: modifiers first, one action key last. */
async function pressKey(browser: WebdriverIO.Browser, key: string): Promise<void> {
  const keys = key.split("+");
  const seen = page(browser);
  const before = (await seen.received()).length;
  await press(browser, keys, async () => (await seen.received()).length > before, `key ${key}`, { wait: 1000 });
}

async function handle(session: Session, fixture: Fixture, request: Request): Promise<unknown> {
  const { browser } = session;
  switch (request.op) {
    case "snapshot":
      return { text: await settled(browser) };
    case "click":
      await (await target(browser, request.element)).click();
      return { text: await settled(browser) };
    case "type":
      if (request.element !== undefined) await (await target(browser, request.element)).click();
      await typeText(browser, request.text);
      return { text: await settled(browser) };
    case "key":
      await pressKey(browser, request.key);
      return { text: await settled(browser) };
    case "cli": {
      const done = spawnSync(cli, ["--repo", fixture.repo, "--state-dir", fixture.state, ...request.args], {
        env: { ...process.env, USER: "alice", ARTIFACTIZE_REMOTE: "off" },
        encoding: "utf8",
        timeout: 60000,
      });
      return { exitCode: done.status, stdout: done.stdout, stderr: done.stderr };
    }
    default:
      throw new Error("unknown operation");
  }
}

async function start(): Promise<void> {
  if (!sessionDir) throw new Error("ARTIFACTIZE_SESSION_DIR is not set; run this as a tool session.");
  process.chdir(sessionDir);
  const fixture = await create();
  let session: Session | undefined;
  try {
    session = await startBuilt(fixture);
    await rendered(session.browser, "monitor");
  } catch (error) {
    try { await session?.stop(); }
    finally {
      try { cancelRuns(fixture); }
      finally { rmSync(fixture.root, { recursive: true, force: true }); }
    }
    throw error;
  }
  let shown = { session, fixture };
  let edits = 0;
  let ending: Promise<void> | undefined;
  const end = () => ending ??= (async () => {
    server.close();
    try { await shown.session.stop(); }
    finally {
      try { cancelRuns(fixture); }
      finally { rmSync(fixture.root, { recursive: true, force: true }); }
    }
  })();
  // A window has one command stream: a concurrent snapshot cannot race a relaunch.
  let pending = Promise.resolve();
  const server = createServer((socket: Socket) => {
    const lines = createInterface({ input: socket });
    socket.once("close", () => lines.close());
    socket.once("error", () => lines.close());
    lines.once("line", (line) => {
      lines.close();
      pending = pending.then(async () => {
        let reply: unknown;
        try {
          const request = JSON.parse(line) as Request;
          if (request.op === "stop") {
            await end();
            socket.end(JSON.stringify({ stopped: true }) + "\n", () => process.exit(0));
            return;
          }
          if (request.op === "launch") {
            await shown.session.stop();
            const target = request.screen === "graph" ? { ...fixture, state: join(fixture.root, "graph-state") } : fixture;
            const args = request.screen === "graph" ? ["graph", "--repo", target.repo] : request.screen === "review" ? ["review"] : [];
            shown = { session: await startBuilt(target, ...args), fixture: target };
            await rendered(shown.session.browser, request.screen);
            reply = { text: await settled(shown.session.browser) };
          } else if (request.op === "change-declaration") {
            const path = join(fixture.repo, "docs", "index.artf");
            const title = `Docs criterion revision ${++edits}`;
            writeFileSync(path, readFileSync(path, "utf8").replace(/title = "[^"]*"/, `title = "${title}"`));
            reply = { text: `Changed docs/lint title to ${title}. The app has not been refreshed.` };
          } else {
            reply = await handle(shown.session, shown.fixture, request);
          }
        } catch (error) {
          reply = { error: error instanceof Error ? error.message : String(error) };
        }
        socket.end(JSON.stringify(reply) + "\n");
      }).catch((error: Error) => { socket.destroy(error); });
    });
  });
  for (const name of ["SIGTERM", "SIGINT"] as const) process.on(name, () => void end().then(
    () => process.exit(0),
    (error: unknown) => { console.error(error); process.exit(1); },
  ));
  try {
    await new Promise<void>((done, fail) => {
      server.once("error", fail);
      server.listen(SOCKET, () => { server.off("error", fail); done(); });
    });
    console.log("ready");
    // The listening server owns the lifetime; no unowned keep-alive timer is needed.
  } catch (error) {
    await end();
    throw error;
  }
}

/** One request to the running session; the socket is relative to its directory. */
function ask(request: Request): Promise<Record<string, unknown>> {
  if (!sessionDir) throw new Error("ARTIFACTIZE_SESSION_DIR is not set; run this as a tool session.");
  process.chdir(sessionDir);
  return new Promise((done, fail) => {
    const socket = createConnection(SOCKET);
    let text = "";
    socket.on("data", (chunk) => (text += chunk));
    socket.once("error", fail);
    socket.once("close", () => {
      try {
        done(JSON.parse(text) as Record<string, unknown>);
      } catch {
        fail(new Error("the app session ended without answering"));
      }
    });
    socket.write(JSON.stringify(request) + "\n");
  });
}

async function input(): Promise<Record<string, unknown>> {
  let text = "";
  for await (const chunk of process.stdin) text += chunk;
  const envelope = JSON.parse(text) as { args?: Record<string, unknown> };
  return envelope.args ?? {};
}

function content(text: string, error = false) {
  console.log(JSON.stringify({ isError: error || undefined, content: [{ type: "text", text }] }));
}

const mode = process.argv[2];
if (mode === "start") {
  await start();
} else if (mode === "stop") {
  await ask({ op: "stop" }).catch(() => ({}));
} else {
  const args = await input();
  const request: Request =
    mode === "snapshot"
      ? { op: "snapshot" }
      : mode === "cli"
        ? { op: "cli", args: args.args as string[] }
        : mode === "launch"
          ? { op: "launch", screen: args.screen as "monitor" | "review" | "graph" }
          : mode === "change-declaration"
            ? { op: "change-declaration" }
            : args.action === "click"
              ? { op: "click", element: args.element as number }
              : args.action === "type"
                ? { op: "type", element: args.element as number | undefined, text: args.text as string }
                : { op: "key", key: args.key as string };
  const reply = await ask(request);
  if (typeof reply.error === "string") content(reply.error, true);
  else if (mode === "cli") content(`exit ${reply.exitCode}\n--- stdout\n${reply.stdout}\n--- stderr\n${reply.stderr}`, reply.exitCode !== 0);
  else content(String(reply.text));
}
