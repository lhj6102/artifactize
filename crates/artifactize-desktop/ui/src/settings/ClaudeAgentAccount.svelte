<script lang="ts">
  // Claude Code's own sign-in, as `artifactize login claude-agent` and `logout claude-agent`
  // run it: Claude Code keeps its login, and its status is what Claude Code reports.
  import { getSettingsState } from "./settings-state.svelte";

  const s = getSettingsState();

  function paste(event: SubmitEvent) {
    event.preventDefault();
    void s.claudePaste();
  }

  const signedIn = $derived(s.claude?.state === "signedIn");
</script>

<section aria-labelledby="claude-agent-title">
  <h2 id="claude-agent-title">Claude Agent</h2>
  <p class="muted" data-testid="claude-agent-sign-in">
    Reviews run with your own Claude Code and its login. Signing in and out runs Claude Code's own <code>claude auth login</code> and
    <code>claude auth logout</code>, as <code>artifactize login claude-agent</code> does; Claude Code keeps its login, and artifactize never reads, copies
    or stores it.
  </p>
  <dl class="facts" aria-label="Claude Code sign-in">
    <dt>Status</dt>
    <dd data-testid="claude-agent-status">
      {#if !s.claude}
        …
      {:else if s.claude.state === "missing"}
        Claude Code (<code>claude</code>) not found on PATH
      {:else if s.claude.state === "signedOut"}
        Not signed in
      {:else if s.claude.state === "signedIn"}
        Signed in · {s.claude.method}{s.claude.plan ? ` · ${s.claude.plan}` : ""}
      {:else}
        <span class="error">Claude Code failed: {s.claude.error}</span>
      {/if}
    </dd>
    {#if s.claudePrompt}
      <dt>Sign-in</dt>
      <dd class="warn">in progress · Claude Code opened the browser</dd>
    {/if}
    {#if s.claudeError}
      <dt>Sign-in</dt>
      <dd class="error" role="alert">{s.claudeError}</dd>
    {/if}
  </dl>
  {#if s.claudePrompt}
    <div class="pending" role="status">
      <p class="muted">If the browser did not open, open this URL:</p>
      <pre class="url">{s.claudePrompt.url}</pre>
      <form onsubmit={paste}>
        <label for="claude-agent-code">When the page gives a code, paste it here</label>
        <div class="row">
          <input id="claude-agent-code" class="input" bind:value={s.claudeCode} autocomplete="off" spellcheck="false" />
          <button class="button" type="submit" disabled={!s.claudeCode.trim()}>Finish sign-in</button>
          <button class="button" type="button" onclick={() => s.cancelClaudeSignIn()}>Cancel sign-in</button>
        </div>
      </form>
    </div>
  {/if}
  <div class="row">
    <button class="button" disabled={s.claudeBusy} onclick={() => void s.loadClaude()}>Check again</button>
    <button class="button primary" disabled={s.claudeBusy || s.claude?.state === "missing"} onclick={() => void s.claudeSignIn()}
      >{signedIn ? "Sign in again" : "Sign in with Claude Code"}</button
    >
    <button class="button" disabled={s.claudeBusy || !signedIn} onclick={() => void s.claudeSignOut()}>Sign out of Claude Code</button>
  </div>
  {#if s.claudeNotice}<p class="notice" role="status">{s.claudeNotice}</p>{/if}
</section>
