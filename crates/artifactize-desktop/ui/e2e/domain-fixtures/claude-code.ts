// A scripted Claude Code for the settings cases: the stand-in program Settings runs as
// `claude auth ...`, its login server and its accepted code. What differs between operating
// systems (how a command is found on PATH, the desktop opener) comes from platform.ts.
import { renameSync } from "node:fs";
import { join } from "node:path";
import { commandFile, folderHoldsCommand, nodeProgram, processRunning, unixOpener, withPathFirst, writeNodeCommand, type Environment } from "../platform.ts";
import { closeDirectory, exists, listFiles, makeDirectory, nameOf, nativePath, readText, readTextIfExists, writeText, type NativePath } from "../portable.ts";

/** A scripted Claude Code (`claude auth status|login|logout`) first on the app's PATH, keeping
 * its sign-in as the file `signed-in` in `root`. `login` serves a loopback origin, prints its
 * sign-in URL there and signs in when the browser returns to its `/callback` or the code
 * `good-code` is pasted; any other path of it answers while it runs, and its input closing
 * stops it. Claude Code's own configuration points into `root`, never the user's. */
export function claudeCodeEnvironment(root: NativePath, env: Environment, watchdogMs = 120000): Environment {
  const bin = makeDirectory(nativePath(root, "claude-bin"));
  const program = nativePath(bin, "claude.mjs");
  const logins = makeDirectory(nativePath(root, "claude-logins"), { private: true });
  writeText(program, `import { appendFileSync, existsSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { join } from "node:path";
const NL = String.fromCharCode(10);
const [group, command] = process.argv.slice(2);
const signedIn = process.env.DESKTOP_TEST_CLAUDE_SIGNED_IN;
const vault = process.env.DESKTOP_TEST_CLAUDE_VAULT;
const account = (path) => fetch(vault + path);
const say = (text) => process.stdout.write(text);
if (group !== "auth") process.exit(2);
// An older Claude Code, which has no auth command: it reports nothing and says why.
if (existsSync(process.env.DESKTOP_TEST_CLAUDE_BROKEN)) {
  process.stderr.write("error: unknown command 'auth'" + NL);
  process.exit(1);
}
if (command === "status") {
  const signed = vault ? !!(await (await account("/credential")).text()) : existsSync(signedIn);
  say(JSON.stringify(signed
    ? { loggedIn: true, authMethod: "claude.ai", subscriptionType: vault ? await (await fetch(vault + "/plan")).text() : "max", email: "person@example.com" }
    : { loggedIn: false, authMethod: "none" }) + NL);
  process.exit(signed ? 0 : 1);
}
if (command === "logout") {
  if (vault) {
    if (!(await account("/logout")).ok) process.exit(1);
  } else rmSync(signedIn, { force: true });
  say("Successfully logged out." + NL);
  process.exit(0);
}
if (command !== "login") process.exit(2);
// Admit the actual login process before allocating any resources. Teardown can
// atomically close admission; late starts then cannot serve or open anything.
const logins = process.env.DESKTOP_TEST_CLAUDE_LOGINS;
writeFileSync(join(logins, process.pid + "-" + randomUUID() + ".pid"), "", { flag: "wx" });
setInterval(() => { if (!existsSync(logins)) process.exit(1); }, 100).unref();
setTimeout(() => process.exit(1), ${watchdogMs}).unref();
let finishing = false;
const finish = async () => {
  if (finishing) return;
  finishing = true;
  if (vault) {
    if (!(await account("/login")).ok) process.exit(1);
  } else writeFileSync(signedIn, "claude.ai");
  say("Login successful." + NL);
  process.exit(0);
};
const server = createServer((request, response) => {
  // What the browser visits is recorded, for the case to see that it opened.
  if (request.url.startsWith("/oauth/authorize")) appendFileSync(process.env.DESKTOP_TEST_CLAUDE_VISITS, request.url + NL);
  response.end("signed in");
  if (request.url.startsWith("/callback")) finish();
});
server.listen(0, "127.0.0.1", () => {
  const address = \`http://127.0.0.1:\${server.address().port}\`;
  appendFileSync(process.env.DESKTOP_TEST_CLAUDE_OWNERS, JSON.stringify({ address, pid: process.pid }) + NL);
  const url = \`\${address}/oauth/authorize?code=true\`;
  say(\`Opening browser to sign in\${NL}If the browser didn't open, visit: \${url}\${NL}Paste code here if prompted > \`);
  // As Claude Code does, it opens the system browser itself: only the case's fixture browser,
  // never the person's own.
  if (process.env.DESKTOP_TEST_BROWSER_ENTRIES) {
    appendFileSync(process.env.DESKTOP_TEST_BROWSER_ENTRIES, url + NL);
  }
  const failed = (reason) => {
    const text = "Claude fixture browser opener failed: " + String(reason);
    appendFileSync(process.env.DESKTOP_TEST_CLAUDE_OPEN_ERRORS, text + NL);
    process.stderr.write(text + NL);
    process.exit(1);
  };
  const opener = process.env.DESKTOP_TEST_BROWSER_CLIENT
    ? spawn(process.env.DESKTOP_TEST_BROWSER_NODE, [process.env.DESKTOP_TEST_BROWSER_CLIENT, url], { stdio: "inherit" })
    : process.env.DESKTOP_TEST_BROWSER_PROGRAM
      ? spawn(${JSON.stringify(unixOpener)}, [url], { stdio: "inherit" })
      : null;
  if (!opener) return failed("no owned browser client was configured");
  opener.on("error", failed);
  // Admitted from its spawn, like the login itself: teardown joins it even if it never
  // reaches the fixture's server.
  if (opener.pid !== undefined) {
    try { writeFileSync(join(logins, opener.pid + "-" + randomUUID() + ".pid"), "", { flag: "wx" }); }
    // Unregistered, it is stopped and its exit awaited before the login fails.
    catch (error) { opener.once("exit", () => failed(error)); opener.kill(); }
  }
  opener.on("close", (code, signal) => {
    if (code !== 0) return failed(signal ?? "exit " + code);
    appendFileSync(process.env.DESKTOP_TEST_CLAUDE_OPENED, url + NL);
  });
});
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (text) => {
  input += text;
  if (!input.includes(NL)) return;
  if (input.split(NL)[0].trim() === "good-code") { void finish(); return; }
  process.stderr.write("Login failed: That code was not accepted." + NL);
  process.exit(1);
});
process.stdin.on("end", () => { if (!finishing) process.exit(1); });
`);
  writeNodeCommand(bin, "claude", "DESKTOP_TEST_CLAUDE_PROGRAM");
  // The fixture's Claude Code is the only one the app can find: a folder of PATH holding
  // another one, such as the developer's own, is left out, so hiding the fixture's leaves none.
  return {
    ...withPathFirst(env, [bin], (folder) => !folderHoldsCommand(folder, "claude")),
    DESKTOP_TEST_NODE: nodeProgram,
    DESKTOP_TEST_CLAUDE_PROGRAM: program,
    DESKTOP_TEST_CLAUDE_SIGNED_IN: nativePath(root, "signed-in"),
    DESKTOP_TEST_CLAUDE_BROKEN: nativePath(root, "claude-broken"),
    DESKTOP_TEST_CLAUDE_VISITS: nativePath(root, "claude-visits"),
    DESKTOP_TEST_CLAUDE_OWNERS: nativePath(root, "claude-owners.jsonl"),
    DESKTOP_TEST_CLAUDE_LOGINS: logins,
    DESKTOP_TEST_CLAUDE_OPEN_ERRORS: nativePath(root, "claude-browser-errors"),
    DESKTOP_TEST_CLAUDE_OPENED: nativePath(root, "claude-browser-opened"),
    CLAUDE_CONFIG_DIR: makeDirectory(nativePath(root, "claude-config"), { private: true }),
  };
}

/** A result recorded by the actual login command, not just its printed sign-in URL. */
export function claudeBrowserOpened(root: NativePath): string[] {
  const failures = readTextIfExists(nativePath(root, "claude-browser-errors"));
  if (failures) throw new Error(failures.trim());
  return (readTextIfExists(nativePath(root, "claude-browser-opened")) ?? "").split("\n").filter(Boolean);
}

/** Stop and join admitted Claude logins after the app stops, before closing their browser. */
export async function releaseClaudeLogins(root: NativePath): Promise<void> {
  const stopped = nativePath(root, "closed-claude-logins");
  closeDirectory(nativePath(root, "claude-logins"), stopped);
  if (!exists(stopped)) return;
  const pids = listFiles(stopped).map((file) => {
    const pid = Number(/^(\d+)-[a-f0-9-]+\.pid$/.exec(nameOf(file))?.[1]);
    if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error(`invalid Claude login owner: ${file}`);
    return pid;
  });
  const deadline = Date.now() + 15000;
  while (pids.some(processRunning)) {
    if (Date.now() >= deadline) throw new Error(`Claude login still owns fixture ${root}`);
    await new Promise((done) => setTimeout(done, 25));
  }
  // The joined records stay with the fixture: a retained fixture keeps them for diagnosis.
}

/** Read the actual login process that announced this origin while it was live. */
export function claudeLoginOwner(root: NativePath, address: string): number {
  const records = readText(nativePath(root, "claude-owners.jsonl")).split("\n").filter(Boolean)
    .map((line) => JSON.parse(line) as { address: string; pid: number });
  const pid = records.findLast((entry) => entry.address === address)?.pid;
  if (pid === undefined || !Number.isSafeInteger(pid) || pid <= 0) throw new Error(`Claude fixture did not record the owner of ${address}`);
  return pid;
}

/** The file name the fixture's Claude Code is found by on PATH. */
const CLAUDE_CODE = commandFile("claude");

/** Take the fixture's Claude Code off PATH, or put it back: `claudeCodeEnvironment(root, …)`
 * left no other one there, so the app then finds none. */
export function claudeCodeInstalled(root: NativePath, installed: boolean): void {
  const bin = nativePath(root, "claude-bin");
  const [from, to] = installed ? [`${CLAUDE_CODE}.off`, CLAUDE_CODE] : [CLAUDE_CODE, `${CLAUDE_CODE}.off`];
  renameSync(join(bin, from), join(bin, to));
}

