<script lang="ts">
  // A reference to another mode's exact target (D-REF-1), by its words alone: no border and no
  // face, its icon and words in --ink-2, --ink under the pointer. A way back stands first in its
  // header row as a tool of that row, led by the back icon, cut at 320px; elsewhere a text
  // button led by the open icon (Open in Verify, Open in Review, Open in Chat).
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let { back = false, onclick, children }: { back?: boolean; onclick: () => void; children: Snippet } = $props();
</script>

<button class="reference" class:back {onclick}><Icon name={back ? "back" : "open"} /><span>{@render children()}</span></button>

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
  .reference:hover {
    color: var(--ink);
  }
  .reference:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  /* A way back: a tool of its header row, as tall as the other tools. */
  .back {
    flex: 0 1 auto;
    height: var(--control);
    max-width: min(320px, 100%);
    padding: 0 6px;
  }
</style>
