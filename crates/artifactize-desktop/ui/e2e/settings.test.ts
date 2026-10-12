// Settings in the real app: API-key presence from the environment (never a value), and a
// remote store sign-in, status and sign-out against a loopback fake store, saved where the
// CLI's own `remote status` reads it.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createServer, type Server } from "node:http";
import { after, before, describe, it } from "node:test";
import { cli, createOutside } from "./fixture.ts";
import { type Session, startEnv } from "./driver.ts";
import { typeText } from "./keys.ts";

const KEY = "sk-e2e-settings-value";
const TOKEN = "azt_e2esettingstoken0123456789abcdef";

/** `artifactize remote status --json` on the fixture's state, without blocking the fake store
 * that runs in this process. */
function cliStatus(state: string, repo: string): Promise<{ code: number; stdout: string }> {
  return new Promise((done) => {
    execFile(
      cli,
      ["--state-dir", state, "--repo", repo, "remote", "status", "--json"],
      { encoding: "utf8", env: { ...process.env, ARTIFACTIZE_REMOTE: "", ARTIFACTIZE_REMOTE_TOKEN: "" } },
      (error, stdout) => done({ code: error ? Number(error.code ?? 1) : 0, stdout }),
    );
  });
}

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("settings", () => {
  const fixture = createOutside();
  let session: Session;
  let browser: WebdriverIO.Browser;
  let store: Server;
  let url = "";
  const text = () => browser.execute(() => document.body.innerText);
  const html = () => browser.execute(() => document.documentElement.outerHTML);

  before(async () => {
    // The store's whoami: this token is alice-laptop's; any other is refused.
    store = createServer((request, response) => {
      const ok = request.url === "/v1/whoami" && request.headers.authorization === `Bearer ${TOKEN}`;
      response.writeHead(ok ? 200 : 401, { "content-type": "application/json" });
      response.end(JSON.stringify(ok ? { principal: "alice-laptop", scopes: ["read", "publish"] } : { error: "Invalid or revoked token." }));
    });
    await new Promise<void>((done) => store.listen(0, "127.0.0.1", done));
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

  after(async () => {
    await session?.stop();
    store?.close();
    await fixture.remove();
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
    const status = await cliStatus(fixture.state, fixture.repo);
    assert.equal(status.code, 0, status.stdout);
    const parsed = JSON.parse(status.stdout);
    assert.equal(parsed.principal, "alice-laptop");
    assert.equal(parsed.tokenSource, "file");
    assert.equal(parsed.share, "full", "the chosen share level was saved");
    assert.ok((await text()).includes("full"), "and shown");
    assert.ok(!status.stdout.includes(TOKEN));

    await browser.$("button=Check status").click();
    await eventually(
      () => browser.execute(() => [...document.querySelectorAll("dt")].some((term) => term.textContent === "Reachable" && term.nextElementSibling?.textContent === "yes")),
      "reachable",
    );
    // A dry run reports what a push would do and changes nothing in the store.
    await browser.$("button=Push (dry run)").click();
    await eventually(async () => /Would push \d+, already in the store \d+, skipped \d+\./.test(await text()), "the dry run's report");
    await browser.$("button=Sign out of the store").click();
    await eventually(async () => (await text()).includes("No remote review store is configured."), "signed out");
    const after = await cliStatus(fixture.state, fixture.repo);
    assert.deepEqual(JSON.parse(after.stdout), { configured: false });
  });
});
