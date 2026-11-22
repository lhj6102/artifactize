<script lang="ts">
  // An expanded branch's timeline, newest first: the Run in progress or else the Current, then
  // its Runs, joined by a line through their dots. A Run requested with a comment shows it,
  // cut short to one line, under its Run ID. The entry shown in the Main area is marked;
  // **Older runs** at the end brings older Runs in below the oldest.
  import type { BranchView, Chip } from "../ipc/client";
  import { tone, toneColour } from "../shell/tone";
  import { chipKey, keyOf } from "./monitor-state.svelte";

  let {
    branch,
    filled,
    selected,
    onchoose,
    onolder,
  }: {
    branch: BranchView;
    /** The entry shown in the Main area, by its index. */
    filled: number | null;
    /** The selected entry's name, if the selection is in this timeline. */
    selected: string | null;
    onchoose: (chip: Chip) => void;
    /** Older Runs are asked for. */
    onolder: () => void;
  } = $props();
</script>

<ol class="timeline" role="group" aria-label="Runs of {branch.project} {branch.name}">
  {#each branch.chips as chip, at (chipKey(chip))}
    <li
      role="treeitem"
      aria-selected={selected === chipKey(chip)}
      class:selected={selected === chipKey(chip)}
      data-key="{keyOf(branch.key)} {chipKey(chip)}"
    >
      <button
        class="entry"
        data-chip={chipKey(chip)}
        class:filled={at === filled}
        class:running={chip.kind === "run" && chip.running}
        tabindex="-1"
        aria-pressed={at === filled}
        onclick={() => onchoose(chip)}
      >
        <span
          class="dot"
          class:hollow={chip.kind === "current"}
          style:--dot={chip.kind === "run" ? toneColour(chip.mark.status) : null}
          aria-hidden="true"
        ></span>
        <span class="status {chip.kind === 'run' ? tone(chip.mark.status) : ''}">{chip.kind === "run" ? chip.status : "Current"}</span>
        <span class="age">{chip.kind === "run" ? chip.age : ""}</span>
        {#if chip.kind === "run"}<span class="sub">{chip.comment ?? chip.id}</span>{/if}
      </button>
    </li>
  {/each}
</ol>
{#if branch.older}
  <button class="older" onclick={onolder} onkeydown={(event) => { if (event.key === "Enter") event.stopPropagation(); }}>Older runs</button>
{/if}

<style>
  .timeline {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .timeline li {
    position: relative;
  }
  /* The line joining the dots, from each dot down to the next. */
  .timeline li:not(:last-child)::before {
    content: "";
    position: absolute;
    z-index: 1;
    pointer-events: none;
    left: 38.5px;
    top: 17px;
    bottom: -10px;
    width: 1px;
    background: var(--line-group);
  }
  .entry {
    position: relative;
    width: 100%;
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    column-gap: 8px;
    align-items: baseline;
    align-content: center;
    height: var(--row-pane);
    margin: 0;
    padding: 0 var(--pad) 0 30px;
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    line-height: 15px;
    text-align: left;
    cursor: pointer;
  }
  li:not(.selected) .entry:not(.filled):hover {
    background: var(--surface-group);
  }
  /* Selection and the view shown in Main are independent of the keyboard ring. */
  .entry.filled,
  li.selected .entry {
    background: var(--selection);
    color: var(--ink);
  }
  .entry.filled .sub,
  li.selected .sub {
    color: var(--ink);
  }
  .dot {
    position: relative;
    z-index: 2;
    grid-row: 1;
    align-self: center;
    justify-self: center;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--dot);
  }
  /* The Current is no Run: a ring, not a light. */
  .dot.hollow {
    background: var(--surface-panel);
    box-shadow: inset 0 0 0 1.5px var(--ink-2);
  }
  .status {
    grid-column: 2;
    font-size: var(--text-small);
  }
  .age {
    grid-column: 3;
    color: var(--ink-2);
    font-size: var(--text-small);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .sub {
    grid-column: 2 / 4;
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .older {
    width: 100%;
    height: var(--row-pane);
    margin: 0;
    padding: 0 var(--pad) 0 56px;
    border: 0;
    background: none;
    color: var(--ink-2);
    text-align: left;
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .older:hover {
    background: var(--surface-group);
    color: var(--ink);
  }
</style>
