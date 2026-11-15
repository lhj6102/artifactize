<script lang="ts">
  // A section of the inspector, a review or Settings under its header row: the fold icon and its
  // name, nothing else. The whole row folds or unfolds the section, which keeps that by its name
  // (sections.svelte.ts); folded, the section is its row alone. A section that does not fold
  // (a review's Tools, Tool log and Result, Settings' sections) has its row without the icon.
  import type { Snippet } from "svelte";
  import { getAppState } from "./app.svelte";
  import Icon from "./Icon.svelte";

  let {
    name,
    folds = true,
    unfolded = undefined,
    ontoggle,
    label,
    children,
  }: {
    /** The section's name in sentence case, as its row shows it. */
    name: string;
    /** It folds by its row. */
    folds?: boolean;
    /** Whether it is unfolded, when its owner keeps that rather than the folds by name. */
    unfolded?: boolean;
    ontoggle?: () => void;
    /** Its accessible name, when it is not the shown one. */
    label?: string;
    children: Snippet;
  } = $props();
  const { sections } = getAppState();
  const open = $derived(!folds || (unfolded ?? sections.open(name)));

  function toggle() {
    if (unfolded === undefined) sections.toggle(name);
    else ontoggle?.();
  }
</script>

<section class="section" aria-label={label ?? name}>
  {#if folds}
    <button class="head" aria-expanded={open} onclick={toggle}>
      <span class="fold"><Icon name={open ? "unfold" : "fold"} /></span>{name}
    </button>
  {:else}
    <h3 class="head">{name}</h3>
  {/if}
  {#if open}<div class="body">{@render children()}</div>{/if}
</section>

<style>
  /* Opened above by a group line that runs the panel's whole width. */
  .section {
    min-width: 0;
    border-top: 1px solid var(--line-group);
  }
  /* The header row: the fold icon in secondary ink, the name at 12px semibold in ink. */
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: var(--row);
    margin: 0;
    padding: 0 var(--pad);
    border: 0;
    background: none;
    color: var(--ink);
    font: inherit;
    font-size: var(--text-small);
    font-weight: 600;
    text-align: left;
  }
  button.head {
    cursor: pointer;
  }
  button.head:hover {
    background: var(--surface-group);
  }
  button.head:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .fold {
    display: inline-flex;
    color: var(--ink-2);
  }
  .body {
    min-width: 0;
    padding: 0 var(--pad) 10px;
    overflow-wrap: anywhere;
  }
</style>
