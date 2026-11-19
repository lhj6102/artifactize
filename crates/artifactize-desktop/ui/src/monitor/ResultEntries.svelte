<script lang="ts">
  // A result's fields by name, in their order: a text after its key (a summary alone), a list as
  // numbered items, each led by its head (a finding's priority, or its rule and place).
  import type { ResultEntry } from "../ipc/client";

  let { entries }: { entries: ResultEntry[] } = $props();
</script>

{#each entries as entry (entry.key)}
  {#if entry.kind === "text"}
    <p class="text">{#if entry.key !== "summary"}<span class="key">{entry.key}</span> {/if}{entry.text}</p>
  {:else}
    <p class="key">{entry.key}</p>
    <ol class="items">
      {#each entry.items as item, at (at)}
        <li>
          {#if item.head}<p class="head">{item.head}</p>{/if}
          {#each item.fields as field, part (part)}
            <p class="line">{#if item.fields.length > 1}<span class="key">{field.key}</span> {/if}{field.value}</p>
          {/each}
        </li>
      {/each}
    </ol>
  {/if}
{/each}

<style>
  p {
    margin: 0 0 4px;
    white-space: pre-wrap;
  }
  .key {
    color: var(--ink-2);
  }
  .items {
    margin: 0;
    padding-left: 20px;
  }
  .items li {
    margin-bottom: 8px;
  }
  .head {
    font-weight: 600;
  }
</style>
