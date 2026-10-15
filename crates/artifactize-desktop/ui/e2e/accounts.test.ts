// The Codex sign-in and the model listing in the real app, against loopback fakes of the
// sign-in service and the provider: cancelling a sign-in, finishing one with the pasted
// redirect URL, signing out, and listing a backend's models. The system browser is a stand-in
// that records the address it was given.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { join } from "node:path";
import { afterEach, beforeEach, describe, it } from "node:test";
import { createOutside } from "./fixture.ts";
import { browserStandIn, type Session, startEnv } from "./driver.ts";
import { networkTests } from "./network.ts";

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
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      assert.ok(address && typeof address === "object");
      done(`http://127.0.0.1:${address.port}`);
    });
  });
}

// The provider callback is fixed at 1455: this fixture owns its loopback namespace,
// never the host port shared with a person or another evaluator.
networkTests(import.meta.url, () => {
  describe("accounts and models", () => {
    let fixture: ReturnType<typeof createOutside>;
    let session: Session;
    let browser: WebdriverIO.Browser;
    let auth: Server;
    let provider: Server;
    let opened: string;
    const status = () => browser.$('[data-testid="codex-status"]').getText();
    const text = () => browser.execute(() => document.body.innerText);
    const browserLog = () => existsSync(opened) ? readFileSync(opened, "utf8").trim().split("\n").filter(Boolean) : [];
    async function openedAddress(baseline: number): Promise<string> {
      let address: string | undefined;
      await eventually(async () => {
        const displayed = await text();
        address = browserLog().slice(baseline).find((entry) => displayed.includes(entry));
        return address !== undefined;
      }, "a new browser entry matches this displayed sign-in address");
      return address!;
    }


    beforeEach(async () => {
      fixture = createOutside();
      opened = join(fixture.root, "opened");
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
      const authUrl = await listen(auth);
      const providerUrl = await listen(provider);
      // The browser stand-in: records the address instead of opening anything.
      session = await startEnv(fixture, {
        ...browserStandIn(fixture, opened),
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

    afterEach(async () => {
      const failures: unknown[] = [];
      try { await session?.stop(); } catch (error) { failures.push(error); }
      const closed = await Promise.allSettled([auth, provider].filter(Boolean).map((server) => new Promise<void>((done, fail) => {
        server.closeAllConnections();
        if (!server.listening) return done();
        server.close((error) => error ? fail(error) : done());
      })));
      for (const result of closed) if (result.status === "rejected") failures.push(result.reason);
      try { await fixture.remove(); } catch (error) { failures.push(error); }
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1) throw new AggregateError(failures, "account fixture cleanup failed");
    });

    // UX edges: E-SET-CODEX-LOGIN E-SET-CODEX-CANCEL
    it("starts a Codex sign-in and cancels it, saving nothing", async () => {
      const baseline = browserLog().length;
      await browser.$("button=Sign in with ChatGPT").click();
      await browser.$("button=Cancel sign-in").waitForExist({ timeout: 15000 });
      await openedAddress(baseline);
      assert.match(await text(), /Waiting for the browser/);
      await browser.$("button=Cancel sign-in").click();
      await eventually(async () => !(await browser.$("button=Cancel sign-in").isExisting()), "the sign-in cancelled");
      assert.match(await status(), /Not signed in/);
    });

    async function signInWithPaste() {
      const baseline = browserLog().length;
      await browser.$("button=Sign in with ChatGPT").click();
      await browser.$("#codex-redirect").waitForExist({ timeout: 15000 });
      // The address the browser stand-in was given carries the state the redirect must return.
      const address = await openedAddress(baseline);
      const state = new URL(address).searchParams.get("state");
      assert.ok(state, "the sign-in address names a state");
      // The same address is shown in Settings, for a browser that did not open.
      assert.ok((await text()).includes(address), "the sign-in address is displayed");
      await browser.$("#codex-redirect").setValue(`http://localhost:1455/auth/callback?code=e2e-code&state=${state}`);
      await browser.$("button=Finish sign-in").click();
      await eventually(async () => (await status()).startsWith("Signed in"), "signed in");
      assert.match(await text(), /Signed in with Codex\./);
    }

    // UX edges: E-SET-CODEX-LOGIN E-SET-CODEX-PASTE
    it("finishes a sign-in with the pasted redirect URL", signInWithPaste);

    // UX edges: E-SET-CODEX-RETURN
    it("finishes a sign-in when the browser returns to the app's loopback address", async () => {
      // This case starts signed out; only its own browser return can change that.
      assert.match(await status(), /Not signed in/);
      const baseline = browserLog().length;
      await browser.$("button=Sign in with ChatGPT").click();
      await browser.$("button=Cancel sign-in").waitForExist({ timeout: 15000 });
      assert.doesNotMatch(await text(), /Port 1455 is in use/, "the app listens for the browser");
      const address = await openedAddress(baseline);
      const state = new URL(address).searchParams.get("state");
      const reply = await fetch(`http://localhost:1455/auth/callback?code=e2e-return&state=${state}`);
      assert.equal(reply.status, 200);
      await eventually(async () => (await status()).startsWith("Signed in"), "the status became signed in");
    });

    // UX edges: E-SET-CODEX-LOGOUT
    it("signs out of Codex", async () => {
      await signInWithPaste();
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
});
