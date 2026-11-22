<script lang="ts">
  // The shared remote review store, as `artifactize remote login|logout|status|push` use it, in
  // one section that shows only what its state can use: signed out, the fields that sign in;
  // signed in, the store's URL and share level as facts with its status, and its pushes. A
  // typed token leaves the state as soon as it is submitted.
  import { getSettingsState, principalOf } from "./settings-state.svelte";

  const s = getSettingsState();
  const principal = $derived(principalOf(s.remote));

  function login(event: SubmitEvent) {
    event.preventDefault();
    void s.remoteLogin();
  }
  /** What this machine pushed since the local day began when the status was read, by the
   * pushes the status lists. */
  const pushedToday = $derived.by(() => {
    if (s.remote?.state !== "configured") return 0;
    const read = new Date(s.remoteRead);
    const midnight = new Date(read.getFullYear(), read.getMonth(), read.getDate()).getTime();
    return s.remote.pushes.filter((push) => (push.at ?? 0) >= midnight).reduce((sum, push) => sum + push.count, 0);
  });
  /** Signed in: the store's URL and share level as facts, without the fields that sign in. */
  const signedIn = $derived(s.remote?.state === "configured" && s.remote.connection.state === "signedIn");
</script>

<section class="remote" aria-labelledby="remote-title" aria-busy={s.remotePending > 0 || s.working}>
  <h2 id="remote-title">Shared review store</h2>
  {#if s.remote?.state === "configured" && s.remote.connection.state === "signedIn"}
    {@const connection = s.remote.connection}
    <dl class="facts" aria-label="Remote store">
      <dt>Store URL</dt>
      <dd data-testid="remote-status">{s.remote.url}</dd>
      <dt>Share level</dt>
      <dd>{s.remote.share}</dd>
      <dt>Status</dt>
      <!-- One line in its tone: signed in and the reviewer, or what a push or a check said. -->
      {#if s.remoteError}
        <dd class="error" role="alert">{s.remoteError}</dd>
      {:else if s.remoteNotice}
        <dd class="notice" role="status">{s.remoteNotice.text}{#if !s.remoteNotice.dryRun} · <span data-testid="remote-pushed">{pushedToday} pushed today</span>{/if}</dd>
      {:else}
        <dd class="notice">Signed in · <span data-testid="remote-principal">{connection.principal} (scopes: {connection.scopes.join(", ")})</span> · <span data-testid="remote-pushed">{pushedToday} pushed today</span></dd>
      {/if}
    </dl>
    <div class="row">
      <button class="button" disabled={s.working} onclick={() => void s.loadRemote()}>Check status</button>
      <button class="button" disabled={s.working || !principal} onclick={() => void s.push(true)}>Push (dry run)</button>
      <button class="button" disabled={s.working || !principal} onclick={() => void s.push(false)}>Push results</button>
      <span class="spacer"></span>
      <button class="button" disabled={s.working} onclick={() => void s.remoteLogout()}>Sign out of the store</button>
    </div>
  {:else}
    <form class="login" onsubmit={login} aria-label="Sign in to a remote store">
      <label for="remote-url">Store URL</label>
      <input id="remote-url" class="input" bind:value={s.url} placeholder="https://reviews.example/" autocomplete="off" spellcheck="false" />
      <span class="label">Share level</span>
      <div class="row" role="radiogroup" aria-label="Share level">
        <label><input type="radio" name="share" value="summary" bind:group={s.share} /> summary</label>
        <label><input type="radio" name="share" value="full" bind:group={s.share} /> full</label>
      </div>
      <label for="remote-token">Token</label>
      <input id="remote-token" class="input" type="password" bind:value={s.token} autocomplete="off" spellcheck="false" placeholder="azt_…" />
      <div class="row">
        <button class="button primary" type="submit" disabled={s.working || !s.url.trim() || !s.token}>Sign in to the store</button>
        <button class="button" type="button" disabled={s.working} onclick={() => void s.loadRemote()}>Check status</button>
      </div>
    </form>
    {#if s.remoteError}
      <p class="error" role="alert" aria-label="Remote store">{s.remoteError}</p>
    {:else if s.remote?.state === "configured" && !signedIn && s.remote.connection.state !== "signedIn"}
      <p class="error" aria-label="Remote store"><span data-testid="remote-status">{s.remote.url}</span> · {s.remote.connection.state === "unreachable" ? "unreachable" : "not signed in"}: {s.remote.connection.error}</p>
    {:else if s.remote?.state === "unconfigured"}
      <p class="muted" role="status" aria-label="Remote store">Signed out</p>
    {/if}
  {/if}
</section>

<style>
  /* One section of at most 640px that shows only what its state can use. */
  .remote {
    max-width: 640px;
  }
  .spacer {
    flex: 1;
  }
  .label {
    color: var(--ink-2);
  }
</style>
