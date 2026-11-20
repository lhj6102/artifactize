<script lang="ts">
  // The Markdown viewer (J-VIEWER-MARKDOWN, D-VIEW-4): the Artifact's Markdown rendered in a
  // column of the reading measure; an Artifact of several Markdown files lists them at the left.
  // While pointing, a click on a block appends its line and the column the click landed on.
  import { onMount } from "svelte";
  import Icon from "../shell/Icon.svelte";
  import type { Span } from "./markdown";
  import { MarkdownState } from "./markdown-state.svelte";
  import { position } from "./point";
  import { caretOffset, getViewerState } from "./viewer-state.svelte";

  const v = getViewerState();
  const m = new MarkdownState(v);
  const single = $derived(v.context?.targetKind === "file");

  /** A click on the block starting at `line` while pointing: that line, and where in it. */
  function pointAt(event: MouseEvent, line: number) {
    if (!v.pointing) return;
    event.preventDefault();
    event.stopPropagation();
    const caret = caretOffset(event.clientX, event.clientY);
    void v.point(position(single || m.files.length < 2 ? null : m.shown, line, (caret?.offset ?? 0) + 1));
  }

  onMount(() => void m.load(single));
</script>

{#snippet spans(items: Span[])}
  {#each items as span, at (at)}
    {#if span.kind === "code"}<code>{span.text}</code>{:else if span.kind === "strong"}<strong>{span.text}</strong>{:else if span.kind === "em"}<em>{span.text}</em>{:else if span.kind === "link"}<span class="link">{span.text}</span>{:else}{span.text}{/if}
  {/each}
{/snippet}

<div class="markdown" class:single={single || m.files.length < 2}>
  {#if !single && m.files.length >= 2}
    <ul class="tree" aria-label="Markdown files">
      {#each m.files as path (path)}
        <li><button class:selected={m.shown === path} aria-current={m.shown === path ? "true" : undefined} onclick={() => void m.show(path)}><span class="quiet"><Icon name="file" /></span>{path}</button></li>
      {/each}
    </ul>
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <article class="page" class:pointing={v.pointing} aria-label="Document">
    {#if m.error}<p class="error">{m.error}</p>{/if}
    {#each m.read as block, at (at)}
      {#if block.kind === "heading"}
        <svelte:element this={`h${Math.min(block.level, 3)}`} role="heading" aria-level={block.level} tabindex="-1" onclick={(event: MouseEvent) => pointAt(event, block.line)}>{@render spans(block.spans)}</svelte:element>
      {:else if block.kind === "paragraph"}
        <p onclick={(event) => pointAt(event, block.line)}>{@render spans(block.spans)}</p>
      {:else if block.kind === "list"}
        <svelte:element this={block.ordered ? "ol" : "ul"} class="list">
          {#each block.items as item (item.line)}<li onclick={(event) => pointAt(event, item.line)}>{@render spans(item.spans)}</li>{/each}
        </svelte:element>
      {:else if block.kind === "code"}
        <pre onclick={(event) => pointAt(event, block.line)}>{block.text}</pre>
      {:else if block.kind === "table"}
        <table onclick={(event) => pointAt(event, block.line)}>
          <thead><tr>{#each block.head as cell, c (c)}<th>{@render spans(cell)}</th>{/each}</tr></thead>
          <tbody>{#each block.rows as row, r (r)}<tr>{#each row as cell, c (c)}<td>{@render spans(cell)}</td>{/each}</tr>{/each}</tbody>
        </table>
      {:else}
        <hr />
      {/if}
    {/each}
  </article>
</div>

<style>
  .markdown {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .markdown.single {
    grid-template-columns: minmax(0, 1fr);
  }
  .tree {
    margin: 0;
    padding: 4px 0;
    overflow: auto;
    list-style: none;
    background: var(--surface-panel);
    border-right: 1px solid var(--line-panel);
    font-size: var(--text-small);
  }
  .tree button {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: var(--row-dense);
    margin: 0;
    padding: 0 var(--pad);
    border: 0;
    background: none;
    color: var(--ink);
    font: inherit;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .tree button:hover {
    background: var(--surface-group);
  }
  .tree button.selected {
    background: var(--selection);
  }
  .quiet {
    display: inline-flex;
    color: var(--ink-2);
  }
  /* The rendered document on the panel, a column of the reading measure. */
  .page {
    min-width: 0;
    overflow: auto;
    padding: 16px 24px;
    background: var(--surface-panel);
  }
  .page > :global(*) {
    max-width: var(--measure);
  }
  h1 {
    margin: 0 0 8px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line-group);
    font-size: var(--text-heading);
  }
  h2 {
    margin: 14px 0 6px;
    font-size: var(--text-title);
  }
  h3 {
    margin: 12px 0 4px;
    font-size: var(--text-body);
  }
  p,
  .list {
    margin: 0 0 8px;
    overflow-wrap: anywhere;
  }
  code,
  pre {
    border-radius: var(--radius-control);
    background: var(--surface-input);
    font: var(--text-small) var(--mono);
  }
  code {
    padding: 0 4px;
  }
  pre {
    margin: 0 0 8px;
    padding: 8px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .link {
    color: var(--accent);
  }
  table {
    margin: 0 0 8px;
    border-collapse: collapse;
  }
  th,
  td {
    padding: 2px 8px;
    border: 1px solid var(--line-group);
    text-align: left;
  }
  hr {
    border: 0;
    border-top: 1px solid var(--line-group);
  }
  /* While pointing, the block under the pointer is outlined in the accent. */
  .pointing :global(:is(h1, h2, h3, p, li, pre, table):hover) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
    cursor: crosshair;
  }
  .error {
    color: var(--red);
  }
</style>
