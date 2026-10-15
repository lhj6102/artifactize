// Settings in the real app: API-key presence from the environment (never a value), and a
// remote store sign-in, status and sign-out against a loopback fake store, saved where the
// CLI's own `remote status` reads it.
import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import { afterEach, beforeEach, describe, it } from "node:test";
import { createOutside, remoteStatus } from "./fixture.ts";
import { type Session, startEnv } from "./driver.ts";
import { typeText } from "./keys.ts";

const KEY = "sk-e2e-settings-value";
const TOKEN = "azt_e2esettingstoken0123456789abcdef";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("settings", () => {
  let fixture: ReturnType<typeof createOutside>;
  let session: Session;
  let browser: WebdriverIO.Browser;
  let store: Server;
  let url = "";
  /** Who the fake store says the token belongs to; a test changes it between checks. */
  let principal = "alice-laptop";
  const text = () => browser.execute(() => document.body.innerText);
  const html = () => browser.execute(() => document.documentElement.outerHTML);

  beforeEach(async () => {
    fixture = createOutside();
    principal = "alice-laptop";
    // The store's whoami: this token is alice-laptop's; any other is refused.
    store = createServer((request, response) => {
      const ok = request.url === "/v1/whoami" && request.headers.authorization === `Bearer ${TOKEN}`;
      response.writeHead(ok ? 200 : 401, { "content-type": "application/json" });
      response.end(JSON.stringify(ok ? { principal, scopes: ["read", "publish"] } : { error: "Invalid or revoked token." }));
    });
    await new Promise<void>((done, fail) => { store.once("error", fail); store.listen(0, "127.0.0.1", done); });
    const address = store.address();
    assert.ok(address && typeof address === "object");
    url = `http://127.0.0.1:${address.port}/`;
    session = await startEnv(fixture, {
      ARTIFACTIZE_REMOTE: undefined,
      ARTIFACTIZE_REMOTE_TOKEN: undefined,
      ARTIFACTIZE_REMOTE_SHARE: undefined,
      OPENAI_API_KEY: KEY,
      ANTHROPIC_API_KEY: undefined,
    });
    browser = session.browser;
    await browser.$("button=Settings").waitForExist({ timeout: 15000 });
  });

  afterEach(async () => {
    const failures: unknown[] = [];
    try { await session?.stop(); } catch (error) { failures.push(error); }
    try {
      if (store) await new Promise<void>((done, fail) => {
        store.closeAllConnections();
        if (!store.listening) return done();
        store.close((error) => error ? fail(error) : done());
      });
    } catch (error) { failures.push(error); }
    try { await fixture.remove(); } catch (error) { failures.push(error); }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures, "store fixture cleanup failed");
  });

  // UX edges: E-SET-KEYS
  it("shows which API keys the environment sets, never their values", async () => {
    await browser.$("button=Settings").click();
    await eventually(async () => (await browser.$('[data-testid="key-openai"]').isExisting()), "the key list");
    const keys = await browser.execute(() => ({
      openai: document.querySelector('[data-testid="key-openai"]')?.textContent,
      anthropic: document.querySelector('[data-testid="key-anthropic"]')?.textContent,
    }));
    assert.deepEqual(keys, { openai: "Set", anthropic: "Not set" });
    assert.ok(!(await html()).includes(KEY), "the key's value is nowhere on the page");
  });

  // UX edges: E-SET-REMOTE-LOGIN E-SET-REMOTE-STATUS E-SET-REMOTE-PUSH E-SET-REMOTE-LOGOUT
  it("signs in to a remote store, shows its status, and signs out", async () => {
    await browser.$("button=Settings").click();
    await eventually(async () => (await text()).includes("No remote review store is configured."), "no store yet");
    await browser.$("#remote-url").click();
    await typeText(browser, url);
    await browser.$("#remote-token").click();
    await typeText(browser, TOKEN);
    // Not the default share level: it is saved with the store.
    await browser.$('input[name="share"][value="full"]').click();
    assert.equal(await browser.$("#remote-token").getAttribute("type"), "password");
    await browser.$("button=Sign in to the store").click();
    await eventually(async () => (await text()).includes("alice-laptop (scopes: read, publish)"), "the principal");
    assert.equal(await browser.$("#remote-token").getValue(), "", "the token left the field");
    assert.ok(!(await html()).includes(TOKEN), "the token is nowhere on the page");

    // The CLI reads the same saved sign-in.
    const status = await remoteStatus(fixture);
    assert.equal(status.code, 0, status.stdout);
    const parsed = JSON.parse(status.stdout);
    assert.equal(parsed.principal, "alice-laptop");
    assert.equal(parsed.tokenSource, "file");
    assert.equal(parsed.share, "full", "the chosen share level was saved");
    assert.ok((await text()).includes("full"), "and shown");
    assert.ok(!status.stdout.includes(TOKEN));

    // The store now answers differently: Check status shows the fresh answer.
    principal = "alice-desktop";
    await browser.$("button=Check status").click();
    await eventually(async () => (await text()).includes("alice-desktop (scopes: read, publish)"), "the fresh principal");
    await eventually(
      () => browser.execute(() => [...document.querySelectorAll("dt")].some((term) => term.textContent === "Reachable" && term.nextElementSibling?.textContent === "yes")),
      "reachable",
    );
    // A dry run reports what a push would do and changes nothing in the store.
    await browser.$("button=Push (dry run)").click();
    await eventually(async () => /Would push \d+, already in the store \d+, skipped \d+\./.test(await text()), "the dry run's report");
    await browser.$("button=Sign out of the store").click();
    await eventually(async () => (await text()).includes("No remote review store is configured."), "signed out");
    const after = await remoteStatus(fixture);
    assert.deepEqual(JSON.parse(after.stdout), { configured: false });
  });
});
