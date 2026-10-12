<script lang="ts">
  // The request's Human tools and the last output. Enter or a click on the selected tool
  // runs it at once; the selected tool shows what it runs.
  import type { Attachment } from "svelte/attachments";
  import type { Review } from "./review.svelte";
  import { plain } from "../shell/keys";

  let { review }: { review: Review } = $props();

  const tools = $derived(review.view?.tools ?? []);
  const focused = $derived(review.area === "tools");
  /** `Tab or click: tools` and the `tools` command move focus here. */
  const takeFocus: Attachment<HTMLElement> = (list) => {
    if (focused && !list.contains(document.activeElement)) list.focus();
  };

  function keydown(event: KeyboardEvent) {
    if (plain(event, "Enter")) void review.run();
    else if (plain(event, "ArrowDown") || plain(event, "j")) review.tool = Math.min(review.tool + 1, tools.length - 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) review.tool = Math.max(review.tool - 1, 0);
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  function click(index: number) {
    const run = focused && review.tool === index;
    review.area = "tools";
    review.tool = index;
    if (run) void review.run(index);
  }
</script>

<div class="tools" class:focused>
  <div class="box-title">Tools ({tools.length})</div>
  {#if tools.length === 0}
    <p class="muted">
      {review.view?.recorded
        ? "No Human tools are declared in this eval's scope."
        : "Unknown: no Human definition is recorded."}
    </p>
  {:else}
    <ul
      role="listbox"
      aria-label="Human tools"
      tabindex="0"
      aria-activedescendant="tool-{review.tool}"
      {@attach takeFocus}
      onfocus={() => (review.area = "tools")}
      onkeydown={keydown}
    >
      {#each tools as tool, index (tool.name)}
        <li
          id="tool-{index}"
          role="option"
          aria-selected={index === review.tool}
          class:selected={index === review.tool}
          onclick={() => click(index)}
          onkeydown={() => {}}
        >
          <span class="name">{tool.name}</span>
          <span class="kind">{tool.kind}</span>
        </li>
      {/each}
    </ul>
    <!-- What Enter runs, below the list so selecting never moves the rows. -->
    {#if tools[review.tool]}
      {@const tool = tools[review.tool]}
      <div class="selected-tool" aria-live="polite">
        <span class="action">{tool.action}</span>
        {#if tool.error}<span class="reason">{tool.error}</span>{/if}
        <span class="description">{tool.description}</span>
      </div>
    {/if}
  {/if}
  <!-- Focusable so PgUp/PgDn and arrows scroll long output. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="output" tabindex="0" role="region" aria-label="Tool output">
    {#if review.output}
      <div class="box-title" class:error={review.output.error}>Output · {review.output.title}</div>
      <pre>{review.output.text}</pre>
    {:else}
      <div class="box-title">Output</div>
      <p class="muted">Run a tool to see its output here.</p>
    {/if}
  </div>
</div>

<style>
  .tools {
    display: grid;
    grid-template-rows: auto auto auto minmax(0, 1fr);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 12px;
    min-height: 0;
    min-width: 0;
    height: 100%;
  }
  .focused {
    border-color: var(--accent);
  }
  .box-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    margin-bottom: 4px;
  }
  .box-title.error {
    color: var(--red);
  }
  ul {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
    outline: none;
    max-height: 30vh;
    overflow: auto;
  }
  li {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 2px 8px;
    padding: 4px 8px;
    border-radius: 6px;
    cursor: pointer;
  }
  li:hover {
    background: var(--surface-2);
  }
  .selected {
    background: var(--selection);
  }
  ul:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .kind {
    color: var(--muted);
    font-size: 12px;
  }
  .selected-tool {
    display: grid;
    gap: 2px;
    padding: 0 8px 8px;
  }
  .action,
  .reason,
  .description {
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .action {
    font-family: var(--mono);
  }
  .reason {
    color: var(--red);
  }
  .description {
    color: var(--muted);
  }
  .output {
    overflow: auto;
    border-top: 1px solid var(--border);
    padding-top: 6px;
    outline: none;
    min-height: 0;
  }
  pre {
    margin: 0;
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
  }
  .muted {
    color: var(--muted);
    margin: 0;
  }
</style>
