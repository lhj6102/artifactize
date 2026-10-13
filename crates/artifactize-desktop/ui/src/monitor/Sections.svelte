<script lang="ts">
  // Outcome → Waits for → What → Provenance → Technical. A peek shows Outcome and Waits for.
  import type { SectionView } from "../ipc/client";

  let {
    sections,
    peek = false,
    technical = false,
    pending = false,
    ontechnical,
    onpending,
  }: {
    sections: SectionView[];
    peek?: boolean;
    technical?: boolean;
    pending?: boolean;
    ontechnical?: () => void;
    onpending?: () => void;
  } = $props();

  /** What each shown section lists: its waits (the pending ones only on request) with how many
   * are pending, and the keys its folded technical summary names. */
  const shown = $derived(
    (peek ? sections.filter((section) => section.name === "outcome" || section.name === "waits") : sections).map(
      (section) => ({
        section,
        waits: section.waits.filter((line) => pending || !line.pending),
        pendingCount: section.waits.filter((line) => line.pending).length,
        keys: section.fields.map((field) => field.key).join(", "),
      }),
    ),
  );
</script>

{#each shown as { section, waits, pendingCount, keys } (section.name)}
  <section class="section">
    {#if section.name === "technical" && !peek}
      <button class="heading toggle" aria-expanded={technical} onclick={ontechnical}>
        {technical ? "▾" : "▸"} Technical
        {#if !technical}<span class="keys">{keys} · t shows</span>{/if}
      </button>
    {:else if !peek}
      <h3 class="heading">{section.title}</h3>
    {/if}
    {#if section.name === "waits"}
      {#if peek}<h3 class="heading inline">Waits for</h3>{/if}
      <ul class="waits">
        {#each waits as line, at (at)}
          <li class:pending={line.pending}>{line.text}</li>
        {/each}
      </ul>
      {#if !pending && pendingCount > 0}
        <button class="link" onclick={onpending}>
          {pendingCount} pending evals · w shows
        </button>
      {/if}
    {:else if section.name !== "technical" || technical}
      <dl>
        <!-- A Detail can repeat a key (a dependency eval's two Source lines): key by position. -->
        {#each section.fields as field, at (at)}
          <dt>{field.key}</dt>
          <dd>{field.value}</dd>
        {/each}
      </dl>
    {/if}
  </section>
{/each}

<style>
  .section {
    padding: 0 var(--pad) 12px;
  }
  .heading {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .inline {
    color: var(--text);
  }
  .toggle {
    border: 0;
    background: none;
    padding: 0;
    cursor: pointer;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .keys {
    color: var(--muted);
    font-weight: 400;
    margin-left: 8px;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .waits {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .pending {
    padding-left: 18px;
    color: var(--muted);
  }
  .link {
    border: 0;
    background: none;
    color: var(--muted);
    padding: 0;
    cursor: pointer;
    font: inherit;
    font-size: 12px;
  }
</style>
