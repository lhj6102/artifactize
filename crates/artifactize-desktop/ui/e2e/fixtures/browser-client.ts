// The system browser's stand-in, run with the address or file the app hands its browser: it
// fetches an address as a browser would, then records what it was handed.
import { appendFileSync } from "node:fs";

const target = process.argv[2];
const log = process.env.DESKTOP_TEST_BROWSER_LOG;
if (!target || !log) throw new Error("usage: browser-client.ts <address or file> with DESKTOP_TEST_BROWSER_LOG");
if (/^https?:/.test(target) && target !== process.env.DESKTOP_TEST_BROWSER_RECORD_ONLY) {
  const reply = await fetch(target, { signal: AbortSignal.timeout(30000) });
  await reply.arrayBuffer();
  if (!reply.ok) throw new Error(`the browser got HTTP ${reply.status} for ${target}`);
}
appendFileSync(log, `${JSON.stringify(target)}${String.fromCharCode(10)}`);
