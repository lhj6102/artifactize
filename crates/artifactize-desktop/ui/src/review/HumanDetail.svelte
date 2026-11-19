<script lang="ts">
  // The one Human review component, shared by the Detail and the Review view. It has one order,
  // the instruction, the Tools, their Tool log and the Result with its row of actions, then
  // Debug, and two arrangements by the room it has. In one column (a Human eval's tab in the
  // Detail, or a Main area narrower than twice the Detail's width) it reads top to bottom, no
  // wider than the measure, the Tool log a box between the Tools and the Result. In two (a Main
  // area at least twice as wide), the Tool log is the view, a panel filling the left, and the
  // rest is a column at the right, where the Detail docks and of its width. Its eval's title is
  // said once, above it; the review opens with its stage alone.
  import { HumanDetailState, setHumanDetailState } from "./human-detail-state.svelte";
  import { onMount, tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { onStateChanged, type Settled } from "../ipc/client";
  import type { Review } from "./review.svelte";
  import Tools from "./Tools.svelte";
  import ToolLog from "./ToolLog.svelte";
  import VerdictForm from "./VerdictForm.svelte";
  import ResultEntries from "../monitor/ResultEntries.svelte";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
  import Section from "../shell/Section.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, composing, editable, label, plain } from "../shell/keys";
  import { clampWidth } from "../monitor/detail-width";

  let {
    review,
    active = true,
    wide = false,
    onback,
    onsubmitted,
  }: {
    review: Review;
    /** Its screen is shown: only then it offers palette commands. */
    active?: boolean;
    /** It fills the Main area: with room for two, the Tool log is the view at the left. */
    wide?: boolean;
    onback: () => void;
    /** A submission settled the request, published or saved locally only. */
    onsubmitted?: (settled: Settled) => void;
  } = $props();
  const { sections, monitor: m } = getAppState();
  const h = new HumanDetailState();
  setHumanDetailState(h);

  const view = $derived(review.view);
  /** The stage pill's words; a submitted review has none. */
  const stage = $derived(view?.stage === "completed" ? null : view?.stage === "review" ? "REVIEW (yours)" : "CLAIM");
  /** The default share of the area the Detail takes, as the dock's width says. */
  const DOCK_SHARE = 0.46;
  const DOCK_MOST = 680;
  /** Two panels: the Main area is at least twice as wide as the Detail would be. */
  const split = $derived.by(() => {
    if (!wide || h.width === 0) return false;
    const dock = clampWidth(m.detailWidth ?? Math.min(h.width * DOCK_SHARE, DOCK_MOST), h.width);
    return h.width >= 2 * dock;
  });

  /** The review's width, measured as it changes. */
  const measure: Attachment<HTMLElement> = (root) => {
    const sized = new ResizeObserver(() => (h.width = root.clientWidth));
    sized.observe(root);
    return () => sized.disconnect();
  };

  onMount(() => {
    void review.load();
    // Waiting time and the busy seconds redraw without reading the state.
    const clock = setInterval(() => {
      h.now = Date.now();
      if (!review.busy) void review.load(false);
    }, 1000);
    // A settlement or claim elsewhere shows on the next read; an open form is set aside.
    const stop = onStateChanged(() => void review.load());
    // Tools that already took focus (a reopened request with Tools focused) keep it.
    if (h.root && !h.root.contains(document.activeElement)) h.root.focus();
    return () => {
      clearInterval(clock);
      stop();
    };
  });

  async function submit() {
    const settled = await review.submit();
    if (!settled) return;
    onsubmitted?.(settled);
    await tick();
    // Submission removes the focused form or Submit button. Keep Esc reachable, without
    // taking focus back if the person moved to another control or screen while it ran.
    if (active && h.root?.isConnected && document.activeElement === document.body) h.root.focus();
  }

  /** Esc: cancel the job, then stop editing, then leave. Stopping editing removes the
   * focused field, so focus returns to the review and the next Esc still reaches it. */
  async function back() {
    if (review.escape()) {
      onback();
      return;
    }
    await tick();
    if (h.root && !h.root.contains(document.activeElement)) h.root.focus();
  }

  function scrollInstruction(rows: number) {
    h.instruction?.scrollBy({ top: rows * 20 });
  }

  function keydown(event: KeyboardEvent) {
    // Keys of an input method's composition, Escape included, belong to the composition.
    if (event.defaultPrevented || composing(event)) return;
    const handled = () => {
      event.preventDefault();
      event.stopPropagation();
    };
    if ((event.ctrlKey || event.metaKey) && (event.key === "PageUp" || event.key === "PageDown")) {
      scrollInstruction(event.key === "PageDown" ? 10 : -10);
      return handled();
    }
    if (event.key === "Escape") {
      void back();
      return handled();
    }
    // `q` quits only outside a form or work, as in the terminal.
    if ((review.busy || review.editing) && plain(event, "q")) return handled();
    // Submit is open while a tool runs: it cancels the tool first (Review.submit).
    if (chord(event, "s") && (review.busy === null || review.busy.tool)) void submit();
    else if (review.busy || editable(event.target)) return;
    else if (plain(event, "c")) void review.claim();
    else if (plain(event, "g")) void review.choose("GREEN");
    else if (plain(event, "r")) void review.choose("RED");
    else if (plain(event, "u")) void review.release();
    else if (plain(event, "t")) sections.toggle("Debug");
    else if (plain(event, "ArrowLeft") && !review.editing) onback();
    else return;
    handled();
  }

  useCommands(() => {
    if (!active) return [];
    const ready = () => !review.busy && !review.settled;
    return [
      { id: "human.claim", group: "Human review", label: "Claim to review", keys: "c", enabled: () => ready() && review.claimable, run: () => void review.claim() },
      { id: "human.green", group: "Human review", label: "Open the GREEN form", keys: "g", enabled: () => ready() && review.owned && !review.editing, run: () => void review.choose("GREEN") },
      { id: "human.red", group: "Human review", label: "Open the RED form", keys: "r", enabled: () => ready() && review.owned && !review.editing, run: () => void review.choose("RED") },
      { id: "human.to-green", group: "Human review", label: "Choose GREEN", enabled: () => ready() && review.editing, run: () => void review.choose("GREEN") },
      { id: "human.to-red", group: "Human review", label: "Choose RED", enabled: () => ready() && review.editing, run: () => void review.choose("RED") },
      { id: "human.submit", group: "Human review", label: "Submit", keys: "Ctrl+S", enabled: () => review.submittable, run: () => void submit() },
      { id: "human.release", group: "Human review", label: "Release the claim", keys: "u", enabled: () => ready() && review.owned, run: () => void review.release() },
      { id: "human.cancel", group: "Human review", label: "Cancel the running work", keys: "Esc", enabled: () => review.busy !== null, run: () => review.cancel() },
      { id: "human.debug", group: "Human review", label: "Show or hide Debug", keys: "t", run: () => sections.toggle("Debug") },
    ];
  });
</script>

{#snippet head()}
  <!-- The stage alone: CLAIM, or REVIEW (yours) once claimed here; the claimant beside it while
       another reviewer holds the claim. -->
  <div class="head">
    {#if stage}<span class="stage" data-stage={view?.stage}>{stage}</span>{/if}
    {#if view?.claim && !view.claim.mine && !review.owned}<span class="claimant">{view.claim.reviewer}</span>{/if}
  </div>
{/snippet}

{#snippet column()}
  {#if view}
    {@render head()}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="instruction"
      tabindex="0"
      role="region"
      aria-label="Instruction"
      bind:this={h.instruction}
      onfocusin={() => {
        if (!review.editing) review.area = "instruction";
      }}
    >
      <p class="text">{view.instruction}</p>
      {#if view.shared}<p class="shared">{view.shared}</p>{/if}
    </div>
    <Section name="Tools" folds={false}>
      <Tools {review} />
    </Section>
    {#if !split && !(review.settled && review.log.length === 0)}
      <Section name="Tool log" folds={false}>
        <ToolLog entries={review.log} empty="What the tools print shows here." />
      </Section>
    {/if}
    <Section name="Result" folds={false}>
      <div class="result" role="group" onfocusin={() => (review.area = "fields")}>
        {#if review.settled}
          <!-- The completed result by field, read-only; a request settled without one, its error. -->
          <div class="completed">
            {#if view.completed}<ResultEntries entries={view.completed} />{:else}<pre>{view.result}</pre>{/if}
          </div>
          <!-- Settled here but not published: what failed, so it is pushed again, not resubmitted. -->
          {#if review.notice?.error}<p class="error" role="status">{review.notice.text}</p>{/if}
        {:else if review.owned && review.form}
          <VerdictForm {review} form={review.form} />
        {:else if review.owned}
          <div class="verdicts">
            <button class="button" aria-label="GREEN (g)" aria-keyshortcuts="g" onclick={() => void review.choose("GREEN")}>GREEN</button>
            <button class="button" aria-label="RED (r)" aria-keyshortcuts="r" onclick={() => void review.choose("RED")}>RED</button>
          </div>
        {:else if view.claim}
          <p class="muted">Read-only until released.</p>
        {:else}
          <button class="button primary" aria-label="Claim to review (c)" aria-keyshortcuts="c" disabled={!review.claimable || review.busy !== null} onclick={() => void review.claim()}>Claim to review</button>
        {/if}
        <!-- One row of actions, the last of the Result: Release claim at the left, Submit at the
             right, and between them what running work or a failure has to say. -->
        {#if !review.settled && (review.owned || review.busy || review.notice?.error)}
          <div class="actions">
            {#if review.owned}
              <button class="button" aria-label="Release claim (u)" aria-keyshortcuts="u" disabled={review.busy !== null} onclick={() => void review.release()}>Release claim</button>
            {/if}
            <span class="said">
              {#if review.busy}
                <span class="busy" role="status"><span class="running"><Icon name="running" label={review.busy.label} /></span><span class="seconds">{Math.floor((h.now - review.busy.since) / 1000)}s</span><button class="button small" aria-label="Cancel (Esc)" aria-keyshortcuts="Escape" onclick={() => review.cancel()}>Cancel</button></span>
              {:else if review.notice?.error}
                <span class="error" role="status">{review.notice.text}</span>
              {/if}
            </span>
            {#if review.form}
              <button class="button primary" aria-label="Submit {review.form.verdict} ({label('Ctrl+S')})" aria-keyshortcuts="Control+S" disabled={!review.submittable} onclick={() => void submit()}>Submit {review.form.verdict}</button>
            {/if}
          </div>
        {/if}
      </div>
    </Section>
    <Section name="Debug">
      <dl>
        <dt>State</dt>
        <dd>{view.meta}</dd>
        {#each view.technical as field, at (at)}
          <dt>{field.key}</dt>
          <dd>{field.value}</dd>
        {/each}
      </dl>
    </Section>
  {/if}
{/snippet}

<!-- The review takes focus so its letter keys work before any control is chosen; keys from
     its controls and fields bubble up here. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<section class="human" class:split tabindex="0" aria-label="Human review" bind:this={h.root} onkeydown={keydown} {@attach measure}>
  {#if !view}
    <p class="loading">{review.notice?.text ?? "Loading…"}</p>
  {:else if split}
    <div class="viewer">
      <div class="head"><span class="name">Tool log</span></div>
      <ToolLog entries={review.log} view />
    </div>
    <div class="side">{@render column()}</div>
  {:else}
    <div class="one">{@render column()}</div>
  {/if}
</section>

<style>
  /* Sizes flow from the parent down: every column is at most the width it is given, and long
   * text wraps inside it. */
  .human {
    height: 100%;
    min-height: 0;
    min-width: 0;
    outline: none;
  }
  .human:focus-visible,
  .instruction:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* One column, left-aligned and no wider than the measure, scrolling as a whole. */
  .one {
    height: 100%;
    max-width: var(--measure);
    overflow: auto;
  }
  /* Two panels under the header row's panel line: the Tool log's view at the left and the column
     at the right, of the Detail's width, meeting at a panel line. */
  .human.split {
    display: grid;
    grid-template-columns: minmax(0, 1fr) var(--dock-width);
    border-top: 1px solid var(--line-panel);
  }
  .viewer {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-height: 0;
    min-width: 0;
    background: var(--surface-input);
  }
  .side {
    min-height: 0;
    min-width: 0;
    overflow: auto;
    border-left: 1px solid var(--line-panel);
    background: var(--surface-panel);
  }
  /* A panel's head row: the view's name, or the column's stage pill; the two in one band. */
  .head {
    display: flex;
    align-items: center;
    gap: var(--gap);
    height: var(--row-head);
    padding: 0 var(--pad);
    border-bottom: 1px solid var(--line-group);
    background: var(--surface-panel);
  }
  .name {
    font-size: var(--text-small);
    font-weight: 600;
  }
  .stage {
    font-size: var(--text-label);
    font-weight: 600;
    padding: 1px 8px;
    border-radius: var(--radius-chip);
    background: var(--surface-group);
    color: var(--ink-2);
  }
  .stage[data-stage="review"] {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .claimant,
  .muted,
  .loading {
    color: var(--ink-2);
  }
  .loading {
    margin: 0;
    padding: 6px var(--pad);
  }
  .instruction {
    padding: 10px var(--pad);
    outline: none;
  }
  .text {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .shared {
    margin: 6px 0 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }
  .result {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  .result p {
    margin: 0;
  }
  .verdicts {
    display: flex;
    gap: 6px;
  }
  /* The last row of the Result: Release claim at the left, Submit at the column's right edge. */
  /* In a narrow column the row wraps rather than letting its groups run into each other. */
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px var(--gap);
  }
  /* What the work says keeps its own width: never narrower than its content. */
  .said {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
  }
  .actions > .primary {
    margin-left: auto;
  }
  .busy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .running {
    display: inline-flex;
    color: var(--running);
  }
  .seconds {
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .error {
    color: var(--red);
    overflow-wrap: anywhere;
  }
  dl {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 16px;
    margin: 0;
  }
  dt {
    color: var(--ink-2);
    text-align: right;
    overflow-wrap: anywhere;
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }
  /* The completed result: one step below the panel, outlined by the faintest line. */
  pre {
    margin: 0;
    font: var(--text-small) / 1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-input);
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    padding: 6px 8px;
  }
</style>
