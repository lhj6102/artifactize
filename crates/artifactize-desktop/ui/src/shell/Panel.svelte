<script lang="ts">
  // A titled, focusable pane under one header row: its name, what the name names (a star), its
  // headline cut at the row's end, a notice and its actions. Keys reach `onkeydown` while focus
  // is anywhere inside it. The pane is never wider than its place: a long notice is cut to the
  // header's room, not widening it, and a click on it shows it in full.
  import type { Snippet } from "svelte";
  import type { Attachment } from "svelte/attachments";

  let {
    title,
    focused,
    element = $bindable(),
    onfocus,
    onkeydown,
    notice = null,
    noticeOpen = $bindable(),
    actions,
    beside,
    lead,
    headline,
    children,
    label,
    attach,
    wholeTitle = false,
  }: {
    title: string;
    focused: boolean;
    element?: HTMLElement;
    onfocus?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    actions?: Snippet;
    /** Controls of what the title names, right after it. */
    beside?: Snippet;
    /** A way back to where the shown view was opened from, first in the row (D-REF-1). */
    lead?: Snippet;
    /** What the view shows, in the same row after the title, cut at the row's end. */
    headline?: Snippet;
    children: Snippet;
    label?: string;
    /** Follows the pane's element for as long as it exists (an `{@attach}`). */
    attach?: Attachment<HTMLElement>;
    /** The title wraps to show a long name whole rather than cut to one line. */
    wholeTitle?: boolean;
  } & (
    | { notice?: null; noticeOpen?: undefined }
    | {
        /** What went wrong last: it takes the header's room between the title and the actions. */
        notice: string | null;
        /** The notice shows in full rather than cut to one line; the caller owns it. */
        noticeOpen: boolean;
      }
  ) = $props();
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
    {#if lead}{@render lead()}{/if}
    <h2 class:whole={wholeTitle}>{title}</h2>
    {#if beside}{@render beside()}{/if}
    {#if headline}<div class="headline">{@render headline()}</div>{/if}
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
    background: var(--surface-panel);
    outline: none;
  }
  /* A 32px header row; which panel has focus shows in its title's ink, not a line. */
  header {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-height: 32px;
    padding: 0 var(--pad);
  }
  h2 {
    margin: 0;
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--ink-2);
    flex: 0 1 auto;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  h2.whole {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
    overflow-wrap: anywhere;
  }
  .focused h2 {
    color: var(--ink);
  }
  /* The view's headline takes the room it needs, cut at the row's end when the row is short. */
  .headline {
    display: flex;
    align-items: center;
    flex: 0 1000 auto;
    min-width: 0;
    overflow: hidden;
    font-size: var(--text-small);
    white-space: nowrap;
  }
  /* A few words that say what went wrong, taking the remaining room. */
  .notice {
    position: relative;
    flex: 1 1 0;
    min-width: 96px;
    height: 1.5em;
  }
  /* Folded, the notice highlights under the pointer like any borderless control. */
  .notice:not(.open) button:hover {
    background: var(--surface-group);
    border-radius: var(--radius-control);
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
    font-size: var(--text-small);
    text-align: end;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Expanded, the notice keeps its place in the header row (D-MAIN-1): raised inside a
   * panel line, its whole text wrapped at that place's width and scrolling within the row's
   * height, so the header keeps its height and nothing below it moves or is covered. */
  .notice.open {
    box-sizing: border-box;
    height: 28px;
    overflow-x: hidden;
    overflow-y: auto;
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    background: var(--surface-raised);
  }
  .notice.open button {
    padding: 2px 8px;
    line-height: 1.25;
    text-align: start;
    white-space: normal;
    overflow: visible;
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
