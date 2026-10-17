<script lang="ts">
  // The Detail of an Artifact or of the Run, floating over the graph. An Artifact's Detail has
  // tabs: the Artifact's own, which lists its evals with their verdicts, then one per eval. An
  // eval's tab shows its sections and saved logs, an Agent transcript, or the shared Human
  // Detail. Everything in it wraps or is cut to its width.
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, message, type TreeNode } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import type { Review } from "../review/review.svelte";
  import { DetailPaneState, setDetailPaneState } from "./detail-pane-state.svelte";
  import Panel from "../shell/Panel.svelte";
  import RecordDetail from "./RecordDetail.svelte";
  import HumanDetail from "../review/HumanDetail.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    run,
    node,
    tab,
    tree,
    focused,
    active,
    review,
    openReview,
    element = $bindable(),
    onfocus,
    ontab,
    onback,
  }: {
    run: string;
    /** The Artifact (or the Run) the Detail is of. */
    node: string;
    /** The eval whose tab is shown, or `null` for the Artifact's own. */
    tab: string | null;
    /** The Run's tree: an Artifact's evals and their verdicts come from there. */
    tree: TreeNode[];
    focused: boolean;
    /** The Monitor is the shown screen; only then its review offers palette commands. */
    active: boolean;
    review: (id: string) => Review | undefined;
    /** Open the Review of a request: called as its Detail arrives. */
    openReview: (id: string) => void;
    element?: HTMLElement;
    onfocus: () => void;
    /** Show the tab of an eval, or the Artifact's own for `null`. */
    ontab: (tab: string | null) => void;
    onback: () => void;
  } = $props();
  const commands = useIpc();
  const d = new DetailPaneState();
  setDetailPaneState(d);
  const { browse } = getAppState();

  const row = $derived(tree.find((top) => top.id === node) ?? null);
  const evals = $derived(row?.kind === "artifact" ? row.children : []);
  const tabbed = $derived(row?.kind === "artifact");
  /** The node whose Detail is read: the shown eval, else the Artifact or the Run. */
  const target = $derived(tab ?? node);

  /** The folds belong to one node: another opened node starts folded, as the terminal's
   * Detail does. */
  const technical = $derived(d.folds.node === target && d.folds.technical);
  const pending = $derived(d.folds.node === target && d.folds.pending);
  /** An Agent's summary sections instead of its transcript. */
  const details = $derived(d.folds.node === target && d.folds.details);
  function fold(change: Partial<{ technical: boolean; pending: boolean; details: boolean }>) {
    d.folds = { node: target, technical, pending, details, ...change };
  }

  /** Read again for every accepted snapshot; the Detail comes from the backend's copy. */
  const read: Attachment<HTMLElement> = () => {
    void browse.revision;
    const [at, wanted] = [run, target];
    let current = true;
    call(commands.detail(at, wanted)).then(
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
  const detail = $derived(d.detail?.run === run && d.detail.node === target ? d.detail : null);
  const human = $derived(detail?.profile === "human" && detail.request !== null);
  const agent = $derived(detail?.profile === "agent" && detail.request !== null);

  /** The wheel scrolls the tab strip sideways when its tabs overflow it. */
  function wheel(event: WheelEvent) {
    const strip = event.currentTarget as HTMLElement;
    if (event.deltaY === 0 || strip.scrollWidth <= strip.clientWidth) return;
    strip.scrollLeft += event.deltaY;
    event.preventDefault();
  }

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
    <!-- A Human review has its own Close among its controls. -->
    {#if !human}
      {#if agent}
        <button class="button small" onclick={() => fold({ details: !details })} aria-pressed={details}>
          Details <kbd>d</kbd>
        </button>
      {/if}
      <button class="button small" onclick={onback}>Close <kbd>Esc</kbd></button>
    {/if}
  {/snippet}
  <div class="detail" class:tabbed>
    {#if tabbed && row}
      <div class="strip" role="tablist" aria-label="Tabs of {row.name}" onwheel={wheel}>
        <button class="pick" role="tab" aria-selected={tab === null} onclick={() => ontab(null)}>Artifact</button>
        {#each evals as item (item.id)}
          <button class="pick" role="tab" aria-selected={tab === item.id} data-tab={item.id} onclick={() => ontab(item.id)}>
            <span class="glyph {tone(item.tone)}">{item.glyph}</span>{item.name}
          </button>
        {/each}
      </div>
    {/if}
    <div class="content" class:listing={tabbed && tab === null} role={tabbed ? "tabpanel" : undefined}>
      {#if tabbed && row && tab === null}
        <ul class="evals" aria-label="Evals of {row.name}">
          {#each evals as item (item.id)}
            <li>
              <button class="eval weight-{item.weight}" data-eval={item.id} onclick={() => ontab(item.id)}>
                <span class="glyph {tone(item.tone)}">{item.glyph}</span>
                <span class="name">{item.name}{#if item.marks}<span class="muted"> {item.marks}</span>{/if}</span>
                <span class="status">
                  {#each item.text as segment, part (part)}<span class={tone(segment.tone)}>{segment.text}</span>{/each}
                </span>
                <span class="muted right">{item.right}</span>
              </button>
            </li>
          {:else}
            <li class="muted none">No evals.</li>
          {/each}
        </ul>
      {/if}
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
    </div>
  </div>
</Panel>

<style>
  .detail {
    height: 100%;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  .detail.tabbed {
    grid-template-rows: auto minmax(0, 1fr);
  }
  /* Many tabs scroll sideways; they never widen the Detail. */
  .strip {
    display: flex;
    gap: 2px;
    min-width: 0;
    overflow-x: auto;
    padding: 0 var(--pad);
    border-bottom: 1px solid var(--border);
  }
  /* A thin scrollbar of its own, below the tabs rather than over their labels. */
  .strip::-webkit-scrollbar {
    height: 4px;
  }
  .strip::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 2px;
  }
  .pick {
    flex: none;
    display: inline-flex;
    gap: 6px;
    align-items: baseline;
    max-width: 24ch;
    margin: 0;
    padding: 6px 10px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--muted);
    font: inherit;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .pick:hover {
    color: var(--text);
  }
  .pick[aria-selected="true"] {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .content {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    min-height: 0;
    overflow: auto;
    padding-top: 8px;
  }
  .content.listing {
    grid-template-rows: auto minmax(0, 1fr);
  }
  .evals {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
  }
  .eval {
    display: grid;
    grid-template-columns: 16px minmax(0, auto) minmax(0, 1fr) auto;
    gap: 8px;
    width: 100%;
    margin: 0;
    padding: 4px var(--pad);
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .eval:hover {
    background: var(--surface-2);
  }
  .eval .name,
  .eval .status {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .weight-bold .name,
  .weight-bold .status {
    font-weight: 600;
  }
  .weight-dim {
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
  .none {
    padding: 0 var(--pad);
  }
  .empty {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .error {
    color: var(--red);
    padding: 0 var(--pad);
    overflow-wrap: anywhere;
  }
</style>
