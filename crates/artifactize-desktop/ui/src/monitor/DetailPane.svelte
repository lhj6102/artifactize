<script lang="ts">
  // The Detail of an Artifact or of the Run, docked over the graph of a Run or a Current: the
  // inspector, in sections under header rows. An Artifact's own tab lists its evals (each opens
  // its tab), then Neighbours (folded until asked for), its own Human tools and their Tool log,
  // its Path and Debug. An eval's tab has the chips that read as a path, `[Artifact icon] {name}
  // / [eval] [eval] …`, and shows its Summary or its log (an Agent's transcript, a runtime's
  // saved output), or in a Run the shared Human Detail. Its width is the Detail's: everything in
  // it wraps or scrolls in its own box.
  import { tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, message, type BranchView, type Source } from "../ipc/client";
  import type { Review } from "../review/review.svelte";
  import { getAppState } from "../shell/app.svelte";
  import { DetailPaneState, setDetailPaneState } from "./detail-pane-state.svelte";
  import { keyOf } from "./monitor-state.svelte";
  import Panel from "../shell/Panel.svelte";
  import Section from "../shell/Section.svelte";
  import Icon from "../shell/Icon.svelte";
  import { markIcon } from "../shell/icons";
  import RecordDetail from "./RecordDetail.svelte";
  import { NeighboursState } from "./neighbours-state.svelte";
  import Neighbours from "./Neighbours.svelte";
  import ArtifactTools from "./ArtifactTools.svelte";
  import ArtifactStar from "./ArtifactStar.svelte";
  import Definition from "./Definition.svelte";
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

  /** The opened Artifact's neighbours, read by its Neighbours section. */
  const neighbours = new NeighboursState();
  /** A part of several wholes is drawn in each: Debug names them, in graph order. */
  const drawnIn = $derived.by((): [string, string][] => {
    const graph = neighbours.graph;
    if (!graph || graph.copies <= 1 || !graph.shape.nodes.some((item) => item.id === node)) return [];
    return [["Drawn in", graph.wholes.join(", ")]];
  });
  const commands = useIpc();
  const app = getAppState();
  const { sections } = app;
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

  /** The eval's log rather than its Summary: one node's, another opened node shows its Summary. */
  const log = $derived(d.log === target);

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
  /** The section `t` folds and unfolds: a Run's Technical, else Debug. */
  const last = $derived(detail?.target === "run" ? "Technical" : "Debug");

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

  /** The Artifact's own tab, chosen by its chip: its tab has no chips, so the keyboard stays in
   * the Detail rather than going with the chip. */
  async function showArtifact() {
    ontab(null);
    await tick();
    element?.focus();
  }

  function keydown(event: KeyboardEvent) {
    // A Human review takes its own keys; from the tabs only Esc and ← close.
    if (human && (event.target as HTMLElement | null)?.closest(".human")) return;
    if (plain(event, "ArrowLeft") && holding) return;
    if (plain(event, "Escape") || plain(event, "ArrowLeft")) onback();
    else if (human || log) return;
    else if (plain(event, "t")) sections.toggle(last);
    else return;
    event.preventDefault();
  }
</script>

<!-- The title names what the tab shows, the Artifact's name whole in it: the Artifact on its own tab, `{Artifact}/{eval} · {title}` on an eval's tab, a Human eval's included. -->
<Panel title={detail?.title ?? "Detail"} wholeTitle focused={focused} bind:element {onfocus} onkeydown={keydown} attach={read}>
  {#snippet beside()}
    <!-- An Artifact's Detail, on any of its tabs, stars the Artifact. -->
    {#if tabbed && row && branch}<ArtifactStar branch={branch.key} artifact={row.name} />{/if}
  {/snippet}
  {#snippet actions()}
    <!-- The one window-level control, on every tab: on a Human review's it first cancels the
         review's running work, then leaves its form, as Esc does. -->
    <button class="button small" aria-label="Close (Esc)" aria-keyshortcuts="Escape" onclick={onback}>Close</button>
  {/snippet}
  <div class="detail" class:tabbed={tabbed && tab !== null}>
    {#if tabbed && row && tab !== null}
      <div class="chips strip" role="tablist" aria-label="Tabs of {row.name}">
        <!-- The `/` stays with the Artifact's chip: only eval chips flow onto further lines. -->
        <span class="head">
          <button class="chip" role="tab" aria-selected={false} aria-label="Artifact {row.name}" onclick={() => void showArtifact()}>
            <img class="mark" src={brandMark} alt="" /><span class="label">{row.name}</span>
          </button>
          <span class="slash" aria-hidden="true">/</span>
        </span>
        {#each evals as item (item.id)}
          {@const icon = markIcon(item.glyph, item.tone)}
          <button class="chip" role="tab" aria-selected={tab === item.id} data-tab={item.id} onclick={() => ontab(item.id)}>
            <span class="glyph {tone(item.tone)}">{#if icon}<Icon name={icon} />{:else}{item.glyph}{/if}</span><span class="label">{item.name}</span>
          </button>
        {/each}
      </div>
    {/if}
    <div class="content" class:listing={tabbed && tab === null} role={tabbed && tab !== null ? "tabpanel" : undefined}>
      {#if tabbed && row && tab === null}
        <!-- A section with nothing to hold is not shown: an Artifact without evals has no Evals. -->
        {#if evals.length > 0}
        <Section name="Evals">
          <ul class="evals" aria-label="Evals of {row.name}">
            {#each evals as item (item.id)}
              {@const icon = markIcon(item.glyph, item.tone)}
              <li>
                <button class="eval weight-{item.weight}" data-eval={item.id} onclick={() => void showEval(item.id)}>
                  <span class="glyph {tone(item.tone)}">{#if icon}<Icon name={icon} />{:else}{item.glyph}{/if}</span>
                  <span class="name">{item.name}{#if item.marks}<span class="muted"> {item.marks}</span>{/if}</span>
                  <span class="status">
                    {#each item.text as segment, part (part)}<span class={tone(segment.tone)}>{segment.text}</span>{/each}
                  </span>
                  <span class="muted right">{item.right}</span>
                </button>
              </li>
            {/each}
          </ul>
        </Section>
        {/if}
        <Neighbours {source} {revision} {node} {onopen} state={neighbours} />
        {#if branch}
          {#key `${branch.workspace}\n${row.name}`}<ArtifactTools branch={branch.key} workspace={branch.workspace} artifact={row.name} />{/key}
        {/if}
        <!-- On a Current the Artifact's definition, which its graph and its Detail edit, stands
             where a Run's Path does. -->
        {#if branch && source.kind === "current"}
          {#key row.name}<Definition branch={branch.key} name={row.name} />{/key}
        {/if}
      {/if}
      {#if d.error}
        <p class="error">{d.error}</p>
      {:else if !detail}
        <p class="empty">Loading…</p>
      {:else if human && detail.request}
        {#key detail.request}
          {@const reviewing = review(detail.request)}
          {@const request = detail.request}
          {#if reviewing}<HumanDetail review={reviewing} {active} {onback} onopenreview={() => app.showReview(request)} />{/if}
        {/key}
      {:else}
        <RecordDetail {detail} path={source.kind !== "current"} debugFacts={tab === null ? drawnIn : []} bind:log={() => log, (value) => (d.log = value ? target : null)} />
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
  /* The chips, a path back to the Artifact: many wrap onto further lines, closed below by a
     group line; they never widen the Detail. */
  .strip {
    align-items: center;
    padding: 2px var(--pad) 8px;
    border-bottom: 1px solid var(--line-group);
  }
  /* The Artifact's chip and its `/` wrap as one; a long name is cut inside the chip. */
  .head {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    min-width: 0;
  }
  /* The icon always shows; only the name is cut short. */
  .head .chip {
    flex: 0 1 auto;
    min-width: 0;
  }
  .chip {
    align-items: center;
  }
  .mark {
    flex: none;
    width: var(--icon);
    height: var(--icon);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .slash {
    flex: none;
    color: var(--ink-2);
  }
  .glyph {
    display: inline-flex;
    font-family: var(--mono);
  }
  .content {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    min-height: 0;
    overflow: auto;
  }
  .content:not(.listing) {
    padding-top: 8px;
  }
  /* The Artifact's sections each keep their full height and the tab scrolls as one, so no
     part is squeezed under the next. */
  .content.listing {
    grid-template-rows: none;
    grid-auto-rows: max-content;
  }
  .evals {
    list-style: none;
    margin: 0 calc(-1 * var(--pad));
    padding: 0;
  }
  /* Four columns: the verdict's icon, the name, the status words in the status colour, and
     time and tokens pinned to the panel's right edge. The name and the status keep one line
     while there is room; what the right column says wraps in what is left. */
  .eval {
    display: grid;
    grid-template-columns: var(--icon) minmax(0, max-content) minmax(0, max-content) minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: var(--row-dense);
    margin: 0;
    padding: 1px var(--pad);
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .eval:hover {
    background: var(--surface-group);
  }
  .eval .name {
    min-width: 0;
    overflow-wrap: break-word;
  }
  .eval .status {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .eval .right {
    font-size: var(--text-small);
    justify-self: end;
    min-width: 0;
    text-align: right;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }
  .weight-bold .name,
  .weight-bold .status {
    font-weight: 600;
  }
  .weight-dim {
    color: var(--ink-2);
  }
  .muted {
    color: var(--ink-2);
  }
  .empty {
    color: var(--ink-2);
    padding: 0 var(--pad);
  }
  .error {
    color: var(--red);
    padding: 0 var(--pad);
    overflow-wrap: anywhere;
  }
</style>
