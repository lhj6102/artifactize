<script lang="ts">
  // The Detail of the selected tree row: a peek that follows the selection, or the opened
  // Detail — sections and saved logs, an Agent transcript, or the shared Human Detail.
  import { call, commands, message, type DetailView } from "../ipc/client";
  import type { Browse } from "./browse.svelte";
  import type { Review } from "../review/review.svelte";
  import Panel from "../shell/Panel.svelte";
  import Sections from "./Sections.svelte";
  import Transcript from "./Transcript.svelte";
  import HumanDetail from "../review/HumanDetail.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    browse,
    run,
    node,
    full,
    focused,
    review,
    element = $bindable(),
    onfocus,
    onopen,
    onback,
  }: {
    browse: Browse;
    run: string | null;
    node: string | null;
    full: boolean;
    focused: boolean;
    review: (id: string) => Review;
    element?: HTMLElement;
    onfocus: () => void;
    onopen: () => void;
    onback: () => void;
  } = $props();

  let detail = $state<DetailView | null>(null);
  let error = $state<string | null>(null);
  let technical = $state(false);
  let pending = $state(false);
  /** An Agent's summary sections instead of its transcript. */
  let details = $state(false);

  $effect(() => {
    // Read again for every accepted snapshot; the Detail comes from the backend's copy.
    void browse.revision;
    const [at, target] = [run, node];
    if (!at || !target) {
      detail = null;
      return;
    }
    let current = true;
    call(commands.detail(at, target)).then(
      (view) => {
        if (current) {
          detail = view;
          error = null;
        }
      },
      (failure) => {
        if (current) error = message(failure);
      },
    );
    return () => {
      current = false;
    };
  });

  $effect(() => {
    // Another opened row starts folded, as the terminal's Detail does.
    void node;
    if (full) {
      technical = false;
      pending = false;
      details = false;
    }
  });

  const human = $derived(full && detail?.profile === "human" && detail.request !== null);
  const agent = $derived(detail?.profile === "agent" && detail.request !== null);

  function keydown(event: KeyboardEvent) {
    if (human) return;
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) {
      if (details) details = false;
      else onback();
    } else if (plain(event, "t") && (!agent || details)) technical = !technical;
    else if (plain(event, "w") && (!agent || details)) pending = !pending;
    else if (plain(event, "d") && agent) details = !details;
    else return;
    event.preventDefault();
  }
</script>

<Panel title={human ? "Human review" : (detail?.title ?? "Detail")} focused={focused} bind:element {onfocus} onkeydown={keydown}>
  {#snippet actions()}
    {#if full && !human}
      {#if agent}
        <button class="button small" onclick={() => (details = !details)} aria-pressed={details}>
          Details <kbd>d</kbd>
        </button>
      {/if}
      <button class="button small" onclick={onback} title="Back to the tree">Close <kbd>Esc</kbd></button>
    {/if}
  {/snippet}
  {#if error}
    <p class="error">{error}</p>
  {:else if !detail}
    <p class="empty">Select a node.</p>
  {:else if human && detail.request}
    {#key detail.request}
      <HumanDetail review={review(detail.request)} {onback} />
    {/key}
  {:else if full}
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
  {:else}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="peek" role="button" tabindex="-1" onclick={onopen} title="Open">
      {#if detail.summary}<p class="summary {tone(detail.mark?.status)}">{detail.summary}</p>{/if}
      <Sections sections={detail.sections} peek />
      {#if agent && detail.running && detail.request}
        {#key detail.request}
          <Transcript id={detail.request} peek />
        {/key}
      {/if}
      <p class="hint">Enter or click: open</p>
    </div>
  {/if}
</Panel>

<style>
  .empty,
  .hint {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .error {
    color: var(--red);
    padding: 0 var(--pad);
  }
  .summary {
    margin: 0 0 10px;
    padding: 0 var(--pad);
    font-weight: 500;
  }
  .peek {
    cursor: pointer;
    height: 100%;
    outline: none;
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
