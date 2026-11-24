<script lang="ts">
  import Icon from "../shell/Icon.svelte";
  import { activityLabel, toolLine, type Tool } from "./tool-line";

  let { tools, grouped = false, running }: { tools: Tool[]; grouped?: boolean; running: boolean } = $props();

  /** A library failure's first nonempty line, not the whole output in the row. */
  function reason(tool: Tool): string {
    return tool.output?.split("\n").find((line) => line.trim()) ?? (tool.exitCode !== null ? `Exit code ${tool.exitCode}` : "Tool failed");
  }
</script>

{#snippet line(tool: Tool)}
  {@const busy = running && tool.output === null && tool.failed === null}
  {@const item = toolLine(tool, busy)}
  <details>
    <summary>
      <span class={busy ? "tone-running" : "icon"}><Icon name={busy ? "running" : item.icon} /></span>
      <span class="verb">{item.verb}</span>
      {#if item.target}<span class="target">{item.target}</span>{/if}
      {#if tool.failed}<span class="failed">{reason(tool)}</span>{/if}
    </summary>
    <div class="details">
      <dl>
        <dt>Asked with</dt><dd>{tool.arguments}</dd>
        {#if tool.exitCode !== null}<dt>Exit code</dt><dd>{tool.exitCode}</dd>{/if}
      </dl>
      {#if tool.output || tool.outputTruncated || tool.omittedLines}
        <pre>{tool.omittedLines ? `… ${tool.omittedLines} earlier ${tool.omittedLines === 1 ? "line" : "lines"}\n` : tool.outputTruncated ? "… Output truncated\n" : ""}{tool.output ?? ""}</pre>
      {/if}
    </div>
  </details>
{/snippet}

{#if grouped}
  <details class="group">
    <summary>
      <span class="icon fold"><Icon name="fold" /></span><span class="icon unfold"><Icon name="unfold" /></span>
      <span class="verb">{activityLabel(tools)}</span>
    </summary>
    <div class="group-lines">
      {#each tools as tool, index (index)}{@render line(tool)}{/each}
    </div>
  </details>
{:else}
  {@render line(tools[0])}
{/if}

<style>
  details { min-width: 0; color: var(--ink-2); font-size: var(--text-small); }
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: var(--row-dense);
    white-space: nowrap;
    list-style: none;
    cursor: pointer;
  }
  summary::-webkit-details-marker { display: none; }
  summary:hover { background: var(--surface-group); }
  summary:focus-visible { outline-offset: -2px; }
  .icon, .tone-running { display: inline-flex; flex: none; }
  .icon { color: var(--ink-3); }
  .verb, .target, .failed { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .target, dd, pre { font-family: var(--mono); }
  .failed { color: var(--red); }
  .group:not([open]) > summary .unfold, .group[open] > summary .fold { display: none; }
  .group-lines, .details { margin-left: 22px; }
  .details { padding: 2px 0 4px; }
  dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 12px; align-items: baseline; margin: 0; }
  dd { margin: 0; overflow-wrap: anywhere; white-space: pre-wrap; }
  pre { margin: 4px 0 0; padding: 6px 8px; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--surface-input); border-radius: var(--radius-control); color: var(--ink); }
</style>
