<script lang="ts">
  // A Tool log: each run as `▶ HH:MM:SS · operation · Artifact`, the lines it printed (stderr
  // in red) and its end line. It follows its newest line unless the reader scrolled up; its
  // text selects and copies like any other. A review's and an Artifact's tools both log here.
  import type { Attachment } from "svelte/attachments";
  import type { LogEntry } from "../ipc/client";
  import { ToolsState, setToolsState } from "./tools-state.svelte";

  let { entries, empty }: { entries: LogEntry[]; /** What shows before the first run. */ empty: string } = $props();
  const t = new ToolsState();
  setToolsState(t);

  /** `HH:MM:SS` of `at` (milliseconds since the epoch), local time. */
  const clock = (at: number | null) => (at === null ? "--:--:--" : new Date(at).toTimeString().slice(0, 8));

  /** Rows from the bottom within which the log still counts as followed. */
  const FOLLOW_SLACK = 24;

  /** The log follows its newest line unless the reader scrolled up; scrolled back to the
   * bottom, it follows again. Only the DOM is touched, as lines are added. */
  const follow: Attachment<HTMLElement> = (log) => {
    const scrolled = () => {
      t.following = log.scrollHeight - log.scrollTop - log.clientHeight < FOLLOW_SLACK;
    };
    const added = new MutationObserver(() => {
      if (t.following) log.scrollTop = log.scrollHeight;
    });
    added.observe(log, { childList: true, subtree: true, characterData: true });
    log.addEventListener("scroll", scrolled);
    return () => {
      added.disconnect();
      log.removeEventListener("scroll", scrolled);
    };
  };
</script>

<section class="logbox" aria-label="Tool log">
  <h4>Tool log</h4>
  <!-- Focusable so the keys scroll it; its text selects and copies like any other. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="log" tabindex="0" role="log" aria-label="Tool log" {@attach follow}>
    {#each entries as entry (entry.run)}
      <p class="start">▶ {clock(entry.at)} · {entry.operation} · {entry.artifact}</p>
      {#each entry.lines as line, at (at)}
        <p class:error={line.error}>{line.text}</p>
      {/each}
      {#if entry.end}<p class="end" class:error={entry.end.error}>{entry.end.text}</p>{/if}
    {:else}
      <p class="muted">{empty}</p>
    {/each}
  </div>
</section>

<style>
  h4 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .logbox {
    min-width: 0;
  }
  .log {
    max-height: 260px;
    min-height: 72px;
    overflow: auto;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--surface-2);
    font: 12px/1.5 var(--mono);
    outline: none;
    user-select: text;
  }
  .log:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .log p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
  }
  .start,
  .end {
    color: var(--muted);
  }
  .end {
    margin-bottom: 6px !important;
  }
  .error {
    color: var(--red) !important;
  }
  .muted {
    color: var(--muted);
    margin: 0 0 6px;
    font-family: inherit;
  }
</style>
