// The stand-in `claude` program (claude.ts installs it). Run as `claude auth status|login|logout`.
import { spawn } from "node:child_process";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";

const NL = String.fromCharCode(10);
const [group, command] = process.argv.slice(2);
const signedIn = process.env.DESKTOP_TEST_CLAUDE_SIGNED_IN;
if (group !== "auth" || !signedIn) process.exit(2);

if (command === "status") {
  const signed = existsSync(signedIn);
  process.stdout.write(JSON.stringify(signed ? { loggedIn: true, authMethod: "claude.ai", subscriptionType: "max", email: "person@example.com" }
    : { loggedIn: false, authMethod: "none" }) + NL);
  process.exit(signed ? 0 : 1);
} else if (command === "logout") {
  rmSync(signedIn, { force: true });
  process.stdout.write("Successfully logged out." + NL);
} else if (command === "login") {
  // A sign-in nobody finishes ends by itself.
  setTimeout(() => process.exit(1), 120000).unref();
  const server = createServer((request, response) => {
    if (request.url?.startsWith("/oauth/authorize")) {
      response.writeHead(302, { location: "/callback" }).end();
    } else if (request.url === "/callback") {
      writeFileSync(signedIn, "claude.ai");
      response.end("signed in", () => {
        process.stdout.write("Login successful." + NL);
        process.exit(0);
      });
    } else response.writeHead(404).end();
  });
  server.listen(0, "127.0.0.1", () => {
    const address = server.address();
    if (!address || typeof address === "string") process.exit(1);
    const url = `http://127.0.0.1:${address.port}/oauth/authorize?code=true`;
    process.stdout.write(`Opening browser to sign in${NL}If the browser didn't open, visit: ${url}${NL}`);
    // As Claude Code does, it opens the system browser itself: the fixture's stand-in.
    const browser = spawn(process.env.DESKTOP_TEST_NODE ?? "node", [process.env.DESKTOP_TEST_BROWSER_CLIENT ?? "", url], { stdio: "inherit" });
    browser.once("error", () => process.exit(1));
  });
} else process.exit(2);
