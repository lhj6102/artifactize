// Settings through the real app and CLI: the modal over the screen, where each provider key
// comes from (never its value), saved keys and custom providers as the CLI sees them, and a
// remote store's HTTP identity, persisted sign-in, status, dry run and logout.
import assert from "node:assert/strict";
import { create, createOutside, environment, providerList, remoteStatus, type Fixture } from "../fixture.ts";
import { press, typeText } from "../keys.ts";
import { graphShown } from "../graph-view.ts";
import { mainTitle } from "../pane.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativePath, primaryModifier, readText, serveLoopbackHttp, writeText } from "../platform.ts";

const KEY = "sk-e2e-settings-value";
const SAVED = "sk-e2e-saved-gemini-key";
const ENDPOINT_KEY = "sk-e2e-endpoint-key";
const TOKEN = "azt_e2esettingstoken0123456789abcdef";

function settingsEnvironment(fixture: Fixture): NodeJS.ProcessEnv {
  return {
    ...environment(fixture),
    ARTIFACTIZE_REMOTE: undefined,
    ARTIFACTIZE_REMOTE_TOKEN: undefined,
    ARTIFACTIZE_REMOTE_SHARE: undefined,
    OPENAI_API_KEY: KEY,
    ANTHROPIC_API_KEY: undefined,
    GEMINI_API_KEY: undefined,
  };
}

/** Settings is open over the screen. */
const settingsOpen = async (browser: WebdriverIO.Browser) =>
  (await browser.$("#settings-title").isExisting()) && (await browser.$("button=Settings").getAttribute("aria-pressed")) === "true";

async function openSettings(browser: WebdriverIO.Browser) {
  await browser.$("button=Settings").waitForExist({ timeout: 15000 });
  await browser.$("button=Settings").click();
  await browser.waitUntil(() => settingsOpen(browser), { timeout: 15000, timeoutMsg: "Settings opens" });
}

/** The names the Providers tab lists, in order. */
const listed = (browser: WebdriverIO.Browser) =>
  browser.execute(() => [...document.querySelectorAll('nav[aria-label="Providers"] button')].map((button) => button.textContent?.trim() ?? ""));

/** Choose `name` in the Providers list and wait until its setup shows. */
async function choose(browser: WebdriverIO.Browser, name: string) {
  await browser.$(`nav[aria-label="Providers"]`).$(`button=${name}`).click();
  await browser.waitUntil(async () => (await browser.$(`nav[aria-label="Providers"]`).$(`button=${name}`).getAttribute("aria-current")) === "true",
    { timeout: 15000, timeoutMsg: `${name} is chosen` });
}

const keyText = (browser: WebdriverIO.Browser, provider: string) =>
  browser.execute((id) => document.querySelector(`[data-testid="key-${id}"]`)?.textContent?.trim() ?? null, provider);

export const scenarios: Scenario[] = [
  // UX journey: J-SETTINGS; UX edges: E-SCREEN-SETTINGS E-SETTINGS-BACK
  scenario<null>(
    "settings-back",
    "opens Settings over the screen by Ctrl+, or its button, and closes it by Esc, Close or the button again, the screen beneath unchanged",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      const before = await mainTitle(browser);
      assert.match(before, /^Run /);
      const button = () => browser.$("button=Settings");
      const open = () => settingsOpen(browser);
      const closed = async () => !(await browser.$("#settings-title").isExisting()) && (await button().getAttribute("aria-pressed")) === "false";
      await press(browser, [primaryModifier, ","], open, "Settings by Ctrl+,");
      assert.equal(await mainTitle(browser), before, "the screen beneath Settings stays as it was");
      assert.equal(await browser.$('[role="tab"][aria-selected="true"]').getText(), "Providers", "Settings opens on the Providers tab");
      await press(browser, ["Escape"], closed, "Settings closed by Esc");
      assert.equal(await mainTitle(browser), before);
      await graphShown(browser, "a:release");
      await button().click();
      await browser.waitUntil(open, { timeout: 15000, timeoutMsg: "Settings by its button" });
      await button().click();
      await browser.waitUntil(closed, { timeout: 15000, timeoutMsg: "Settings closed by the button again" });
      await press(browser, [primaryModifier, ","], open, "Settings by Ctrl+, again");
      await browser.$('[role="dialog"]').$("button*=Close").click();
      await browser.waitUntil(closed, { timeout: 15000, timeoutMsg: "Settings closed by Close" });
      assert.equal(await mainTitle(browser), before, "the screen is still the one before Settings");
      await graphShown(browser, "a:release");
    },
  ),
  // UX journeys: J-SET-PROVIDERS J-SET-KEY; UX edges: E-SET-PROVIDER E-SET-KEY E-SET-KEY-SAVE E-SET-KEY-REMOVE
  scenario(
    "settings-keys",
    "settings: lists the providers, shows where each key comes from without its value, and saves and removes a key as the CLI sees it",
    async () => {
      const fixture = createOutside();
      return { fixture, env: settingsEnvironment(fixture), data: { key: KEY, saved: SAVED } };
    },
    async (browser, { fixture, data: { key, saved } }) => {
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      await openSettings(browser);
      await browser.waitUntil(async () => (await listed(browser)).length === 5, { timeout: 15000, timeoutMsg: "the providers are listed" });
      assert.deepEqual(await listed(browser), ["Codex (ChatGPT)", "OpenAI", "Anthropic", "Gemini", "+ Custom provider"]);
      await choose(browser, "OpenAI");
      await browser.waitUntil(async () => (await keyText(browser, "openai")) === "Set from OPENAI_API_KEY", { timeout: 15000, timeoutMsg: "the OpenAI key is set from its variable" });
      await choose(browser, "Anthropic");
      await browser.waitUntil(async () => (await keyText(browser, "anthropic")) === "Not set", { timeout: 15000, timeoutMsg: "the Anthropic key is not set" });
      assert.ok(!(await html()).includes(key), "the key's value is nowhere on the page");

      await choose(browser, "Gemini");
      await browser.$("#provider-key").click();
      await typeText(browser, saved);
      assert.equal(await browser.$("#provider-key").getAttribute("type"), "password");
      await browser.$("button=Save key").click();
      await browser.waitUntil(async () => (await keyText(browser, "gemini")) === "Saved", { timeout: 15000, timeoutMsg: "the Gemini key is saved" });
      assert.equal(await browser.$("#provider-key").getValue(), "", "the key left the field");
      assert.ok(!(await html()).includes(saved), "the saved key is nowhere on the page");
      const after = await providerList(fixture);
      assert.equal(after.code, 0, after.stdout);
      assert.ok(!after.stdout.includes(saved), "the CLI never prints the key");
      const sources = (stdout: string) => Object.fromEntries((JSON.parse(stdout).keys as { provider: string; source: string }[]).map((one) => [one.provider, one.source]));
      assert.deepEqual(sources(after.stdout), { openai: "environment", anthropic: "absent", gemini: "saved" });

      await browser.$("button=Remove saved key").click();
      await browser.waitUntil(async () => (await keyText(browser, "gemini")) === "Not set", { timeout: 15000, timeoutMsg: "the saved Gemini key is removed" });
      const removed = await providerList(fixture);
      assert.equal(sources(removed.stdout).gemini, "absent");
    },
  ),
  // UX journey: J-SET-KEY
  scenario(
    "settings-keys-anthropic",
    "shows Anthropic set and OpenAI not set without exposing the Anthropic fixture value",
    async () => {
      const fixture = createOutside();
      return { fixture, env: { ...settingsEnvironment(fixture), OPENAI_API_KEY: undefined, ANTHROPIC_API_KEY: KEY }, data: { key: KEY } };
    },
    async (browser, { data: { key } }) => {
      await openSettings(browser);
      await choose(browser, "OpenAI");
      await browser.waitUntil(async () => (await keyText(browser, "openai")) === "Not set", { timeout: 15000, timeoutMsg: "the OpenAI key is not set" });
      await choose(browser, "Anthropic");
      await browser.waitUntil(async () => (await keyText(browser, "anthropic")) === "Set from ANTHROPIC_API_KEY", { timeout: 15000, timeoutMsg: "the Anthropic key is set from its variable" });
      assert.ok(!(await browser.execute(() => document.documentElement.outerHTML)).includes(key), "the Anthropic fixture value is nowhere in the page");
    },
  ),
  // UX journey: J-SET-CUSTOM; UX edges: E-SET-CUSTOM-NEW E-SET-CUSTOM-SAVE E-SET-CUSTOM-REMOVE
  scenario(
    "settings-custom",
    "settings: adds a custom endpoint with a key, lists its models with that key, and removes it, as the CLI sees it",
    async () => {
      const fixture = createOutside();
      const seen = nativePath(fixture.root, "endpoint-authorization");
      writeText(seen, "");
      const endpoint = await serveLoopbackHttp((request, response) => {
        request.resume();
        const listing = request.url === "/v1/models";
        if (listing) writeText(seen, request.headers.authorization ?? "");
        response.writeHead(listing ? 200 : 404, { "content-type": "application/json" });
        response.end(JSON.stringify(listing ? { object: "list", data: [{ id: "local-model", object: "model", created: 0, owned_by: "e2e" }] } : {}));
      });
      return {
        fixture,
        env: settingsEnvironment(fixture),
        data: { url: `${endpoint.url}/v1`, seen, key: ENDPOINT_KEY },
        release: () => endpoint.close(),
      };
    },
    async (browser, { fixture, data: { url, seen, key } }) => {
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      const text = () => browser.execute(() => document.body.innerText);
      await openSettings(browser);
      await choose(browser, "+ Custom provider");
      await browser.$("#custom-name").waitForExist({ timeout: 15000 });
      assert.equal(await browser.$("#custom-name").getValue(), "", "a new custom provider starts empty");
      await browser.$("#custom-name").click();
      await typeText(browser, "local");
      await browser.$("#custom-url").click();
      await typeText(browser, url);
      await browser.$("#custom-key").click();
      await typeText(browser, key);
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => (await listed(browser)).includes("local") && (await text()).includes("Saved local."),
        { timeout: 15000, timeoutMsg: "the custom provider is saved and listed" });
      assert.equal(await browser.$('nav[aria-label="Providers"]').$("button=local").getAttribute("aria-current"), "true", "the saved provider is chosen");
      assert.ok(!(await html()).includes(key), "the endpoint key is nowhere on the page");
      const saved = await providerList(fixture);
      assert.equal(saved.code, 0, saved.stdout);
      assert.ok(!saved.stdout.includes(key), "the CLI never prints the endpoint key");
      assert.deepEqual(JSON.parse(saved.stdout).custom, { local: { kind: "endpoint", api: "openai-chat", url, key: true } });

      await browser.$("button=List models").click();
      await browser.waitUntil(async () => (await browser.$('table[aria-label="Models"]').getText()).includes("local-model"), { timeout: 15000, timeoutMsg: "the endpoint's models appear" });
      assert.equal(readText(seen), `Bearer ${key}`, "the listing used the saved key");

      await browser.$("button=Remove provider").click();
      await browser.waitUntil(async () => !(await listed(browser)).includes("local"), { timeout: 15000, timeoutMsg: "the custom provider is removed" });
      const removed = await providerList(fixture);
      assert.deepEqual(JSON.parse(removed.stdout).custom, {});
    },
  ),
  // UX journey: J-SET-REMOTE; UX edges: E-SET-TAB E-SET-REMOTE-LOGIN E-SET-REMOTE-STATUS E-SET-REMOTE-PUSH E-SET-REMOTE-LOGOUT
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
      await openSettings(browser);
      await browser.$("button=Remote").click();
      await browser.waitUntil(async () => (await browser.$('[role="tab"][aria-selected="true"]').getText()) === "Remote", { timeout: 15000, timeoutMsg: "the Remote tab shows" });
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
