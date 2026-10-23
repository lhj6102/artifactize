<script lang="ts">
  // Settings: what the CLI configures, through the same library calls — the Codex (ChatGPT)
  // sign-in, the API keys the environment holds, the models each backend lists, the shared
  // remote review store and the sign-off idle grace. Secrets are never shown; a typed store
  // token leaves this screen's state as soon as it is submitted.
  import {
    call,
    useIpc,
    message,
    type Provider,
    type Pushed,
    type RemoteStatus,
    type SignoffGrace,
  } from "../ipc/client";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { SettingsState, setSettingsState } from "./settings-state.svelte";
  import { useCommands } from "../shell/commands.svelte";

  let { active }: { active: boolean } = $props();
  const commands = useIpc();
  const s = new SettingsState();
  setSettingsState(s);

  /** A sign-in in progress: what it asked the person to open. */

  /** The store token while typed; cleared the moment it is submitted. */

  const NAMES: Record<Provider, string> = { openai: "OpenAI", anthropic: "Anthropic", codex: "Codex (ChatGPT)" };

  async function loadCodex() {
    try {
      s.codex = await call(commands.codexStatus());
      s.codexError = null;
    } catch (failure) {
      s.codexError = message(failure);
    }
  }

  /** Each remote call is numbered; only the newest one may show what it found, so a slow
   * status check never shows a store signed in after a sign-out. */

  async function remoteCall(run: () => Promise<RemoteStatus>, notice?: (status: RemoteStatus) => string) {
    const mine = ++s.remoteCalls;
    s.remotePending += 1;
    s.remoteError = null;
    try {
      const status = await run();
      if (mine !== s.remoteCalls) return;
      s.remote = status;
      s.remoteNotice = notice ? notice(status) : s.remoteNotice;
    } catch (failure) {
      if (mine === s.remoteCalls) s.remoteError = message(failure);
    } finally {
      s.remotePending -= 1;
    }
  }

  async function loadRemote() {
    await remoteCall(() => call(commands.remoteStatus()));
  }

  /** The grace in force, and the seconds field set to it. */
  function showGrace(grace: SignoffGrace) {
    s.grace = grace;
    s.graceSeconds = grace.seconds;
  }

  async function loadGrace() {
    try {
      showGrace(await call(commands.signoffGrace()));
      s.graceError = null;
    } catch (failure) {
      s.graceError = message(failure);
    }
  }

  async function load() {
    s.keys = await commands.settingsKeys();
    await Promise.all([loadCodex(), loadRemote(), loadGrace()]);
  }

  /** Read every time the screen shows: another process may have signed in or out. A sign-in
   * in progress keeps what it shows. */
  const reload: Attachment = () => {
    if (active) untrack(() => !s.prompt && void load());
  };

  async function signIn() {
    s.codexError = null;
    s.codexNotice = null;
    s.busy = true;
    try {
      s.prompt = await call(commands.codexLoginStart(true));
      s.codex = await call(commands.codexLoginWait(s.prompt.signIn));
      s.codexNotice = "Signed in with Codex.";
    } catch (failure) {
      s.codexError = message(failure);
      await loadCodex();
    } finally {
      s.prompt = null;
      s.redirect = "";
      s.busy = false;
    }
  }

  async function paste(event: SubmitEvent) {
    event.preventDefault();
    const text = s.redirect.trim();
    if (!text) return;
    try {
      await call(commands.codexLoginPaste(text));
    } catch (failure) {
      s.codexError = message(failure);
    }
  }

  async function signOut() {
    s.codexError = null;
    s.busy = true;
    try {
      const out = await call(commands.codexLogout());
      s.codex = out.status;
      s.codexNotice = out.revoked ? "Signed out of Codex." : "Local tokens removed; their revocation was not confirmed.";
    } catch (failure) {
      s.codexError = message(failure);
    } finally {
      s.busy = false;
    }
  }

  async function listModels() {
    s.listing = true;
    s.models = null;
    s.modelsError = null;
    try {
      s.models = await call(commands.modelsList(s.provider));
    } catch (failure) {
      s.modelsError = message(failure);
    } finally {
      s.listing = false;
    }
  }

  async function remoteLogin(event: SubmitEvent) {
    event.preventDefault();
    // The token leaves this screen's state before the store is even asked.
    const typed = s.token;
    s.token = "";
    s.remoteNotice = null;
    s.working = true;
    await remoteCall(
      () => call(commands.remoteLogin(s.url.trim(), s.share, typed)),
      (status) =>
        status.state === "configured"
          ? `Signed in to ${status.url}${principalOf(status) ? ` as ${principalOf(status)}` : ""}.`
          : "Signed in.",
    );
    s.working = false;
  }

  async function remoteLogout() {
    s.working = true;
    await remoteCall(
      () => call(commands.remoteLogout()),
      () => "Signed out of the remote review store. The server keeps the token valid until `artifactize server token revoke`.",
    );
    s.working = false;
  }

  async function push(dryRun: boolean) {
    s.remoteError = null;
    s.working = true;
    try {
      const report: Pushed = await call(commands.remotePush(dryRun));
      s.remoteNotice = `${dryRun ? "Would push" : "Pushed"} ${report.pushed}, already in the store ${report.existing}, skipped ${report.skipped}.`;
    } catch (failure) {
      s.remoteError = message(failure);
    } finally {
      s.working = false;
    }
  }

  async function saveGrace(event: SubmitEvent) {
    event.preventDefault();
    // Whether the number is a grace the library accepts is the library's answer, shown on Save.
    if (s.graceSeconds === null) return;
    s.graceSaving = true;
    s.graceError = null;
    s.graceNotice = null;
    try {
      showGrace(await call(commands.signoffGraceSet(s.graceSeconds)));
      s.graceNotice = "Saved. Runs started from now on wait this long.";
    } catch (failure) {
      s.graceError = message(failure);
    } finally {
      s.graceSaving = false;
    }
  }

  /** A grace in whole seconds, as a person reads it: `2 minutes`, `90 seconds`. */
  function span(seconds: number): string {
    if (seconds % 60 === 0 && seconds > 0) {
      const minutes = seconds / 60;
      return `${minutes} minute${minutes === 1 ? "" : "s"}`;
    }
    return `${seconds} second${seconds === 1 ? "" : "s"}`;
  }

  /** A moment the library counts in whole seconds, as text: shown in the person's locale when
   * the calendar reaches it, else as the count itself. */
  function when(seconds: string | null): string {
    if (!seconds) return "no expiry";
    const date = new Date(Number(seconds) * 1000);
    return Number.isNaN(date.getTime()) ? `${seconds} s` : date.toLocaleString();
  }

  const signedIn = $derived(s.codex?.state === "stored");

  /** The principal the store named, when it was reachable and named one. */
  const principalOf = (status: RemoteStatus | null) =>
    status?.state === "configured" && status.connection.state === "signedIn" ? status.connection.principal : null;
  const principal = $derived(principalOf(s.remote));

  useCommands(() =>
    active
      ? [
          { id: "settings.refresh", group: "Settings", label: "Refresh settings", run: () => void load() },
          {
            id: "settings.codex",
            group: "Settings",
            label: signedIn ? "Sign in to Codex again" : "Sign in to Codex",
            enabled: () => !s.busy && s.codex?.state !== "file" && s.codex?.state !== "refused",
            run: () => void signIn(),
          },
        ]
      : [],
  );
</script>

<div class="settings" {@attach reload}>
  <h1>Settings</h1>
  <p class="lead">The same settings as the CLI's <code>login</code>, <code>models</code>, <code>remote</code> and <code>limits</code> commands.</p>

  <section aria-labelledby="codex-title">
    <h2 id="codex-title">Codex (ChatGPT) account</h2>
    <p class="muted">Signs in through the system browser and saves artifactize's own tokens, as <code>artifactize login codex</code> does.</p>
    <dl class="facts" aria-label="Codex sign-in">
      <dt>Status</dt>
      <dd data-testid="codex-status">
        {#if !s.codex}
          …
        {:else if s.codex.state === "absent"}
          Not signed in
        {:else if s.codex.state === "stored"}
          Signed in{s.codex.expiry.state === "expired" ? "; the access token expired and refreshes on next use" : ""} · token until {when(s.codex.expiry.expiresAt)}
        {:else if s.codex.state === "file"}
          Read-only Codex auth file {s.codex.path}{s.codex.expiry.state === "expired" ? " (expired; sign in with Codex again)" : ""}
        {:else}
          Cannot store tokens here: {s.codex.reason}
        {/if}
      </dd>
    </dl>
    {#if s.prompt}
      <div class="pending" role="status">
        <p>{s.prompt.callback ? "Waiting for the browser." : "Finish signing in with the browser, then paste its redirect URL below."} If it did not open, open this URL:</p>
        <p class="url"><code>{s.prompt.url}</code></p>
        {#if !s.prompt.callback}
          <p class="warn">Port 1455 is in use (another sign-in, such as the Codex CLI or app, may hold it), so the browser cannot return here. Paste the full URL it was sent to below.</p>
        {/if}
        <form onsubmit={paste}>
          <label for="codex-redirect">When the browser cannot return here, paste the URL it was sent to</label>
          <p id="codex-redirect-help" class="muted">Copy the full URL starting <code>http://localhost:1455/auth/callback?code=</code> from the browser's address bar after the failed load, or from DevTools → Network if the page keeps loading.</p>
          <div class="row">
            <input id="codex-redirect" class="input" bind:value={s.redirect} aria-describedby="codex-redirect-help" autocomplete="off" spellcheck="false" />
            <button class="button" type="submit" disabled={!s.redirect.trim()}>Finish sign-in</button>
          </div>
        </form>
        <button class="button" onclick={() => void commands.codexLoginCancel()}>Cancel sign-in</button>
      </div>
    {/if}
    <div class="row">
      <button
        class="button primary"
        disabled={s.busy || s.codex?.state === "file" || s.codex?.state === "refused"}
        onclick={() => void signIn()}>{signedIn ? "Sign in again" : "Sign in with ChatGPT"}</button
      >
      <button class="button" disabled={s.busy || !signedIn} onclick={() => void signOut()}>Sign out of Codex</button>
    </div>
    {#if s.codexNotice}<p class="notice" role="status">{s.codexNotice}</p>{/if}
    {#if s.codexError}<p class="error" role="alert">{s.codexError}</p>{/if}
  </section>

  <section aria-labelledby="keys-title">
    <h2 id="keys-title">OpenAI and Anthropic</h2>
    <p class="muted">API keys come only from the environment artifactize starts in; they are never entered or stored here.</p>
    <dl class="facts" aria-label="API keys">
      {#each s.keys as key (key.variable)}
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
      <select id="models-provider" class="input narrow" bind:value={s.provider}>
        {#each Object.entries(NAMES) as [value, name] (value)}
          <option {value}>{name}</option>
        {/each}
      </select>
      <button class="button" disabled={s.listing} onclick={() => void listModels()}>{s.listing ? "Listing…" : "List models"}</button>
    </div>
    {#if s.modelsError}<p class="error" role="alert">{s.modelsError}</p>{/if}
    {#if s.models}
      {#if s.models.length === 0}
        <p class="muted">No models listed.</p>
      {:else}
        <table aria-label="Models">
          <thead><tr><th>Model</th><th>Name</th></tr></thead>
          <tbody>
            {#each s.models as model (model.slug)}
              <tr><td><code>{model.slug}</code></td><td>{model.displayName}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </section>

  <section aria-labelledby="remote-title" aria-busy={s.remotePending > 0 || s.working}>
    <h2 id="remote-title">Shared remote review store</h2>
    <p class="muted">Signs in, out and checks status as <code>artifactize remote</code> does; the token is checked with the store and saved with remote.json.</p>
    <dl class="facts" aria-label="Remote store">
      {#if !s.remote}
        <dt>Status</dt>
        <dd>…</dd>
      {:else if s.remote.state === "unconfigured"}
        <dt>Status</dt>
        <dd data-testid="remote-status">No remote review store is configured.</dd>
      {:else}
        <dt>Remote</dt>
        <dd data-testid="remote-status">{s.remote.url}</dd>
        <dt>Share</dt>
        <dd>{s.remote.share}</dd>
        <dt>Token</dt>
        <dd>{s.remote.tokenSource}</dd>
        <dt>Reachable</dt>
        <dd>{s.remote.connection.state === "unreachable" ? "no" : "yes"}</dd>
        {#if s.remote.connection.state === "signedIn"}
          <dt>Principal</dt>
          <dd data-testid="remote-principal">{s.remote.connection.principal} (scopes: {s.remote.connection.scopes.join(", ")})</dd>
        {:else}
          <dt>Error</dt>
          <dd class="error">{s.remote.connection.error}</dd>
        {/if}
      {/if}
    </dl>
    <div class="row">
      <button class="button" disabled={s.working} onclick={() => void loadRemote()}>Check status</button>
      {#if s.remote?.state === "configured"}
        <button class="button" disabled={s.working} onclick={() => void remoteLogout()}>Sign out of the store</button>
        <button class="button" disabled={s.working || !principal} onclick={() => void push(true)}>Push (dry run)</button>
        <button class="button" disabled={s.working || !principal} onclick={() => void push(false)}>Push results</button>
      {/if}
    </div>
    <form class="login" onsubmit={remoteLogin} aria-label="Sign in to a remote store">
      <label for="remote-url">Store URL</label>
      <input id="remote-url" class="input" bind:value={s.url} placeholder="https://reviews.example/" autocomplete="off" spellcheck="false" />
      <span class="label">Share</span>
      <div class="row" role="radiogroup" aria-label="Share">
        <label><input type="radio" name="share" value="summary" bind:group={s.share} /> summary</label>
        <label><input type="radio" name="share" value="full" bind:group={s.share} /> full</label>
      </div>
      <label for="remote-token">Token</label>
      <input
        id="remote-token"
        class="input"
        type="password"
        bind:value={s.token}
        autocomplete="off"
        spellcheck="false"
        placeholder="azt_…"
      />
      <div class="row">
        <button class="button primary" type="submit" disabled={s.working || !s.url.trim() || !s.token}>Sign in to the store</button>
      </div>
    </form>
    {#if s.remoteNotice}<p class="notice" role="status">{s.remoteNotice}</p>{/if}
    {#if s.remoteError}<p class="error" role="alert">{s.remoteError}</p>{/if}
  </section>

  <section aria-labelledby="grace-title" aria-busy={s.graceSaving}>
    <h2 id="grace-title">Sign-off idle grace</h2>
    <p class="muted">
      How long a Run with only sign-offs left, and none of them claimed, waits before it ends, as <code
        >artifactize limits</code
      > shows and sets it. A claim keeps the Run open; a change applies to the Runs started afterwards.
    </p>
    <dl class="facts" aria-label="Sign-off idle grace">
      <dt>In force</dt>
      <dd data-testid="grace-status">{s.grace ? span(s.grace.seconds) : "…"}</dd>
    </dl>
    <form class="row" onsubmit={saveGrace} aria-label="Change the sign-off idle grace">
      <label for="grace-seconds">Seconds</label>
      <input
        id="grace-seconds"
        class="input narrow"
        type="number"
        min="0"
        max={s.grace?.maxSeconds}
        step="1"
        bind:value={s.graceSeconds}
        aria-describedby="grace-help"
      />
      <button
        class="button"
        type="submit"
        disabled={s.graceSaving || s.grace === null || s.graceSeconds === null || s.graceSeconds === s.grace.seconds}>Save</button
      >
    </form>
    <p id="grace-help" class="muted">
      Whole seconds from 0 to {s.grace?.maxSeconds ?? "…"}; 0 ends the Run as soon as nothing is claimed.
    </p>
    {#if s.graceNotice}<p class="notice" role="status">{s.graceNotice}</p>{/if}
    {#if s.graceError}<p class="error" role="alert">{s.graceError}</p>{/if}
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
    color: var(--yellow);
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
