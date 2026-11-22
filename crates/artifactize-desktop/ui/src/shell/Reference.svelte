<script lang="ts">
  // A reference to another mode's exact target (D-REF-1): in a header row a chip, its icon and
  // words in the accent inside a group line (a way back, led by the back icon); in a list or a
  // card a text button in the accent led by the open icon (Open in Verify, Open in Review).
  // Hovering only steps its edge or face up.
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let { chip = false, onclick, children }: { chip?: boolean; onclick: () => void; children: Snippet } = $props();
</script>

<button class="reference" class:chip {onclick}><Icon name={chip ? "back" : "open"} /><span>{@render children()}</span></button>

<style>
  .reference {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    margin: 0;
    padding: 0 4px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    font: inherit;
    font-size: var(--text-small);
    white-space: nowrap;
    overflow: hidden;
    cursor: pointer;
  }
  .reference span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .reference:not(.chip):hover {
    background: var(--surface-group);
  }
  .chip {
    gap: 6px;
    height: 20px;
    max-width: min(320px, 100%);
    padding: 0 8px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
  }
  .chip:hover {
    border-color: var(--line-input);
  }
</style>
