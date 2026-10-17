<script lang="ts">
  // The selected Run: its headline (a click opens the Run's Detail), what needs attention, and
  // the graph of its Artifacts. The graph takes the keys it moves and pops up evals with; what it
  // leaves reaches here.
  import { getAppState } from "../shell/app.svelte";
  import type { Tree } from "./tree.svelte";
  import Panel from "../shell/Panel.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    tree,
    focused,
    notice,
    popup = $bindable(null),
    active = true,
    element = $bindable(),
    onfocus,
    onopen,
    onrun,
    onback,
    onblocker,
    onjumpback,
    onready,
    onhover,
  }: {
    tree: Tree;
    focused: boolean;
    /** What went wrong last, shown above the graph. */
    notice: string | null;
    /** The Artifact whose evals pop up beside it. */
    popup?: string | null;
    /** The Monitor is the shown screen. */
    active?: boolean;
    element?: HTMLElement;
    onfocus: () => void;
    /** Open the Detail of the selection. */
    onopen: () => void;
    /** Open the Detail of the Run. */
    onrun: () => void;
    onback: () => void;
    onblocker: () => void;
    onjumpback: () => void;
    onready: (focus: (() => boolean) | null) => void;
    onhover: (path: string[] | null) => void;
  } = $props();
  const { browse } = getAppState();

  const run = $derived(browse.view?.run ?? null);

  function keydown(event: KeyboardEvent) {
    if (plain(event, "Escape")) onback();
    else if (plain(event, "o")) onopen();
    else if (plain(event, "b")) onblocker();
    else if (plain(event, "Backspace")) onjumpback();
    else if (plain(event, "r")) browse.refresh();
    else return;
    event.preventDefault();
  }
</script>

<Panel title={run ? `Run ${run.id}` : "Run"} {focused} bind:element {onfocus} onkeydown={keydown}>
  {#snippet actions()}
    {#if notice}<span class="notice" role="status">{notice}</span>{/if}
  {/snippet}
  {#if !run}
    <p class="empty">Select a Run.</p>
  {:else}
    <div class="run">
      <button class="headline" onclick={onrun} title="Open the Run's Detail">
        <span class="status {tone(run.mark.status)}">{run.mark.glyph} {run.mark.status}</span>
        <span class="text">{run.headline}</span>
      </button>
      {#if run.attention.length > 0}
        <ul class="attention">
          {#each run.attention as line, at (at)}
            <li><span class={tone(line.mark.status)}>{line.mark.glyph}</span> {line.text}</li>
          {/each}
          {#if run.more > 0}
            <li class="more">+{run.more} more · <kbd>!</kbd> visits each</li>
          {/if}
        </ul>
      {/if}
      <div class="area">
        <!-- Loaded on first use: the graph's libraries stay out of the window's start-up. -->
        {#await import("./RunGraph.svelte")}
          <p class="empty">Loading the graph…</p>
        {:then { default: RunGraph }}
          <RunGraph
            run={run.id}
            revision={browse.revision}
            tree={run.tree}
            selected={tree.selected}
            bind:popup
            {active}
            onselect={(path) => tree.select(path)}
            {onopen}
            {onready}
            {onhover}
          />
        {/await}
      </div>
    </div>
  {/if}
</Panel>

<style>
  .run {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .area {
    flex: 1;
    min-height: 240px;
  }
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .notice {
    color: var(--red);
    font-weight: 400;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .headline {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    align-items: baseline;
    margin: 0;
    padding: 0 var(--pad) 6px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .headline:hover .text {
    color: var(--text);
  }
  .headline .status {
    font-weight: 600;
  }
  .headline .text {
    color: var(--muted);
  }
  .attention {
    list-style: none;
    margin: 0;
    padding: 0 var(--pad) 8px;
  }
  .attention li {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more {
    color: var(--muted);
    font-size: 12px;
  }
</style>
