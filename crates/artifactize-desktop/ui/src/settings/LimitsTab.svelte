<script lang="ts">
  // The sign-off idle grace, as `artifactize limits` shows and sets it.
  import { getSettingsState } from "./settings-state.svelte";

  const s = getSettingsState();

  function save(event: SubmitEvent) {
    event.preventDefault();
    void s.saveGrace();
  }

  /** A grace in whole seconds, as a person reads it: `2 minutes`, `90 seconds`. */
  function span(seconds: number): string {
    if (seconds % 60 === 0 && seconds > 0) {
      const minutes = seconds / 60;
      return `${minutes} minute${minutes === 1 ? "" : "s"}`;
    }
    return `${seconds} second${seconds === 1 ? "" : "s"}`;
  }
</script>

<section aria-labelledby="grace-title" aria-busy={s.graceSaving}>
  <h2 id="grace-title">Sign-off idle grace</h2>
  <p class="muted">
    How long a Run with only sign-offs left, and none of them claimed, waits before it ends, as <code>artifactize limits</code> shows and
    sets it. A claim keeps the Run open; a change applies to the Runs started afterwards.
  </p>
  <dl class="facts" aria-label="Sign-off idle grace">
    <dt>In force</dt>
    <dd data-testid="grace-status">{s.grace ? span(s.grace.seconds) : "…"}</dd>
  </dl>
  <form class="row" onsubmit={save} aria-label="Change the sign-off idle grace">
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
