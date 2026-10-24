// Declared portable programs used by real runtime evaluations and Human tools.
import { randomUUID } from "node:crypto";
import { absolutePath, appendText, exists, readText, replaceText, serveLoopback, writeText, type NativePath } from "../platform.ts";

/** How often a stand-in looks for the gate its test opens: a poll, never a wait for time. */
const GATE_POLL = 20;

const [operation, argument, record] = process.argv.slice(2);

/** Until the test writes `gate`, or its fixture is gone. */
async function opened(gate: NativePath): Promise<void> {
  await new Promise<void>((done) => {
    const poll = setInterval(() => {
      if (!exists(gate) && exists(absolutePath(gate, ".."))) return;
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
    // then, once its test opens the gate (or its fixture is gone), one on each stream.
    const gate = absolutePath(argument);
    process.stdout.write("opened the stand-in\n");
    await opened(gate);
    process.stdout.write("chose: B\n");
    process.stderr.write("warning: B is slower\n");
    break;
  }
  case "fingerprint":
    // A fingerprint script that records each of its runs, then answers once its test opens
    // the gate.
    appendText(absolutePath(record), "run\n");
    await opened(absolutePath(argument));
    process.stdout.write("fixture:v1\n");
    break;
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
