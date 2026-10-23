<script lang="ts">
  // The Codex (ChatGPT) sign-in, as `artifactize login codex` and `logout codex` do it.
  import { getSettingsState } from "./settings-state.svelte";

  const s = getSettingsState();

  /** A moment the library counts in whole seconds, as text: shown in the person's locale when
   * the calendar reaches it, else as the count itself. */
  function when(seconds: string | null): string {
    if (!seconds) return "no expiry";
    const date = new Date(Number(seconds) * 1000);
    return Number.isNaN(date.getTime()) ? `${seconds} s` : date.toLocaleString();
  }

  function paste(event: SubmitEvent) {
    event.preventDefault();
    void s.paste();
  }

  const signedIn = $derived(s.codex?.state === "stored");
</script>

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
      <button class="button" onclick={() => s.cancelSignIn()}>Cancel sign-in</button>
    </div>
  {/if}
  <div class="row">
    <button
      class="button primary"
      disabled={s.busy || s.codex?.state === "file" || s.codex?.state === "refused"}
      onclick={() => void s.signIn()}>{signedIn ? "Sign in again" : "Sign in with ChatGPT"}</button
    >
    <button class="button" disabled={s.busy || !signedIn} onclick={() => void s.signOut()}>Sign out of Codex</button>
  </div>
  {#if s.codexNotice}<p class="notice" role="status">{s.codexNotice}</p>{/if}
  {#if s.codexError}<p class="error" role="alert">{s.codexError}</p>{/if}
</section>
