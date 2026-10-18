<script lang="ts">
  // A titled, focusable pane. Keys reach `onkeydown` while focus is anywhere inside it. The pane
  // is never wider than its place: a long notice is cut to the header's room, not widening it,
  // and a click on it shows it in full.
  import type { Snippet } from "svelte";
  import type { Attachment } from "svelte/attachments";

  let {
    title,
    focused,
    element = $bindable(),
    onfocus,
    onkeydown,
    notice = null,
    noticeOpen = $bindable(false),
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
    /** What went wrong last: it takes the header's room between the title and the actions. */
    notice?: string | null;
    /** The notice shows in full rather than cut to one line. */
    noticeOpen?: boolean;
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
    {#if notice}
      <div class="notice" class:open={noticeOpen} role="status">
        <button type="button" aria-expanded={noticeOpen} onclick={() => (noticeOpen = !noticeOpen)}>{notice}</button>
      </div>
    {/if}
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
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    height: 1.5em;
  }
  .notice button {
    display: block;
    width: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: var(--red);
    text-align: end;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Expanded text floats from its one-line header slot: reading a notice never
   * moves or resizes the graph underneath it. */
  .notice.open button {
    position: absolute;
    inset: 0 0 auto;
    z-index: 30;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 4px 12px rgb(0 0 0 / 15%);
    max-height: 40vh;
    overflow: auto;
    text-align: start;
    white-space: normal;
    overflow-wrap: anywhere;
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
