<script lang="ts">
  // The Detail of one node of the Run, floating over the graph: sections and saved logs, an
  // Agent transcript, or the shared Human Detail.
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, message } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import type { Review } from "../review/review.svelte";
  import { DetailPaneState, setDetailPaneState } from "./detail-pane-state.svelte";
  import Panel from "../shell/Panel.svelte";
  import RecordDetail from "./RecordDetail.svelte";
  import HumanDetail from "../review/HumanDetail.svelte";
  import { plain } from "../shell/keys";

  let {
    run,
    node,
    focused,
    active,
    review,
    openReview,
    element = $bindable(),
    onfocus,
    onback,
  }: {
    run: string;
    node: string;
    focused: boolean;
    /** The Monitor is the shown screen; only then its review offers palette commands. */
    active: boolean;
    review: (id: string) => Review | undefined;
    /** Open the Review of a request: called as its Detail arrives. */
    openReview: (id: string) => void;
    element?: HTMLElement;
    onfocus: () => void;
    onback: () => void;
  } = $props();
  const commands = useIpc();
  const d = new DetailPaneState();
  setDetailPaneState(d);
  const { browse } = getAppState();

  /** The folds belong to one node: another opened node starts folded, as the terminal's
   * Detail does. */
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

  /** Only this node's Detail: another node's, still held while this one is read, is not. */
  const detail = $derived(d.detail?.run === run && d.detail.node === node ? d.detail : null);
  const human = $derived(detail?.profile === "human" && detail.request !== null);
  const agent = $derived(detail?.profile === "agent" && detail.request !== null);

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

<Panel title={human ? "Human review" : (detail?.title ?? "Detail")} focused={focused} bind:element {onfocus} onkeydown={keydown} attach={read}>
  {#snippet actions()}
    {#if !human}
      {#if agent}
        <button class="button small" onclick={() => fold({ details: !details })} aria-pressed={details}>
          Details <kbd>d</kbd>
        </button>
      {/if}
      <button class="button small" onclick={onback} title="Back to the graph">Close <kbd>Esc</kbd></button>
    {/if}
  {/snippet}
  {#if d.error}
    <p class="error">{d.error}</p>
  {:else if !detail}
    <p class="empty">Loading…</p>
  {:else if human && detail.request}
    {#key detail.request}
      {@const reviewing = review(detail.request)}
      {#if reviewing}<HumanDetail review={reviewing} {active} {onback} />{/if}
    {/key}
  {:else}
    <RecordDetail
      {detail}
      bind:technical={() => technical, (value) => fold({ technical: value })}
      bind:pending={() => pending, (value) => fold({ pending: value })}
      bind:details={() => details, (value) => fold({ details: value })}
    />
  {/if}
</Panel>

<style>
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .error {
    color: var(--red);
    padding: 0 var(--pad);
  }
</style>
