<script lang="ts">
  // Each run of the review's tools as one line of the conversation (D-REV-3, J-HUMAN-LOG), with
  // a Chat tool line's anatomy: the fold icon (the in-progress icon while it runs), Ran or Opened
  // with the operation and its Artifact, its last stderr line behind a red rule, and at the right
  // its end with how many lines it printed, or while it runs its newest line, its seconds and
  // Cancel. A click on the line (also Enter) unfolds its lines under it, and another folds them.
  import type { Attachment } from "svelte/attachments";
  import { SvelteSet } from "svelte/reactivity";
  import type { LogEntry } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import ToolLog from "./ToolLog.svelte";
  import type { Review } from "./review.svelte";

  let { review }: { review: Review } = $props();
  /** The runs unfolded, by run. */
  const open = new SvelteSet<string>();

  let now = $state(Date.now());
  /** The seconds of a running tool tick only while one is shown. */
  const ticking: Attachment = () => {
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  };

  /** A launch or viewer tool opens; an output tool runs. */
  function opens(entry: LogEntry): boolean {
    const tool = review.view?.tools.find((each) => each.operation === entry.operation && each.artifact === entry.artifact);
    return tool !== undefined && tool.kind !== "output";
  }
  const count = (entry: LogEntry) => `${entry.lines.length} ${entry.lines.length === 1 ? "line" : "lines"}`;
  const stderr = (entry: LogEntry) => entry.lines.findLast((line) => line.error)?.text ?? null;
  /** The end as the line says it: `exit 0 · 8 lines`, `launched · 2 lines`, `error: …`. */
  const end = (entry: LogEntry) => {
    const text = entry.end?.text.replace(/^■\s*/, "") ?? "";
    return text.startsWith("error:") ? text : `${text} · ${count(entry)}`;
  };
  const failed = (entry: LogEntry) => !!entry.end?.error && !/^■ exit 0\b/.test(entry.end.text);

  function toggle(run: string) {
    if (open.has(run)) open.delete(run);
    else open.add(run);
  }
</script>

{#each review.log as entry (entry.run)}
  {@const running = entry.end === null}
  {@const verb = opens(entry) ? (running ? "Opening" : "Opened") : running ? "Running" : "Ran"}
  {@const error = running ? null : stderr(entry)}
  <div class="run">
    <div class="line">
      <button class="fold" aria-expanded={open.has(entry.run)} aria-label="{verb} {entry.operation} · {entry.artifact}" onclick={() => toggle(entry.run)}>
        <span class={running ? "tone-running icon" : "icon"}><Icon name={running ? "running" : open.has(entry.run) ? "unfold" : "fold"} /></span>
        <span class="verb">{verb}</span>
        <span class="target">{entry.operation}</span>
        <span class="artifact">· {entry.artifact}</span>
        {#if error}<span class="stderr">{error}</span>{/if}
        {#if running}
          <span class="newest" {@attach ticking}>{entry.lines.at(-1)?.text ?? ""}</span>
          <span class="seconds">{entry.at === null ? "" : `${Math.max(0, Math.floor((now - entry.at) / 1000))}s`}</span>
        {:else}
          <span class="end" class:failed={failed(entry)}>{end(entry)}</span>
        {/if}
      </button>
      {#if running && review.busy?.tool}
        <button class="button small quiet" aria-label="Cancel (Esc)" aria-keyshortcuts="Escape" onclick={() => review.cancel()}>Cancel</button>
      {/if}
    </div>
    {#if open.has(entry.run)}
      <div class="lines"><ToolLog entries={[entry]} /></div>
    {/if}
  </div>
{/each}

<style>
  .run {
    min-width: 0;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .fold {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    min-height: var(--row-dense);
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    font-size: var(--text-small);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .fold:hover {
    background: var(--surface-group);
  }
  .fold:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .icon {
    display: inline-flex;
    flex: none;
    color: var(--ink-3);
  }
  .verb,
  .artifact {
    flex: none;
  }
  .target,
  .newest {
    font-family: var(--mono);
  }
  .target {
    flex: none;
  }
  .stderr,
  .newest {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stderr {
    padding-left: 6px;
    box-shadow: inset 2px 0 0 var(--red);
    color: var(--ink);
  }
  .newest {
    flex: 1;
  }
  .end,
  .seconds {
    flex: none;
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .newest + .seconds {
    margin-left: 0;
  }
  .end.failed {
    color: var(--red);
  }
  .lines {
    margin: 2px 0 4px 22px;
  }
  .run .lines :global(.log) {
    max-height: 50cqh;
  }
</style>
