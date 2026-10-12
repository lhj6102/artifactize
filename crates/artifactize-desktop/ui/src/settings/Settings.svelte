<script lang="ts">
  // Settings: what the CLI configures, through the same library calls — the Codex (ChatGPT)
  // sign-in, the API keys the environment holds, the models each backend lists, and the
  // shared remote review store. Secrets are never shown; a typed store token leaves this
  // screen's state as soon as it is submitted.
  import {
    call,
    commands,
    message,
    type CodexPrompt,
    type CodexStatus,
    type KeyStatus,
    type Model,
    type Provider,
    type Pushed,
    type RemoteStatus,
    type Share,
  } from "../ipc/client";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { useCommands } from "../shell/commands.svelte";

  let { active }: { active: boolean } = $props();

  let keys = $state<KeyStatus[]>([]);
  let codex = $state<CodexStatus | null>(null);
  let codexError = $state<string | null>(null);
  let codexNotice = $state<string | null>(null);
  /** A sign-in in progress: what it asked the person to open. */
  let prompt = $state<CodexPrompt | null>(null);
  let redirect = $state("");
  let busy = $state(false);

  let provider = $state<Provider>("openai");
  let models = $state<Model[] | null>(null);
  let modelsError = $state<string | null>(null);
  let listing = $state(false);

  let remote = $state<RemoteStatus | null>(null);
  let remoteError = $state<string | null>(null);
  let remoteNotice = $state<string | null>(null);
  let url = $state("");
  let share = $state<Share>("summary");
  /** The store token while typed; cleared the moment it is submitted. */
  let token = $state("");
  let working = $state(false);

  const NAMES: Record<Provider, string> = { openai: "OpenAI", anthropic: "Anthropic", codex: "Codex (ChatGPT)" };

  async function loadCodex() {
    try {
      codex = await call(commands.codexStatus());
      codexError = null;
    } catch (failure) {
      codexError = message(failure);
    }
  }

  /** Each remote call is numbered; only the newest one may show what it found, so a slow
   * status check never shows a store signed in after a sign-out. */
  let remoteCalls = 0;

  async function remoteCall(run: () => Promise<RemoteStatus>, notice?: (status: RemoteStatus) => string) {
    const mine = ++remoteCalls;
    remoteError = null;
    try {
      const status = await run();
      if (mine !== remoteCalls) return;
      remote = status;
      remoteNotice = notice ? notice(status) : remoteNotice;
    } catch (failure) {
      if (mine === remoteCalls) remoteError = message(failure);
    }
  }

  async function loadRemote() {
    await remoteCall(() => call(commands.remoteStatus()));
  }

  async function load() {
    keys = await commands.settingsKeys();
    await Promise.all([loadCodex(), loadRemote()]);
  }

  /** Read every time the screen shows: another process may have signed in or out. A sign-in
   * in progress keeps what it shows. */
  const reload: Attachment = () => {
    if (active) untrack(() => !prompt && void load());
  };

  async function signIn() {
    codexError = null;
    codexNotice = null;
    busy = true;
    try {
      prompt = await call(commands.codexLoginStart(true));
      codex = await call(commands.codexLoginWait(prompt.signIn));
      codexNotice = "Signed in with Codex.";
    } catch (failure) {
      codexError = message(failure);
      await loadCodex();
    } finally {
      prompt = null;
      redirect = "";
      busy = false;
    }
  }

  async function paste(event: SubmitEvent) {
    event.preventDefault();
    const text = redirect.trim();
    if (!text) return;
    try {
      await call(commands.codexLoginPaste(text));
    } catch (failure) {
      codexError = message(failure);
    }
  }

  async function signOut() {
    codexError = null;
    busy = true;
    try {
      const out = await call(commands.codexLogout());
      codex = out.status;
      codexNotice = out.revoked ? "Signed out of Codex." : "Local tokens removed; their revocation was not confirmed.";
    } catch (failure) {
      codexError = message(failure);
    } finally {
      busy = false;
    }
  }

  async function listModels() {
    listing = true;
    models = null;
    modelsError = null;
    try {
      models = await call(commands.modelsList(provider));
    } catch (failure) {
      modelsError = message(failure);
    } finally {
      listing = false;
    }
  }

  async function remoteLogin(event: SubmitEvent) {
    event.preventDefault();
    // The token leaves this screen's state before the store is even asked.
    const typed = token;
    token = "";
    remoteNotice = null;
    working = true;
    await remoteCall(
      () => call(commands.remoteLogin(url.trim(), share, typed)),
      (status) => `Signed in to ${status.url} as ${status.principal}.`,
    );
    working = false;
  }

  async function remoteLogout() {
    working = true;
    await remoteCall(
      () => call(commands.remoteLogout()),
      () => "Signed out of the remote review store. The server keeps the token valid until `artifactize server token revoke`.",
    );
    working = false;
  }

  async function push(dryRun: boolean) {
    remoteError = null;
    working = true;
    try {
      const report: Pushed = await call(commands.remotePush(dryRun));
      remoteNotice = `${dryRun ? "Would push" : "Pushed"} ${report.pushed}, already in the store ${report.existing}, skipped ${report.skipped}.`;
    } catch (failure) {
      remoteError = message(failure);
    } finally {
      working = false;
    }
  }

  function when(iso: string | null): string {
    if (!iso) return "no expiry";
    const date = new Date(iso);
    return Number.isNaN(date.getTime()) ? iso : date.toLocaleString();
  }

  const signedIn = $derived(codex?.state === "stored");

  useCommands(() =>
    active
      ? [
          { id: "settings.refresh", group: "Settings", label: "Refresh settings", run: () => void load() },
          {
            id: "settings.codex",
            group: "Settings",
            label: signedIn ? "Sign in to Codex again" : "Sign in to Codex",
            enabled: () => !busy && codex?.state !== "file" && codex?.state !== "refused",
            run: () => void signIn(),
          },
        ]
      : [],
  );
</script>

<div class="settings" {@attach reload}>
  <h1>Settings</h1>
  <p class="lead">The same settings as the CLI's <code>login</code>, <code>models</code> and <code>remote</code> commands.</p>

  <section aria-labelledby="codex-title">
    <h2 id="codex-title">Codex (ChatGPT) account</h2>
    <p class="muted">Signs in through the system browser and saves artifactize's own tokens, as <code>artifactize login codex</code> does.</p>
    <dl class="facts" aria-label="Codex sign-in">
      <dt>Status</dt>
      <dd data-testid="codex-status">
        {#if !codex}
          …
        {:else if codex.state === "absent"}
          Not signed in
        {:else if codex.state === "stored"}
          Signed in{codex.expired ? "; the access token expired and refreshes on next use" : ""} · token until {when(codex.expiresAt)}
        {:else if codex.state === "file"}
          Read-only Codex auth file {codex.path}{codex.expired ? " (expired; sign in with Codex again)" : ""}
        {:else}
          Cannot store tokens here: {codex.reason}
        {/if}
      </dd>
    </dl>
    {#if prompt}
      <div class="pending" role="status">
        <p>Waiting for the browser. If it did not open, open this URL:</p>
        <p class="url"><code>{prompt.url}</code></p>
        {#if !prompt.callback}
          <p class="warn">Port 1455 is in use (another Codex sign-in?), so the browser cannot return here; paste the URL it was sent to.</p>
        {/if}
        <form onsubmit={paste}>
          <label for="codex-redirect">When the browser cannot return here, paste the URL it was sent to</label>
          <div class="row">
            <input id="codex-redirect" class="input" bind:value={redirect} autocomplete="off" spellcheck="false" />
            <button class="button" type="submit" disabled={!redirect.trim()}>Finish sign-in</button>
          </div>
        </form>
        <button class="button" onclick={() => void commands.codexLoginCancel()}>Cancel sign-in</button>
      </div>
    {/if}
    <div class="row">
      <button
        class="button primary"
        disabled={busy || codex?.state === "file" || codex?.state === "refused"}
        onclick={() => void signIn()}>{signedIn ? "Sign in again" : "Sign in with ChatGPT"}</button
      >
      <button class="button" disabled={busy || !signedIn} onclick={() => void signOut()}>Sign out of Codex</button>
    </div>
    {#if codexNotice}<p class="notice" role="status">{codexNotice}</p>{/if}
    {#if codexError}<p class="error" role="alert">{codexError}</p>{/if}
  </section>

  <section aria-labelledby="keys-title">
    <h2 id="keys-title">OpenAI and Anthropic</h2>
    <p class="muted">API keys come only from the environment artifactize starts in; they are never entered or stored here.</p>
    <dl class="facts" aria-label="API keys">
      {#each keys as key (key.variable)}
        <dt><code>{key.variable}</code></dt>
        <dd data-testid="key-{key.provider}">{key.present ? "Set" : "Not set"}</dd>
      {/each}
    </dl>
  </section>

  <section aria-labelledby="models-title">
    <h2 id="models-title">Models</h2>
    <p class="muted">What each backend offers to your key or account, as <code>artifactize models</code> lists them.</p>
    <div class="row">
      <label for="models-provider" class="sr-only">Backend</label>
      <select id="models-provider" class="input narrow" bind:value={provider}>
        {#each Object.entries(NAMES) as [value, name] (value)}
          <option {value}>{name}</option>
        {/each}
      </select>
      <button class="button" disabled={listing} onclick={() => void listModels()}>{listing ? "Listing…" : "List models"}</button>
    </div>
    {#if modelsError}<p class="error" role="alert">{modelsError}</p>{/if}
    {#if models}
      {#if models.length === 0}
        <p class="muted">No models listed.</p>
      {:else}
        <table aria-label="Models">
          <thead><tr><th>Model</th><th>Name</th></tr></thead>
          <tbody>
            {#each models as model (model.slug)}
              <tr><td><code>{model.slug}</code></td><td>{model.displayName}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </section>

  <section aria-labelledby="remote-title">
    <h2 id="remote-title">Shared remote review store</h2>
    <p class="muted">Signs in, out and checks status as <code>artifactize remote</code> does; the token is checked with the store and saved with remote.json.</p>
    <dl class="facts" aria-label="Remote store">
      {#if !remote}
        <dt>Status</dt>
        <dd>…</dd>
      {:else if !remote.configured}
        <dt>Status</dt>
        <dd data-testid="remote-status">No remote review store is configured.</dd>
      {:else}
        <dt>Remote</dt>
        <dd data-testid="remote-status">{remote.url}</dd>
        <dt>Share</dt>
        <dd>{remote.share}</dd>
        <dt>Token</dt>
        <dd>{remote.tokenSource}</dd>
        <dt>Reachable</dt>
        <dd>{remote.reachable ? "yes" : "no"}</dd>
        {#if remote.principal}
          <dt>Principal</dt>
          <dd data-testid="remote-principal">{remote.principal} (scopes: {remote.scopes.join(", ")})</dd>
        {:else if remote.error}
          <dt>Error</dt>
          <dd class="error">{remote.error}</dd>
        {/if}
      {/if}
    </dl>
    <div class="row">
      <button class="button" disabled={working} onclick={() => void loadRemote()}>Check status</button>
      {#if remote?.configured}
        <button class="button" disabled={working} onclick={() => void remoteLogout()}>Sign out of the store</button>
        <button class="button" disabled={working || !remote.principal} onclick={() => void push(true)}>Push (dry run)</button>
        <button class="button" disabled={working || !remote.principal} onclick={() => void push(false)}>Push results</button>
      {/if}
    </div>
    <form class="login" onsubmit={remoteLogin} aria-label="Sign in to a remote store">
      <label for="remote-url">Store URL</label>
      <input id="remote-url" class="input" bind:value={url} placeholder="https://reviews.example/" autocomplete="off" spellcheck="false" />
      <span class="label">Share</span>
      <div class="row" role="radiogroup" aria-label="Share">
        <label><input type="radio" name="share" value="summary" bind:group={share} /> summary</label>
        <label><input type="radio" name="share" value="full" bind:group={share} /> full</label>
      </div>
      <label for="remote-token">Token</label>
      <input
        id="remote-token"
        class="input"
        type="password"
        bind:value={token}
        autocomplete="off"
        spellcheck="false"
        placeholder="azt_…"
      />
      <div class="row">
        <button class="button primary" type="submit" disabled={working || !url.trim() || !token}>Sign in to the store</button>
      </div>
    </form>
    {#if remoteNotice}<p class="notice" role="status">{remoteNotice}</p>{/if}
    {#if remoteError}<p class="error" role="alert">{remoteError}</p>{/if}
  </section>
</div>

<style>
  .settings {
    height: 100%;
    overflow: auto;
    padding: 16px var(--pad) 32px;
    max-width: 860px;
  }
  h1 {
    margin: 0 0 4px;
    font-size: 18px;
  }
  .lead,
  .muted {
    color: var(--muted);
    margin: 0 0 10px;
  }
  section {
    border-top: 1px solid var(--border);
    padding: 14px 0;
  }
  h2 {
    font-size: 14px;
    margin: 0 0 4px;
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 8px 0 12px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    margin: 6px 0;
  }
  .input {
    font: inherit;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    min-width: 0;
    flex: 1;
  }
  .narrow {
    flex: 0 0 auto;
  }
  .login {
    display: grid;
    grid-template-columns: max-content minmax(0, 420px);
    gap: 8px 12px;
    align-items: center;
    margin-top: 12px;
  }
  .login .row {
    grid-column: 2;
  }
  .pending {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    margin: 8px 0;
    background: var(--surface-2);
  }
  .pending p {
    margin: 0 0 6px;
  }
  .url code {
    overflow-wrap: anywhere;
    font-size: 11px;
  }
  .warn {
    color: var(--human);
  }
  .notice {
    color: var(--green);
  }
  .error {
    color: var(--red);
  }
  table {
    border-collapse: collapse;
    margin-top: 8px;
  }
  th,
  td {
    text-align: left;
    padding: 3px 16px 3px 0;
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 12px;
  }
</style>
