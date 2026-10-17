<script lang="ts">
  // A titled, focusable pane. Keys reach `onkeydown` while focus is anywhere inside it. The pane
  // is never wider than its place: a long notice is cut to the header's room, not widening it.
  import type { Snippet } from "svelte";
  import type { Attachment } from "svelte/attachments";

  let {
    title,
    focused,
    element = $bindable(),
    onfocus,
    onkeydown,
    notice = null,
    actions,
    children,
    label,
    attach,
  }: {
    title: string;
    focused: boolean;
    element?: HTMLElement;
    onfocus?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    /** What went wrong last: it takes the header's room between the title and the actions,
     * and shows in full on hover. */
    notice?: string | null;
    actions?: Snippet;
    children: Snippet;
    label?: string;
    /** Follows the pane's element for as long as it exists (an `{@attach}`). */
    attach?: Attachment<HTMLElement>;
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
  {@attach attach}
  onfocusin={onfocus}
  {onkeydown}
>
  <header>
    <h2>{title}</h2>
    {#if notice}<span class="notice" role="status" title={notice}>{notice}</span>{/if}
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  <div class="body">{@render children()}</div>
</section>

<style>
  .panel {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr;
    grid-template-columns: minmax(0, 1fr);
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
    flex: 0 1 auto;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .focused h2 {
    color: var(--text);
  }
  .notice {
    flex: 1 1 0;
    min-width: 0;
    text-align: end;
    color: var(--red);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .actions {
    display: flex;
    flex: none;
    gap: 6px;
    margin-left: auto;
  }
  .body {
    min-height: 0;
    overflow: auto;
  }
</style>
