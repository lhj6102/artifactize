// Settings through the real app and CLI: environment-key presence without values,
// and a remote store's HTTP identity, persisted sign-in, status, dry run and logout.
import assert from "node:assert/strict";
import { create, createOutside, environment, remoteStatus, signoff, type Fixture } from "../fixture.ts";
import { press, typeText } from "../keys.ts";
import { graphShown } from "../graph-view.ts";
import { mainTitle, openWaiting } from "../pane.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativePath, primaryModifier, readText, serveLoopbackHttp, writeText } from "../platform.ts";

const KEY = "sk-e2e-settings-value";
const TOKEN = "azt_e2esettingstoken0123456789abcdef";

function settingsEnvironment(fixture: Fixture): NodeJS.ProcessEnv {
  return {
    ...environment(fixture),
    ARTIFACTIZE_REMOTE: undefined,
    ARTIFACTIZE_REMOTE_TOKEN: undefined,
    ARTIFACTIZE_REMOTE_SHARE: undefined,
    OPENAI_API_KEY: KEY,
    ANTHROPIC_API_KEY: undefined,
  };
}

export const scenarios: Scenario[] = [
  // UX journey: J-SETTINGS; UX edges: E-SCREEN-SETTINGS E-SETTINGS-BACK
  scenario<null>(
    "settings-back",
    "shows Settings in the Main area by Ctrl+, or its button, and leaves it by Esc, the button again or a choice in the Pane",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    async (browser, { fixture }) => {
      await graphShown(browser, "a:release");
      const before = await mainTitle(browser);
      assert.match(before, /^Run /);
      const button = () => browser.$("button=Settings");
      const shows = (title: string) => async () => (await mainTitle(browser)) === title;
      const settings = async () => (await shows("Settings")()) && (await button().getAttribute("aria-pressed")) === "true";
      await press(browser, [primaryModifier, ","], settings, "Settings by Ctrl+,");
      await press(browser, ["Escape"], shows(before), "the Run again after Esc");
      await graphShown(browser, "a:release");
      assert.equal(await button().getAttribute("aria-pressed"), "false");
      await button().click();
      await browser.waitUntil(settings, { timeout: 15000, timeoutMsg: "Settings by its button" });
      await button().click();
      await browser.waitUntil(shows(before), { timeout: 15000, timeoutMsg: "the Run again after the button" });
      await press(browser, [primaryModifier, ","], settings, "Settings by Ctrl+, again");
      await openWaiting(browser, signoff(fixture).id);
      await browser.waitUntil(shows("Review"), { timeout: 15000, timeoutMsg: "the review chosen in the Pane" });
      assert.equal(await button().getAttribute("aria-pressed"), "false", "choosing in the Pane left Settings");
    },
  ),
  // UX edges: E-SET-KEYS
  scenario(
    "settings-keys",
    "settings: shows API-key presence without exposing values",
    async () => {
      const fixture = createOutside();
      return { fixture, env: settingsEnvironment(fixture), data: { key: KEY } };
    },
    async (browser, { data: { key } }) => {
      await browser.$("button=Settings").waitForExist({ timeout: 15000 });
      await browser.$("button=Settings").click();
      await browser.waitUntil(() => browser.execute(() =>
        document.querySelector('[data-testid="key-openai"]')?.textContent === "Set"
        && document.querySelector('[data-testid="key-anthropic"]')?.textContent === "Not set"),
      { timeout: 15000, timeoutMsg: "the loaded Settings key statuses" });
      const keys = await browser.execute(() => ({
        openai: document.querySelector('[data-testid="key-openai"]')?.textContent,
        anthropic: document.querySelector('[data-testid="key-anthropic"]')?.textContent,
      }));
      assert.deepEqual(keys, { openai: "Set", anthropic: "Not set" });
      const html = await browser.execute(() => document.documentElement.outerHTML);
      assert.ok(!html.includes(key), "the key's value is nowhere on the page");
    },
  ),
  // UX journey: J-SET-KEYS
  scenario(
    "settings-keys-anthropic",
    "shows Anthropic present and OpenAI absent without exposing the Anthropic fixture value",
    async () => {
      const fixture = createOutside();
      return { fixture, env: { ...settingsEnvironment(fixture), OPENAI_API_KEY: undefined, ANTHROPIC_API_KEY: KEY }, data: { key: KEY } };
    },
    async (browser, { data: { key } }) => {
      await browser.$("button=Settings").waitForExist({ timeout: 15000 });
      await browser.$("button=Settings").click();
      await browser.waitUntil(() => browser.execute(() => document.querySelector('[data-testid="key-openai"]')?.textContent === "Not set"
        && document.querySelector('[data-testid="key-anthropic"]')?.textContent === "Set"), { timeout: 15000, timeoutMsg: "the inverse key-presence statuses" });
      assert.ok(!(await browser.execute(() => document.documentElement.outerHTML)).includes(key), "the Anthropic fixture value is nowhere in the page");
    },
  ),
  // UX edges: E-SET-REMOTE-LOGIN E-SET-REMOTE-STATUS E-SET-REMOTE-PUSH E-SET-REMOTE-LOGOUT
  scenario(
    "settings-remote",
    "settings: remote sign-in, shared CLI status, fresh identity, dry run and sign-out",
    async () => {
      const fixture = createOutside();
      const env = settingsEnvironment(fixture);
      // The outer owner serves HTTP while WDIO runs elsewhere. Only this fixture's
      // domain state crosses that process boundary, never a server or live closure.
      const principalFile = nativePath(fixture.root, "remote-principal");
      writeText(principalFile, "alice-laptop");
      const store = await serveLoopbackHttp((request, response) => {
        request.resume();
        const ok = request.url === "/v1/whoami" && request.headers.authorization === `Bearer ${TOKEN}`;
        response.writeHead(ok ? 200 : 401, { "content-type": "application/json" });
        response.end(JSON.stringify(ok ? { principal: readText(principalFile), scopes: ["read", "publish"] } : { error: "Invalid or revoked token." }));
      });
      return {
        fixture,
        env,
        data: { url: `${store.url}/`, principalFile, token: TOKEN },
        release: () => store.close(),
      };
    },
    async (browser, { fixture, data: { url, principalFile, token } }) => {
      const text = () => browser.execute(() => document.body.innerText);
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      await browser.$("button=Settings").waitForExist({ timeout: 15000 });
      await browser.$("button=Settings").click();
      await browser.waitUntil(async () => (await text()).includes("No remote review store is configured."), { timeout: 15000, timeoutMsg: "no store is configured yet" });
      await browser.$("#remote-url").click();
      await typeText(browser, url);
      await browser.$("#remote-token").click();
      await typeText(browser, token);
      await browser.$('input[name="share"][value="full"]').click();
      assert.equal(await browser.$("#remote-token").getAttribute("type"), "password");
      await browser.$("button=Sign in to the store").click();
      await browser.waitUntil(async () => (await text()).includes("alice-laptop (scopes: read, publish)"), { timeout: 15000, timeoutMsg: "the remote principal appears" });
      assert.equal(await browser.$("#remote-token").getValue(), "", "the token left the field");
      assert.ok(!(await html()).includes(token), "the token is nowhere on the page");

      // The asynchronous CLI query shares saved state without blocking this store.
      const status = await remoteStatus(fixture);
      assert.equal(status.code, 0, status.stdout);
      const parsed = JSON.parse(status.stdout);
      assert.equal(parsed.principal, "alice-laptop");
      assert.equal(parsed.tokenSource, "file");
      assert.equal(parsed.share, "full", "the chosen share level was saved");
      assert.ok((await text()).includes("full"), "the share level is also shown");
      assert.ok(!status.stdout.includes(token));

      writeText(principalFile, "alice-desktop");
      await browser.$("button=Check status").click();
      await browser.waitUntil(async () => (await text()).includes("alice-desktop (scopes: read, publish)"), { timeout: 15000, timeoutMsg: "Check status displays the fresh remote principal" });
      await browser.waitUntil(() => browser.execute(() => [...document.querySelectorAll("dt")].some((term) => term.textContent === "Reachable" && term.nextElementSibling?.textContent === "yes")), { timeout: 15000, timeoutMsg: "the store is reachable" });
      await browser.$("button=Push (dry run)").click();
      await browser.waitUntil(async () => /Would push \d+, already in the store \d+, skipped \d+\./.test(await text()), { timeout: 15000, timeoutMsg: "the dry run reports its result" });
      await browser.$("button=Sign out of the store").click();
      await browser.waitUntil(async () => (await text()).includes("No remote review store is configured."), { timeout: 15000, timeoutMsg: "the store is signed out" });
      const after = await remoteStatus(fixture);
      assert.equal(after.code, 1, after.stdout);
      assert.deepEqual(JSON.parse(after.stdout), { configured: false });
    },
  ),
];
