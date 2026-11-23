<script lang="ts">
  // Review and Detail share the instruction, Tools, Tool log, Points, Result and Debug.
  // With room for two columns the Result moves to the dock at the right.
  import { HumanDetailState, setHumanDetailState } from "./human-detail-state.svelte";
  import { onMount, tick } from "svelte";
  import { onStateChanged, type Settled } from "../ipc/client";
  import { Coalesced } from "../ipc/coalesced";
  import type { Review } from "./review.svelte";
  import Tools from "./Tools.svelte";
  import ToolLog from "./ToolLog.svelte";
  import ReviewPoints from "./ReviewPoints.svelte";
  import { FormCaret } from "./form-caret";
  import VerdictChoices from "./VerdictChoices.svelte";
  import VerdictForm from "./VerdictForm.svelte";
  import ResultEntries from "../monitor/ResultEntries.svelte";
  import { getAppState } from "../shell/app.svelte";
  import ReviewProgress from "./ReviewProgress.svelte";
  import Reference from "../shell/Reference.svelte";
  import Section from "../shell/Section.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, composing, editable, label, plain } from "../shell/keys";
  import Icon from "../shell/Icon.svelte";

  let {
    review,
    active = true,
    wide = false,
    title = null,
    onback,
    onsubmitted,
    onopenreview,
  }: {
    review: Review;
    /** Its screen is shown: only then it offers palette commands. */
    active?: boolean;
    /** It fills Review's Main area: its column is centred at the reading measure. */
    wide?: boolean;
    /** The eval's title, said at the column's head (in the Detail, its title says it). */
    title?: string | null;
    onback: () => void;
    /** A submission settled the request, published or saved locally only. */
    onsubmitted?: (settled: Settled) => void;
    /** Shown outside Review (a Human eval's tab in the Detail): open it there (D-INS-12). */
    onopenreview?: () => void;
  } = $props();
  const app = getAppState();
  const { sections } = app;
  const h = new HumanDetailState();
  setHumanDetailState(h);
  const caret = new FormCaret();

  const view = $derived(review.view);
  /** Where the claim stands, quietly at the column's head; a submitted review says nothing. */
  const state = $derived(
    view?.stage === "completed" ? null
      : view?.stage === "review" || review.owned ? "Yours"
        : view?.claim ? `Taken · ${view.claim.reviewer}` : "Waiting",
  );
  /** The Tool log's last run, as its folded line says it. */
  const last = $derived(review.log.at(-1));
  const lastError = $derived(last?.lines.findLast((line) => line.error)?.text ?? null);
  const lastEnd = $derived.by(() => {
    if (!last) return "";
    if (!last.end) return last.lines.at(-1)?.text ?? "running";
    const count = last.lines.length;
    return `${last.end.text} · ${count} ${count === 1 ? "line" : "lines"}`;
  });
  const clock = (at: number | null) => (at === null ? "--:--:--" : new Date(at).toTimeString().slice(0, 8));

  onMount(() => {
    void review.load();
    const clock = setInterval(() => {
      if (!review.busy && active) void review.load(false);
    }, 1000);
    // A settlement or claim elsewhere shows on the next read, one read at a time and only while
    // the review is on screen; an open form is set aside.
    const reading = new Coalesced(() => review.load(), () => active);
    const stop = onStateChanged(() => reading.request());
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
      { id: "human.to-green", group: "Human review", label: "Choose GREEN", keys: "g", enabled: () => ready() && review.editing, run: () => void review.choose("GREEN") },
      { id: "human.to-red", group: "Human review", label: "Choose RED", keys: "r", enabled: () => ready() && review.editing, run: () => void review.choose("RED") },
      { id: "human.submit", group: "Human review", label: "Submit", keys: "Ctrl+S", enabled: () => review.submittable, run: () => void submit() },
      { id: "human.release", group: "Human review", label: "Release the claim", keys: "u", enabled: () => ready() && review.owned, run: () => void review.release() },
      { id: "human.cancel", group: "Human review", label: "Cancel the running work", keys: "Esc", enabled: () => review.busy !== null, run: () => review.cancel() },
      { id: "human.debug", group: "Human review", label: "Show or hide Debug", keys: "t", run: () => sections.toggle("Debug") },
    ];
  });
</script>

{#snippet head()}
  <div class="rhead">
    {#if title}<span class="rtitle">{title}</span>{:else}<span class="spacer"></span>{/if}
    {#if state}<span class="rstate">{state}</span>{/if}
    {#if onopenreview}
      <Reference onclick={onopenreview}>Open in Review</Reference>
    {/if}
  </div>
{/snippet}

<!-- The instruction: what the reviewer reads first. -->
{#snippet reading()}
  {#if view}
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
  {/if}
{/snippet}

<!-- Evidence: the tools, then the Tool log once a tool has run, folded to its last run on one
     line, then the Points once there is one. -->
{#snippet evidence()}
  {@const logged = review.busy?.tool || review.log.length > 0}
  {@const pointed = review.owned && (review.draft?.points.length ?? 0) > 0}
  {#if view && (view.tools.length > 0 || logged || pointed)}
    <Section name="Evidence" folds={false}>
      {#if view.tools.length > 0}<Tools {review} />{/if}
      {#if logged}
        <button class="logline" aria-expanded={h.logOpen} aria-label="Tool log" onclick={() => (h.logOpen = !h.logOpen)}>
          <Icon name={h.logOpen ? "unfold" : "fold"} />
          {#if last}
            <span class="start">▶ {clock(last.at)} · {last.operation}</span>
            {#if lastError}<span class="stderr">{lastError}</span>{/if}
            <span class="spacer"></span>
            <span class="end" class:failed={last.end?.error}>{lastEnd}</span>
          {/if}
        </button>
        {#if h.logOpen}<ToolLog entries={review.log} />{/if}
      {/if}
      {#if pointed}
        <ReviewPoints {review} limited insert={(text) => caret.insert(text)} />
      {/if}
    </Section>
  {/if}
{/snippet}

<!-- The Verdict: before a claim Claim to review; once claimed the verdict and only its fields;
     settled, the result. -->
{#snippet result()}
  {#if view}
    <div class="result" role="group" onfocusin={() => (review.area = "fields")}>
      {#if review.settled}
        <!-- The completed result by field, read-only; a request settled without one, its error. -->
        <div class="completed">
          {#if view.completed}<ResultEntries entries={view.completed} />{:else}<pre>{view.result}</pre>{/if}
        </div>
        <!-- Settled here but not published: what failed, so it is pushed again, not resubmitted. -->
        {#if review.notice?.error}<p class="error" role="status">{review.notice.text}</p>{/if}
      {:else if review.owned}
        {#if review.form}
          {#key review.form}<VerdictForm {review} form={review.form} {caret} />{/key}
        {:else}
          <VerdictChoices {review} shortcuts />
        {/if}
      {:else if view.claim}
        <p class="muted">Taken by {view.claim.reviewer}</p>
      {:else}
        <div class="claiming">
          <button class="button primary" aria-label="Claim to review (c)" aria-keyshortcuts="c" disabled={!review.claimable || review.busy !== null} onclick={() => void review.claim()}>Claim to review</button>
          <ReviewProgress {review} />
        </div>
      {/if}
    </div>
  {/if}
{/snippet}

<!-- One row of actions, kept in view at the column's bottom: Release claim at the left, Submit
     at the right, and between them what running work or a failure has to say. -->
{#snippet actions()}
  {#if view && !review.settled && review.owned}
    <div class="actions">
      <button class="button quiet" aria-label="Release claim (u)" aria-keyshortcuts="u" disabled={review.busy !== null} onclick={() => void review.release()}>Release claim</button>
      <span class="said">
        <ReviewProgress {review} />
      </span>
      {#if review.form}
        <button class="button primary" aria-label="Submit {review.form.verdict} ({label('Ctrl+S')})" aria-keyshortcuts="Control+S" disabled={!review.submittable} onclick={() => void submit()}>Submit {review.form.verdict}</button>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet debug()}
  {#if view}
    <div class="debug">
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
    </div>
  {/if}
{/snippet}

<!-- The review takes focus so its letter keys work before any control is chosen; keys from
     its controls and fields bubble up here. One column, as an editor's Inspector (D-REV-1). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<section class="human" class:wide tabindex="0" aria-label="Human review" bind:this={h.root} onkeydown={keydown}>
  {#if !view}
    <p class="loading">{review.notice?.text ?? "Loading…"}</p>
  {:else}
    <div class="column">
      {@render head()}
      {@render reading()}
      {@render evidence()}
      <Section name="Verdict" folds={false}>
        {@render result()}
        {@render actions()}
      </Section>
      {@render debug()}
    </div>
  {/if}
</section>

<style>
  /* Sizes flow from the parent down: the column is at most the width it is given, and long
   * text wraps inside it. */
  .human {
    height: 100%;
    min-height: 0;
    min-width: 0;
    outline: none;
    overflow: auto;
  }
  .human:focus-visible,
  .instruction:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  /* One column; in Review's Main area the reading measure, centred. */
  .column {
    display: flex;
    flex-direction: column;
    min-height: 100%;
    max-width: var(--measure);
  }
  .wide .column {
    margin: 0 auto;
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-height: var(--row-head);
    padding: 12px var(--pad) 0;
  }
  .rtitle {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-bright);
    font-size: var(--text-lead);
  }
  .spacer {
    flex: 1;
    min-width: 0;
  }
  .rstate {
    flex: none;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  /* Debug, folded last, is quiet: its row in --ink-2 at the text's weight (D-REV-1). */
  .debug :global(.section > .head) {
    color: var(--ink-2);
    font-weight: 400;
  }
  .muted,
  .loading {
    color: var(--ink-2);
  }
  .loading {
    margin: 0;
    padding: 6px var(--pad);
  }
  .instruction {
    padding: 6px var(--pad) 18px;
    outline: none;
  }
  .text {
    margin: 0;
    font-size: var(--text-lead);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .shared {
    margin: 6px 0 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }
  /* The Tool log folded to its last run: one row in mono, the fold icon first. */
  .logline {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--row);
    margin: 4px 0;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    font: var(--text-small) var(--mono);
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    cursor: pointer;
  }
  .logline:hover {
    color: var(--ink);
  }
  .logline:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .logline .start,
  .logline .end {
    flex: none;
  }
  .logline .stderr {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-left: 6px;
    box-shadow: inset 2px 0 0 var(--red);
    color: var(--ink);
  }
  .logline .end.failed {
    color: var(--red);
  }
  .result {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  .result p {
    margin: 0;
  }
  .claiming {
    display: flex;
    align-items: center;
    gap: var(--gap);
  }
  /* The Verdict's last row, kept in view at the column's bottom however long the form; one row
     also in the narrowest (320px) Detail. */
  .actions {
    position: sticky;
    bottom: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 3px;
    margin-top: 12px;
    padding: 8px 0;
    background: var(--surface-panel);
  }
  .actions > :global(.button) {
    padding-inline: 5px;
    flex: none;
  }
  .said {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .actions > .primary {
    margin-left: auto;
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
