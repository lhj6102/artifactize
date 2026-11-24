<script lang="ts">
  import Section from "../shell/Section.svelte";
  import { getSettingsState } from "./settings-state.svelte";

  const s = getSettingsState();
  const status = $derived(s.antigravity);
  const signingIn = $derived(s.antigravityAttempt !== null);
  const signedIn = $derived(status?.state === "signedIn");
  const installation = $derived(status && (status.path || status.version)
    ? [status.path, status.version, status.state === "tooOld" ? `requires ${status.requiredVersion} or newer` : null].filter(Boolean).join(" · ")
    : null);
  const error = $derived(s.antigravityError ?? (status && "error" in status ? status.error : null));

  function paste(event: SubmitEvent) {
    event.preventDefault();
    void s.antigravityPaste();
  }
</script>

<section aria-labelledby="antigravity-title">
  <h2 id="antigravity-title">Antigravity</h2>
  <dl class="facts" aria-label="Antigravity sign-in">
    {#if installation}
      <dt>agy</dt>
      <dd>{installation}</dd>
    {/if}
    <dt>Sign-in</dt>
    <dd data-testid="antigravity-status" class:warn={signingIn && error === null} class:error={error !== null} role={error === null ? undefined : "alert"}>
      {#if error !== null}
        {error}
      {:else if signingIn}
        sign-in in progress
      {:else if status?.state === "missing"}
        agy not found
      {:else if status?.state === "signedIn"}
        Signed in with Google{status.account ? ` · ${status.account}` : ""}
      {:else if status?.state === "signedOut"}
        Not signed in
      {:else}
        …
      {/if}
    </dd>
    <dt>Login kept by</dt>
    <dd>Antigravity CLI</dd>
    <dt>Serves</dt>
    <dd>Chat only</dd>
  </dl>
  {#if signingIn}
    <div class="row">
      <button class="button" type="button" onclick={() => void s.cancelAntigravitySignIn()}>Cancel sign-in</button>
    </div>
    {#if s.antigravityPrompt}
      <Section name="Finish by hand" unfolded={s.antigravityByHand} ontoggle={() => (s.antigravityByHand = !s.antigravityByHand)}>
        <pre class="url">{s.antigravityPrompt.url}</pre>
        <form class="row" onsubmit={paste}>
          <input class="input" bind:value={s.antigravityCode} aria-label="Code" placeholder="Code" autocomplete="off" spellcheck="false" />
          <button class="button" type="submit" disabled={!s.antigravityCode.trim()}>Finish sign-in</button>
        </form>
      </Section>
    {/if}
  {:else}
    <div class="row">
      <button class="button" disabled={s.antigravityBusy} onclick={() => void s.loadAntigravity()}>Check again</button>
      {#if signedIn}
        <button class="button" disabled={s.antigravityBusy} onclick={() => void s.antigravitySignOut()}>Sign out of Antigravity</button>
      {:else}
        <button class="button primary" disabled={s.antigravityBusy || status?.state === "missing" || status?.state === "tooOld"} onclick={() => void s.antigravitySignIn()}>Sign in with Antigravity</button>
      {/if}
    </div>
  {/if}
</section>
