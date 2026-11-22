<script lang="ts">
  // The Markdown viewer (J-VIEWER-MARKDOWN, D-VIEW-4): the Artifact's Markdown rendered in a
  // column of the reading measure; an Artifact of several Markdown files lists them at the left.
  // While pointing, a click appends the line and column the pointer landed on.
  import { onMount } from "svelte";
  import { call, message } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import type { Span } from "./markdown";
  import { MarkdownState } from "./markdown-state.svelte";
  import { position } from "./point";
  import { caretOffset, getViewerState } from "./viewer-state.svelte";

  const v = getViewerState();
  const m = new MarkdownState(v);
  const single = $derived(v.context?.targetKind === "file");

  const handle = $derived(v.markdownHandle);

  function sourceLine(event: MouseEvent) {
    return (event.target as HTMLElement).closest<HTMLElement>("[data-line]");
  }

  function pointAt(event: MouseEvent) {
    if (!v.pointing) return;
    event.preventDefault();
    const line = sourceLine(event);
    if (!line) return;
    const caret = caretOffset(event.clientX, event.clientY);
    const before = document.createRange();
    before.selectNodeContents(line);
    if (caret && line.contains(caret.node)) before.setEnd(caret.node, caret.offset);
    else before.collapse(true);
    v.markdownHandle = null;
    void v.point(position(single || m.files.length < 2 ? null : m.shown, Number(line.dataset.line), before.toString().length + 1));
  }

  /** Highlight only the visible line, even when a single source line wraps. */
  function hover(event: MouseEvent) {
    const line = v.pointing ? sourceLine(event) : null;
    if (!line) { v.markdownHandle = null; return; }
    const range = document.createRange();
    range.selectNodeContents(line);
    const band = Array.from(range.getClientRects()).filter((rect) => rect.width > 0 && rect.top <= event.clientY && rect.bottom >= event.clientY);
    const rects = band.length ? band : line.matches("tr, .code-line") ? [line.getBoundingClientRect()] : [];
    if (!rects.length) { v.markdownHandle = null; return; }
    const page = event.currentTarget as HTMLElement;
    const bounds = page.getBoundingClientRect();
    const left = Math.min(...rects.map((rect) => rect.left));
    const top = Math.min(...rects.map((rect) => rect.top));
    v.markdownHandle = {
      left: left - bounds.left + page.scrollLeft,
      top: top - bounds.top + page.scrollTop,
      width: Math.max(...rects.map((rect) => rect.right)) - left,
      height: Math.max(...rects.map((rect) => rect.bottom)) - top,
    };
  }

  async function openLink(event: MouseEvent, url: string) {
    event.preventDefault();
    if (v.pointing || (event.type === "auxclick" && event.button !== 1)) return;
    try {
      await call(v.ipc.chatOpenLink(url));
      v.error = null;
    } catch (failure) {
      v.error = message(failure);
    }
  }

  onMount(() => void m.load(single));
</script>

{#snippet spans(items: Span[])}
  {#each items as span, at (at)}
    {#if span.kind === "code"}<code>{span.text}</code>{:else if span.kind === "strong"}<strong>{span.text}</strong>{:else if span.kind === "em"}<em>{span.text}</em>{:else if span.kind === "link"}<a href={span.url} draggable="false" onclick={(event) => void openLink(event, span.url)} onauxclick={(event) => void openLink(event, span.url)}>{span.text}</a>{:else}{span.text}{/if}
  {/each}
{/snippet}

{#snippet folder(path: string, depth: number)}
  {#each m.tree.get(path) ?? [] as entry (entry.path)}
    <li>
      {#if entry.kind === "directory"}
        {@const open = m.unfolded.has(entry.path)}
        <button style:--depth={depth} aria-expanded={open} onclick={() => m.toggle(entry.path)}>
          <span class="quiet"><Icon name={open ? "unfold" : "fold"} /></span><span class="quiet"><Icon name="folder" /></span><span class="filename">{entry.name}</span>
        </button>
        {#if open}<ul>{@render folder(entry.path, depth + 1)}</ul>{/if}
      {:else}
        <button style:--depth={depth} class:selected={m.shown === entry.path} aria-current={m.shown === entry.path ? "true" : undefined} onclick={() => void m.show(entry.path)}>
          <span class="quiet"><Icon name="file" /></span><span class="filename">{entry.name}</span>
        </button>
      {/if}
    </li>
  {/each}
{/snippet}

<div class="markdown" class:single={single || m.files.length < 2}>
  {#if !single && m.files.length >= 2}
    <ul class="tree" aria-label="Markdown files">
      {@render folder("", 0)}
    </ul>
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <article class="page" class:pointing={v.pointing} aria-label="Document" onclick={pointAt} onmousemove={hover} onmouseleave={() => (v.markdownHandle = null)} onscroll={() => (v.markdownHandle = null)}>
    {#if v.pointing && handle}
      <span class="handle" aria-hidden="true" style:left="{handle.left}px" style:top="{handle.top}px" style:width="{handle.width}px" style:height="{handle.height}px"></span>
    {/if}
    {#if m.error}<p class="error">{m.error}</p>{/if}
    {#each m.read as block, at (at)}
      {#if block.kind === "heading"}
        <svelte:element this={`h${Math.min(block.level, 3)}`} role="heading" aria-level={block.level} tabindex="-1" data-line={block.line}>{@render spans(block.spans)}</svelte:element>
      {:else if block.kind === "paragraph"}
        <p>{#each block.lines as items, n (n)}{n ? " " : ""}<span data-line={block.line + n}>{@render spans(items)}</span>{/each}</p>
      {:else if block.kind === "list"}
        <svelte:element this={block.ordered ? "ol" : "ul"} class="list">
          {#each block.items as item (item.line)}<li data-line={item.line}>{@render spans(item.spans)}</li>{/each}
        </svelte:element>
      {:else if block.kind === "code"}
        <pre>{#each block.text.split("\n") as text, n (n)}<span class="code-line" data-line={block.line + n + 1}>{text}</span>{/each}</pre>
      {:else if block.kind === "table"}
        <table>
          <thead><tr data-line={block.line}>{#each block.head as cell, c (c)}<th>{@render spans(cell)}</th>{/each}</tr></thead>
          <tbody>{#each block.rows as row, r (r)}<tr data-line={block.line + r + 2}>{#each row as cell, c (c)}<td>{@render spans(cell)}</td>{/each}</tr>{/each}</tbody>
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
    grid-template-columns: min(240px, 40%) minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .markdown.single {
    grid-template-columns: minmax(0, 1fr);
  }
  .tree {
    margin: 0;
    padding: 4px 0;
    overflow: hidden auto;
    list-style: none;
    background: var(--surface-panel);
    border-right: 1px solid var(--line-panel);
    font-size: var(--text-small);
  }
  .tree ul { margin: 0; padding: 0; list-style: none; }
  .tree button {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: var(--row-dense);
    margin: 0;
    padding: 0 var(--pad) 0 min(50%, calc(var(--pad) + var(--depth, 0) * 14px));
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .tree button:not(.selected):hover {
    background: var(--surface-group);
  }
  .tree button.selected {
    background: var(--selection);
    color: var(--ink);
  }
  .filename {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .quiet {
    flex: none;
    display: inline-flex;
    color: var(--ink-2);
  }
  /* The rendered document on the panel, a column of the reading measure. */
  .page {
    position: relative;
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
  h2,
  h3 { font-weight: 600; }
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
    line-height: 1.45;
  }
  .code-line {
    display: block;
    min-height: 1lh;
  }
  a {
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
    overflow-wrap: anywhere;
  }
  hr {
    border: 0;
    border-top: 1px solid var(--line-group);
  }
  .pointing, .pointing a {
    cursor: crosshair;
  }
  .handle {
    position: absolute;
    pointer-events: none;
    outline: 2px dashed var(--handle);
    box-shadow: inset 0 0 0 2px var(--handle-edge);
    outline-offset: -2px;
  }
  .error {
    color: var(--red);
  }
</style>
