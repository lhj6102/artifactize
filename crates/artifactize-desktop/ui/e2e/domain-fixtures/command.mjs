// Declared portable programs used by real runtime evaluations and Human tools.
import { randomUUID } from "node:crypto";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";

const [operation, path] = process.argv.slice(2);
switch (operation) {
  case "pass":
    break;
  case "fail":
    process.exitCode = 1;
    break;
  case "read":
    process.stdout.write(readFileSync(path, "utf8"));
    break;
  case "launch":
    writeFileSync(path, "launched");
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
    const server = createServer((socket) => socket.end(token));
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") throw new Error("tool did not acquire a TCP port");
      const pending = `${path}.${token}`;
      writeFileSync(pending, JSON.stringify({ port: address.port, token }));
      renameSync(pending, path);
    });
    break;
  }
  default:
    throw new Error(`unknown fixture operation ${operation}`);
}
