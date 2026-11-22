<script lang="ts">
  // The same rows for a review's evidence and for an Artifact's tools in the Detail.
  import type { Attachment } from "svelte/attachments";
  import type { LogEntry, LogText } from "../ipc/client";
  import { ToolsState, setToolsState } from "./tools-state.svelte";

  let { entries, empty = null, folded = false }: {
    entries: LogEntry[];
    empty?: string | null;
    folded?: boolean;
  } = $props();
  const t = new ToolsState();
  setToolsState(t);

  const visible = $derived(folded ? entries.slice(-1) : entries);
  const clock = (at: number | null) => at === null ? "--:--:--" : new Date(at).toTimeString().slice(0, 8);
  const prefix = (line: string) => /^\s*(?:warning|error):/i.exec(line)?.[0] ?? "";
  const lineCount = (count: number) => `${count} ${count === 1 ? "line" : "lines"}`;
  const FOLLOW_SLACK = 24;

  /** Follow the newest text unless the reader scrolled back. */
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

{#snippet printed(line: LogText)}
  {@const mark = line.error ? prefix(line.text) : ""}
  <p class:stderr={line.error}>{#if mark}<span class="red">{mark}</span>{/if}{line.text.slice(mark.length)}</p>
{/snippet}

{#snippet ended(end: LogText, count: number)}
  <p class="end">{#if end.error && end.text.startsWith("■ exit ")}<span class="red">■</span>{end.text.slice(1)}{:else if end.error && end.text.startsWith("■ error:")}■ <span class="red">error:</span>{end.text.slice("■ error:".length)}{:else}{end.text}{/if}{folded ? ` · ${lineCount(count)}` : ""}</p>
{/snippet}

<!-- Focusable so the keys scroll it; its text selects and copies like any other. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="log" tabindex="0" role="log" aria-label="Tool log" {@attach follow}>
  {#each visible as entry (entry.run)}
    <p class="start">▶ {clock(entry.at)} · {entry.operation} · {entry.artifact}</p>
    {#if folded}
      {@const line = entry.end ? entry.lines.findLast((each) => each.error) : entry.lines.at(-1)}
      {#if line}{@render printed(line)}{/if}
    {:else}
      {#each entry.lines as line, at (at)}{@render printed(line)}{/each}
    {/if}
    {#if entry.end}{@render ended(entry.end, entry.lines.length)}{/if}
  {:else}
    {#if empty}<p class="muted">{empty}</p>{/if}
  {/each}
</div>

<style>
  .log {
    min-width: 0;
    max-height: 260px;
    overflow: auto;
    padding: 6px 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    font: var(--text-small) / 1.5 var(--mono);
    outline: none;
    user-select: text;
  }
  .log:focus-visible { box-shadow: inset 0 0 0 2px var(--focus); }
  .log p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
  }
  .start, .end, .muted { color: var(--ink-2); }
  .log .end:not(:last-child) { margin-bottom: 6px; }
  .red { color: var(--red); }
  .stderr {
    padding-left: 6px;
    box-shadow: inset 2px 0 0 var(--red);
  }
</style>
