<script lang="ts">
  // The Codex sign-in, as `artifactize login codex` and `logout codex` do it: the account as a
  // fact, one row of actions, and, while a sign-in is in progress, Cancel sign-in and what
  // finishes it by hand folded under its own row.
  import Section from "../shell/Section.svelte";
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
  <h2 id="codex-title">Codex</h2>
  <dl class="facts" aria-label="Codex sign-in">
    <dt>Account</dt>
    <dd data-testid="codex-status" class:warn={s.prompt !== null}>
      {#if s.prompt}
        sign-in in progress
      {:else if !s.codex}
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
    {#if s.codexError}
      <dt>Sign-in</dt>
      <dd class="error" role="alert">{s.codexError}</dd>
    {:else if s.codexWarning}
      <dt>Sign-out</dt>
      <dd class="warn" role="status">{s.codexWarning}</dd>
    {/if}
  </dl>
  {#if s.prompt}
    <div class="row">
      <button class="button" type="button" onclick={() => s.cancelSignIn()}>Cancel sign-in</button>
    </div>
    <Section name="Finish by hand" unfolded={s.byHand} ontoggle={() => (s.byHand = !s.byHand)}>
      <pre class="url">{s.prompt.url}</pre>
      {#if !s.prompt.callback}<p class="warn">Port 1455 is in use, so the browser cannot return here.</p>{/if}
      <form class="row" onsubmit={paste}>
        <input id="codex-redirect" class="input" bind:value={s.redirect} aria-label="Redirect URL" placeholder="http://localhost:1455/auth/callback?code=…" autocomplete="off" spellcheck="false" />
        <button class="button" type="submit" disabled={!s.redirect.trim()}>Finish sign-in</button>
      </form>
    </Section>
  {:else}
    <div class="row">
      {#if signedIn}
        <button class="button" disabled={s.busy} onclick={() => void s.signOut()}>Sign out of Codex</button>
      {:else}
        <button class="button primary" disabled={s.busy || s.codex?.state === "file" || s.codex?.state === "refused"} onclick={() => void s.signIn()}>Sign in with Codex</button>
      {/if}
    </div>
  {/if}
</section>
