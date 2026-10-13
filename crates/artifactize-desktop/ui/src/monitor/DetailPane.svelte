<script lang="ts">
  // The Detail of the selected tree row: a peek that follows the selection, or the opened
  // Detail — sections and saved logs, an Agent transcript, or the shared Human Detail.
  import type { Attachment } from "svelte/attachments";
  import { call, commands, message } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import type { Review } from "../review/review.svelte";
  import { DetailPaneState, setDetailPaneState } from "./detailPane.svelte";
  import Panel from "../shell/Panel.svelte";
  import RecordDetail from "./RecordDetail.svelte";
  import Sections from "./Sections.svelte";
  import Transcript from "./Transcript.svelte";
  import HumanDetail from "../review/HumanDetail.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    run,
    node,
    full,
    focused,
    active,
    review,
    openReview,
    element = $bindable(),
    onfocus,
    onopen,
    onback,
  }: {
    run: string | null;
    node: string | null;
    full: boolean;
    focused: boolean;
    /** The Monitor is the shown screen; only then its review offers palette commands. */
    active: boolean;
    review: (id: string) => Review | undefined;
    /** Open the Review of a request: called as its Detail arrives. */
    openReview: (id: string) => void;
    element?: HTMLElement;
    onfocus: () => void;
    onopen: () => void;
    onback: () => void;
  } = $props();
  const d = new DetailPaneState();
  setDetailPaneState(d);
  const { browse } = getAppState();

  
  
  /** The folds belong to one row: another opened row starts folded, as the terminal's Detail
   * does. */
  
  const technical = $derived(d.folds.node === node && d.folds.technical);
  const pending = $derived(d.folds.node === node && d.folds.pending);
  /** An Agent's summary sections instead of its transcript. */
  const details = $derived(d.folds.node === node && d.folds.details);
  function fold(change: Partial<{ technical: boolean; pending: boolean; details: boolean }>) {
    d.folds = { node, technical, pending, details, ...change };
  }

  /** Read again for every accepted snapshot; the Detail comes from the backend's copy. */
  const read: Attachment<HTMLElement> = () => {
    void browse.revision;
    const [at, target] = [run, node];
    if (!at || !target) {
      d.detail = null;
      return;
    }
    let current = true;
    call(commands.detail(at, target)).then(
      (view) => {
        if (current) {
          // A Human review gets its owner now, so rendering never has to make one.
          if (view.profile === "human" && view.request) openReview(view.request);
          d.detail = view;
          d.error = null;
        }
      },
      (failure) => {
        if (current) d.error = message(failure);
      },
    );
    return () => {
      current = false;
    };
  };

  const human = $derived(full && d.detail?.profile === "human" && d.detail.request !== null);
  const agent = $derived(d.detail?.profile === "agent" && d.detail.request !== null);

  function keydown(event: KeyboardEvent) {
    if (human) return;
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) {
      if (details) fold({ details: false });
      else onback();
    } else if (plain(event, "t") && (!agent || details)) fold({ technical: !technical });
    else if (plain(event, "w") && (!agent || details)) fold({ pending: !pending });
    else if (plain(event, "d") && agent) fold({ details: !details });
    else return;
    event.preventDefault();
  }
</script>

<Panel title={human ? "Human review" : (d.detail?.title ?? "Detail")} focused={focused} bind:element {onfocus} onkeydown={keydown} attach={read}>
  {#snippet actions()}
    {#if full && !human}
      {#if agent}
        <button class="button small" onclick={() => fold({ details: !details })} aria-pressed={details}>
          Details <kbd>d</kbd>
        </button>
      {/if}
      <button class="button small" onclick={onback} title="Back to the tree">Close <kbd>Esc</kbd></button>
    {/if}
  {/snippet}
  {#if d.error}
    <p class="error">{d.error}</p>
  {:else if !d.detail}
    <p class="empty">Select a node.</p>
  {:else if human && d.detail.request}
    {#key d.detail.request}
      {@const reviewing = review(d.detail.request)}
      {#if reviewing}<HumanDetail review={reviewing} {active} {onback} />{/if}
    {/key}
  {:else if full}
    <RecordDetail
      detail={d.detail}
      bind:technical={() => technical, (value) => fold({ technical: value })}
      bind:pending={() => pending, (value) => fold({ pending: value })}
      bind:details={() => details, (value) => fold({ details: value })}
    />
  {:else}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="peek" role="button" tabindex="-1" onclick={onopen} title="Open">
      {#if d.detail.summary}<p class="summary {tone(d.detail.mark?.status)}">{d.detail.summary}</p>{/if}
      <Sections sections={d.detail.sections} peek />
      {#if agent && d.detail.running && d.detail.request}
        {#key d.detail.request}
          <Transcript id={d.detail.request} peek />
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
