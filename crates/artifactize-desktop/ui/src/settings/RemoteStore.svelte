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

<div class="remote">
<section aria-labelledby="remote-title" aria-busy={s.remotePending > 0 || s.working}>
  <h2 id="remote-title">Shared remote review store</h2>
  <p class="muted">Signs in, out and checks status as <code>artifactize remote</code> does; the token is checked with the store and saved with remote.json, never shown.</p>
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
      <button class="button" type="button" disabled={s.working} onclick={() => void s.loadRemote()}>Check status</button>
    </div>
  </form>
</section>
<section aria-labelledby="remote-status-title" aria-busy={s.remotePending > 0 || s.working}>
  <h2 id="remote-status-title">Status</h2>
  <!-- One line in its tone: the store, the share level, the token's source, and the principal
       or the error. -->
  <p class="status" aria-label="Remote store">
    {#if !s.remote}
      …
    {:else if s.remote.state === "unconfigured"}
      <span class="muted" data-testid="remote-status">No remote review store is configured.</span>
    {:else if s.remote.connection.state === "signedIn"}
      <span class="notice">Signed in to <span data-testid="remote-status">{s.remote.url}</span> as
        <span data-testid="remote-principal">{s.remote.connection.principal} (scopes: {s.remote.connection.scopes.join(", ")})</span>
        · share {s.remote.share} · token {s.remote.tokenSource}</span>
    {:else}
      <span class="error"><span data-testid="remote-status">{s.remote.url}</span> · share {s.remote.share} · token {s.remote.tokenSource} ·
        {s.remote.connection.state === "unreachable" ? "unreachable" : "not signed in"}: {s.remote.connection.error}</span>
    {/if}
    {#if s.remoteNotice}<span class="muted"> · </span><span class="notice" role="status">{s.remoteNotice}</span>{/if}
    {#if s.remoteError}<span class="muted"> · </span><span class="error" role="alert">{s.remoteError}</span>{/if}
  </p>
  {#if s.remote?.state === "configured"}
    <div class="row">
      <button class="button" disabled={s.working || !principal} onclick={() => void s.push(true)}>Push (dry run)</button>
      <button class="button" disabled={s.working || !principal} onclick={() => void s.push(false)}>Push results</button>
      <span class="spacer"></span>
      <button class="button" disabled={s.working} onclick={() => void s.remoteLogout()}>Sign out of the store</button>
    </div>
  {/if}
</section>
</div>

<style>
  /* One column of at most 640px: the sign-in first, then the status with its pushes. */
  .remote {
    max-width: 640px;
  }
  .spacer {
    flex: 1;
  }
  .status {
    margin: 0 0 6px;
    overflow-wrap: anywhere;
  }
</style>
