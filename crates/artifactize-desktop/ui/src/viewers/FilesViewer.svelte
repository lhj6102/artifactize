<script lang="ts">
  // The File Explorer (J-VIEWER-FILES, D-VIEW-3): the Artifact's files as a tree at the left,
  // folders folding and unfolding by a click, and the shown file's text at the right with line
  // numbers, read-only. A one-file Artifact has one tree row. While pointing, a
  // click on a place in the text appends its line and column.
  import { onMount } from "svelte";
  import Icon from "../shell/Icon.svelte";
  import { caretOffset, getViewerState } from "./viewer-state.svelte";
  import { FilesState } from "./files-state.svelte";
  import { position } from "./point";

  const v = getViewerState();
  const f = new FilesState(v);
  const single = $derived(v.context?.targetKind === "file");
  const fileName = $derived(v.context?.target.split(/[\\/]/).pop() ?? "");
  const lines = $derived(f.text === null ? [] : f.text.split("\n"));

  /** A click on line `line` while pointing: its line and column, with the file's path when the
   * Artifact holds several files. */
  function pointAt(event: MouseEvent, line: number) {
    if (!v.pointing || f.shown === null) return;
    event.preventDefault();
    const caret = caretOffset(event.clientX, event.clientY);
    void v.point(position(single ? null : f.shown, line, (caret?.offset ?? 0) + 1));
  }

  onMount(() => {
    if (single) void f.show("");
    else void f.list("");
  });
</script>

{#snippet folder(path: string, depth: number)}
  {#each f.entries.get(path) ?? [] as entry (entry.path)}
    <li>
      {#if entry.kind === "directory"}
        {@const open = f.unfolded.has(entry.path)}
        <button style:--depth={depth} aria-expanded={open} onclick={() => void f.toggle(entry.path)}>
          <span class="quiet"><Icon name={open ? "unfold" : "fold"} /></span><span class="quiet"><Icon name="folder" /></span><span class="filename">{entry.name}</span>
        </button>
        {#if open}<ul>{@render folder(entry.path, depth + 1)}</ul>{/if}
      {:else}
        <button style:--depth={depth} class:selected={f.shown === entry.path} aria-current={f.shown === entry.path ? "true" : undefined} onclick={() => void f.show(entry.path)}>
          <span class="quiet"><Icon name="file" /></span><span class="filename">{entry.name}</span>
        </button>
      {/if}
    </li>
  {/each}
{/snippet}

<div class="files">
  <ul class="tree" aria-label="Files">
    {#if single}
      <li><button class:selected={f.shown === ""} aria-current={f.shown === "" ? "true" : undefined} onclick={() => void f.show("")}>
        <span class="quiet"><Icon name="file" /></span><span class="filename">{fileName}</span>
      </button></li>
    {:else}
      {@render folder("", 0)}
    {/if}
  </ul>
  <section class="doc" aria-label="Document">
    {#if f.shown !== null}
      <div class="head mono">{single ? (v.context?.target ?? "") : f.shown}</div>
      {#if f.error}<p class="error" role="status">{f.error}</p>{/if}
      {#if f.text !== null}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <ol class="source" class:pointing={v.pointing}>
          {#each lines as text, at (at)}
            <li onclick={(event) => pointAt(event, at + 1)}>{text}</li>
          {/each}
        </ol>
      {/if}
    {:else}
      <p class={f.error ? "error" : "empty"}>{f.error ?? "Choose a file."}</p>
    {/if}
  </section>
</div>

<style>
  .files {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .mono {
    font-family: var(--mono);
  }
  /* The tree: dense rows indented by depth, the shown file on the selection. */
  .tree {
    margin: 0;
    padding: 4px 0;
    overflow: hidden auto;
    background: var(--surface-panel);
    border-right: 1px solid var(--line-panel);
    font-size: var(--text-small);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
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
  /* The document view: the path's head row, then the text with its line numbers. */
  .doc {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--surface-input);
  }
  .head {
    display: flex;
    align-items: center;
    min-height: var(--row-head);
    padding: 0 var(--pad);
    background: var(--surface-panel);
    border-bottom: 1px solid var(--line-group);
    font-size: var(--text-small);
  }
  .source {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 8px 0;
    list-style: none;
    counter-reset: line;
    font: var(--text-small) / 1.5 var(--mono);
  }
  .source li {
    padding: 0 var(--pad) 0 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .source li::before {
    counter-increment: line;
    content: counter(line);
    display: inline-block;
    width: 36px;
    margin-right: 8px;
    color: var(--ink-2);
    text-align: right;
    user-select: none;
  }
  /* While pointing, the line under the pointer is outlined by the viewer's handle edge. */
  .source.pointing li:hover {
    outline: 2px dashed var(--handle);
    box-shadow: inset 0 0 0 2px var(--handle-edge);
    outline-offset: -2px;
    cursor: crosshair;
  }
  .empty,
  .error {
    margin: 0;
    padding: 8px var(--pad);
    color: var(--ink-2);
  }
  .error {
    color: var(--red);
  }
</style>
