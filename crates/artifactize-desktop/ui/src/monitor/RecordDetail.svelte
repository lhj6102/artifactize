<script lang="ts">
  // The read-only Detail of a request that is not an open Human review. A request with a log
  // switches between its Summary (sections) and its log: an Agent's live transcript, or a
  // runtime's saved output. The monitor's opened Detail and the review screen's Detail of any
  // other request both show it.
  import type { DetailView } from "../ipc/client";
  import Sections from "./Sections.svelte";
  import Transcript from "./Transcript.svelte";
  import { tone } from "../shell/tone";

  let {
    detail,
    technical = $bindable(false),
    pending = $bindable(false),
    log = $bindable(false),
  }: {
    detail: DetailView;
    technical?: boolean;
    pending?: boolean;
    /** The log rather than the Summary. */
    log?: boolean;
  } = $props();

  const agent = $derived(detail.profile === "agent" && detail.request !== null);
  /** The name of the request's log, if it has one. */
  const logName = $derived(agent ? "Agent log" : detail.evidence ? "Runtime log" : null);
  const showLog = $derived(log && logName !== null);
</script>

<!-- Focusable so the arrows and PgUp/PgDn scroll the sections. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="full" tabindex="0" role="document">
  {#if logName}
    <div class="chips small views" role="tablist" aria-label="Views">
      <button class="chip" role="tab" aria-selected={!showLog} onclick={() => (log = false)}>Summary</button>
      <button class="chip" role="tab" aria-selected={showLog} onclick={() => (log = true)}>{logName}</button>
    </div>
  {/if}
  {#if detail.summary}
    <p class="summary {tone(detail.mark?.status)}">{detail.summary}</p>
  {/if}
  {#if showLog && agent && detail.request}
    <div class="shown">
      {#key detail.request}
        <Transcript id={detail.request} />
      {/key}
    </div>
  {:else if showLog && detail.evidence}
    <div class="shown evidence">
      <h3>{detail.evidence.title}</h3>
      <pre>{detail.evidence.text}</pre>
    </div>
  {:else}
    <div class="shown">
      <Sections
        sections={detail.sections}
        {technical}
        {pending}
        ontechnical={() => (technical = !technical)}
        onpending={() => (pending = !pending)}
      />
    </div>
  {/if}
</div>

<style>
  .views {
    grid-row: 1;
    padding: 0 var(--pad) 8px;
  }
  .summary {
    grid-row: 2;
    margin: 0 0 10px;
    padding: 0 var(--pad);
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  /* Sizes flow from the Detail down: every column is at most the width it is given. The
   * switch and the summary take their rows only when shown. */
  .full {
    height: 100%;
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    outline: none;
    min-height: 0;
  }
  .shown {
    grid-row: 3;
    min-width: 0;
    min-height: 0;
  }
  .evidence {
    padding: 0 var(--pad) 12px;
  }
  .evidence h3 {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--accent);
    overflow-wrap: anywhere;
  }
  pre {
    margin: 0;
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 10px 12px;
  }
</style>
