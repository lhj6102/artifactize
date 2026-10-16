// Declared portable programs used by real runtime evaluations and Human tools.
import { randomUUID } from "node:crypto";
import { absolutePath, readText, replaceText, serveLoopback, writeText } from "../platform.ts";

const [operation, argument] = process.argv.slice(2);
switch (operation) {
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
