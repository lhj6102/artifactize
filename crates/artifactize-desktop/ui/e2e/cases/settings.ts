// Settings: the modal and its tabs, each provider's setup (keys, custom providers and their
// models, the Codex and Claude Code sign-ins), the shared review store and the sign-off grace.
import assert from "node:assert/strict";
import { renameSync } from "node:fs";
import { antigravity, AGY_ANSWER, AGY_MODELS, AGY_PARTIAL, AGY_PERMISSION_OPTIONS, AGY_PERMISSION_PROMPT, AGY_PERMISSION_TITLE, AGY_PROMPT, agyChose, SIGN_IN_CODE, SIGN_IN_URL } from "../fixtures/antigravity.ts";
import { browserHandoffs, standInBrowser } from "../fixtures/browser.ts";
import { claudeCode } from "../fixtures/claude.ts";
import { check } from "../fixtures/declarations.ts";
import { artifactizeAsync, createEmpty, createProject, createRelease, declareRelease, environment, newFixture, signoffGrace, type Fixture } from "../fixtures/repo.ts";
import { portablePath } from "../platform.ts";
import { appendText, exists, fileSha256, nativePath, readText, readTextIfExists, realNativePath, serveLoopbackHttp, writeText } from "../portable.ts";
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

/** The fixture only returns to the test app's loopback callback; never to Codex's fixed port. */
function fixtureCodexCallback(authorization: URL): URL {
  const raw = authorization.searchParams.get("redirect_uri");
  if (raw === null) throw new Error("Codex sign-in did not supply a fixture callback");
  const url = new URL(raw);
  if (url.protocol !== "http:" || !(["localhost", "127.0.0.1"].includes(url.hostname)) || !url.port || url.port === "1455"
    || url.pathname !== "/auth/callback" || url.username || url.password) {
    throw new Error(`Refusing to visit a Codex callback outside the fixture's ephemeral listener: ${raw}`);
  }
  return url;
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
      assert.deepEqual(shown.tabs, [
        { name: "Providers", selected: true },
        { name: "Chat Agents", selected: false },
        { name: "Remote", selected: false },
        { name: "Limits", selected: false },
      ]);
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
    prepare: async (own) => {
      const fixture = createEmpty(own);
      const returns = nativePath(own.root, "browser-returns");
      const redirected = nativePath(own.root, "callback-redirected");
      const refused = nativePath(own.root, "callback-refused");
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
          try {
            const callback = fixtureCodexCallback(url);
            callback.searchParams.set("code", "e2e-return");
            callback.searchParams.set("state", url.searchParams.get("state") ?? "");
            writeText(redirected, "redirecting to the fixture listener");
            response.writeHead(302, { location: callback.href }).end();
          } catch (error) {
            writeText(refused, String(error));
            response.writeHead(400).end("No isolated callback listener; no redirect sent.");
          }
        } else response.writeHead(200).end("sign in");
      });
      own.defer(() => auth.close());
      const env = standInBrowser(fixture, { ...environment(fixture), ARTIFACTIZE_CODEX_AUTH_URL: auth.url, ARTIFACTIZE_CODEX_AUTH_FILE: undefined });
      return { fixture, env, data: { returns, redirected, refused } };
    },
    run: async (app, { fixture, data: { returns, redirected, refused } }) => {
      await app.settings.open();
      await app.settings.provider("Codex");
      await app.settings.factUntil("codex-status", (text) => /Not signed in/.test(text), "Codex signed out");
      // A sign-in in progress can be cancelled.
      await app.settings.button("Sign in with Codex");
      await app.settings.factUntil("codex-status", (text) => text === "sign-in in progress", "the sign-in in progress");
      await app.eventually(() => browserHandoffs(fixture).length === 1, "the cancelled attempt sent its browser request before the next attempt");
      await app.settings.button("Cancel sign-in");
      await app.settings.factUntil("codex-status", (text) => /Codex sign-in was cancelled/.test(text), "the sign-in cancelled");
      // The browser cannot return: Finish by hand takes the redirect URL.
      await app.settings.button("Sign in with Codex");
      await app.settings.button("Finish by hand");
      const address = new URL(await app.settings.shownAddress());
      const callback = fixtureCodexCallback(address);
      await app.eventually(() => browserHandoffs(fixture).includes(address.href), "the manual attempt finished its browser request before the automatic attempt");
      callback.searchParams.set("code", "e2e-code");
      callback.searchParams.set("state", address.searchParams.get("state") ?? "");
      await app.settings.enter("codex-redirect", callback.href);
      await app.settings.button("Finish sign-in");
      await app.settings.factUntil("codex-status", (text) => text.startsWith("Signed in"), "signed in by hand");
      await app.settings.button("Sign out of Codex");
      await app.settings.factUntil("codex-status", (text) => /Not signed in/.test(text), "signed out");
      // The browser returns to the app: signed in.
      writeText(returns, "yes");
      await app.settings.button("Sign in with Codex");
      await app.eventually(() => {
        assert.equal(readTextIfExists(refused), null, "the browser must not reach a foreign callback");
        return exists(redirected);
      }, "the fixture OAuth server redirected to the isolated callback");
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
    id: "settings-antigravity",
    title: "Chat Agents lists built-in ACP presets, Registry, and Custom agent; Antigravity checks its CLI installation, cancels and finishes sign-in by hand, probes account models, trusts the workspace in Chat, revokes trust, and signs out",
    journeys: ["J-SET-ANTIGRAVITY", "J-SET-CHAT-AGENTS"],
    prepare: async (own) => {
      const fixture = await createRelease(own);
      return { fixture, args: ["--repo", fixture.repo], ...antigravity(fixture, standInBrowser(fixture, environment(fixture))) };
    },
    run: async (app, { fixture, data: { staged, installed, version, answer, prompts } }) => {
      const status = (ok: (text: string) => boolean, what: string) => app.settings.factUntil("antigravity-status", ok, what);
      await app.settings.open();
      await app.settings.tab("Chat Agents");
      const shown = await app.settings.chatAgent("Antigravity");
      assert.ok(shown.chatAgents.some((agent) => agent.name === "Browse Registry"), "Chat Agents offers Browse Registry");
      assert.ok(shown.chatAgents.some((agent) => agent.name === "Custom agent"), "Chat Agents offers Custom agent");
      assert.match(shown.text, /Login kept by Antigravity CLI.*Serves Chat only/);
      await status((text) => text === "agy not found", "Antigravity CLI not installed");
      renameSync(staged, installed);
      await app.settings.button("Check again");
      await status((text) => /1\.5\.0.*too old.*1\.5\.1/.test(text), "the installed and required CLI versions");
      writeText(version, "not-a-version");
      await app.settings.button("Check again");
      await status((text) => /Cannot read the Antigravity CLI version/.test(text), "the CLI's failed version probe");
      writeText(version, "1.5.1");
      await app.settings.button("Check again");
      await status((text) => text === "Not signed in", "Antigravity CLI signed out");
      await app.settings.button("Sign in with Antigravity");
      await status((text) => text === "sign-in in progress", "the sign-in in progress");
      await app.settings.button("Finish by hand");
      assert.equal(await app.settings.shownAddress(), SIGN_IN_URL);
      await app.eventually(() => browserHandoffs(fixture).includes(SIGN_IN_URL), "the fixture browser got the CLI's sign-in URL");
      await app.settings.button("Cancel sign-in");
      await status((text) => /sign-in was cancelled/.test(text), "the sign-in was cancelled");
      await app.settings.button("Check again");
      await status((text) => text === "Not signed in", "the CLI stayed signed out after cancellation");
      await app.settings.button("Sign in with Antigravity");
      await app.settings.button("Finish by hand");
      assert.equal(await app.settings.shownAddress(), SIGN_IN_URL);
      await app.settings.enterCode(SIGN_IN_CODE);
      await app.settings.button("Finish sign-in");
      await status((text) => text === "Signed in with Google", "Antigravity CLI signed in with Google");
      await app.settings.button("Probe agent");
      const models = await app.settings.modelsUntil((list) => list.length === AGY_MODELS.length, "the CLI's account models");
      assert.deepEqual(models, AGY_MODELS.map(({ name, value }) => `${name} · ${value}`));
      await app.settings.close("Close");
      await app.mode("Chat");
      await app.chat.create();
      await app.chat.model(AGY_MODELS[1].name);
      await app.chat.send(AGY_PROMPT, false);
      // Until the reviewer trusts the workspace, the agent receives no prompt.
      const trust = await app.chat.trustWorkspace(async () => {
        assert.equal(readTextIfExists(prompts), null, "the agent got no prompt while Trust waited");
      });
      assert.match(trust.agent, /Antigravity.*antigravity/, `trust dialog names the agent: ${trust.agent}`);
      assert.ok(trust.scope.includes(portablePath(realNativePath(fixture.repo))), `trust dialog names the repository scope: ${trust.scope}`);
      assert.equal(trust.executable, portablePath(realNativePath(installed)), "trust dialog names the resolved executable");
      assert.equal(trust.sha256, fileSha256(installed), "trust dialog shows the executable SHA-256");
      await app.chat.says(AGY_PROMPT);
      await app.chat.says(AGY_PARTIAL);
      await app.chat.working();
      assert.ok((readTextIfExists(prompts) ?? "").includes(AGY_PROMPT), "trusted, the agent got the prompt");
      writeText(answer, "finish the ACP turn");
      await app.chat.says(AGY_ANSWER);
      await app.chat.finished();
      // The agent asks before it edits: the card offers its options, and the one chosen reaches it.
      await app.chat.send(AGY_PERMISSION_PROMPT, false);
      const asked = await app.chat.permissionUntil(AGY_PERMISSION_TITLE, (card) => card.status === "Waiting", "the permission card waits for an answer");
      assert.deepEqual(asked.options, AGY_PERMISSION_OPTIONS.map(({ name }) => name));
      await app.chat.choosePermission(AGY_PERMISSION_TITLE, "Allow once");
      await app.chat.says(agyChose("allow-once"));
      await app.chat.permissionUntil(AGY_PERMISSION_TITLE, (card) => card.status === "Chosen: Allow once" && card.options.length === 0, "the card records the choice");
      await app.chat.finished();
      await app.settings.open();
      await app.settings.tab("Chat Agents");
      await app.settings.chatAgent("Antigravity");
      await app.settings.button("Revoke");
      await app.settings.until((each) => /No workspaces trusted for Antigravity yet/.test(each.text), "trusted workspace revoked");
      await app.settings.button("Sign out of Antigravity");
      await status((text) => text === "Not signed in", "Antigravity CLI signed out again");
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
    title: "Save changes the grace in force, and a new Run ends YELLOW with an unclaimed sign-off after the saved grace",
    journeys: ["J-SET-SIGNOFF-GRACE"],
    prepare: async (own) => {
      const fixture = newFixture(own);
      declareRelease(fixture);
      return { fixture, args: ["--repo", fixture.repo], data: null };
    },
    run: async (app, { fixture }) => {
      await app.settings.open();
      await app.settings.tab("Limits");
      assert.equal(await app.settings.value("grace-seconds"), "120", "the grace in force");
      assert.equal(await app.settings.usable("Save"), false, "Save waits for another value");
      const grace = 5;
      await app.settings.enter("grace-seconds", String(grace));
      await app.settings.button("Save");
      await app.eventually(() => signoffGrace(fixture) === grace, "the CLI reads the new grace");
      // Saved, the field holds the grace in force and Save waits for another value again.
      await app.until(async () => !(await app.settings.usable("Save")) && (await app.settings.value("grace-seconds")) === String(grace), "the window shows the grace saved");
      await app.settings.close("Close");
      // Observe the live Run as the command starts, not after its completion. The elapsed
      // window starts with a visible human wait and is bounded by the product's grace.
      if (!(await app.pane.branch("repo  master")).expanded) await app.pane.toggle("repo  master");
      const started = performance.now();
      let settled: number | undefined;
      const verifying = artifactizeAsync(fixture, ["verify", "--all", "--wait", "--json"]).finally(() => { settled = performance.now(); });
      await app.waitingBadge(1);
      await app.until(async () => (await app.pane.branch("repo  master")).entries.some((entry) => /YELLOW/.test(entry.text)),
        "the new Run ends YELLOW within the saved grace", (grace + 5) * 1000);
      const done = await verifying;
      // The grace cannot start before the command does. Use the command boundary, never
      // the delayed observation of its waiting badge, to reject an immediate expiry.
      assert.ok(settled! - started >= grace * 1000, "the Run did not expire before its saved grace could elapse");
      assert.equal(done.status, 5, done.stderr);
      const { id: run } = JSON.parse(done.stdout) as { id: string };
      await app.pane.choose("repo  master", run);
      await app.graph.titled(run);
      await app.graph.openRunDetail();
      assert.match(await app.detail.section("Outcome"), /YELLOW/);
    },
  }),
];
