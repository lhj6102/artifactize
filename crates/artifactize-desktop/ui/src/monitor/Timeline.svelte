<script lang="ts">
  // An expanded branch's timeline, newest first: the Run in progress or else the Current, then
  // its Runs, joined by a line through their dots. The entry shown in the Main area is marked;
  // **Older runs** at the end brings older Runs in below the oldest.
  import type { BranchView, Chip } from "../ipc/client";
  import { toneColour } from "../shell/tone";
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
        <span class="status">{chip.kind === "run" ? chip.status : "Current"}</span>
        <span class="age">{chip.kind === "run" ? chip.age : ""}</span>
        <span class="sub">{chip.kind === "run" ? chip.id : "Working tree"}</span>
      </button>
    </li>
  {/each}
</ol>
{#if branch.older}
  <button class="older" tabindex="-1" onclick={onolder}>Older runs</button>
{/if}

<style>
  .timeline {
    list-style: none;
    margin: 2px 0 0;
    padding: 0;
  }
  .timeline li {
    position: relative;
  }
  /* The line joining the dots, from each dot down to the next. */
  .timeline li:not(:last-child)::before {
    content: "";
    position: absolute;
    left: 8.5px;
    top: 20px;
    bottom: -12px;
    width: 1px;
    background: var(--border);
  }
  .entry {
    position: relative;
    width: 100%;
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    column-gap: 10px;
    align-items: baseline;
    margin: 0;
    padding: 5px 10px 5px 0;
    border: 0;
    border-radius: 10px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .entry:hover {
    background: color-mix(in srgb, var(--surface-2) 60%, var(--surface));
  }
  /* What the Main area shows: a soft grey pill around the entry. */
  .entry.filled {
    background: var(--surface-2);
  }
  .dot {
    grid-row: 1;
    align-self: center;
    justify-self: center;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--dot);
  }
  /* The Current is no Run: a ring, not a light. */
  .dot.hollow {
    background: var(--surface);
    box-shadow: inset 0 0 0 1.5px var(--muted);
  }
  .status {
    grid-column: 2;
    font-weight: 600;
    font-size: 13px;
  }
  .running .status {
    color: var(--running);
  }
  .age {
    grid-column: 3;
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }
  .sub {
    grid-column: 2 / 4;
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .older {
    margin: 4px 0 0 28px;
    padding: 2px 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .older:hover {
    text-decoration: underline;
  }
</style>
