// One real-app account journey owns the product's fixed callback port, without
// serializing otherwise independent capabilities. The system browser must GET
// the displayed authorization URL; only the domain server records that evidence.
import assert from "node:assert/strict";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { createServer as createSocket } from "node:net";
import { join } from "node:path";
import { createOutside, environment } from "../fixture.ts";
import { scenario, type Scenario } from "../scenario.ts";

/** An unsigned fixture access token naming the account returned by the auth server. */
function token(): string {
  const part = (value: unknown) => Buffer.from(JSON.stringify(value)).toString("base64url");
  return `${part({ alg: "RS256", typ: "JWT" })}.${part({ exp: 4102444800, "https://api.openai.com/auth": { chatgpt_account_id: "acct-e2e" } })}.signature`;
}

function listen(server: Server): Promise<string> {
  return new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", fail);
      const address = server.address();
      assert.ok(address && typeof address === "object");
      done(`http://127.0.0.1:${address.port}`);
    });
  });
}

async function close(servers: Server[]): Promise<void> {
  const results = await Promise.allSettled(servers.map((server) => new Promise<void>((done, fail) => {
    server.closeAllConnections();
    if (!server.listening) return done();
    server.close((error) => error ? fail(error) : done());
  })));
  const failures = results.flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
  if (failures.length) throw new AggregateError(failures, "account fixture servers did not close");
}

/** This prerequisite is a failure, not a skip or a lock; the app binds it itself. */
async function requireCallbackPort(): Promise<void> {
  const socket = createSocket();
  await new Promise<void>((done, fail) => {
    socket.once("error", (cause) => fail(new Error("accounts requires free loopback callback port 1455", { cause })));
    socket.listen(1455, "127.0.0.1", () => socket.close((error) => error ? fail(error) : done()));
  });
}

export const scenarios: Scenario[] = [
  // UX edges: E-SET-CODEX-LOGIN E-SET-CODEX-CANCEL E-SET-CODEX-PASTE
  // E-SET-CODEX-LOGOUT E-SET-CODEX-RETURN E-SET-MODELS
  scenario(
    "accounts",
    "accounts and models: cancel, paste, sign out, browser callback, sign out, list models",
    async () => {
      await requireCallbackPort();
      const fixture = createOutside();
      const authorizationLog = join(fixture.root, "auth-get-urls.jsonl");
      writeFileSync(authorizationLog, "");
      let authUrl = "";
      const auth = createServer((request, response) => {
        request.resume();
        if (request.method === "GET") appendFileSync(authorizationLog, `${JSON.stringify(`${authUrl}${request.url}`)}\n`);
        if (request.url === "/oauth/token") {
          response.writeHead(200, { "content-type": "application/json" });
          response.end(JSON.stringify({ access_token: token(), refresh_token: "refresh-e2e", expires_in: 3600 }));
        } else {
          response.writeHead(200).end();
        }
      });
      const provider = createServer((request, response) => {
        request.resume();
        const listed = request.url?.endsWith("/models");
        response.writeHead(listed ? 200 : 404, { "content-type": "application/json" });
        response.end(JSON.stringify(listed ? { object: "list", data: [{ id: "gpt-e2e", object: "model", created: 0, owned_by: "e2e" }] } : {}));
      });
      try {
        authUrl = await listen(auth);
        const providerUrl = await listen(provider);
        return {
          fixture,
          env: {
            ...environment(fixture),
            ARTIFACTIZE_CODEX_AUTH_URL: authUrl,
            ARTIFACTIZE_CODEX_AUTH_FILE: undefined,
            ARTIFACTIZE_OPENAI_BASE_URL: `${providerUrl}/v1`,
            OPENAI_API_KEY: "sk-e2e-accounts",
          },
          data: { authorizationLog },
          release: () => close([auth, provider]),
        };
      } catch (error) {
        try { await close([auth, provider]); }
        catch (cleanup) { throw new AggregateError([error, cleanup], "account preparation and cleanup failed"); }
        throw error;
      }
    },
    async (browser, { data: { authorizationLog } }) => {
      const status = () => browser.$('[data-testid="codex-status"]').getText();
      const text = () => browser.execute(() => document.body.innerText);
      const visited = (): string[] => readFileSync(authorizationLog, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);

      await browser.$("button=Settings").waitForExist({ timeout: 15000 });
      await browser.$("button=Settings").click();
      async function signedOut() {
        await browser.waitUntil(async () => /Not signed in/.test(await status()) &&
          await browser.$("button=Sign in with ChatGPT").isEnabled(), { timeout: 15000, timeoutMsg: "Codex is signed out and ready for a new sign-in" });
      }
      async function begin() {
        const baseline = visited().length;
        await browser.$("button=Sign in with ChatGPT").click();
        await browser.$("#codex-redirect").waitForExist({ timeout: 15000 });
        assert.match(await text(), /Waiting for the browser/);
        assert.doesNotMatch(await text(), /Port 1455 is in use/, "the product must own its loopback callback port");
        const address = await browser.$(".pending .url code").getText();
        assert.equal(new URL(address).pathname, "/oauth/authorize");
        const state = new URL(address).searchParams.get("state");
        assert.ok(state, "the displayed sign-in address names a state");
        await browser.waitUntil(() => visited().slice(baseline).includes(address), {
          timeout: 15000,
          timeoutMsg: "the system browser reaches the exact authorization URL displayed for this sign-in",
        });
        return state;
      }
      async function signedIn() {
        await browser.waitUntil(async () => (await status()).startsWith("Signed in") &&
          !(await browser.$("#codex-redirect").isExisting()) &&
          await browser.$("button=Sign out of Codex").isEnabled(), { timeout: 15000, timeoutMsg: "Codex sign-in completes" });
        assert.match(await text(), /Signed in with Codex\./);
      }
      async function signOut() {
        await browser.$("button=Sign out of Codex").click();
        await signedOut();
        assert.equal(await browser.$("#codex-redirect").isExisting(), false, "logout leaves no pending sign-in");
        assert.equal(await browser.$("button=Sign out of Codex").isEnabled(), false);
      }

      await signedOut();
      await begin();
      await browser.$("button=Cancel sign-in").click();
      await browser.waitUntil(async () => !(await browser.$("button=Cancel sign-in").isExisting()), { timeout: 15000, timeoutMsg: "the sign-in is cancelled" });
      await signedOut();

      const pastedState = await begin();
      await browser.$("#codex-redirect").setValue(`http://localhost:1455/auth/callback?code=e2e-code&state=${pastedState}`);
      await browser.$("button=Finish sign-in").click();
      await signedIn();
      await signOut();

      // A fresh signed-out state makes this callback, not the earlier paste, causal.
      const returnedState = await begin();
      const reply = await fetch(`http://localhost:1455/auth/callback?code=e2e-return&state=${returnedState}`);
      assert.equal(reply.status, 200);
      await signedIn();
      await signOut();

      await browser.$("#models-provider").selectByAttribute("value", "openai");
      await browser.$("button=List models").click();
      await browser.waitUntil(async () => (await browser.$('table[aria-label="Models"]').getText()).includes("gpt-e2e"), { timeout: 15000, timeoutMsg: "the backend model list appears" });
    },
  ),
];
