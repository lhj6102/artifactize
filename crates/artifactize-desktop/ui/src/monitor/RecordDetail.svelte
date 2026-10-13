<script lang="ts">
  // The read-only Detail of a request that is not an open Human review: its sections and saved
  // runtime logs, or an Agent's live transcript (`d` shows its sections). The monitor's opened
  // Detail and the review screen's Detail of any other request both show it.
  import type { DetailView } from "../ipc/client";
  import Sections from "./Sections.svelte";
  import Transcript from "./Transcript.svelte";
  import { tone } from "../shell/tone";

  let {
    detail,
    technical = $bindable(false),
    pending = $bindable(false),
    details = $bindable(false),
  }: {
    detail: DetailView;
    technical?: boolean;
    pending?: boolean;
    /** An Agent's summary sections instead of its transcript. */
    details?: boolean;
  } = $props();

  const agent = $derived(detail.profile === "agent" && detail.request !== null);
</script>

<!-- Focusable so the arrows and PgUp/PgDn scroll the sections. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="full" tabindex="0" role="document">
  {#if detail.summary}
    <p class="summary {tone(detail.mark?.status)}">{detail.summary}</p>
  {/if}
  {#if agent && !details && detail.request}
    {#key detail.request}
      <Transcript id={detail.request} />
    {/key}
  {:else}
    <div class="split" class:logs={detail.evidence !== null}>
      <div>
        <Sections
          sections={detail.sections}
          {technical}
          {pending}
          ontechnical={() => (technical = !technical)}
          onpending={() => (pending = !pending)}
        />
      </div>
      {#if detail.evidence}
        <div class="evidence">
          <h3>{detail.evidence.title}</h3>
          <pre>{detail.evidence.text}</pre>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .summary {
    margin: 0 0 10px;
    padding: 0 var(--pad);
    font-weight: 500;
  }
  .full {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr;
    outline: none;
    min-height: 0;
  }
  .split {
    display: grid;
    gap: var(--gap);
    min-height: 0;
  }
  .split.logs {
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  }
  .evidence {
    padding: 0 var(--pad) 12px;
    min-width: 0;
  }
  .evidence h3 {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--accent);
  }
  pre {
    margin: 0;
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 10px 12px;
  }
</style>
