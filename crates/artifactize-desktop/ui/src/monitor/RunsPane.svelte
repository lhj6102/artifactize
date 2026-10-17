<script lang="ts">
  // The selected scope's Runs, newest first; moving past either end pages.
  import { getAppState } from "../shell/app.svelte";
  import Panel from "../shell/Panel.svelte";
  import { scrollTo } from "../shell/attach";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    focused,
    element = $bindable(),
    onfocus,
    onnext,
    onback,
  }: {
    focused: boolean;
    element?: HTMLElement;
    onfocus: () => void;
    onnext: () => void;
    onback: () => void;
  } = $props();
  const { browse } = getAppState();

  const view = $derived(browse.view);
  const runs = $derived(view?.runs ?? []);
  const index = $derived(runs.findIndex((run) => run.id === view?.selectedRun));
  const workspace = $derived(view?.scope.kind !== "worktree");

  function select(at: number) {
    const run = runs[at];
    if (run && run.id !== view?.selectedRun) void browse.navigate({ run: run.id });
  }

  function keydown(event: KeyboardEvent) {
    if (plain(event, "ArrowDown") || plain(event, "j")) {
      if (index + 1 < runs.length) select(index + 1);
      else if (view?.older) void browse.navigate({ paging: "older" });
    } else if (plain(event, "ArrowUp") || plain(event, "k")) {
      if (index <= 0 && view?.newer) void browse.navigate({ paging: "newer" });
      else select(Math.max(index - 1, 0));
    } else if (plain(event, "Enter") || plain(event, "ArrowRight")) onnext();
    else if (plain(event, "ArrowLeft") || plain(event, "Escape")) onback();
    else if (plain(event, "r")) browse.refresh();
    else return;
    event.preventDefault();
  }
</script>

<Panel title="Runs ({runs.length})" {focused} bind:element {onfocus} onkeydown={keydown}>
  {#snippet actions()}
    <button class="button small" disabled={!view?.newer} onclick={() => browse.navigate({ paging: "newer" })} aria-label="Newer Runs">↑</button>
    <button class="button small" disabled={!view?.older} onclick={() => browse.navigate({ paging: "older" })} aria-label="Older Runs">↓</button>
  {/snippet}
  {#if runs.length === 0}
    <p class="empty">{view?.refreshed ? "No saved Runs. Start one with `artifactize verify`." : "Loading…"}</p>
  {:else}
    <ul class="rows" role="listbox" aria-label="Runs" tabindex="0" {@attach scrollTo(() => `[data-index="${index}"]`)}>
      {#each runs as run, at (run.id)}
        <li
          role="option"
          aria-selected={at === index}
          class:selected={at === index}
          class:unreadable={!run.readable}
          data-index={at}
          onclick={() => select(at)}
          ondblclick={onnext}
          onkeydown={() => {}}
        >
          <span class="glyph {tone(run.mark.status)}">{run.mark.glyph}</span>
          <span class="main">
            <span class="line">
              <span class="status {tone(run.mark.status)}">{run.mark.status}</span>
              <span class="counts">{run.counts}</span>
              <span class="age">{run.age}{run.took ? ` · took ${run.took}` : ""}</span>
            </span>
            <span class="id">{run.id}{workspace ? ` · ${run.repository}` : ""}</span>
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</Panel>

<style>
  .rows {
    list-style: none;
    margin: 0;
    padding: 0 0 8px;
    outline: none;
  }
  li {
    display: flex;
    gap: 10px;
    padding: 6px 16px;
    cursor: pointer;
  }
  li:hover {
    background: var(--surface-2);
  }
  .selected {
    background: var(--selection);
  }
  .rows:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .unreadable {
    opacity: 0.6;
  }
  .glyph {
    width: 1em;
    text-align: center;
  }
  .main {
    display: grid;
    min-width: 0;
  }
  .line {
    display: flex;
    gap: 8px;
    white-space: nowrap;
  }
  .status {
    font-weight: 600;
    font-size: 11px;
  }
  .age,
  .id {
    color: var(--muted);
  }
  .id {
    font: 11px var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  :global(.button.small) {
    padding: 0 8px;
  }
</style>
