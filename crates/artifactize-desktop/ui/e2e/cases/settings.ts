// Settings through the real app and CLI: the modal over the screen, where each provider key
// comes from (never its value), saved keys and custom providers as the CLI sees them, a
// remote store's HTTP identity, persisted sign-in, status, dry run and logout, and the
// sign-off idle grace both read and set.
import assert from "node:assert/strict";
import {
  artifactize, create, createOutside, createSignoffOnly, environment, newestRunStatus, providerList, remoteStatus, signoffGrace, verifyAll,
  type Fixture,
} from "../fixture.ts";
import { clickOption } from "../input.ts";
import { fill, press, typeText } from "../keys.ts";
import { graphShown } from "../graph-view.ts";
import { mainTitle } from "../pane.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { makeDirectory, nativePath, primaryModifier, readText, serveLoopbackHttp, writeText } from "../platform.ts";

const KEY = "sk-e2e-settings-value";
const SAVED = "sk-e2e-saved-gemini-key";
const SHADOWED = "sk-e2e-saved-openai-key";
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
  await browser.waitUntil(async () => (await browser.$('table[aria-label="Models"]').getText()).includes(expected), { timeout: 15000, timeoutMsg: `${expected} is listed` });
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
    "settings: lists the providers on the left, shows where each key comes from without its value, saves and removes a key as the CLI sees it, and keeps a set variable first",
    async () => {
      const fixture = createOutside();
      const seen = nativePath(fixture.root, "openai-authorization");
      writeText(seen, "");
      const openai = await serveLoopbackHttp((request, response) => {
        request.resume();
        const listing = request.url === "/v1/models";
        if (listing) writeText(seen, request.headers.authorization ?? "");
        response.writeHead(listing ? 200 : 404, { "content-type": "application/json" });
        response.end(JSON.stringify(listing ? { object: "list", data: [{ id: "openai-model", object: "model", created: 0, owned_by: "e2e" }] } : {}));
      });
      return {
        fixture,
        env: { ...settingsEnvironment(fixture), ARTIFACTIZE_OPENAI_BASE_URL: `${openai.url}/v1` },
        data: { key: KEY, saved: SAVED, shadowed: SHADOWED, seen },
        release: () => openai.close(),
      };
    },
    async (browser, { fixture, data: { key, saved, shadowed, seen } }) => {
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      await openSettings(browser);
      await browser.waitUntil(async () => (await listed(browser)).length === 5, { timeout: 15000, timeoutMsg: "the providers are listed" });
      assert.deepEqual(await listed(browser), ["Codex (ChatGPT)", "OpenAI", "Anthropic", "Gemini", "+ Custom provider"]);
      await choose(browser, "OpenAI");
      await browser.waitUntil(async () => (await keyText(browser, "openai")) === "Set from OPENAI_API_KEY", { timeout: 15000, timeoutMsg: "the OpenAI key is set from its variable" });
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
      const after = await providerList(fixture, { OPENAI_API_KEY: key });
      assert.equal(after.code, 0, after.stdout);
      assert.ok(!after.stdout.includes(saved), "the CLI never prints the key");
      assert.deepEqual(sources(after.stdout), { openai: "environment", anthropic: "absent", gemini: "saved" });

      await browser.$("button=Remove saved key").click();
      await browser.waitUntil(async () => (await keyText(browser, "gemini")) === "Not set", { timeout: 15000, timeoutMsg: "the saved Gemini key is removed" });
      const removed = await providerList(fixture, { OPENAI_API_KEY: key });
      assert.equal(sources(removed.stdout).gemini, "absent");

      // A key saved while its variable is set: the variable stays the key shown and used.
      await choose(browser, "OpenAI");
      await browser.$("#provider-key").click();
      await typeText(browser, shadowed);
      await browser.$("button=Save key").click();
      await browser.waitUntil(async () => (await browser.$("#provider-key").getValue()) === "" && (await browser.$("button=Remove saved key").isExisting()),
        { timeout: 15000, timeoutMsg: "the OpenAI key is saved" });
      assert.equal(await keyText(browser, "openai"), "Set from OPENAI_API_KEY; a saved key is kept but not used", "the variable is still the key in use");
      const both = JSON.parse((await providerList(fixture, { OPENAI_API_KEY: key })).stdout).keys.find((one: { provider: string }) => one.provider === "openai");
      assert.deepEqual([both.source, both.saved], ["environment", true]);
      await listModels(browser, "openai-model");
      assert.equal(readText(seen), `Bearer ${key}`, "the listing used the variable's key, not the saved one");
      await browser.$("button=Remove saved key").click();
      await browser.waitUntil(async () => !(await browser.$("button=Remove saved key").isExisting()), { timeout: 15000, timeoutMsg: "the saved OpenAI key is removed" });
      assert.equal(await keyText(browser, "openai"), "Set from OPENAI_API_KEY", "the variable is still set");
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
  // UX journey: J-SET-SIGNOFF-GRACE
  // UX edges: E-SET-TAB E-SET-GRACE E-SET-GRACE-SAVE
  scenario(
    "settings-signoff-grace",
    "settings: shows the sign-off idle grace, saves a new one the CLI reads, and a later Run ends by it",
    async () => {
      const fixture = createSignoffOnly();
      return { fixture, env: settingsEnvironment(fixture), data: null };
    },
    async (browser, { fixture }) => {
      const shown = () => browser.execute(() => document.querySelector('[data-testid="grace-status"]')?.textContent ?? null);
      await showLimits(browser);
      await browser.waitUntil(async () => (await shown()) === "2 minutes", { timeout: 15000, timeoutMsg: "the default idle grace" });
      assert.equal(await browser.$("#grace-seconds").getValue(), "120");
      await fill(browser, "#grace-seconds", "300");
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => (await shown()) === "5 minutes", { timeout: 15000, timeoutMsg: "the saved idle grace" });
      assert.equal(signoffGrace(fixture), 300, "the CLI reads what Settings saved");
      // Set by the CLI, it shows when Settings is shown again.
      assert.equal(artifactize(fixture, "limits", "--signoff-idle-grace", "45"), 0);
      await showLimits(browser);
      await browser.waitUntil(async () => (await shown()) === "45 seconds", { timeout: 15000, timeoutMsg: "the idle grace the CLI set" });
      // A day's grace from the CLI, then none from Settings: a Run started afterwards with only
      // its unclaimed sign-off left ends YELLOW at once, which only the saved 0 allows.
      assert.equal(artifactize(fixture, "limits", "--signoff-idle-grace", "86400"), 0);
      await showLimits(browser);
      await browser.waitUntil(async () => (await shown()) === "1440 minutes", { timeout: 15000, timeoutMsg: "the day the CLI set" });
      await fill(browser, "#grace-seconds", "0");
      await browser.$("button=Save").click();
      await browser.waitUntil(async () => (await shown()) === "0 seconds", { timeout: 15000, timeoutMsg: "no idle grace saved" });
      assert.equal(artifactize(fixture, "verify", "--all"), 0);
      await browser.waitUntil(async () => newestRunStatus(fixture) === "YELLOW", {
        timeout: 15000, timeoutMsg: "the Run started after the save ends YELLOW without a grace",
      });
    },
  ),
  // UX journey: J-SET-CUSTOM; UX edges: E-SET-CUSTOM-NEW E-SET-CUSTOM-SAVE E-SET-CUSTOM-REMOVE
  scenario(
    "settings-custom",
    "settings: adds an endpoint with a key, an alias and a keyless endpoint, changes one, reviews an eval by a custom name, and removes one, as the CLI sees it",
    async () => {
      const fixture = createOutside();
      const seen = nativePath(fixture.root, "endpoint-authorization");
      const alias = nativePath(fixture.root, "alias-authorization");
      const review = nativePath(fixture.root, "endpoint-review");
      for (const path of [seen, alias, review]) writeText(path, "");
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
            json(200, { object: "list", data: [{ id: "local-model", object: "model", created: 0, owned_by: "e2e" }] });
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
        env: { ...settingsEnvironment(fixture), ARTIFACTIZE_OPENAI_BASE_URL: `${endpoint.url}/oai/v1` },
        data: { root: endpoint.url, url: `${endpoint.url}/v1`, seen, alias, review, key: ENDPOINT_KEY },
        release: () => endpoint.close(),
      };
    },
    async (browser, { fixture, data: { root, url, seen, alias, review, key } }) => {
      const html = () => browser.execute(() => document.documentElement.outerHTML);
      const text = () => browser.execute(() => document.body.innerText);
      const custom = async () => {
        const listing = await providerList(fixture);
        assert.equal(listing.code, 0, listing.stdout);
        assert.ok(!listing.stdout.includes(key), "the CLI never prints the endpoint key");
        return JSON.parse(listing.stdout).custom as Record<string, unknown>;
      };
      /** + Custom provider, named `name`, saved after `fill` sets it up. */
      const add = async (name: string, fill: () => Promise<void>) => {
        await choose(browser, "+ Custom provider");
        await browser.$("#custom-name").waitForExist({ timeout: 15000 });
        assert.equal(await browser.$("#custom-name").getValue(), "", "a new custom provider starts empty");
        await browser.$("#custom-name").click();
        await typeText(browser, name);
        await fill();
        await browser.$("button=Save").click();
        await browser.waitUntil(async () => (await listed(browser)).includes(name) && (await text()).includes(`Saved ${name}.`),
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
      writeText(nativePath(folder, "index.artf"), `name = "agent"

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
      assert.deepEqual(await listed(browser), ["Codex (ChatGPT)", "OpenAI", "Anthropic", "Gemini", "claude-local", "local", "work", "+ Custom provider"]);
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
      await browser.waitUntil(async () => (await text()).includes("Saved local."), { timeout: 15000, timeoutMsg: "local is saved again" });
      const changed = await custom();
      assert.deepEqual(changed.local, { kind: "endpoint", api: "openai-responses", url, key: true });
      assert.deepEqual(Object.keys(changed), ["claude-local", "local", "work"]);

      await browser.$("button=Remove provider").click();
      await browser.waitUntil(async () => !(await listed(browser)).includes("local"), { timeout: 15000, timeoutMsg: "local is removed" });
      assert.deepEqual(Object.keys(await custom()), ["claude-local", "work"]);
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
