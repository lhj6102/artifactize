<script lang="ts">
  // Chat's and Verify's Main area: one stage header of controls only (D-MAIN-1, J-STAGE): the
  // way back at its left, the view tabs in its middle, the shown view's tools at its right; then
  // the view, with the stage's status line (D-MAIN-2) over its top left. Nothing in the header
  // names or rates what is shown: the Pane's selected row does. Keys reach `onkeydown` while
  // focus is anywhere inside it.
  import type { Snippet } from "svelte";
  import { StageSlot, setStageSlot } from "./stage.svelte";

  let {
    label,
    element = $bindable(),
    onfocus,
    onkeydown,
    back,
    tabs,
    tools,
    status,
    notice = null,
    noticeOpen = $bindable(false),
    children,
  }: {
    /** The area's accessible name. */
    label: string;
    element?: HTMLElement;
    onfocus?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    /** The way back to where the view was opened from (D-REF-1). */
    back?: Snippet;
    /** The view tabs, when the mode has more than one view (D-CHAT-10). */
    tabs?: Snippet;
    /** The stage's own tools, after those a view shows in the header (a graph's). */
    tools?: Snippet;
    /** The shown view's state, on the status line (a Current's scan). */
    status?: Snippet;
    /** What went wrong last, a few red words on the status line; a click shows it whole. */
    notice?: string | null;
    noticeOpen?: boolean;
    children: Snippet;
  } = $props();

  const slot = new StageSlot();
  setStageSlot(slot);

  /** Give the reason first; preserve the original text for the expanded notice. */
  function shortNotice(text: string): string {
    if (text.length <= 58) return text;
    const problem = /changed on disk|has unsaved text|cannot be read/i.exec(text);
    if (problem) {
      const file = text.slice(0, problem.index).trim().split(/[\\/]/).at(-1);
      return `${problem[0][0].toUpperCase()}${problem[0].slice(1)}${file && file.length <= 24 ? ` · ${file}` : ""}`;
    }
    const path = /^(?:[A-Za-z]:)?(?:[/\\]|[^ :]+[/\\])[^:]+:\s*(.+)/.exec(text);
    const reason = path?.[1] || text;
    return reason.length <= 58 ? reason : `${reason.slice(0, 57).replace(/\s+\S*$/, "")}…`;
  }

  function noticeKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || !noticeOpen) return;
    noticeOpen = false;
    event.stopPropagation();
  }
</script>

<!-- Keys from the area's rows and controls bubble up to its handler. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="stage-area" aria-label={label} tabindex="-1" bind:this={element} onfocusin={onfocus} {onkeydown}>
  <header class="stage-head">
    <div class="back">{@render back?.()}</div>
    <div class="tabs">{@render tabs?.()}</div>
    <div class="tools" role="toolbar" aria-label="Tools">
      <span class="slot" bind:this={slot.tools}></span>
      {@render tools?.()}
    </div>
  </header>
  <div class="body">
    {@render children()}
    {#if status || notice}
      <div class="status-line">
        {@render status?.()}
        {#if notice}
          <div class="notice" class:open={noticeOpen} role="status" onkeydown={noticeKey}>
            <button type="button" aria-expanded={noticeOpen} onclick={() => (noticeOpen = !noticeOpen)}>{noticeOpen ? notice : shortNotice(notice)}</button>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .stage-area {
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
    background: var(--surface-panel);
    outline: none;
  }
  .stage-area:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* One 32px row, the same in Chat and Verify: the way back, the tabs after it (never moved by
   * the tools), the tools at the right. */
  .stage-head {
    position: relative;
    z-index: 30;
    display: grid;
    grid-template-columns: auto auto minmax(max-content, 1fr);
    align-items: center;
    gap: 12px;
    height: 32px;
    padding: 0 var(--pad);
    box-sizing: border-box;
    border-bottom: 1px solid var(--line-panel);
    background: var(--surface-panel);
    font-size: var(--text-small);
  }
  .back {
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .tabs {
    display: flex;
    align-self: stretch;
  }
  .tools {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
    min-width: 0;
    white-space: nowrap;
  }
  .slot {
    display: contents;
  }
  /* A tool of the stage header: borderless and faceless, by luminance alone (D-MAIN-1). */
  .tools :global(.tool) {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    min-width: var(--control);
    height: var(--control);
    margin: 0;
    padding: 0 6px;
    box-sizing: border-box;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    font: inherit;
    font-weight: 400;
    cursor: pointer;
  }
  .tools :global(.tool:hover:not(:disabled)) {
    color: var(--ink);
  }
  .tools :global(.tool[aria-pressed="true"]),
  .tools :global(.tool[aria-expanded="true"]) {
    color: var(--ink-bright);
  }
  .tools :global(.tool:disabled) {
    color: var(--ink-3);
    cursor: default;
  }
  .tools :global(.tool:focus-visible) {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .tools :global(.tool-sep) {
    flex: none;
    width: 1px;
    height: 16px;
    margin: 0 6px;
    background: var(--line-group);
  }
  .body {
    position: relative;
    min-height: 0;
    overflow: auto;
  }
  /* The status line: one quiet line over the view's top left, no face behind it (D-MAIN-2). */
  .status-line {
    position: absolute;
    z-index: 25;
    top: 8px;
    left: var(--pad);
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: calc(100% - 2 * var(--pad));
    color: var(--ink-2);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
  }
  .status-line > :global(*) {
    pointer-events: auto;
  }
  .notice {
    min-width: 0;
  }
  .notice button {
    display: block;
    max-width: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: var(--red);
    text-align: start;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .notice.open {
    width: 460px;
    max-width: 100%;
    max-height: 96px;
    overflow-y: auto;
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    background: var(--surface-raised);
  }
  .notice.open button {
    padding: 6px 8px;
    line-height: 16px;
    white-space: normal;
    overflow-wrap: anywhere;
  }
</style>
