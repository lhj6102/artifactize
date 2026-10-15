// Shared Linux fixture boundary for the low-level harness tests. These scenarios
// exercise procfs/POSIX ownership, not an alternate implementation of the app.
// Missing capabilities on supported Linux fail acquisition; no setup is skipped.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, resourceRoot } from "./build.ts";
import { Owned, withCleanup } from "./owned.ts";

export const nativeUnsupported = process.platform !== "linux" && "these fixtures exercise Linux procfs birth identities and POSIX ownership";
export const moduleURL = (name: string) => new URL(name, import.meta.url).href;
/** Only bounds a real protocol observation, never infers success from elapsed time. */
const PROTOCOL_TIMEOUT = 10000;

export class NativeProcess {
  readonly owned: Owned;
  readonly closed: Promise<{ code: number | null; killed: boolean }>;
  readonly exited: Promise<void>;
  output = "";
  errors = "";
  #inputEnded = false;
  constructor(source: string | Owned, env: NodeJS.ProcessEnv, args: string[] = [], extra = false, launch?: { command: string; args: string[] }) {
    this.owned = typeof source === "string" ? Owned.spawn(launch?.command ?? process.execPath, launch?.args ?? ["--input-type=module", "-e", source, ...args], {
      env, stdio: extra ? ["pipe", "pipe", "pipe", "pipe"] : ["pipe", "pipe", "pipe"],
    }) : source;
    this.owned.child.stdout!.on("data", (chunk: Buffer) => { this.output += chunk; });
    this.owned.child.stderr!.on("data", (chunk: Buffer) => { this.errors += chunk; });
    this.exited = new Promise((done, fail) => {
      this.owned.child.once("error", fail);
      this.owned.child.once("exit", () => done());
    });
    this.closed = new Promise((done, fail) => {
      this.owned.child.once("error", fail);
      this.owned.child.once("close", (code, signal) => done({ code, killed: signal === "SIGKILL" }));
    });
    // Fast acquisition failures remain available to every later observer and stop().
    void this.exited.catch(() => {});
    void this.closed.catch(() => {});
  }
  get exitCode() { return this.owned.child.exitCode; }
  get running() { return this.owned.child.exitCode === null && this.owned.child.signalCode === null; }
  send(text: string) { this.owned.child.stdin!.write(text); }
  endInput(text?: string) {
    if (this.#inputEnded) return;
    this.#inputEnded = true;
    const input = this.owned.child.stdin;
    if (input && !input.destroyed && !input.writableEnded) input.end(text);
  }
  interrupt(kind: "terminate" | "kill") {
    if (this.running && !this.owned.child.kill(kind === "kill" ? "SIGKILL" : "SIGTERM")) throw new Error("fixture interruption was not delivered");
  }
  async line(): Promise<string> {
    await observe(() => this.output.includes("\n"), () => `fixture did not publish readiness: ${this.errors}`, this.closed.then(() => {}));
    return this.output.split("\n")[0];
  }
  stop() { return this.owned.stop(); }
}

/** Every temporary native input is under an existing candidate lease. Enclosing
 * cleanup finds its sibling receipt even if this test process dies before finally. */
export class NativeScratch {
  readonly owner: string;
  readonly root: string;
  readonly processes: NativeProcess[] = [];
  readonly #roots: string[] = [];
  constructor(owner: string) {
    this.owner = owner;
    this.root = join(this.owner, "inputs");
    mkdirSync(this.root);
  }
  childRoot() {
    const root = resourceRoot(join(this.owner, "tmp"));
    this.#roots.push(root);
    return root;
  }
  path(name: string) { return join(this.root, name); }
  exists(name = "") { return existsSync(this.path(name)); }
  read(name: string) { return readFileSync(this.path(name), "utf8"); }
  write(name: string, text: string) { writeFileSync(this.path(name), text); }
  env(overrides: NodeJS.ProcessEnv = {}): NodeJS.ProcessEnv {
    const env: NodeJS.ProcessEnv = { ...process.env, DESKTOP_TEST_RESOURCES: this.owner, TMPDIR: join(this.owner, "tmp"), ...overrides };
    delete env.NODE_TEST_CONTEXT;
    return env;
  }
  node(source: string, overrides: NodeJS.ProcessEnv = {}, args: string[] = [], extra = false): NativeProcess {
    const child = new NativeProcess(source, this.env(overrides), args, extra);
    this.processes.push(child);
    return child;
  }
  async dispose(): Promise<void> {
    // Attempt every independent stop, but never release scratch after any failed join.
    await withCleanup(() => {}, ...this.processes.map((child) => () => child.stop()));
    await withCleanup(() => {}, ...this.#roots.map((root) => () => cleanup(root)));
    await cleanup(this.owner);
  }
}

/** Acquisition failure still joins every process started before setup threw. */
export async function nativeFixture<T>(setup: (scratch: NativeScratch) => T | Promise<T>): Promise<T> {
  if (nativeUnsupported) throw new Error(nativeUnsupported);
  const owner = resourceRoot();
  let scratch: NativeScratch | undefined;
  try {
    scratch = new NativeScratch(owner);
    return await setup(scratch);
  } catch (error) {
    return withCleanup(() => { throw error; }, () => scratch ? scratch.dispose() : cleanup(owner));
  }
}

/** Hold a real detached cleanup after its child closed, independently of signal
 * grace/scheduler speed. Its sibling receipt must join the final output and exit. */
export function holdCleanupOutput(scratch: NativeScratch) {
  const bin = scratch.path("cleanup-bin");
  const reported = scratch.path("cleanup-returned");
  const released = scratch.path("release-final-output");
  mkdirSync(bin);
  writeFileSync(join(bin, "sh"), `#!${process.execPath}
const { spawn } = require("node:child_process");
const { writeFileSync, existsSync } = require("node:fs");
const child = spawn("sh", process.argv.slice(2), { env: { ...process.env, PATH: ${JSON.stringify(process.env.PATH)} }, stdio: "inherit" });
child.once("error", (error) => { console.error(error); process.exit(1); });
child.once("close", (code) => {
  writeFileSync(${JSON.stringify(reported)}, "cleanup process exited");
  setInterval(() => {
    if (!existsSync(${JSON.stringify(released)})) return;
    writeFileSync(1, "detached final output\\n");
    process.exit(code ?? 1);
  }, 5);
});
`, { mode: 0o700 });
  return { env: { PATH: bin },
    reported: () => observe(() => existsSync(reported), () => "nested cleanup did not report"),
    release() { writeFileSync(released, "release"); },
  };
}

export async function observe(check: () => boolean, failure: () => string, ended?: Promise<void>): Promise<void> {
  const deadline = Date.now() + PROTOCOL_TIMEOUT;
  let endedError: unknown;
  void ended?.then(() => { endedError = new Error(failure()); }, (error: unknown) => { endedError = error; });
  for (;;) {
    if (check()) return;
    if (endedError) throw endedError;
    if (Date.now() >= deadline) throw new Error(failure());
    await new Promise((done) => setTimeout(done, 10));
  }
}
