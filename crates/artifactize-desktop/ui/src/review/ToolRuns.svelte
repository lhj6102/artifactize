<script lang="ts">
  // Each run of Human tools as one line (D-REV-3, J-HUMAN-LOG; an Artifact's Tool log, D-INS-4),
  // with a Chat tool line's anatomy: the fold icon (the in-progress icon while it runs), Ran or
  // Opened with the operation and its Artifact, its last stderr line behind a red rule, and at
  // the right its end with how many lines it printed, or while it runs its newest line, its
  // seconds and Cancel when there is one. A click on the line (also Enter) unfolds its lines
  // under it, and another folds them. Where `newestOpen` is set, the newest run is unfolded until
  // folded, so the one running shows its lines as they come.
  import type { Attachment } from "svelte/attachments";
  import { SvelteSet } from "svelte/reactivity";
  import type { LogEntry, ToolView } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import ToolLog from "./ToolLog.svelte";

  let {
    log,
    tools,
    cancel = null,
    newestOpen = false,
  }: {
    log: LogEntry[];
    /** The tools the runs are of: whether a run opens a file or runs a command. */
    tools: ToolView[];
    /** Stops the running tool, offered on its line; none where Cancel is said elsewhere. */
    cancel?: (() => void) | null;
    newestOpen?: boolean;
  } = $props();
  /** The runs unfolded, and the newest folded by hand, by run. */
  const open = new SvelteSet<string>();
  const shut = new SvelteSet<string>();
  const newest = $derived(log.at(-1)?.run ?? null);
  const unfolded = (run: string) => open.has(run) || (newestOpen && run === newest && !shut.has(run));

  let now = $state(Date.now());
  /** The seconds of a running tool tick only while one is shown. */
  const ticking: Attachment = () => {
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  };

  /** A launch or viewer tool opens; an output tool runs. */
  function opens(entry: LogEntry): boolean {
    const tool = tools.find((each) => each.operation === entry.operation && each.artifact === entry.artifact);
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
    if (unfolded(run)) {
      open.delete(run);
      shut.add(run);
    } else {
      open.add(run);
      shut.delete(run);
    }
  }
</script>

{#each log as entry (entry.run)}
  {@const running = entry.end === null}
  {@const verb = opens(entry) ? (running ? "Opening" : "Opened") : running ? "Running" : "Ran"}
  {@const error = running ? null : stderr(entry)}
  <div class="run">
    <div class="line">
      <button class="fold" aria-expanded={unfolded(entry.run)} aria-label="{verb} {entry.operation} · {entry.artifact}" onclick={() => toggle(entry.run)}>
        <span class={running ? "tone-running icon" : "icon"}><Icon name={running ? "running" : unfolded(entry.run) ? "unfold" : "fold"} /></span>
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
      {#if running && cancel}
        <button class="button small quiet" aria-label="Cancel (Esc)" aria-keyshortcuts="Escape" onclick={cancel}>Cancel</button>
      {/if}
    </div>
    {#if unfolded(entry.run)}
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
