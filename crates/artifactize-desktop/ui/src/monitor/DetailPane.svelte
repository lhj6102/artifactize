<script lang="ts">
  // The Detail of the selected tree row: a peek that follows the selection, or the opened
  // Detail — sections and saved logs, an Agent transcript, or the shared Human Detail.
  import { call, commands, message, type DetailView } from "../ipc/client";
  import type { Browse } from "./browse.svelte";
  import type { Review } from "../review/review.svelte";
  import Panel from "../shell/Panel.svelte";
  import RecordDetail from "./RecordDetail.svelte";
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
    active,
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
    /** The Monitor is the shown screen; only then its review offers palette commands. */
    active: boolean;
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
      <HumanDetail review={review(detail.request)} {active} {onback} />
    {/key}
  {:else if full}
    <RecordDetail {detail} bind:technical bind:pending bind:details />
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
</style>
