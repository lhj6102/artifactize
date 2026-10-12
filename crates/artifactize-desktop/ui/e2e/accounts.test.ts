// The Codex sign-in and the model listing in the real app, against loopback fakes of the
// sign-in service and the provider: cancelling a sign-in, finishing one with the pasted
// redirect URL, signing out, and listing a backend's models. The system browser is a stand-in
// that records the address it was given.
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { createOutside } from "./fixture.ts";
import { type Session, startEnv } from "./driver.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

/** An unsigned access token naming a ChatGPT account, as the sign-in service issues them. */
function token(): string {
  const part = (value: unknown) => Buffer.from(JSON.stringify(value)).toString("base64url");
  return `${part({ alg: "RS256", typ: "JWT" })}.${part({ exp: 4102444800, "https://api.openai.com/auth": { chatgpt_account_id: "acct-e2e" } })}.signature`;
}

function listen(server: Server): Promise<string> {
  return new Promise((done) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      assert.ok(address && typeof address === "object");
      done(`http://127.0.0.1:${address.port}`);
    });
  });
}

describe("accounts and models", () => {
  const fixture = createOutside();
  let session: Session;
  let browser: WebdriverIO.Browser;
  let auth: Server;
  let provider: Server;
  const opened = join(fixture.root, "opened");
  const status = () => browser.$('[data-testid="codex-status"]').getText();
  const text = () => browser.execute(() => document.body.innerText);

  before(async () => {
    auth = createServer((request, response) => {
      request.resume();
      if (request.url === "/oauth/token") {
        response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ access_token: token(), refresh_token: "refresh-e2e", expires_in: 3600 }));
      } else {
        response.writeHead(200).end();
      }
    });
    provider = createServer((request, response) => {
      request.resume();
      const listed = request.url?.endsWith("/models");
      response.writeHead(listed ? 200 : 404, { "content-type": "application/json" });
      response.end(JSON.stringify(listed ? { object: "list", data: [{ id: "gpt-e2e", object: "model", created: 0, owned_by: "e2e" }] } : {}));
    });
    const [authUrl, providerUrl] = await Promise.all([listen(auth), listen(provider)]);
    // The browser stand-in: records the address instead of opening anything.
    const bin = join(fixture.root, "bin");
    mkdirSync(bin, { recursive: true });
    writeFileSync(join(bin, "xdg-open"), `#!/bin/sh\necho "$1" >> '${opened}'\n`);
    chmodSync(join(bin, "xdg-open"), 0o755);
    session = await startEnv(fixture, {
      PATH: `${bin}:${process.env.PATH}`,
      ARTIFACTIZE_CODEX_AUTH_URL: authUrl,
      ARTIFACTIZE_CODEX_AUTH_FILE: undefined,
      ARTIFACTIZE_OPENAI_BASE_URL: `${providerUrl}/v1`,
      OPENAI_API_KEY: "sk-e2e-accounts",
    });
    browser = session.browser;
    await browser.$("button=Settings").waitForExist({ timeout: 15000 });
    await browser.$("button=Settings").click();
    await eventually(async () => (await status()).includes("Not signed in"), "no Codex sign-in yet");
  });

  after(async () => {
    await session?.stop();
    auth?.close();
    provider?.close();
    await fixture.remove();
  });

  // UX edges: E-SET-CODEX-LOGIN E-SET-CODEX-CANCEL
  it("starts a Codex sign-in and cancels it, saving nothing", async () => {
    await browser.$("button=Sign in with ChatGPT").click();
    await browser.$("button=Cancel sign-in").waitForExist({ timeout: 15000 });
    assert.match(await text(), /Waiting for the browser/);
    await browser.$("button=Cancel sign-in").click();
    await eventually(async () => !(await browser.$("button=Cancel sign-in").isExisting()), "the sign-in cancelled");
    assert.match(await status(), /Not signed in/);
  });

  // UX edges: E-SET-CODEX-LOGIN E-SET-CODEX-PASTE
  it("finishes a sign-in with the pasted redirect URL", async () => {
    await browser.$("button=Sign in with ChatGPT").click();
    await browser.$("#codex-redirect").waitForExist({ timeout: 15000 });
    // The address the browser stand-in was given carries the state the redirect must return.
    await eventually(() => readFileSync(opened, "utf8").trim().length > 0, "the browser opened");
    const address = readFileSync(opened, "utf8").trim().split("\n").at(-1)!;
    const state = new URL(address).searchParams.get("state");
    assert.ok(state, "the sign-in address names a state");
    await browser.$("#codex-redirect").setValue(`http://localhost:1455/auth/callback?code=e2e-code&state=${state}`);
    await browser.$("button=Finish sign-in").click();
    await eventually(async () => (await status()).startsWith("Signed in"), "signed in");
    assert.match(await text(), /Signed in with Codex\./);
  });

  // UX edges: E-SET-CODEX-RETURN
  it("finishes a sign-in when the browser returns to the app's loopback address", async () => {
    await eventually(async () => (await status()).includes("Signed in"), "signed in by the paste");
    await browser.$("button=Sign in again").click();
    await browser.$("button=Cancel sign-in").waitForExist({ timeout: 15000 });
    assert.doesNotMatch(await text(), /Port 1455 is in use/, "the app listens for the browser");
    await eventually(() => readFileSync(opened, "utf8").trim().split("\n").length >= 3, "the browser opened again");
    const address = readFileSync(opened, "utf8").trim().split("\n").at(-1)!;
    const state = new URL(address).searchParams.get("state");
    const reply = await fetch(`http://localhost:1455/auth/callback?code=e2e-return&state=${state}`);
    assert.equal(reply.status, 200);
    await eventually(async () => !(await browser.$("button=Cancel sign-in").isExisting()), "the sign-in finished");
    assert.match(await text(), /Signed in with Codex\./);
  });

  // UX edges: E-SET-CODEX-LOGOUT
  it("signs out of Codex", async () => {
    await browser.$("button=Sign out of Codex").click();
    await eventually(async () => /Not signed in/.test(await status()), "signed out");
  });

  // UX edges: E-SET-MODELS
  it("lists the models a backend offers", async () => {
    await browser.$("#models-provider").selectByAttribute("value", "openai");
    await browser.$("button=List models").click();
    await eventually(async () => (await browser.$('table[aria-label="Models"]').getText()).includes("gpt-e2e"), "the model list");
  });
});
