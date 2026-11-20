// Settings: the modal and its tabs, each provider's setup (keys, custom providers and their
// models, the Codex and Claude Code sign-ins), the shared review store and the sign-off grace.
import assert from "node:assert/strict";
import { standInBrowser } from "../fixtures/browser.ts";
import { claudeCode } from "../fixtures/claude.ts";
import { check } from "../fixtures/declarations.ts";
import { artifactizeAsync, createEmpty, createProject, createRelease, environment, signoffGrace, type Fixture } from "../fixtures/repo.ts";
import { appendText, nativePath, readText, serveLoopbackHttp, writeText } from "../portable.ts";
import { scenario, type Scenario } from "../scenario.ts";

const KEY = "sk-environment-0123456789";
const SAVED = "gm-saved-0123456789";
const TOKEN = "azt_e2esettingstoken0123456789abcdef";

/** The keys and custom providers this machine has set up, as `provider list --json` prints them. */
async function providers(fixture: Fixture): Promise<{ keys: { provider: string; source: string; saved: boolean }[]; custom: Record<string, unknown> }> {
  const listed = await artifactizeAsync(fixture, ["provider", "list", "--json"], { OPENAI_API_KEY: KEY });
  assert.equal(listed.status, 0, listed.stderr);
  return JSON.parse(listed.stdout);
}

/** The app's environment for a Settings scenario: an OpenAI key from the environment only. */
function settingsEnvironment(fixture: Fixture): NodeJS.ProcessEnv {
  return { ...environment(fixture), OPENAI_API_KEY: KEY, ANTHROPIC_API_KEY: undefined, GEMINI_API_KEY: undefined };
}

export const scenarios: Scenario[] = [
  scenario({
    id: "settings-keys",
    title: "Settings opens over the screen on its Providers tab and closes by Close, its button or a click outside, the screen beneath as it was; it lists the built-in providers, says where a key comes from without its value, and saves and removes a key",
    journeys: ["J-SETTINGS", "J-SET-PROVIDERS", "J-SET-KEY"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], env: settingsEnvironment(fixture), data: null };
    },
    run: async (app, { fixture }) => {
      await app.graph.node("a:release");
      const beneath = await app.graph.title();
      let shown = await app.settings.open();
      shown = await app.settings.until((each) => each.providers.length > 1, "the providers listed");
      assert.deepEqual(shown.tabs, [{ name: "Providers", selected: true }, { name: "Remote", selected: false }, { name: "Limits", selected: false }]);
      assert.deepEqual(shown.providers.map((provider) => provider.name), ["Codex", "Claude Agent", "OpenAI", "Anthropic", "Gemini", "Custom provider"]);
      shown = await app.settings.tab("Limits");
      assert.match(shown.text, /Sign-off idle grace/);
      await app.settings.tab("Providers");
      await app.settings.provider("OpenAI");
      await app.settings.factUntil("key-openai", (text) => text === "set from the environment (OPENAI_API_KEY)", "the OpenAI key is set from its variable");
      assert.ok(!(await app.text()).includes(KEY), "never its value");
      await app.settings.provider("Gemini");
      await app.settings.factUntil("key-gemini", (text) => text === "not set", "no Gemini key");
      await app.settings.enter("provider-key", SAVED);
      await app.settings.button("Save key");
      await app.settings.factUntil("key-gemini", (text) => text === "saved here", "the Gemini key saved");
      assert.equal(await app.settings.value("provider-key"), "", "the key left the field");
      assert.ok(!(await app.text()).includes(SAVED), "and shows nowhere");
      assert.ok((await providers(fixture)).keys.some((key) => key.provider === "gemini" && key.source === "saved"), "the CLI uses the saved key");
      await app.settings.button("Remove saved key");
      await app.settings.factUntil("key-gemini", (text) => text === "not set", "the saved key removed");
      const gemini = (await providers(fixture)).keys.find((key) => key.provider === "gemini");
      assert.deepEqual(gemini && { source: gemini.source, saved: gemini.saved }, { source: "absent", saved: false }, "the CLI has no Gemini key any more");
      // Each way of closing leaves the screen beneath as it was.
      await app.settings.close("Close");
      assert.equal(await app.graph.title(), beneath);
      await app.settings.open();
      await app.settings.close("Settings");
      await app.settings.open();
      await app.settings.close("outside");
      assert.equal(await app.graph.title(), beneath);
    },
  }),

  scenario({
    id: "settings-custom",
    title: "+ Custom provider adds an endpoint by name, API and base URL, which the CLI lists; its List models lists each model by name with its ID, and Remove provider removes it",
    journeys: ["J-SET-CUSTOM", "J-SET-MODELS"],
    prepare: async (own) => {
      const fixture = createEmpty(own);
      const endpoint = await serveLoopbackHttp((request, response) => {
        request.resume();
        const listed = request.url === "/v1/models";
        response.writeHead(listed ? 200 : 404, { "content-type": "application/json" });
        response.end(JSON.stringify(listed ? { object: "list", data: [{ id: "lab-model", object: "model", created: 0, owned_by: "lab" }] } : {}));
      });
      own.defer(() => endpoint.close());
      return { fixture, env: settingsEnvironment(fixture), data: { url: `${endpoint.url}/v1` } };
    },
    run: async (app, { fixture, data: { url } }) => {
      await app.settings.open();
      await app.settings.provider("Custom provider");
      await app.settings.enter("custom-name", "lab");
      await app.settings.choose("custom-kind", "endpoint");
      await app.settings.select("custom-api", "OpenAI Chat Completions");
      await app.settings.enter("custom-url", url);
      await app.settings.button("Save");
      await app.settings.until((shown) => shown.providers.some((provider) => provider.name === "lab" && provider.chosen), "lab saved and chosen");
      assert.deepEqual((await providers(fixture)).custom, { lab: { kind: "endpoint", api: "openai-chat", url, key: false } });
      await app.settings.button("List models");
      assert.deepEqual(await app.settings.modelsUntil((models) => models.length > 0, "the endpoint's models listed"), ["lab-model · lab-model"]);
      await app.settings.button("Remove provider");
      await app.settings.until((shown) => !shown.providers.some((provider) => provider.name === "lab"), "lab removed");
      assert.deepEqual((await providers(fixture)).custom, {});
    },
  }),

  scenario({
    id: "settings-codex",
    title: "Codex signs in through the browser, a sign-in in progress can be cancelled, and Finish by hand takes the redirect URL when the browser cannot return; it signs out again",
    journeys: ["J-SET-CODEX"],
    // The product's sign-in callback is a fixed port: one app at a time.
    alone: true,
    prepare: async (own) => {
      const fixture = createEmpty(own);
      const returns = nativePath(own.root, "browser-returns");
      writeText(returns, "no");
      const part = (value: unknown) => Buffer.from(JSON.stringify(value)).toString("base64url");
      const token = `${part({ alg: "RS256" })}.${part({ exp: 4102444800, "https://api.openai.com/auth": { chatgpt_account_id: "acct-e2e" } })}.signature`;
      const auth = await serveLoopbackHttp((request, response) => {
        request.resume();
        const url = new URL(request.url ?? "/", "http://127.0.0.1");
        if (url.pathname === "/oauth/token") {
          response.writeHead(200, { "content-type": "application/json" });
          response.end(JSON.stringify({ access_token: token, refresh_token: "refresh-e2e", expires_in: 3600 }));
        } else if (url.pathname === "/oauth/authorize" && readText(returns) === "yes") {
          // The browser comes back to the app's own callback.
          const callback = new URL(url.searchParams.get("redirect_uri") ?? "");
          callback.searchParams.set("code", "e2e-return");
          callback.searchParams.set("state", url.searchParams.get("state") ?? "");
          response.writeHead(302, { location: callback.href }).end();
        } else response.writeHead(200).end("sign in");
      });
      own.defer(() => auth.close());
      const env = standInBrowser(fixture, { ...environment(fixture), ARTIFACTIZE_CODEX_AUTH_URL: auth.url, ARTIFACTIZE_CODEX_AUTH_FILE: undefined });
      return { fixture, env, data: { returns } };
    },
    run: async (app, { data: { returns } }) => {
      await app.settings.open();
      await app.settings.provider("Codex");
      await app.settings.factUntil("codex-status", (text) => /Not signed in/.test(text), "Codex signed out");
      // A sign-in in progress can be cancelled.
      await app.settings.button("Sign in with Codex");
      await app.settings.factUntil("codex-status", (text) => text === "sign-in in progress", "the sign-in in progress");
      await app.settings.button("Cancel sign-in");
      await app.settings.factUntil("codex-status", (text) => /Not signed in/.test(text), "the sign-in cancelled");
      // The browser cannot return: Finish by hand takes the redirect URL.
      await app.settings.button("Sign in with Codex");
      await app.settings.button("Finish by hand");
      const address = new URL(await app.settings.shownAddress());
      await app.settings.enter("codex-redirect", `http://localhost:1455/auth/callback?code=e2e-code&state=${address.searchParams.get("state")}`);
      await app.settings.button("Finish sign-in");
      await app.settings.factUntil("codex-status", (text) => text.startsWith("Signed in"), "signed in by hand");
      await app.settings.button("Sign out of Codex");
      await app.settings.factUntil("codex-status", (text) => /Not signed in/.test(text), "signed out");
      // The browser returns to the app: signed in.
      writeText(returns, "yes");
      await app.settings.button("Sign in with Codex");
      await app.settings.factUntil("codex-status", (text) => text.startsWith("Signed in"), "signed in through the browser");
    },
  }),

  scenario({
    id: "settings-claude-agent",
    title: "Claude Agent shows Claude Code's own sign-in, signs in by running Claude Code's login, which opens the browser, Check again asks again, and it signs out by Claude Code's logout",
    journeys: ["J-SET-CLAUDE-AGENT"],
    prepare: async (own) => {
      const fixture = createEmpty(own);
      const env = claudeCode(fixture, standInBrowser(fixture, environment(fixture)));
      return { fixture, env, data: { signedIn: nativePath(own.root, "claude-signed-in") } };
    },
    run: async (app, { data: { signedIn } }) => {
      await app.settings.open();
      await app.settings.provider("Claude Agent");
      await app.settings.factUntil("claude-agent-status", (text) => text === "Not signed in", "Claude Code reports it is not signed in");
      await app.settings.button("Sign in with Claude Code");
      await app.settings.factUntil("claude-agent-status", (text) => text === "Signed in · claude.ai · max", "signed in through the browser");
      await app.settings.button("Sign out of Claude Code");
      await app.settings.factUntil("claude-agent-status", (text) => text === "Not signed in", "signed out");
      // Signed in elsewhere, in a terminal: Check again asks Claude Code again.
      writeText(signedIn, "claude.ai");
      await app.settings.button("Check again");
      await app.settings.factUntil("claude-agent-status", (text) => text === "Signed in · claude.ai · max", "Check again shows it signed in");
    },
  }),

  scenario({
    id: "settings-remote",
    title: "the Remote tab signs in to a shared review store, shows its URL and share level as facts and its status in one line, Check status reads it again, a push says what it pushed, and Sign out signs out",
    journeys: ["J-SET-REMOTE", "J-SET-REMOTE-STATUS"],
    prepare: async (own) => {
      const fixture = createProject(own, [[["app"], "app", check("check", "app")]]);
      const principal = nativePath(own.root, "remote-principal");
      const received = nativePath(own.root, "remote-received");
      writeText(principal, "alice-laptop");
      writeText(received, "");
      const store = await serveLoopbackHttp((request, response) => {
        request.resume();
        const answer = (status: number, body: unknown) => response.writeHead(status, { "content-type": "application/json" }).end(JSON.stringify(body));
        if (request.headers.authorization !== `Bearer ${TOKEN}`) return answer(401, { error: "Invalid or revoked token." });
        if (request.method === "GET" && request.url === "/v1/whoami") return answer(200, { principal: readText(principal), scopes: ["read", "publish"] });
        if (request.method === "POST" && request.url === "/v1/lookup") return answer(200, { entries: [] });
        if (request.method === "PUT" && request.url?.startsWith("/v1/entries/")) {
          appendText(received, `${request.url}\n`);
          return answer(200, { created: true });
        }
        return answer(404, { error: "Not found." });
      });
      own.defer(() => store.close());
      const env = { ...environment(fixture), ARTIFACTIZE_REMOTE: undefined, ARTIFACTIZE_E2E_DATE: String(Date.UTC(2026, 0, 12, 12) / 1000) };
      return { fixture, env, data: { url: `${store.url}/`, principal, received } };
    },
    run: async (app, { data: { url, principal, received } }) => {
      await app.settings.open();
      await app.settings.tab("Remote");
      await app.settings.enter("remote-url", url);
      await app.settings.enter("remote-token", TOKEN);
      await app.settings.choose("share", "full");
      await app.settings.button("Sign in to the store");
      const facts = await app.settings.remoteUntil((shown) => shown.Status === "Signed in · alice-laptop (scopes: read, publish) · 0 pushed today", "signed in, its status in one line");
      assert.equal(facts["Store URL"], url);
      assert.equal(facts["Share level"], "full");
      assert.equal(await app.settings.hasField("remote-token"), false, "without the fields that sign in");
      writeText(principal, "alice-desktop");
      await app.settings.button("Check status");
      await app.settings.remoteUntil((shown) => shown.Status === "Signed in · alice-desktop (scopes: read, publish) · 0 pushed today", "Check status read the status again");
      await app.settings.button("Push results");
      await app.settings.remoteUntil((shown) => /^Pushed 1, already in the store 0, skipped 0\./.test(shown.Status ?? ""), "the push's result takes the status line");
      assert.equal(readText(received).split("\n").filter(Boolean).length, 1, "the store received the result");
      await app.settings.button("Check status");
      await app.settings.remoteUntil((shown) => shown.Status === "Signed in · alice-desktop (scopes: read, publish) · 1 pushed today", "what was pushed today counted");
      await app.settings.button("Sign out of the store");
      await app.settings.remoteUntil((shown) => shown.Said === "Signed out", "signed out");
    },
  }),

  scenario({
    id: "settings-grace",
    title: "the Limits tab's sign-off idle grace holds the grace in force, and Save, usable once the field holds another value, changes it as the CLI reads it",
    journeys: ["J-SET-SIGNOFF-GRACE"],
    prepare: async (own) => {
      const fixture = createEmpty(own);
      return { fixture, data: null };
    },
    run: async (app, { fixture }) => {
      await app.settings.open();
      await app.settings.tab("Limits");
      assert.equal(await app.settings.value("grace-seconds"), "120", "the grace in force");
      assert.equal(await app.settings.usable("Save"), false, "Save waits for another value");
      await app.settings.enter("grace-seconds", "45");
      await app.settings.button("Save");
      await app.eventually(() => signoffGrace(fixture) === 45, "the CLI reads the new grace");
      // Saved, the field holds the grace in force and Save waits for another value again.
      await app.until(async () => !(await app.settings.usable("Save")) && (await app.settings.value("grace-seconds")) === "45", "the window shows the grace saved");
    },
  }),
];
