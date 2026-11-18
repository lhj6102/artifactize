// Declared portable programs used by real runtime evaluations and Human tools.
import { randomUUID } from "node:crypto";
import { absolutePath, appendText, exists, nativePath, parentOf, processId, readText, replaceText, serveLoopback, writeText, type NativePath } from "../platform.ts";

/** How often a stand-in looks for the gate its test opens: a poll, never a wait for time. */
const GATE_POLL = 20;

const [operation, argument, record] = process.argv.slice(2);

/** Register before touching any input; if teardown closed admission first, exit. */
function gated(gate: NativePath): (() => Promise<void>) | null {
  const root = parentOf(gate);
  try {
    writeText(nativePath(root, "gated-tools", `${processId()}-${randomUUID()}.pid`), "", { exclusive: true });
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
  const closed = nativePath(root, "closed-gated-tools");
  return () => new Promise<void>((done) => {
    const poll = setInterval(() => {
      if (!exists(gate) && !exists(closed)) return;
      clearInterval(poll);
      done();
    }, GATE_POLL);
  });
}

switch (operation) {
  case "report":
    for (let line = 0; line < 80; line += 1) {
      process.stdout.write(`fixture stdout ${String(line).padStart(3, "0")} ${line === 0 ? `code_${"unbroken".repeat(240)}` : ""}\n`);
      process.stderr.write(`fixture stderr ${String(line).padStart(3, "0")} ${line === 0 ? `code_${"unbroken".repeat(240)}` : ""}\n`);
    }
    break;
  case "pass":
    break;
  case "fail":
    process.exitCode = 1;
    break;
  case "read":
    process.stdout.write(readText(absolutePath(argument)));
    break;
  case "launch":
    writeText(absolutePath(argument), "launched");
    break;
  case "talk": {
    // A launched application that tells its reviewer what happened in it: one line at once,
    // then, once its gate is released, one on each stream.
    const opened = gated(absolutePath(argument));
    if (!opened) break;
    process.stdout.write("opened the stand-in\n");
    await opened();
    process.stdout.write("chose: B\n");
    process.stderr.write("warning: B is slower\n");
    break;
  }
  case "fingerprint": {
    // A fingerprint script that records each of its runs, then answers once its test opens
    // the gate: it fails when the gate says so.
    const opened = gated(absolutePath(argument));
    if (!opened) break;
    appendText(absolutePath(record), "run\n");
    await opened();
    if (exists(absolutePath(argument)) && readText(absolutePath(argument)) === "fail") process.exitCode = 1;
    else process.stdout.write("fixture:v1\n");
    break;
  }
  case "hold":
    // The product's declared runtime deadline, not an elapsed-time assertion,
    // terminates this command and produces an operational ERROR on every OS.
    setInterval(() => {}, 1000);
    break;
  case "slow": {
    // The listening socket exists only for this process's actual lifetime. Observers
    // issue no command to it: a connection only reads the unique fixture identity.
    const token = randomUUID();
    const server = await serveLoopback((socket) => socket.end(token));
    replaceText(absolutePath(argument), JSON.stringify({ port: server.port, token }));
    break;
  }
  default:
    throw new Error(`unknown fixture operation ${operation}`);
}
