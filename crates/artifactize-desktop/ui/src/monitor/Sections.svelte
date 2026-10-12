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

  const shown = $derived(
    peek ? sections.filter((section) => section.name === "outcome" || section.name === "waits") : sections,
  );
</script>

{#each shown as section (section.name)}
  <section class="section">
    {#if section.name === "technical" && !peek}
      <button class="heading toggle" aria-expanded={technical} onclick={ontechnical}>
        {technical ? "▾" : "▸"} Technical
        {#if !technical}<span class="keys">{section.fields.map((field) => field.key).join(", ")} · t shows</span>{/if}
      </button>
    {:else if !peek}
      <h3 class="heading">{section.title}</h3>
    {/if}
    {#if section.name === "waits"}
      {#if peek}<h3 class="heading inline">Waits for</h3>{/if}
      <ul class="waits">
        {#each section.waits.filter((line) => pending || !line.pending) as line, at (at)}
          <li class:pending={line.pending}>{line.text}</li>
        {/each}
      </ul>
      {#if !pending && section.waits.some((line) => line.pending)}
        <button class="link" onclick={onpending}>
          {section.waits.filter((line) => line.pending).length} pending evals · w shows
        </button>
      {/if}
    {:else if section.name !== "technical" || technical}
      <dl>
        {#each section.fields as field (field.key)}
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
