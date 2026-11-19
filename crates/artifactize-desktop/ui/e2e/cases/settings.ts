import { writeDeclaration } from "../declaration-fixtures.ts";
// Settings through the real app and CLI: the modal over the screen, where each provider key
// comes from (never its value), saved keys and custom providers as the CLI sees them, a
// remote store's HTTP identity, persisted sign-in, status, dry run and logout, and the
// sign-off idle grace both read and set.
import assert from "node:assert/strict";
import {
  artifactize, create, createOutside, createSignoffOnly, ended, environment, newestRunStatus, providerList, remoteStatus, signOff, signoffGrace, verifyAll,
  verifyWaiting, requests, savedRuns, type Fixture,
} from "../fixture.ts";
import { clickOption } from "../input.ts";
import { git } from "../environment.ts";
import { commandFields, runtimeProfile } from "../native-fixtures.ts";
import { fill, press, typeText } from "../keys.ts";
import { graphShown, ZOOM_IN } from "../graph-view.ts";
import { clickAt, pointerGapClick, visibleCenter } from "../pointer.ts";
import { branch, mainTitle, openWaiting, showRun } from "../pane.ts";
import { observe } from "../domain-fixtures/observe.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { childDirectories, claudeBrowserOpened, claudeCodeEnvironment, claudeCodeInstalled, claudeLoginOwner, exists, httpGet, listFiles, makeDirectory, nativePath, portablePath, primaryModifier, processRunning, readBytes, readText, replaceText, removeTree, serveLoopbackHttp, writeText } from "../platform.ts";

const KEY = "sk-e2e-settings-value";
const SAVED = "sk-e2e-saved-gemini-key";
const SHADOWED = "sk-e2e-saved-openai-key";
const ENDPOINT_KEY = "sk-e2e-endpoint-key";
const TOKEN = "azt_e2esettingstoken0123456789abcdef";
const CLAUDE_CREDENTIAL = "fixture-claude-credential-never-returned-with-status";

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

/** Settings opened again on its Limits tab; an open Settings is closed first, so it reads again. */
async function showLimits(browser: WebdriverIO.Browser) {
  if (await settingsOpen(browser)) {
    await browser.$("button=Settings").click();
    await browser.waitUntil(async () => !(await settingsOpen(browser)), { timeout: 15000, timeoutMsg: "Settings closes" });
  }
  await openSettings(browser);
  await browser.$("button=Limits").click();
  await browser.waitUntil(async () => (await browser.$('[role="tab"][aria-selected="true"]').getText()) === "Limits", { timeout: 15000, timeoutMsg: "the Limits tab shows" });
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

/** Each built-in key's source, as `provider list --json` prints it. */
const sources = (stdout: string) =>
  Object.fromEntries((JSON.parse(stdout).keys as { provider: string; source: string }[]).map((one) => [one.provider, one.source]));

/** Choose `value` in the select `selector` by clicking its option. */
async function select(browser: WebdriverIO.Browser, selector: string, value: string) {
  await clickOption(browser, await browser.$(`${selector} option[value="${value}"]`).getElement());
  await browser.waitUntil(async () => (await browser.$(selector).getValue()) === value, { timeout: 15000, timeoutMsg: `${selector} shows ${value}` });
}

/** The models the Models table lists. */
async function listModels(browser: WebdriverIO.Browser, expected: string) {
  await browser.$("button=List models").click();
  await browser.waitUntil(async () => (await browser.$('ul[aria-label="Models"]').getText()).includes(expected), { timeout: 15000, timeoutMsg: `${expected} is listed` });
}

/** A Chat Completions stream answering `text`, as compatible servers send it. */
function streamed(text: string): string {
  const chunk = (delta: object, finish: string | null) =>
    JSON.stringify({ id: "chatcmpl-e2e", object: "chat.completion.chunk", created: 0, model: "local-model", choices: [{ index: 0, delta, finish_reason: finish }] });
  const last = JSON.parse(chunk({}, "stop")) as Record<string, unknown>;
  last.usage = { prompt_tokens: 10, completion_tokens: 2, total_tokens: 12 };
  return [chunk({ role: "assistant", content: text }, null), JSON.stringify(last)].map((record) => `data: ${record}\n\n`).join("") + "data: [DONE]\n\n";
}

export const scenarios: Scenario[] = [
  // UX journey: J-SETTINGS
  scenario<null>(
    "settings-back",
    "opens Settings over the screen by Ctrl+, or its button, and closes it by Esc, Close or the button again, the screen beneath unchanged",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    async (browser) => {
      await graphShown(browser, "a:release");
      await browser.setWindowSize(1440, 850);
      // With the graph across the window, its tool strip begins outside the modal's panel.
      await press(browser, [primaryModifier, "b"], async () => !(await browser.$('[aria-label="Branches"]').isDisplayed()), "the Pane is hidden");
      const before = await mainTitle(browser);
      assert.match(before, /^run-/);
      const button = () => browser.$("button=Settings");
      const open = () => settingsOpen(browser);
      const closed = async () => !(await browser.$("#settings-title").isExisting()) && (await button().getAttribute("aria-pressed")) === "false";
      const zoom = () => browser.$('[aria-label="Graph tools"] [aria-label="Zoom"]').getText();
      const zoomBefore = await zoom();
      const graphControl = browser.$(ZOOM_IN);
      const graphPoint = await visibleCenter(browser, await graphControl.getElement());
      await press(browser, [primaryModifier, ","], open, "Settings by Ctrl+,");
      assert.equal(await browser.$('[role="tab"][aria-selected="true"]').getText(), "Providers", "Settings opens on the Providers tab");
      assert.equal(await browser.execute(({ x, y }) => {
        const box = document.querySelector('[role="dialog"][aria-labelledby="settings-title"]')!.getBoundingClientRect();
        return x < box.left || x >= box.right || y < box.top || y >= box.bottom;
      }, graphPoint), true, "the graph control is outside the dialog, not just covered by its panel");
      await pointerGapClick(browser, graphPoint);
      await browser.waitUntil(closed, { timeout: 15000, timeoutMsg: "an outside click closes Settings" });
      assert.equal(await zoom(), zoomBefore, "the outside click did not reach the graph's Zoom in control");
      await press(browser, [primaryModifier, ","], open, "Settings reopened after outside click");
      assert.equal(await mainTitle(browser), before, "the screen beneath Settings stays as it was");
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
      await clickAt(browser, await visibleCenter(browser, await graphControl.getElement()));
      await browser.waitUntil(async () => await zoom() !== zoomBefore, { timeout: 15000, timeoutMsg: "the graph control works after Settings closes" });
    },
  ),
  // UX journey: J-SET-CLAUDE-AGENT
  scenario(
    "settings-claude-agent",
    "settings: shows Claude Code's own sign-in, asks again, and signs in and out by running Claude Code's own commands: cancelled, refused, with a pasted code and by the browser returning",
    async () => {
      const fixture = createOutside();
      const audit = nativePath(fixture.root, "claude-credential-access.json");
      const accesses: string[] = [];
      replaceText(audit, "[]");
      // The stand-in's credential stays in an owned service; its only read endpoint is audited.
      // Claude Code emits auth facts, never the credential, on its status stdout.
      let signedIn = false;
      const env = claudeCodeEnvironment(fixture.root, settingsEnvironment(fixture));
      const vault = await serveLoopbackHttp((request, response) => {
        request.resume();
        if (request.url === "/state") {
          response.writeHead(signedIn ? 200 : 404).end();
          return;
        }
        const path = request.url ?? "";
        accesses.push(path);
        replaceText(audit, JSON.stringify(accesses));
        switch (path) {
          case "/login": signedIn = true; response.end(); break;
          case "/logout": signedIn = false; response.end(); break;
          case "/credential": response.writeHead(signedIn ? 200 : 404).end(signedIn ? CLAUDE_CREDENTIAL : ""); break;
          default: response.writeHead(404).end();
        }
      });
      return { fixture, env: { ...env, DESKTOP_TEST_CLAUDE_VAULT: vault.url }, data: { vault: vault.url, audit },
        browser: { dynamic: true }, release: vault.close };
    },
    async (browser, { fixture, data: { vault, audit } }) => {
      const status = () => browser.$('[data-testid="claude-agent-status"]').getText();
      const shows = (expected: string, why: string) =>
        browser.waitUntil(async () => (await status()).trim() === expected && !(await browser.$("button=Cancel sign-in").isExisting()),
          { timeout: 15000, timeoutMsg: why });
      const signedIn = "Signed in · claude.ai · max";
      const accesses = () => JSON.parse(readText(audit)) as string[];
      const vaultState = () => httpGet(`${vault}/state`);
      const noCredentialInState = (directory: typeof fixture.state) => {
        if (!exists(directory)) return;
        for (const file of listFiles(directory)) {
          assert.equal(Buffer.from(readBytes(file)).includes(CLAUDE_CREDENTIAL), false, "Artifactize did not retain the Claude credential");
        }
        for (const child of childDirectories(directory)) noCredentialInState(child);
      };
      /** Sign in with Claude Code; returns the loopback origin of the running `claude auth login`. */
      const visits = nativePath(fixture.root, "claude-visits");
      const visited = () => (exists(visits) ? readText(visits).split("\n").filter(Boolean) : []);
      async function begin(): Promise<string> {
        const before = visited().length;
        await browser.$("button=Sign in with Claude Code").click();
        // In progress, said in the sign-in's fact, with Cancel sign-in; what finishes it by hand
        // stays folded until asked for.
        await browser.waitUntil(async () => (await status()).trim() === "sign-in in progress" && (await browser.$("button=Cancel sign-in").isExisting()),
          { timeout: 15000, timeoutMsg: "the sign-in is in progress" });
        assert.equal(await browser.$("#claude-agent-code").isExisting(), false, "Finish by hand starts folded");
        // unfolding shows the URL and the code field, the sign-in
        // still in progress.
        await browser.$("button=Finish by hand").click();
        await browser.$("#claude-agent-code").waitForExist({ timeout: 15000, timeoutMsg: "the sign-in waits for the browser or a code" });
        assert.equal((await status()).trim(), "sign-in in progress", "unfolding Finish by hand leaves the sign-in in progress");
        const address = new URL(await browser.$("pre.url").getText());
        assert.equal(address.pathname, "/oauth/authorize", "the URL Claude Code printed is shown");
        // The shown origin is the running Claude Code's own, and the browser it opened visited
        // that URL.
        assert.equal(await httpGet(`${address.origin}/alive`), 200);
        await browser.waitUntil(() => claudeBrowserOpened(fixture.root).includes(address.href)
          && visited().length > before && visited().at(-1) === `${address.pathname}${address.search}`,
          { timeout: 15000, timeoutMsg: `the browser Claude Code opened visited ${address.pathname}${address.search}: ${JSON.stringify(visited())}` });
        return address.origin;
      }
      async function signOut() {
        await browser.$("button=Sign out of Claude Code").click();
        await shows("Not signed in", "Claude Code signs out");
        assert.equal(await vaultState(), 404, "claude auth logout removed its credential");
      }

      await openSettings(browser);
      await choose(browser, "Claude Agent");
      await shows("Not signed in", "Claude Code reports it is not signed in");
      assert.equal(await browser.$("#provider-key").isExisting(), false, "Claude Agent takes no key");

      // Claude Code that cannot report its sign-in shows the error it failed with, and Claude
      // Code taken off PATH shows as not found, with nothing to sign in with; Check again asks
      // again each time, and shows it signed out once it is back and answers.
      const broken = nativePath(fixture.root, "claude-broken");
      writeText(broken, "");
      await browser.$("button=Check again").click();
      await shows("Claude Code could not report its sign-in: error: unknown command 'auth'", "Claude Code's own error is shown");
      const statusRed = await browser.execute(() => {
        const fact = document.querySelector('[data-testid="claude-agent-status"]');
        const token = getComputedStyle(document.documentElement).getPropertyValue("--red").trim();
        const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(token);
        const red = hex ? `rgb(${hex.slice(1).map((part) => Number.parseInt(part, 16)).join(", ")})` : token;
        return !!fact && getComputedStyle(fact).color === red;
      });
      assert.ok(statusRed, "in red, as a fact of the grid");
      removeTree(broken);
      claudeCodeInstalled(fixture.root, false);
      await browser.$("button=Check again").click();
      await shows("claude not found", "Claude Code is reported as not found");
      assert.equal(await browser.$("button=Sign in with Claude Code").isEnabled(), false, "nothing to sign in with while Claude Code is not found");
      claudeCodeInstalled(fixture.root, true);
      await browser.$("button=Check again").click();
      await shows("Not signed in", "Claude Code is found again and reports it is not signed in");
      assert.equal(await browser.$("button=Sign in with Claude Code").isEnabled(), true, "signing in is offered again");

      // Claude Code signed in elsewhere, in a terminal: Check again asks it again.
      assert.equal(await httpGet(`${vault}/login`), 200);
      assert.equal((await status()).trim(), "Not signed in");
      await browser.$("button=Check again").click();
      await shows(signedIn, "Check again shows Claude Code signed in");
      // Signed in, the facts stand alone.
      assert.equal(await browser.$('[data-testid="claude-agent-sign-in"]').isExisting(), false, "no line about the login once signed in");
      await signOut();

      // A cancelled sign-in stops Claude Code before it signs in.
      const cancelled = claudeLoginOwner(fixture.root, await begin());
      assert.ok(processRunning(cancelled), "the recorded login process is genuinely alive before Cancel");
      await browser.$("button=Cancel sign-in").click();
      await shows("Not signed in", "the cancelled sign-in ends");
      await browser.$("dd*=Claude Code sign-in was cancelled.").waitForExist({ timeout: 15000 });
      await browser.waitUntil(() => !processRunning(cancelled),
        { timeout: 15000, timeoutMsg: "the exact Claude Code process exits after cancellation" });
      assert.equal(await vaultState(), 404, "cancelling left Claude Code signed out");

      // A code Claude Code refuses ends its sign-in with Claude Code's own error.
      await begin();
      await fill(browser, "#claude-agent-code", "bad-code");
      await browser.$("button=Finish sign-in").click();
      await shows("Not signed in", "the refused sign-in ends");
      await browser.$("dd=Claude Code could not sign in: Login failed: That code was not accepted.").waitForExist({ timeout: 15000 });

      // The code the sign-in page gives, pasted.
      await begin();
      await fill(browser, "#claude-agent-code", "good-code");
      const beforeLogin = accesses().length;
      await browser.$("button=Finish sign-in").click();
      await shows(signedIn, "the pasted code signs Claude Code in");
      await commandsRendered(browser);
      assert.equal(await vaultState(), 200);
      assert.deepEqual(accesses().slice(beforeLogin), ["/login", "/credential"], "only Claude Code's login and its subsequent status use the credential service");
      const beforeStatus = accesses().length;
      await browser.$("button=Check again").click();
      await browser.waitUntil(() => accesses().length > beforeStatus, { timeout: 15000, timeoutMsg: "Check again reached Claude Code's credential lookup" });
      await commandsRendered(browser);
      await shows(signedIn, "Check again renders Claude Code's account facts");
      assert.deepEqual(accesses().slice(beforeStatus), ["/credential"], "only Claude Code's one status reads the credential");
      assert.equal((await browser.execute(() => document.documentElement.outerHTML)).includes(CLAUDE_CREDENTIAL), false, "the credential never reaches the settings page");
      noCredentialInState(fixture.state);
      await signOut();
      noCredentialInState(fixture.state);

      // The browser returns to Claude Code.
      const returned = await begin();
      assert.equal(await httpGet(`${returned}/callback?code=e2e`), 200);
      await shows(signedIn, "the browser's return signs Claude Code in");
      await signOut();
    },
  ),
  // UX journeys: J-SET-PROVIDERS J-SET-KEY J-SET-MODELS
  scenario(
    "settings-keys",
    "settings: lists the providers on the left, shows where each key comes from without its value, saves and removes a key as the CLI sees it, and keeps a set variable first",
    async () => {
      const fixture = createOutside();
      const seen = nativePath(fixture.root, "openai-authorization");
      const models = nativePath(fixture.root, "openai-models");
      writeText(seen, "");
      writeText(models, "hold");
      // Everything that can fail comes before the server starts, so it is always released.
      const env = settingsEnvironment(fixture);
      const openai = await serveLoopbackHttp((request, response) => {
        request.resume();
        const listing = request.url === "/v1/models";
        if (!listing) { response.writeHead(404).end(); return; }
        writeText(seen, request.headers.authorization ?? "");
        void observe(() => response.destroyed || readText(models) !== "hold", () => "the fixture never released the model response")
          .then(() => {
            if (response.destroyed) return;
            const mode = readText(models);
            const data = mode === "list" ? [{ id: "openai-model", object: "model", created: 0, owned_by: "e2e" }] : [];
            response.writeHead(mode === "error" ? 503 : 200, { "content-type": "application/json" });
            response.end(JSON.stringify(mode === "error" ? { error: { message: "Fixture catalog unavailable" } } : { object: "list", data }));
          }, (error: Error) => { if (!response.destroyed) response.writeHead(500).end(error.message); });
      });
      return {
        fixture,
        env: { ...env, ARTIFACTIZE_OPENAI_BASE_URL: `${openai.url}/v1` },
        data: { key: KEY, saved: SAVED, shadowed: SHADOWED, seen, models },
        release: () => openai.close(),
      };
    },
    async (browser, { fixture, data: { key, saved, shadowed, seen, models } }) => {
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      await openSettings(browser);
      const providers = ["Codex", "Claude Agent", "OpenAI", "Anthropic", "Gemini", "Custom provider"];
      await browser.waitUntil(async () => JSON.stringify(await listed(browser)) === JSON.stringify(providers), {
        timeout: 15000,
        timeoutMsg: "the providers are listed",
      });
      await choose(browser, "OpenAI");
      await browser.waitUntil(async () => (await keyText(browser, "openai")) === "set from the environment (OPENAI_API_KEY)", { timeout: 15000, timeoutMsg: "the OpenAI key is set from its variable" });
      // A vertical list on the left; the chosen provider's setup on its right.
      const layout = await browser.execute(() => {
        const nav = document.querySelector('nav[aria-label="Providers"]')!.getBoundingClientRect();
        const items = [...document.querySelectorAll('nav[aria-label="Providers"] button')].map((button) => button.getBoundingClientRect());
        const setup = document.querySelector('[data-testid="key-openai"]')!.closest("section")!.getBoundingClientRect();
        return { right: nav.right, setup: setup.left, lefts: items.map((item) => Math.round(item.left)), tops: items.map((item) => item.top) };
      });
      assert.ok(layout.right <= layout.setup, `the list (right ${layout.right}) is left of the setup (left ${layout.setup})`);
      assert.equal(new Set(layout.lefts).size, 1, "the providers line up in one column");
      assert.ok(layout.tops.every((top, index) => index === 0 || top > layout.tops[index - 1]), "each provider is below the one before");
      await choose(browser, "Anthropic");
      await browser.waitUntil(async () => (await keyText(browser, "anthropic")) === "not set", { timeout: 15000, timeoutMsg: "the Anthropic key is not set" });
      assert.ok(!(await html()).includes(key), "the key's value is nowhere on the page");

      await choose(browser, "Gemini");
      await browser.$("#provider-key").click();
      await typeText(browser, saved);
      assert.equal(await browser.$("#provider-key").getAttribute("type"), "password");
      await browser.$("button=Save key").click();
      await browser.waitUntil(async () => (await keyText(browser, "gemini")) === "saved here", { timeout: 15000, timeoutMsg: "the Gemini key is saved" });
      assert.equal(await browser.$("#provider-key").getValue(), "", "the key left the field");
      assert.ok(!(await html()).includes(saved), "the saved key is nowhere on the page");
      const after = await providerList(fixture, { OPENAI_API_KEY: key });
      assert.equal(after.code, 0, after.stdout);
      assert.ok(!after.stdout.includes(saved), "the CLI never prints the key");
      assert.deepEqual(sources(after.stdout), { openai: "environment", anthropic: "absent", gemini: "saved" });

      await browser.$("button=Remove saved key").click();
      await browser.waitUntil(async () => (await keyText(browser, "gemini")) === "not set", { timeout: 15000, timeoutMsg: "the saved Gemini key is removed" });
      const removed = await providerList(fixture, { OPENAI_API_KEY: key });
      assert.equal(sources(removed.stdout).gemini, "absent");

      // A key saved while its variable is set: the variable stays the key shown and used.
      await choose(browser, "OpenAI");
      await browser.$("#provider-key").click();
      await typeText(browser, shadowed);
      await browser.$("button=Save key").click();
      const shadowedMessage = "set from the environment (OPENAI_API_KEY); a saved key is kept but not used";
      await browser.waitUntil(async () => (await keyText(browser, "openai")) === shadowedMessage,
        { timeout: 15000, timeoutMsg: "OpenAI shows that the saved key is shadowed by the environment" });
      assert.equal(await browser.$("#provider-key").getValue(), "", "the stored value left the input");
      assert.equal(await keyText(browser, "openai"), shadowedMessage);
      const both = JSON.parse((await providerList(fixture, { OPENAI_API_KEY: key })).stdout).keys.find((one: { provider: string }) => one.provider === "openai");
      assert.deepEqual([both.source, both.saved], ["environment", true]);
      const list = () => browser.$("button=List models");
      const shown = () => browser.$('ul[aria-label="Models"]');
      await list().click();
      await browser.waitUntil(async () => readText(seen) === `Bearer ${key}` && !(await list().isEnabled()),
        { timeout: 15000, timeoutMsg: "the request uses the environment key and the List models button is disabled until the reply" });
      replaceText(models, "list");
      await browser.waitUntil(async () => await shown().isDisplayed() && (await shown().getText()).includes("openai-model") && await list().isEnabled(),
        { timeout: 15000, timeoutMsg: "the released catalog is rendered and its button re-enabled" });
      await choose(browser, "Gemini");
      await browser.waitUntil(async () => !(await shown().isDisplayed()), { timeout: 15000, timeoutMsg: "changing provider empties the list" });
      await choose(browser, "OpenAI");
      assert.equal(await shown().isDisplayed(), false, "returning to OpenAI does not resurrect its old list");
      replaceText(models, "empty");
      await list().click();
      await browser.$("p=No models listed.").waitForDisplayed();
      replaceText(models, "error");
      await list().click();
      await browser.waitUntil(async () => (await browser.$('[role="dialog"]').getText()).includes("Fixture catalog unavailable"),
        { timeout: 15000, timeoutMsg: "the listing failure is shown in Settings" });
      await browser.$("button=Remove saved key").click();
      await browser.waitUntil(async () => !(await browser.$("button=Remove saved key").isExisting()), { timeout: 15000, timeoutMsg: "the saved OpenAI key is removed" });
      assert.equal(await keyText(browser, "openai"), "set from the environment (OPENAI_API_KEY)", "the variable is still set");
    },
  ),
  // UX journey: J-SET-KEY
  scenario(
    "settings-keys-anthropic",
    "shows Anthropic set and OpenAI not set without exposing the Anthropic fixture value",
    async () => {
      const fixture = createOutside();
      const gemini = `${KEY}-gemini`;
      return { fixture, env: { ...settingsEnvironment(fixture), OPENAI_API_KEY: undefined, ANTHROPIC_API_KEY: KEY, GEMINI_API_KEY: gemini }, data: { key: KEY, gemini } };
    },
    async (browser, { data: { key, gemini } }) => {
      await openSettings(browser);
      await choose(browser, "OpenAI");
      await browser.waitUntil(async () => (await keyText(browser, "openai")) === "not set", { timeout: 15000, timeoutMsg: "the OpenAI key is not set" });
      await choose(browser, "Anthropic");
      await browser.waitUntil(async () => (await keyText(browser, "anthropic")) === "set from the environment (ANTHROPIC_API_KEY)", { timeout: 15000, timeoutMsg: "the Anthropic key is set from its variable" });
      assert.ok(!(await browser.execute(() => document.documentElement.outerHTML)).includes(key), "the Anthropic fixture value is nowhere in the page");
      await choose(browser, "Gemini");
      await browser.waitUntil(async () => (await keyText(browser, "gemini")) === "set from the environment (GEMINI_API_KEY)",
        { timeout: 15000, timeoutMsg: "Gemini reports its environment key source" });
      assert.ok(!(await browser.execute(() => document.documentElement.outerHTML)).includes(gemini), "the Gemini fixture value is nowhere in the page");
    },
  ),
  // Sign-in failure regression; J-SET-CLAUDE-AGENT does not prescribe a deadline.
  scenario<null>(
    "settings-claude-timeout",
    "shows the timeout and refreshes sign-in status when an unanswered Claude Code sign-in ends",
    async () => {
      const fixture = createOutside();
      return { fixture, env: claudeCodeEnvironment(fixture.root, settingsEnvironment(fixture), 660000), data: null, browser: { dynamic: true } };
    },
    async (browser, { fixture }) => {
      const status = () => browser.$('[data-testid="claude-agent-status"]').getText();
      await openSettings(browser);
      await choose(browser, "Claude Agent");
      await browser.waitUntil(async () => (await status()) === "Not signed in", { timeout: 15000, timeoutMsg: "Claude Code starts signed out" });
      await browser.$("button=Sign in with Claude Code").click();
      await browser.waitUntil(async () => (await status()) === "sign-in in progress" && await browser.$("button=Cancel sign-in").isDisplayed(),
        { timeout: 15000, timeoutMsg: "real sign-in starts and waits, with no code or browser callback" });
      await browser.waitUntil(() => claudeBrowserOpened(fixture.root).length === 1
        && exists(nativePath(fixture.root, "claude-visits"))
        && readText(nativePath(fixture.root, "claude-visits")).split("\n").includes("/oauth/authorize?code=true"),
      { timeout: 15000, timeoutMsg: "Claude Code opened its unanswered sign-in page through the fixture browser" });
      await browser.waitUntil(async () => (await status()) === "Not signed in" &&
        await browser.$("dd*=Claude Code sign-in timed out").isDisplayed() && await browser.$("button=Sign in with Claude Code").isEnabled(),
      { timeout: 620000, interval: 1000, timeoutMsg: "the unanswered sign-in reports the timeout and refreshes its status" });
      assert.equal(exists(nativePath(fixture.root, "signed-in")), false);
    },
  ),
  // UX journey: J-SET-SIGNOFF-GRACE
  scenario(
    "settings-signoff-grace",
    "settings: shows the sign-off idle grace, saves a new one the CLI reads, and a later Run ends by it",
    async () => {
      const fixture = createSignoffOnly();
      const later = nativePath(fixture.root, "later");
      const gate = nativePath(fixture.root, "later.gate");
      writeDeclaration(later, nativePath(later, "human", "index.artf"), `name = "human"
[evals.signoff]
title = "Review the later work"
profile = { kind = "human" }
payload.instruction = "Review the later work."
`);
      writeDeclaration(later, nativePath(later, "worker", "index.artf"), `name = "worker"
[evals.check]
title = "Wait until the sign-off is claimed"
profile = { kind = "runtime", ${commandFields("talk", portablePath(gate))} }
payload.instruction = "Check the worker."
`);
      git(later, "init", "-q", "--initial-branch=master", later);
      return { fixture, env: settingsEnvironment(fixture), data: { later, gate } };
    },
    async (browser, { fixture, data: { later, gate } }) => {
      // The grace in force is in the seconds field that changes it; once saved, Save has nothing
      // more to save.
      const shown = () => browser.$("#grace-seconds").getValue();
      const saved = async () => !(await browser.$("button=Save").isEnabled());
      await showLimits(browser);
      await browser.waitUntil(async () => (await shown()) === "120", { timeout: 15000, timeoutMsg: "the default idle grace" });
      // One visible line says what the grace is.
      const said = await browser.$('section[aria-labelledby="grace-title"] p').getText();
      assert.equal(said, "How long a Run with only sign-offs left, none of them claimed, waits before it ends.", "Limits says what the grace is");
      assert.ok(await browser.$('section[aria-labelledby="grace-title"] p').isDisplayed(), "that line is shown");
      assert.ok(await saved(), "Save is disabled for the unchanged grace");
      await fill(browser, "#grace-seconds", "300");
      await browser.$("button=Save").waitForEnabled();
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => signoffGrace(fixture) === 300 && (await saved()) && (await shown()) === "300", { timeout: 15000, timeoutMsg: "the saved idle grace" });
      const limitsSaid = () => browser.execute(() =>
        [...document.querySelectorAll('section[aria-labelledby="grace-title"] [role="status"], section[aria-labelledby="grace-title"] [role="alert"]')]
          .map((line) => line.textContent?.trim() ?? "").filter(Boolean));
      assert.deepEqual(await limitsSaid(), [], "a successful save says nothing; the field holds the new grace");
      // A save the state refuses is said in Limits, and the grace in force stays.
      const limitsFile = nativePath(fixture.state, "limits.json");
      const keptLimits = readText(limitsFile);
      removeTree(limitsFile);
      makeDirectory(limitsFile);
      await fill(browser, "#grace-seconds", "200");
      await browser.$("button=Save").waitForEnabled();
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => (await limitsSaid()).some((line) => /limits\.json/.test(line)), { timeout: 15000, timeoutMsg: "the failed save is said in Limits" });
      removeTree(limitsFile);
      writeText(limitsFile, keptLimits);
      assert.equal(signoffGrace(fixture), 300, "the failed save changed nothing");
      // Set by the CLI, it shows when Settings is shown again.
      assert.equal(artifactize(fixture, "limits", "--signoff-idle-grace", "45"), 0);
      await showLimits(browser);
      await browser.waitUntil(async () => (await shown()) === "45", { timeout: 15000, timeoutMsg: "the idle grace the CLI set" });
      // A day's grace from the CLI, then none from Settings: a Run started afterwards with only
      // its unclaimed sign-off left ends YELLOW at once, which only the saved 0 allows.
      assert.equal(artifactize(fixture, "limits", "--signoff-idle-grace", "86400"), 0);
      await showLimits(browser);
      await browser.waitUntil(async () => (await shown()) === "86400", { timeout: 15000, timeoutMsg: "the day the CLI set" });
      assert.equal(artifactize(fixture, "verify", "--all"), 0);
      await browser.waitUntil(() => newestRunStatus(fixture) === "WAITING_HUMAN", { timeout: 15000, timeoutMsg: "the first Run is already waiting with its original grace" });
      const prior = savedRuns(fixture)[0].id;
      await fill(browser, "#grace-seconds", "0");
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => signoffGrace(fixture) === 0 && (await saved()), { timeout: 15000, timeoutMsg: "no idle grace saved" });
      const next = { ...fixture, repo: later };
      await browser.$("button=Settings").click();
      await browser.waitUntil(async () => !(await settingsOpen(browser)), { timeout: 15000, timeoutMsg: "Settings closed" });
      assert.equal(artifactize(next, "verify", "--all"), 0);
      const laterReview = () => requests(next).find((request) => request.evalId === "human/signoff");
      await browser.waitUntil(() => laterReview()?.status === "DEFERRED", { timeout: 15000, timeoutMsg: "the later sign-off is ready while its machine eval is still gated" });
      await openWaiting(browser, laterReview()!.id);
      await browser.$("button=Claim to review").click();
      await browser.waitUntil(() => laterReview()?.claim?.reviewer === "alice", { timeout: 15000, timeoutMsg: "the later sign-off is claimed before the Run becomes idle" });
      writeText(gate, "go");
      await browser.waitUntil(async () => newestRunStatus(next) === "WAITING_HUMAN" && await browser.$("button=Release claim").isDisplayed(),
        { timeout: 15000, timeoutMsg: "the later Run does not expire after becoming idle with a claimed review and grace zero" });
      assert.equal(laterReview()?.claim?.reviewer, "alice");
      const laterRun = laterReview()!.runId;
      assert.ok(laterRun, "the claimed request belongs to the later Run");
      const laterStatus = async () => (await branch(browser, "later  master"))?.chips.find((chip) => chip.id === laterRun)?.text ?? "";
      await showRun(browser, "later  master", laterRun);
      await browser.waitUntil(async () => /^Waiting for sign-off\b/i.test(await laterStatus()),
        { timeout: 15000, timeoutMsg: "the later Run's timeline shows it waiting before release" });
      await openWaiting(browser, laterReview()!.id);
      await browser.$("button=Release claim").click();
      await browser.waitUntil(async () => /^YELLOW\b/.test(await laterStatus()) && newestRunStatus(next) === "YELLOW", {
        timeout: 15000, timeoutMsg: "the later Run ends YELLOW in the timeline and CLI after release",
      });
      assert.equal(savedRuns(fixture).some(({ id }) => id === prior), true);
      assert.equal(newestRunStatus(fixture), "WAITING_HUMAN", "saving zero never shortened the original Run's grace");
    },
  ),
  // UX journey: J-SET-CUSTOM
  scenario(
    "settings-custom",
    "settings: adds an endpoint with a key, an alias and a keyless endpoint, changes one, reviews an eval by a custom name, and removes one, as the CLI sees it",
    async () => {
      const fixture = createOutside();
      const seen = nativePath(fixture.root, "endpoint-authorization");
      const alias = nativePath(fixture.root, "alias-authorization");
      const review = nativePath(fixture.root, "endpoint-review");
      for (const path of [seen, alias, review]) writeText(path, "");
      // Everything that can fail comes before the server starts, so it is always released.
      const env = settingsEnvironment(fixture);
      const endpoint = await serveLoopbackHttp((request, response) => {
        const chunks: Buffer[] = [];
        request.on("data", (chunk: Buffer) => chunks.push(chunk));
        request.on("end", () => {
          const json = (code: number, body: object) => {
            response.writeHead(code, { "content-type": "application/json" });
            response.end(JSON.stringify(body));
          };
          if (request.method === "GET" && request.url === "/v1/models") {
            writeText(seen, request.headers.authorization ?? "");
            // Anthropic's API names each model; OpenAI's has only its ID.
            json(200, { object: "list", data: [{ id: "local-model", display_name: "Local Model", object: "model", created: 0, owned_by: "e2e" }] });
          } else if (request.method === "GET" && request.url === "/oai/v1/models") {
            writeText(alias, request.headers.authorization ?? "");
            json(200, { object: "list", data: [{ id: "alias-model", object: "model", created: 0, owned_by: "e2e" }] });
          } else if (request.method === "POST" && request.url === "/v1/chat/completions") {
            writeText(review, JSON.stringify({ authorization: request.headers.authorization ?? "", body: JSON.parse(Buffer.concat(chunks).toString("utf8")) }));
            response.writeHead(200, { "content-type": "text/event-stream" });
            response.end(streamed('{"verdict":"GREEN"}'));
          } else json(404, {});
        });
      });
      return {
        fixture,
        env: { ...env, ARTIFACTIZE_OPENAI_BASE_URL: `${endpoint.url}/oai/v1` },
        data: { root: endpoint.url, url: `${endpoint.url}/v1`, seen, alias, review, key: ENDPOINT_KEY },
        release: () => endpoint.close(),
      };
    },
    async (browser, { fixture, data: { root, url, seen, alias, review, key } }) => {
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      const custom = async () => {
        const listing = await providerList(fixture);
        assert.equal(listing.code, 0, listing.stdout);
        assert.ok(!listing.stdout.includes(key), "the CLI never prints the endpoint key");
        return JSON.parse(listing.stdout).custom as Record<string, unknown>;
      };
      /** Custom provider, named `name`, saved after `fill` sets it up. */
      const add = async (name: string, fill: () => Promise<void>) => {
        await choose(browser, "Custom provider");
        await browser.$("#custom-name").waitForExist({ timeout: 15000 });
        assert.equal(await browser.$("#custom-name").getValue(), "", "a new custom provider starts empty");
        await browser.$("#custom-name").click();
        await typeText(browser, name);
        await fill();
        await browser.$("button=Save").click();
        await browser.waitUntil(async () => (await listed(browser)).includes(name) && (await browser.$("button=Save").isEnabled()),
          { timeout: 15000, timeoutMsg: `${name} is saved and listed` });
        assert.equal(await browser.$('nav[aria-label="Providers"]').$(`button=${name}`).getAttribute("aria-current"), "true", `${name} is chosen`);
      };
      await openSettings(browser);

      // An OpenAI-compatible endpoint with a key.
      await add("local", async () => {
        await browser.$("#custom-url").click();
        await typeText(browser, url);
        await browser.$("#custom-key").click();
        await typeText(browser, key);
      });
      assert.ok(!(await html()).includes(key), "the endpoint key is nowhere on the page");
      assert.deepEqual(await custom(), { local: { kind: "endpoint", api: "openai-chat", url, key: true } });
      await listModels(browser, "local-model");
      assert.equal(readText(seen), `Bearer ${key}`, "the listing used the saved key");

      // An eval names it in backend and is reviewed through it.
      const folder = makeDirectory(nativePath(fixture.repo, "agent"));
      writeDeclaration(fixture.repo, nativePath(folder, "index.artf"), `name = "agent"

[evals.review]
title = "Review"
profile = { kind = "agent", backend = "local", model = "local-model", reasoning = "high" }

[evals.review.payload]
instruction = """
Review the agent folder."""
`);
      const run = await verifyAll(fixture);
      assert.equal(run.code, 0, run.stdout);
      assert.equal((JSON.parse(run.stdout) as { requests: { status: string }[] }).requests[0].status, "GREEN");
      const reviewed = JSON.parse(readText(review)) as { authorization: string; body: { model: string; reasoning_effort: string } };
      assert.equal(reviewed.authorization, `Bearer ${key}`, "the review used the saved key");
      assert.equal(reviewed.body.model, "local-model");
      assert.equal(reviewed.body.reasoning_effort, "high");

      // An alias of a built-in provider, with that provider's key (its variable here).
      await add("work", async () => {
        await browser.$('input[name="custom-kind"][value="alias"]').click();
        await select(browser, "#custom-provider", "openai");
      });
      await listModels(browser, "alias-model");
      assert.equal(readText(alias), `Bearer ${KEY}`, "the alias used OpenAI's key");

      // An endpoint of another API without a key.
      await add("claude-local", async () => {
        await select(browser, "#custom-api", "anthropic");
        await browser.$("#custom-url").click();
        await typeText(browser, root);
      });
      assert.deepEqual(await listed(browser), ["Codex", "Claude Agent", "OpenAI", "Anthropic", "Gemini", "claude-local", "local", "work", "Custom provider"]);
      // Each model by its name, its ID beside it.
      await listModels(browser, "Local Model");
      const models = await browser.execute(() => [...document.querySelectorAll('ul[aria-label="Models"] li')]
        .map((item) => [item.firstChild?.textContent?.trim() ?? "", item.querySelector("code")?.textContent?.trim() ?? ""]));
      assert.deepEqual(models, [["Local Model", "local-model"]], "the model's name with its ID beside it");
      assert.deepEqual(await custom(), {
        "claude-local": { kind: "endpoint", api: "anthropic", url: root, key: false },
        local: { kind: "endpoint", api: "openai-chat", url, key: true },
        work: { kind: "alias", provider: "openai" },
      });

      // Changed with its key left empty: the saved key stays, and the others are untouched.
      await choose(browser, "local");
      assert.equal(await browser.$("#custom-name").getValue(), "local");
      assert.equal(await browser.$("#custom-key").getAttribute("placeholder"), "Saved; type to replace");
      await select(browser, "#custom-api", "openaiResponses");
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => ((await custom()).local as { api?: string } | undefined)?.api === "openai-responses",
        { timeout: 15000, timeoutMsg: "local is saved again" });
      const changed = await custom();
      assert.deepEqual(changed.local, { kind: "endpoint", api: "openai-responses", url, key: true });
      assert.deepEqual(Object.keys(changed), ["claude-local", "local", "work"]);
      // Reopen from the saved list rather than reading the draft option left by our own input.
      await choose(browser, "work");
      await choose(browser, "local");
      await browser.waitUntil(async () => await browser.$("#custom-api").getValue() === "openaiResponses"
        && await browser.$("#custom-url").getValue() === url
        && await browser.$("#custom-key").getAttribute("placeholder") === "Saved; type to replace",
      { timeout: 15000, timeoutMsg: "Settings reopens the saved responses endpoint and its kept key" });

      const geminiUrl = `${root}/gemini`;
      await add("gemini-local", async () => {
        await select(browser, "#custom-api", "gemini");
        await fill(browser, "#custom-url", geminiUrl);
      });
      assert.deepEqual((await custom())["gemini-local"], { kind: "endpoint", api: "gemini", url: geminiUrl, key: false });
      await choose(browser, "local");
      await choose(browser, "gemini-local");
      await browser.waitUntil(async () => await browser.$("#custom-api").getValue() === "gemini"
        && await browser.$("#custom-url").getValue() === geminiUrl
        && await browser.$("#custom-key").getValue() === ""
        && await browser.$("#custom-key").getAttribute("placeholder") !== "Saved; type to replace",
      { timeout: 15000, timeoutMsg: "Settings reopens the saved keyless Gemini endpoint" });
      await choose(browser, "local");

      await browser.$("button=Remove provider").click();
      await browser.waitUntil(async () => !(await listed(browser)).includes("local"), { timeout: 15000, timeoutMsg: "local is removed" });
      await browser.waitUntil(async () => !("local" in await custom()),
        { timeout: 15000, timeoutMsg: "the CLI reads the persisted provider removal" });
      assert.deepEqual(Object.keys(await custom()), ["claude-local", "gemini-local", "work"]);
    },
  ),
  // UX journeys: J-SET-REMOTE J-SET-REMOTE-STATUS
  scenario(
    "settings-remote",
    "settings: remote sign-in, shared CLI status, fresh identity, dry run and sign-out",
    async () => {
      const fixture = createOutside();
      const env = settingsEnvironment(fixture);
      // One saved GREEN result to push, and a signed-off Human result the store's token (without
      // the human scope) cannot take, so a push skips it.
      writeDeclaration(fixture.repo, nativePath(fixture.repo, "index.artf"), `name = "repo"\n[evals.check]\ntitle = "Check"\nprofile = ${runtimeProfile()}\npayload.instruction = "Check."\n`
        + `[evals.review]\ntitle = "Review"\nprofile = { kind = "human" }\npayload.instruction = "Review."\n`);
      git(fixture.repo, "init", "-q", "--initial-branch=master", fixture.repo);
      const run = await verifyWaiting(fixture);
      const review = requests(fixture).find((request) => request.evalId === "repo/review");
      if (!review) throw new Error("no repo/review request");
      signOff(fixture, review.id, "{}");
      assert.equal(ended(fixture, run), "GREEN", "both results saved GREEN");
      // The outer owner serves HTTP while WDIO runs elsewhere. Only this fixture's
      // domain state crosses that process boundary, never a server or live closure: the
      // principal it answers with, and each record the store received.
      const principalFile = nativePath(fixture.root, "remote-principal");
      const receivedFile = nativePath(fixture.root, "remote-received");
      writeText(principalFile, "alice-laptop");
      writeText(receivedFile, "");
      // While this file exists the store holds its status answer, so a request stays in flight.
      const holdFile = nativePath(fixture.root, "remote-hold");
      // While this file exists the store refuses records, so a push fails.
      const refuseFile = nativePath(fixture.root, "remote-refuse");
      // Teardown stops held handlers and awaits every one before the files go.
      let closing = false;
      const inFlight = new Set<Promise<void>>();
      const store = await serveLoopbackHttp((request, response) => {
        const handled = (async () => {
          request.resume();
          for (let waited = 0; exists(holdFile) && !closing && waited < 30000; waited += 50) await new Promise((done) => setTimeout(done, 50));
          if (closing) { response.destroy(); return; }
          serve(request, response);
        })();
        inFlight.add(handled);
        void handled.finally(() => inFlight.delete(handled));
      });
      function serve(...[request, response]: Parameters<Parameters<typeof serveLoopbackHttp>[0]>) {
        const signedIn = request.headers.authorization === `Bearer ${TOKEN}`;
        const answer = (status: number, body: unknown) => {
          response.writeHead(status, { "content-type": "application/json" });
          response.end(JSON.stringify(body));
        };
        if (!signedIn) return answer(401, { error: "Invalid or revoked token." });
        if (request.method === "GET" && request.url === "/v1/whoami") {
          const principal = readText(principalFile);
          return principal ? answer(200, { principal, scopes: ["read", "publish"] }) : answer(503, { error: "Store temporarily unavailable." });
        }
        // Nothing is in the store yet; each record put is received once.
        if (request.method === "POST" && request.url === "/v1/lookup") return answer(200, { entries: [] });
        if (request.method === "PUT" && request.url?.startsWith("/v1/entries/")) {
          if (exists(refuseFile)) return answer(503, { error: "Store refused the record." });
          // The store keeps each record once: one sent again is not created again.
          if (readText(receivedFile).split("\n").includes(request.url)) return answer(200, { created: false });
          writeText(receivedFile, `${readText(receivedFile)}${request.url}\n`);
          return answer(200, { created: true });
        }
        return answer(404, { error: "Not found." });
      }
      return {
        fixture,
        env: { ...env, ARTIFACTIZE_E2E_DATE: String(Date.UTC(2026, 0, 12, 12) / 1000) },
        data: { url: `${store.url}/`, principalFile, receivedFile, holdFile, refuseFile, token: TOKEN },
        release: async () => {
          closing = true;
          // Every handler settles and the listener closes even when one of them failed.
          const failures = (await Promise.allSettled(inFlight))
            .flatMap((result) => result.status === "rejected" ? [result.reason as unknown] : []);
          try { await store.close(); } catch (error) { failures.push(error); }
          if (failures.length) throw new AggregateError(failures, "the remote store fixture failed");
        },
      };
    },
    async (browser, { fixture, data: { url, principalFile, receivedFile, holdFile, refuseFile, token } }) => {
      const received = () => readText(receivedFile).split("\n").filter(Boolean).length;
      const text = () => browser.execute(() => document.body.innerText);
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      await openSettings(browser);
      await browser.$("button=Remote").click();
      await browser.waitUntil(async () => (await browser.$('[role="tab"][aria-selected="true"]').getText()) === "Remote", { timeout: 15000, timeoutMsg: "the Remote tab shows" });
      // No store yet: the fields that sign in to one, and nothing said about it.
      await browser.$("button=Sign in to the store").waitForExist({ timeout: 15000, timeoutMsg: "no store is configured yet" });
      const signedOutCheck = () => browser.$("button=Check status");
      assert.ok(await signedOutCheck().isDisplayed() && await signedOutCheck().isEnabled(), "signed-out Settings offers Check status beside Sign in");
      // This click's own status request completes before the panel is read.
      const statusCalls = async () => (await checkpoint(browser)).activity.commands.remoteStatus?.completed ?? 0;
      const checked = await statusCalls();
      await signedOutCheck().click();
      await browser.waitUntil(async () => await statusCalls() > checked, { timeout: 15000, timeoutMsg: "the signed-out Check status request completed" });
      await commandsRendered(browser);
      assert.ok(await browser.$("button=Sign in to the store").isDisplayed(), "checking without a store keeps the sign-in view");
      assert.ok(!(await text()).includes("No remote review store is configured."), "no sentence for the normal state");
      await browser.$("#remote-url").click();
      await typeText(browser, url);
      await browser.$("#remote-token").click();
      await typeText(browser, token);
      await browser.$('input[name="share"][value="full"]').click();
      assert.equal(await browser.$("#remote-token").getAttribute("type"), "password");
      // While a request is in flight every button of the tab is disabled; the store holds its
      // answer while the hold file exists.
      const remoteButtons = () => browser.execute(() =>
        [...document.querySelectorAll<HTMLButtonElement>('section[aria-labelledby="remote-title"] button')].map((button) => button.disabled));
      const whileHeld = async (button: string, what: string) => {
        writeText(holdFile, "");
        await browser.$(`button=${button}`).click();
        await browser.waitUntil(async () => { const states = await remoteButtons(); return states.length > 0 && states.every(Boolean); },
          { timeout: 15000, timeoutMsg: `every Remote button disabled while ${what} is in flight` });
        removeTree(holdFile);
      };
      await whileHeld("Sign in to the store", "the sign-in");
      const storeFacts = 'section[aria-labelledby="remote-title"] dl[aria-label="Remote store"]';
      const facts = () => browser.execute((selector) => Object.fromEntries([...document.querySelectorAll(`${selector} > dt`)]
        .map((label) => [label.textContent?.trim(), (label.nextElementSibling as HTMLElement | null)?.innerText.trim()])), storeFacts);
      const statusLine = () => browser.$(`${storeFacts} dt:last-of-type + dd`);
      await browser.waitUntil(async () => await statusLine().isDisplayed()
        && await statusLine().getText() === "Signed in · alice-laptop (scopes: read, publish) · 0 pushed today",
      { timeout: 15000, timeoutMsg: "the Remote panel shows the reviewer and push count together in its status" });
      // Signed in: the store's URL and share level as facts, without the fields that sign in.
      assert.equal(await browser.$("#remote-token").isExisting(), false, "the token field is gone once signed in");
      assert.equal(await browser.$("#remote-url").isExisting(), false, "so is the URL field");
      assert.equal(await browser.$('input[name="share"]').isExisting(), false, "and the share level's choices");
      assert.equal(await browser.$("button=Sign in to the store").isExisting(), false, "and Sign in");
      const signedIn = await facts();
      assert.equal(signedIn["Store URL"], url, "the Remote panel's Store URL fact");
      assert.equal(signedIn["Share level"], "full", "the Remote panel's chosen Share level fact");
      assert.equal(signedIn.Status, "Signed in · alice-laptop (scopes: read, publish) · 0 pushed today", "both parts are the same Status fact");
      assert.ok(!(await html()).includes(token), "the token is nowhere on the page");

      // The asynchronous CLI query shares saved state without blocking this store.
      const status = await remoteStatus(fixture);
      assert.equal(status.code, 0, status.stdout);
      const parsed = JSON.parse(status.stdout);
      assert.equal(parsed.principal, "alice-laptop");
      assert.equal(parsed.tokenSource, "file");
      assert.equal(parsed.share, "full", "the chosen share level was saved");
      assert.ok(!status.stdout.includes(token));

      writeText(principalFile, "alice-desktop");
      await browser.$("button=Check status").click();
      await browser.waitUntil(async () => (await text()).includes("alice-desktop (scopes: read, publish)"), { timeout: 15000, timeoutMsg: "Check status displays the fresh remote principal" });
      await browser.waitUntil(() => browser.execute(() => document.querySelector('[aria-label="Remote store"] .notice [data-testid="remote-principal"]') !== null), { timeout: 15000, timeoutMsg: "the store is reachable" });
      /** What the status says was pushed today, or "" while it says nothing. */
      const pushedToday = async () => {
        const pushed = statusLine().$('[data-testid="remote-pushed"]');
        return await pushed.isDisplayed() ? await pushed.getText() : "";
      };
      // A dry run's summary takes the status line and stays there until Check status.
      await whileHeld("Push (dry run)", "the dry run");
      await browser.waitUntil(async () => await statusLine().isDisplayed() && /^Would push 1, already in the store 0, skipped 1\./.test(await statusLine().getText()),
        { timeout: 15000, timeoutMsg: "the dry run's summary takes the status line" });
      await commandsRendered(browser);
      assert.match(await statusLine().getText(), /^Would push 1, already in the store 0, skipped 1\./, "the summary stays until Check status");
      // A dry run pushes nothing: the store received nothing, and the status, read again,
      // says nothing was pushed today.
      assert.equal(received(), 0, "the dry run sent no record");
      // Only the response to the fresh status request has this principal; the old
      // zero counter cannot satisfy the wait while Check status is pending.
      writeText(principalFile, "alice-after-dry-run");
      await browser.$("button=Check status").click();
      await browser.waitUntil(async () => await statusLine().isDisplayed()
        && await statusLine().getText() === "Signed in · alice-after-dry-run (scopes: read, publish) · 0 pushed today",
      { timeout: 15000, timeoutMsg: "the fresh status after the dry run shows nothing pushed today" });
      // A real push: the store receives the record, and the same status counts it.
      await whileHeld("Push results", "the push");
      await browser.waitUntil(async () => await statusLine().isDisplayed() && /^Pushed 1, already in the store 0, skipped 1\./.test(await statusLine().getText()),
        { timeout: 15000, timeoutMsg: "the push's summary takes the status line" });
      assert.equal(received(), 1, "the store received the one record");
      // The push reports success before its follow-up status request enables the button.
      const checkStatus = browser.$("button=Check status");
      await checkStatus.waitForEnabled({ timeout: 15000, timeoutMsg: "the push's status refresh finished" });
      writeText(principalFile, "alice-after-push");
      await checkStatus.click();
      await browser.waitUntil(async () => await statusLine().isDisplayed()
        && await statusLine().getText() === "Signed in · alice-after-push (scopes: read, publish) · 1 pushed today",
      { timeout: 15000, timeoutMsg: "the fresh status after the real push counts it today" });
      // Pushed again, the record the store holds counts as already there and the sign-off is
      // skipped again; nothing new is sent.
      await whileHeld("Push results", "the second push");
      await browser.waitUntil(async () => await statusLine().isDisplayed() && /^Pushed 0, already in the store 1, skipped 1\./.test(await statusLine().getText()),
        { timeout: 15000, timeoutMsg: "the second push's summary counts the record already in the store and the skipped sign-off" });
      assert.equal(received(), 1, "the store received no record twice");
      await checkStatus.waitForEnabled({ timeout: 15000, timeoutMsg: "the second push's status refresh finished" });
      writeText(principalFile, "");
      await browser.$("button=Check status").click();
      const error = browser.$('section[aria-labelledby="remote-title"]').$('p[aria-label="Remote store"], [role="alert"]');
      await browser.waitUntil(async () => await error.isDisplayed() && /503|Store temporarily unavailable/.test(await error.getText()),
        { timeout: 15000, timeoutMsg: "the signed-in store's status error is shown" });
      assert.equal(await browser.$('[data-testid="remote-principal"]').isDisplayed(), false, "the prior signed-in status no longer shows");
      assert.equal(await browser.$('[data-testid="remote-pushed"]').isDisplayed(), false, "the prior push count no longer shows");
      // Restore the stand-in before proceeding through the ordinary signed-in sign-out path.
      writeText(principalFile, "alice-desktop");
      await browser.$("button=Check status").click();
      await browser.waitUntil(async () => (await pushedToday()) === "1 pushed today", { timeout: 15000, timeoutMsg: "the store recovered" });
      // A failed push takes the same status line, in red, in place of the status.
      writeText(refuseFile, "");
      await browser.$("button=Push results").click();
      let failedLine = "";
      await browser.waitUntil(async () => await statusLine().isDisplayed() && /HTTP 503/.test(failedLine = await statusLine().getText()),
        { timeout: 15000, timeoutMsg: "the push's failure takes the status line" })
        .catch((error: Error) => { throw new Error(`${error.message}: the line says ${JSON.stringify(failedLine)}`); });
      const red = await browser.execute((selector) => {
        const token = getComputedStyle(document.documentElement).getPropertyValue("--red").trim();
        const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(token);
        const wanted = hex ? `rgb(${hex.slice(1).map((part) => Number.parseInt(part, 16)).join(", ")})` : token;
        const line = document.querySelector(selector);
        return !!line && getComputedStyle(line).color === wanted;
      }, `${storeFacts} dt:last-of-type + dd`);
      assert.ok(red, "the failure is painted red");
      assert.equal(await browser.$('[data-testid="remote-principal"]').isDisplayed(), false, "the failure replaced the signed-in status");
      removeTree(refuseFile);
      await browser.$("button=Check status").click();
      await browser.waitUntil(async () => (await pushedToday()) === "1 pushed today", { timeout: 15000, timeoutMsg: "Check status puts the status back" });
      // Sign-out asks the store nothing, so it cannot be held: the window records whether every
      // Remote button was disabled in some state it rendered while the sign-out was pending.
      await browser.execute(() => {
        const seen = { allDisabled: false };
        (window as unknown as { remoteSignOut: typeof seen }).remoteSignOut = seen;
        const section = document.querySelector('section[aria-labelledby="remote-title"]')!;
        const check = () => {
          const buttons = [...section.querySelectorAll<HTMLButtonElement>("button")];
          if (buttons.length > 0 && buttons.every((button) => button.disabled)) seen.allDisabled = true;
        };
        new MutationObserver(check).observe(section, { attributes: true, subtree: true, childList: true });
      });
      await browser.$("button=Sign out of the store").click();
      const remote = browser.$('section[aria-labelledby="remote-title"]');
      const signedOut = remote.$('p[aria-label="Remote store"]');
      await browser.waitUntil(async () => await signedOut.isDisplayed() && await signedOut.getText() === "Signed out"
        && await remote.$("#remote-url").isDisplayed() && await remote.$("#remote-token").isDisplayed()
        && await remote.$('[role="radiogroup"][aria-label="Share level"]').isDisplayed()
        && await remote.$("button=Sign in to the store").isDisplayed(),
      { timeout: 15000, timeoutMsg: "the Remote tab shows Signed out and restores the URL, share level and token fields" });
      assert.equal(await remote.$("#remote-token").getValue(), "", "the token field is empty");
      assert.ok(await browser.execute(() => (window as unknown as { remoteSignOut: { allDisabled: boolean } }).remoteSignOut.allDisabled),
        "every Remote button was disabled while the sign-out was pending");
      const after = await remoteStatus(fixture);
      assert.equal(after.code, 1, after.stdout);
      assert.deepEqual(JSON.parse(after.stdout), { configured: false });
    },
  ),
];
