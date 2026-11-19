<script lang="ts">
  // A result's fields by name as a key/value grid, in their order: keys right-aligned in
  // --ink-2, a list's value as numbered items, each led by its head (a finding's priority, or
  // its rule and place).
  import type { ResultEntry } from "../ipc/client";

  let { entries }: { entries: ResultEntry[] } = $props();
</script>

<dl class="entries">
  {#each entries as entry (entry.key)}
    <dt>{entry.key}</dt>
    {#if entry.kind === "text"}
      <dd>{entry.text}</dd>
    {:else}
      <dd>
        <ol class="items">
          {#each entry.items as item, at (at)}
            <li>
              {#if item.head}<p class="head">{item.head}</p>{/if}
              {#each item.fields as field, part (part)}
                <p>{#if item.fields.length > 1}<span class="key">{field.key}</span> {/if}{field.value}</p>
              {/each}
            </li>
          {/each}
        </ol>
      </dd>
    {/if}
  {/each}
</dl>

<style>
  .entries {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 16px;
    margin: 0;
  }
  dt {
    color: var(--ink-2);
    text-align: right;
    overflow-wrap: anywhere;
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
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
