<script lang="ts">
  // The selected Run: its headline (a click opens the Run's Detail), what needs attention, and
  // the graph of its Artifacts (a click on one opens its Detail). Esc closes an open Detail,
  // else steps back to the Runs.
  import { getAppState } from "../shell/app.svelte";
  import Panel from "../shell/Panel.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    focused,
    notice,
    noticeOpen = $bindable(false),
    opened = null,
    cover = null,
    element = $bindable(),
    onfocus,
    onopen,
    onclose,
    onrun,
    onback,
  }: {
    focused: boolean;
    /** What went wrong last, shown above the graph. */
    notice: string | null;
    /** The notice shows in full. */
    noticeOpen?: boolean;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** The open Detail, docked over the graph. */
    cover?: HTMLElement | null;
    element?: HTMLElement;
    onfocus: () => void;
    /** Open the Detail of an Artifact. */
    onopen: (id: string) => void;
    /** Close the open Detail. */
    onclose: () => void;
    /** Open the Detail of the Run. */
    onrun: () => void;
    onback: () => void;
  } = $props();
  const { browse } = getAppState();

  const run = $derived(browse.view?.run ?? null);

  function keydown(event: KeyboardEvent) {
    if (plain(event, "Escape")) onback();
    else if (plain(event, "r")) browse.refresh();
    else return;
    event.preventDefault();
  }
</script>

<Panel title={run ? `Run ${run.id}` : "Run"} {focused} {notice} bind:noticeOpen bind:element {onfocus} onkeydown={keydown}>
  {#if !run}
    <p class="empty">Select a Run.</p>
  {:else}
    <div class="run">
      <button class="headline" onclick={onrun}>
        <span class="status {tone(run.mark.status)}">{run.mark.glyph} {run.mark.status}</span>
        <span class="text">{run.headline}</span>
      </button>
      {#if run.attention.length > 0}
        <ul class="attention">
          {#each run.attention as line, at (at)}
            <li><span class={tone(line.mark.status)}>{line.mark.glyph}</span> {line.text}</li>
          {/each}
          {#if run.more > 0}
            <li class="more">+{run.more} more</li>
          {/if}
        </ul>
      {/if}
      <div class="area">
        <!-- Loaded on first use: the graph's libraries stay out of the window's start-up. -->
        {#await import("./RunGraph.svelte")}
          <p class="empty">Loading the graph…</p>
        {:then { default: RunGraph }}
          <RunGraph run={run.id} revision={browse.revision} {opened} {cover} {onopen} {onclose} />
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
