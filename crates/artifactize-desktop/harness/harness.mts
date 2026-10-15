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
// Needs an explicit DESKTOP_TEST_CANDIDATE manifest prepared by ui/e2e/run.ts: a review
// snapshots that candidate into its session, never searches shared targets or rebuilds it.
import { createConnection, createServer, type Socket } from "node:net";
import { spawnSync } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { cancelRuns, cli, create, type Fixture } from "../ui/e2e/fixture.ts";
import { LAUNCH_TIMEOUT, startBuilt, type Session } from "../ui/e2e/driver.ts";
import { page, press, typeText } from "../ui/e2e/keys.ts";
import { cargoGate, cleanup, snapshotCandidate } from "../ui/e2e/build.ts";
import { checkpoint, commandsRendered, type Checkpoint } from "../ui/e2e/commands.ts";

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

// WebDriver's element references, kept outside the page, retain identity across reorders.
// A removed control is stale; its number must never target its replacement at the same index.
const controls = new WeakMap<WebdriverIO.Browser, ReturnType<WebdriverIO.Browser["$"]>[]>();

/** The window as text: its focus, every visible control with a number to act on, then its text. */
export async function snapshot(browser: WebdriverIO.Browser): Promise<string> {
  const shown = await browser.execute(() => {
    const references: HTMLElement[] = [];
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
      references.push(element as HTMLElement);
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
    return { text: `controls:\n${lines.join("\n")}\n\ntext:\n${document.body.innerText}`, references };
  });
  controls.set(browser, shown.references.map((reference) => browser.$(reference)));
  return shown.text;
}

/** Native completion identifies the current search even before its debounce dispatches. */
async function settled(browser: WebdriverIO.Browser, before?: Checkpoint): Promise<string> {
  await commandsRendered(browser, before, LAUNCH_TIMEOUT);
  return snapshot(browser);
}

/** A launch is ready when the fixture's actual entry-point content has rendered. */
async function rendered(browser: WebdriverIO.Browser, screen: "monitor" | "review" | "graph") {
  await browser.waitUntil(() => browser.execute((entry) => {
    if (entry === "graph") return !!document.querySelector('.svelte-flow__node[data-id="a:release"]');
    const selector = entry === "review" ? '[aria-label="Requests"] [role="option"]' : '[role="treeitem"]';
    return [...document.querySelectorAll(selector)].some((row) => row.textContent?.includes("signoff"));
  }, screen), { timeout: LAUNCH_TIMEOUT, timeoutMsg: `${screen} fixture content did not render` });
}

export async function target(browser: WebdriverIO.Browser, element: number) {
  const found = controls.get(browser)?.[element - 1];
  if (!Number.isInteger(element) || element < 1 || !found || !(await found.isExisting()) || !(await found.isDisplayed())) {
    throw new Error(`no element [${element}] from the last snapshot remains visible; take a new snapshot`);
  }
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
  const before = await checkpoint(browser);
  switch (request.op) {
    case "snapshot":
      return { text: await settled(browser, before) };
    case "click":
      await (await target(browser, request.element)).click();
      return { text: await settled(browser, before) };
    case "type":
      if (request.element !== undefined) await (await target(browser, request.element)).click();
      await typeText(browser, request.text);
      return { text: await settled(browser, before) };
    case "key":
      await pressKey(browser, request.key);
      return { text: await settled(browser, before) };
    case "cli": {
      const done = spawnSync(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, ...request.args], {
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

/** Close admission immediately, then release the latest owner after the active command settles. */
export function commandStream(close: () => void, release: () => Promise<void>) {
  let pending = Promise.resolve();
  let ending: Promise<void> | undefined;
  return {
    run<T>(operation: () => Promise<T>): Promise<T> {
      if (ending) return Promise.reject(new Error("the app session is stopping"));
      const result = pending.then(() => {
        if (ending) throw new Error("the app session is stopping");
        return operation();
      });
      // A failed command must not prevent shutdown from releasing its replacement owner.
      pending = result.then(() => {}, () => {});
      return result;
    },
    end(): Promise<void> {
      if (!ending) {
        ending = pending.then(release);
        close();
      }
      return ending;
    },
  };
}

async function start(): Promise<void> {
  if (!sessionDir) throw new Error("ARTIFACTIZE_SESSION_DIR is not set; run this as a tool session.");
  process.chdir(sessionDir);
  const candidate = snapshotCandidate(sessionDir);
  let fixture: Fixture;
  try { fixture = await create(); }
  catch (error) { await cleanup(candidate.root); throw error; }
  const exposed = new Map([[fixture.state, fixture]]);
  const removeFixture = async () => {
    for (const target of exposed.values()) cancelRuns(target);
    rmSync(fixture.root, { recursive: true, force: true });
    await cleanup(candidate.root);
  };
  let session: Session | undefined;
  try {
    session = await startBuilt(fixture);
    await rendered(session.browser, "monitor");
  } catch (error) {
    await session?.stop();
    await removeFixture();
    throw error;
  }
  let shown = { session, fixture };
  let edits = 0;
  // Signals and socket stop share admission closure and drain before reading shown.
  const commands = commandStream(() => { server.close(); }, async () => {
    await shown.session.stop();
    await removeFixture();
  });
  const end = () => commands.end();
  const server = createServer((socket: Socket) => {
    const lines = createInterface({ input: socket });
    socket.once("close", () => lines.close());
    socket.once("error", () => lines.close());
    lines.once("line", (line) => {
      lines.close();
      void (async () => {
        let reply: unknown;
        try {
          const request = JSON.parse(line) as Request;
          if (request.op === "stop") {
            await end();
            socket.end(JSON.stringify({ stopped: true }) + "\n", () => process.exit(0));
            return;
          }
          reply = await commands.run(async () => {
            if (request.op === "launch") {
              await shown.session.stop();
              const target = request.screen === "graph" ? { ...fixture, state: join(fixture.root, "graph-state") } : fixture;
              exposed.set(target.state, target);
              const args = request.screen === "graph" ? ["graph", "--repo", target.repo] : request.screen === "review" ? ["review"] : [];
              shown = { session: await startBuilt(target, ...args), fixture: target };
              await rendered(shown.session.browser, request.screen);
              return { text: await settled(shown.session.browser) };
            } else if (request.op === "change-declaration") {
              const path = join(fixture.repo, "docs", "index.artf");
              const title = `Docs criterion revision ${++edits}`;
              writeFileSync(path, readFileSync(path, "utf8").replace(/title = "[^"]*"/, `title = "${title}"`));
              return { text: `Changed docs/lint title to ${title}. The app has not been refreshed.` };
            } else {
              return handle(shown.session, shown.fixture, request);
            }
          });
        } catch (error) {
          reply = { error: error instanceof Error ? error.message : String(error) };
        }
        socket.end(JSON.stringify(reply) + "\n");
      })().catch((error: Error) => { socket.destroy(error); });
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

if (import.meta.main) {
  const mode = process.argv[2];
  if (mode === "clippy" || mode === "test") {
    await cargoGate(mode);
  } else if (mode === "start") {
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
}
