<script lang="ts">
  // A titled, focusable pane. Keys reach `onkeydown` while focus is anywhere inside it.
  import type { Snippet } from "svelte";

  let {
    title,
    focused,
    element = $bindable(),
    onfocus,
    onkeydown,
    actions,
    children,
    label,
  }: {
    title: string;
    focused: boolean;
    element?: HTMLElement;
    onfocus?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    actions?: Snippet;
    children: Snippet;
    label?: string;
  } = $props();
</script>

<!-- Keys from the pane's focusable rows and controls bubble up to its handler. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="panel"
  class:focused
  aria-label={label ?? title}
  tabindex="-1"
  bind:this={element}
  onfocusin={onfocus}
  {onkeydown}
>
  <header>
    <h2>{title}</h2>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  <div class="body">{@render children()}</div>
</section>

<style>
  .panel {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr;
    min-height: 0;
    background: var(--surface);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px var(--pad) 6px;
    border-top: 2px solid transparent;
  }
  .focused header {
    border-top-color: var(--accent);
  }
  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .focused h2 {
    color: var(--text);
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .body {
    min-height: 0;
    overflow: auto;
  }
</style>
