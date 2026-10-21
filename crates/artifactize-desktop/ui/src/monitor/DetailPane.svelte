<script lang="ts">
  // The Detail of an Artifact or of the Run, floating over the graph of a Run or a Current. An
  // Artifact's Detail has tabs, a chip choice that reads as a path: `[Artifact icon] {name} /
  // [eval] [eval] …`. The Artifact's own draws it between its neighbours, lists its evals with
  // their verdicts (on a Current whether each would run or be reused), then its own Human tools
  // and their Tool log; then comes one tab per eval. An eval's tab shows its Summary or its log
  // (an Agent's transcript, a runtime's saved output), or in a Run the shared Human Detail. Its
  // width is the Detail's: everything in it wraps or scrolls in its own box.
  import { tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, message, type BranchView, type Source } from "../ipc/client";
  import type { Review } from "../review/review.svelte";
  import { DetailPaneState, setDetailPaneState } from "./detail-pane-state.svelte";
  import { keyOf } from "./monitor-state.svelte";
  import Panel from "../shell/Panel.svelte";
  import RecordDetail from "./RecordDetail.svelte";
  import Neighbours from "./Neighbours.svelte";
  import ArtifactTools from "./ArtifactTools.svelte";
  import HumanDetail from "../review/HumanDetail.svelte";
  import { plain } from "../shell/keys";
  import { tone } from "../shell/tone";
  import brandMark from "../../../../../assets/brand/artifactize-icon-mark.svg";

  let {
    source,
    revision,
    branch,
    node,
    tab,
    focused,
    active,
    review,
    openReview,
    element = $bindable(),
    onfocus,
    ontab,
    onopen,
    onback,
    ongone,
  }: {
    /** The Run or Current the Detail is of. */
    source: Source;
    /** The snapshot's or scan's revision: the Detail reads again when it grows. */
    revision: number;
    /** The branch shown: an Artifact's tools run in its worktree. */
    branch: BranchView | null;
    /** The Artifact (or the Run) the Detail is of. */
    node: string;
    /** The eval whose tab is shown, or `null` for the Artifact's own. */
    tab: string | null;
    focused: boolean;
    /** The Main area shows this Detail; only then its review offers palette commands. */
    active: boolean;
    review: (id: string) => Review | undefined;
    /** Open the Review of a request: called as its Detail arrives. */
    openReview: (id: string) => void;
    element?: HTMLElement;
    onfocus: () => void;
    /** Show the tab of an eval, or the Artifact's own for `null`. */
    ontab: (tab: string | null) => void;
    /** Open another Artifact's Detail: a neighbour in the Artifact tab. */
    onopen: (node: string) => void;
    onback: () => void;
    /** The Run no longer has the node: the Detail has nothing left to show. */
    ongone: () => void;
  } = $props();
  const commands = useIpc();
  const d = new DetailPaneState();
  setDetailPaneState(d);
  const key = $derived(keyOf(source));

  /** The node's row of the Run's or Current's tree: an Artifact's evals and their verdicts come
   * from there. */
  const row = $derived(d.row?.source === key && d.row.view.id === node ? d.row.view : null);
  const evals = $derived(row?.kind === "artifact" ? row.children : []);
  const tabbed = $derived(row?.kind === "artifact");
  /** The node whose Detail is read: the shown eval, else the Artifact or the Run. */
  const target = $derived(tab ?? node);

  /** The folds belong to one node: another opened node starts folded, as the terminal's
   * Detail does. */
  const technical = $derived(d.folds.node === target && d.folds.technical);
  const pending = $derived(d.folds.node === target && d.folds.pending);
  /** The eval's log rather than its Summary. */
  const log = $derived(d.folds.node === target && d.folds.log);
  /** The eval's Debug facts unfolded. */
  const debug = $derived(d.folds.node === target && d.folds.debug);
  function fold(change: Partial<{ technical: boolean; pending: boolean; log: boolean; debug: boolean }>) {
    d.folds = { node: target, technical, pending, log, debug, ...change };
  }

  /** Read again for every new snapshot or scan; the Detail and the node's row come from the
   * backend's copy. */
  const read: Attachment<HTMLElement> = () => {
    void revision;
    const [at, wanted, of] = [source, target, node];
    let current = true;
    call(commands.detailRow(at, of)).then(
      (view) => {
        if (!current) return;
        if (view) d.row = { source: keyOf(at), view };
        else ongone();
      },
      (failure) => {
        if (current) d.error = message(failure);
      },
    );
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
  const detail = $derived(d.detail && keyOf(d.detail.source) === key && d.detail.node === target ? d.detail : null);
  const human = $derived(detail?.profile === "human" && detail.request !== null);
  /** An Artifact's or an eval's Summary keeps its technical facts under Debug; a Run's
   * sections keep theirs under Technical. */
  const folded = $derived(detail !== null && (detail.card !== null || detail.target === "artifact"));

  /** The Human review this tab shows, while it runs work or edits its form: ← leaves it alone,
   * and Esc steps through it first (the Monitor's close asks it). */
  const holding = $derived.by(() => {
    const reviewing = human && detail?.request ? review(detail.request) : undefined;
    return reviewing !== undefined && (reviewing.busy !== null || reviewing.editing);
  });

  /** An eval chosen from the Artifact tab's list: the list goes with that tab, so the keyboard
   * moves to the chosen eval's chip rather than out of the Detail. */
  async function showEval(id: string) {
    ontab(id);
    await tick();
    element?.querySelector<HTMLElement>(`[role="tab"][data-tab="${CSS.escape(id)}"]`)?.focus();
  }

  function keydown(event: KeyboardEvent) {
    // A Human review takes its own keys; from the tabs only Esc and ← close.
    if (human && (event.target as HTMLElement | null)?.closest(".human")) return;
    if (plain(event, "ArrowLeft") && holding) return;
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) onback();
    else if (human || log) return;
    else if (plain(event, "t")) fold(folded ? { debug: !debug } : { technical: !technical });
    else if (plain(event, "w")) fold({ pending: !pending });
    else return;
    event.preventDefault();
  }
</script>

<Panel title={human ? "Human review" : (detail?.title ?? "Detail")} wholeTitle focused={focused} bind:element {onfocus} onkeydown={keydown} attach={read}>
  {#snippet actions()}
    <!-- The one window-level control; a Human review has its own Close among its controls. -->
    {#if !human}<button class="button small" onclick={onback}>Close <kbd>Esc</kbd></button>{/if}
  {/snippet}
  <div class="detail" class:tabbed>
    {#if tabbed && row}
      <div class="chips strip" role="tablist" aria-label="Tabs of {row.name}">
        <!-- The `/` stays with the Artifact's chip: only eval chips flow onto further lines. -->
        <span class="head">
          <button class="chip" role="tab" aria-selected={tab === null} aria-label="Artifact {row.name}" onclick={() => ontab(null)}>
            <img class="mark" src={brandMark} alt="" /><span class="label">{row.name}</span>
          </button>
          {#if evals.length > 0}<span class="slash" aria-hidden="true">/</span>{/if}
        </span>
        {#each evals as item (item.id)}
          <button class="chip" role="tab" aria-selected={tab === item.id} data-tab={item.id} onclick={() => ontab(item.id)}>
            <span class="glyph {tone(item.tone)}">{item.glyph}</span>{item.name}
          </button>
        {/each}
      </div>
    {/if}
    <div class="content" class:listing={tabbed && tab === null} role={tabbed ? "tabpanel" : undefined}>
      {#if tabbed && row && tab === null}
        <Neighbours {source} {revision} {node} {onopen} />
        <ul class="evals" aria-label="Evals of {row.name}">
          {#each evals as item (item.id)}
            <li>
              <button class="eval weight-{item.weight}" data-eval={item.id} onclick={() => void showEval(item.id)}>
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
        {#if branch}
          {#key `${branch.workspace}\n${row.name}`}<ArtifactTools branch={branch.key} workspace={branch.workspace} artifact={row.name} />{/key}
        {/if}
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
          bind:log={() => log, (value) => fold({ log: value })}
          bind:debug={() => debug, (value) => fold({ debug: value })}
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
  /* Many tabs wrap onto further lines; they never widen the Detail. */
  .strip {
    align-items: baseline;
    padding: 2px var(--pad) 8px;
    border-bottom: 1px solid var(--border);
  }
  /* The Artifact's chip and its `/` wrap as one; a long name is cut inside the chip. */
  .head {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    max-width: 100%;
    min-width: 0;
  }
  /* The icon always shows; only the name is cut short. */
  .head .chip {
    flex: 0 1 auto;
    min-width: 0;
    align-items: center;
  }
  .mark {
    flex: none;
    width: 1.1em;
    height: 1.1em;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .slash {
    flex: none;
    color: var(--muted);
  }
  .content {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    min-height: 0;
    overflow: auto;
    padding-top: 8px;
  }
  /* The neighbours, the eval list and the Artifact's own detail each keep their full
     height and the tab scrolls as one, so no part is squeezed under the next. */
  .content.listing {
    grid-template-rows: none;
    grid-auto-rows: max-content;
  }
  .evals {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
  }
  /* The name and status take their words first, never cut inside one; the title takes what is
     left and wraps there. */
  .eval {
    display: grid;
    grid-template-columns: 16px minmax(0, auto) auto minmax(0, 1fr);
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
  .eval .name {
    min-width: 0;
    overflow-wrap: break-word;
  }
  .eval .right {
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
