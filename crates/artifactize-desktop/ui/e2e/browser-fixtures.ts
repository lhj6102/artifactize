// The system browser a case's app hands addresses to: the fixture's own loopback endpoint that
// every opener reports to, the addresses and file it may be handed, what it was handed, and the
// processes it ran. How the desktop's opener is found and how a real disposable browser starts
// are the platform layer's (installBrowserOpener, nativeBrowser).
import { awaitOpenersEnded, canonicalPath, installBrowserOpener, isInside, nativeBrowser, processRunning, type Environment } from "./platform.ts";
import { appendText, asNativePath, exists, listFiles, makeDirectory, nativePath, readText, readTextIfExists, replaceText, serveLoopbackHttp, writeText, type NativePath } from "./portable.ts";

/** Some origins are known at setup, others are announced by the owned launcher. */
export type BrowserEntry = string | { dynamic: true } | { file: NativePath };

const handoffLog = (root: NativePath) => nativePath(root, "browser-handoffs.jsonl");
const downloadsOf = (root: NativePath) => nativePath(root, "browser-downloads");
const jobsOf = (root: NativePath) => nativePath(root, "browser-jobs");

/** URLs received by this fixture's desktop opener, independently of visits to the site. */
export function browserHandoffs(root: NativePath): string[] {
  return (readTextIfExists(handoffLog(root)) ?? "").split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);
}

/** The opener received `url` after the first `after` handoffs, and its browser has the whole
 * transfer: a real browser recorded the page as loaded or saved the complete download; the
 * stand-in records a handoff only once it has consumed the complete HTTP body. */
export function browserTransferComplete(root: NativePath, url: string, after: number, transfer:
  { pageLoaded: NativePath } | { download: string; body: string }): boolean {
  if (!browserHandoffs(root).slice(after).includes(url)) return false;
  if (!exists(downloadsOf(root))) return true;
  if ("pageLoaded" in transfer) return exists(transfer.pageLoaded);
  return readTextIfExists(nativePath(downloadsOf(root), transfer.download)) === transfer.body;
}

/** Whether a process this fixture's browser recorded (an opener, a browser's job) still runs. */
function browserJobsAlive(root: NativePath): boolean {
  const owners = jobsOf(root);
  return exists(owners) && listFiles(owners).filter((file) => file.endsWith(".pid")).some((file) => {
    const pid = Number(readText(file));
    if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error(`invalid browser job record ${file}`);
    return processRunning(pid);
  });
}

/** A restarted cleanup may delete browser inputs only after every recorded process has exited. */
export async function releaseRecordedBrowser(root: NativePath): Promise<void> {
  if (browserJobsAlive(root)) throw new Error(`a native browser job still owns ${root}`);
}

/** The address a handoff page sends its browser to: its owned redirect on the fixture's site. */
function redirectOf(file: NativePath): URL {
  const redirect = readText(file).match(/content="0;url=(http:\/\/127\.0\.0\.1:[0-9]+\/file-opened)"/i)?.[1];
  if (!redirect) throw new Error("fixture handoff page has no owned redirect");
  return new URL(redirect);
}

/** The system browser of a case that opens `entry`: an address on its private loopback server,
 * one below it, or the fixture's own handoff file. Every opener the app starts reports what it
 * was handed here; a real disposable browser then opens it, or else this stand-in performs the
 * real HTTP GET itself. */
export async function browserEnvironment(root: NativePath, env: Environment, entry: BrowserEntry): Promise<{
  env: Environment; release: () => Promise<void>;
}> {
  const file = typeof entry !== "string" && "file" in entry ? entry.file : undefined;
  // Containment by canonical paths: a link under the fixture cannot make an outside file its own.
  if (file && !isInside(canonicalPath(root), canonicalPath(file))) throw new Error("desktop handoff file must be owned by the fixture");
  const real = nativeBrowser(root, env, downloadsOf(root));
  const client = nativePath(root, "browser-client.mjs");
  const entries = nativePath(root, "browser-entries.jsonl");
  const owners = makeDirectory(jobsOf(root), { private: true });
  const stop = new AbortController();
  const released = new Error("fixture browser released");
  const requests: Promise<void>[] = [];
  const jobs: Promise<void>[] = [];
  const server = await serveLoopbackHttp((request, response) => {
    const opener = Number(request.headers["x-opener-pid"]);
    if (request.method !== "POST" || request.url !== "/open" || stop.signal.aborted || !Number.isSafeInteger(opener) || opener <= 0) {
      response.writeHead(404).end();
      return;
    }
    const handled = (async () => {
      replaceText(nativePath(owners, `opener-${opener}.pid`), String(opener));
      let target = "";
      for await (const chunk of request) {
        target += String(chunk);
        if (target.length > 16384) throw new Error("browser request is too long");
      }
      // Closing the listener interrupts bodies in flight: none still being read at the
      // teardown boundary starts a browser.
      stop.signal.throwIfAborted();
      if (file) {
        if (canonicalPath(asNativePath(target)) !== canonicalPath(file)) throw new Error("the handoff did not name the owned file");
      } else {
        const address = new URL(target);
        const agreed = typeof entry === "string" ? new URL(entry).origin === address.origin
          : (readTextIfExists(entries) ?? "").split("\n").includes(address.href);
        if (address.protocol !== "http:" || address.hostname !== "127.0.0.1" || !agreed) throw new Error("the handoff did not name the owned browser origin");
      }
      if (real) {
        if (!file) appendText(handoffLog(root), `${JSON.stringify(target)}\n`);
        const job = real.launch(file ? canonicalPath(file) : target, nativePath(owners, `job-${jobs.length}.pid`), stop.signal)
          .catch((error: unknown) => { if (error !== released) throw error; });
        jobs.push(job);
        void job.catch(() => {}); // joined by release
      } else {
        const reply = await fetch(file ? redirectOf(file) : new URL(target), { signal: AbortSignal.any([stop.signal, AbortSignal.timeout(30000)]) });
        await reply.arrayBuffer();
        if (!reply.ok) throw new Error(`browser HTTP ${reply.status}`);
        if (!file) appendText(handoffLog(root), `${JSON.stringify(target)}\n`);
      }
      response.writeHead(204).end();
    })().catch((error: unknown) => {
      // A real browser's request cut off by teardown started nothing; a stand-in's transfer cut
      // off by it is a handoff that did not complete.
      if (real && (error === released || (stop.signal.aborted && !request.complete))) return;
      if (!response.headersSent && !response.destroyed) response.writeHead(500).end(String(error));
      throw error;
    });
    requests.push(handled);
    void handled.catch(() => {}); // joined by the owning fixture even if the app ignores opener failure
  });
  let opener: ReturnType<typeof installBrowserOpener>;
  try {
    writeText(client, `const reply = await fetch(${JSON.stringify(`${server.url}/open`)}, {
  method: "POST", headers: { "x-opener-pid": String(process.pid) }, body: process.argv[2],
  signal: AbortSignal.timeout(30000),
});
if (!reply.ok) throw new Error(await reply.text());
`);
    opener = installBrowserOpener(root, env, client);
  } catch (error) {
    await server.close();
    throw error;
  }
  return {
    env: { ...opener.env, DESKTOP_TEST_BROWSER_ENTRIES: entries },
    release: async () => {
      stop.abort(released);
      const outcomes = await Promise.allSettled([server.close(), ...requests, ...jobs]);
      const failures = outcomes.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
      try { await awaitOpenersEnded(root, opener.programs); } catch (error) { failures.push(error); }
      const deadline = Date.now() + 30000;
      while (browserJobsAlive(root)) {
        if (Date.now() >= deadline) {
          failures.push(new Error(`fixture browser opener or job still owns ${root}`));
          break;
        }
        await new Promise((done) => setTimeout(done, 25));
      }
      if (failures.length) throw new AggregateError(failures, "owned browser handoffs failed");
    },
  };
}
