<script lang="ts">
  // Claude Code's own sign-in, as `artifactize login claude-agent` and `logout claude-agent`
  // run it: Claude Code keeps its login, and its status is what Claude Code reports, a fact of
  // the grid. While a sign-in is in progress, Cancel sign-in, and what finishes it by hand
  // folded under its own row.
  import Section from "../shell/Section.svelte";
  import { getSettingsState } from "./settings-state.svelte";

  const s = getSettingsState();

  function paste(event: SubmitEvent) {
    event.preventDefault();
    void s.claudePaste();
  }

  const error = $derived(s.claudeError ?? (s.claude?.state === "failed" ? s.claude.error : null));
  const signedIn = $derived(s.claude?.state === "signedIn");
</script>

<section aria-labelledby="claude-agent-title">
  <h2 id="claude-agent-title">Claude Agent</h2>
  <dl class="facts" aria-label="Claude Code sign-in">
    <dt>Sign-in</dt>
    <dd data-testid="claude-agent-status" class:warn={error === null && s.claudeAttempt !== null} class:error={error !== null} role={error === null ? undefined : "alert"}>
      {#if error !== null}
        {error}
      {:else if s.claudeAttempt !== null}
        sign-in in progress
      {:else if !s.claude}
        …
      {:else if s.claude.state === "missing"}
        claude not found
      {:else if s.claude.state === "signedOut"}
        Not signed in
      {:else if s.claude.state === "signedIn"}
        Signed in · {s.claude.method}{s.claude.plan ? ` · ${s.claude.plan}` : ""}
      {/if}
    </dd>
    <dt>Login kept by</dt>
    <dd>Claude Code</dd>
  </dl>
  {#if s.claudeAttempt !== null}
    <div class="row">
      <button class="button" type="button" onclick={() => s.cancelClaudeSignIn()}>Cancel sign-in</button>
    </div>
  {/if}
  {#if s.claudePrompt}
    <Section name="Finish by hand" unfolded={s.byHand} ontoggle={() => (s.byHand = !s.byHand)}>
      <pre class="url">{s.claudePrompt.url}</pre>
      <form class="row" onsubmit={paste}>
        <input id="claude-agent-code" class="input" bind:value={s.claudeCode} aria-label="Code" placeholder="Code" autocomplete="off" spellcheck="false" />
        <button class="button" type="submit" disabled={!s.claudeCode.trim()}>Finish sign-in</button>
      </form>
    </Section>
  {:else}
    <div class="row">
      <button class="button" disabled={s.claudeBusy} onclick={() => void s.loadClaude()}>Check again</button>
      {#if signedIn}
        <button class="button" disabled={s.claudeBusy} onclick={() => void s.claudeSignOut()}>Sign out of Claude Code</button>
      {:else}
        <button class="button primary" disabled={s.claudeBusy || s.claude?.state === "missing"} onclick={() => void s.claudeSignIn()}>Sign in with Claude Code</button>
      {/if}
    </div>
  {/if}
</section>
