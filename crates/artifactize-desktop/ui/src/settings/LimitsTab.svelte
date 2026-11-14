<script lang="ts">
  // The sign-off idle grace, as `artifactize limits` shows and sets it: one line says what it
  // is, and the seconds field holds the grace in force; only a failure to save is said.
  import { getSettingsState } from "./settings-state.svelte";

  const s = getSettingsState();

  function save(event: SubmitEvent) {
    event.preventDefault();
    void s.saveGrace();
  }

</script>

<section aria-labelledby="grace-title" aria-busy={s.graceSaving}>
  <h2 id="grace-title">Sign-off idle grace</h2>
  <p class="muted">How long a Run with only sign-offs left, none of them claimed, waits before it ends.</p>
  <form class="row" onsubmit={save} aria-label="Sign-off idle grace">
    <input
      id="grace-seconds"
      class="input narrow num"
      type="number"
      min="0"
      max={s.grace?.maxSeconds}
      step="1"
      data-testid="grace-status"
      aria-label="Seconds"
      bind:value={s.graceSeconds}
    />
    <span class="unit">seconds</span>
    <button
      class="button primary"
      type="submit"
      disabled={s.graceSaving || s.grace === null || s.graceSeconds === null || s.graceSeconds === s.grace.seconds}>Save</button
    >
  </form>
  {#if s.graceError}<p class="error" role="alert">{s.graceError}</p>{/if}
</section>

<style>
  .unit {
    color: var(--ink-2);
  }
  .narrow {
    width: 96px;
  }
</style>
