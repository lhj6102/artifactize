<script lang="ts">
  import { getAppState } from "../shell/app.svelte";
  import { markdown, type Block } from "./markdown";

  let { text }: { text: string } = $props();
  const c = getAppState().chat;
  const document = $derived(markdown(text));
</script>

{#snippet items(list: Block[][])}
  {#each list as item, index (index)}
    <li>{@render blocks(item)}</li>
  {/each}
{/snippet}

{#snippet blocks(content: Block[])}
  {#each content as block, index (index)}
    {#if block.kind === "paragraph"}
      <p>{#each block.parts as part, at (at)}{#if part.kind === "link"}<a href={part.url} draggable="false" onclick={(event) => { event.preventDefault(); void c.openLink(part.url); }} onauxclick={(event) => { event.preventDefault(); if (event.button === 1) void c.openLink(part.url); }}>{part.text}</a>{:else if part.kind === "code"}<code>{part.text}</code>{:else}{part.text}{/if}{/each}</p>
    {:else if block.kind === "code"}
      <div class="code-block">
        <div class="code-actions"><button class="button small quiet" onclick={() => void c.copy(block.text)}>Copy</button></div>
        <pre><code>{block.text}</code></pre>
      </div>
    {:else if block.start === null}
      <ul>{@render items(block.items)}</ul>
    {:else}
      <ol start={block.start}>{@render items(block.items)}</ol>
    {/if}
  {/each}
{/snippet}

<div class="answer">{@render blocks(document)}</div>

<style>
  .answer { min-width: 0; overflow-wrap: anywhere; }
  p { margin: 0; white-space: pre-wrap; }
  p + p, p + ul, p + ol, ul + p, ol + p, .code-block + p, p + .code-block, .code-block + .code-block { margin-top: 8px; }
  ul, ol { margin: 0; padding-left: 20px; }
  a { color: var(--accent); text-decoration: underline; }
  code { font: var(--text-small) var(--mono); }
  p code { padding: 1px 3px; background: var(--surface-group); border-radius: var(--radius-control); }
  .code-block { min-width: 0; background: var(--surface-panel); border: 1px solid var(--line-group); border-radius: var(--radius-control); }
  .code-actions { display: flex; justify-content: flex-end; padding: 4px 4px 0; }
  pre { margin: 0; overflow-x: auto; padding: 0 8px 8px; white-space: pre; }
</style>
