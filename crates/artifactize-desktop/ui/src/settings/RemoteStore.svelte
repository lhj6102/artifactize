<script lang="ts">
  // The shared remote review store, as `artifactize remote login|logout|status|push` use it. A
  // typed token leaves the state as soon as it is submitted.
  import { getSettingsState, principalOf } from "./settings-state.svelte";

  const s = getSettingsState();
  const principal = $derived(principalOf(s.remote));

  function login(event: SubmitEvent) {
    event.preventDefault();
    void s.remoteLogin();
  }
</script>

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
    <button class="button" disabled={s.working} onclick={() => void s.loadRemote()}>Check status</button>
    {#if s.remote?.state === "configured"}
      <button class="button" disabled={s.working} onclick={() => void s.remoteLogout()}>Sign out of the store</button>
      <button class="button" disabled={s.working || !principal} onclick={() => void s.push(true)}>Push (dry run)</button>
      <button class="button" disabled={s.working || !principal} onclick={() => void s.push(false)}>Push results</button>
    {/if}
  </div>
  <form class="login" onsubmit={login} aria-label="Sign in to a remote store">
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
