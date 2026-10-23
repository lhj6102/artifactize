<script lang="ts">
  // The one Human review component, shared by the monitor's Detail and the Review screen: its
  // status and instruction, then the tools to use, their Tool log, and the result to write.
  import { HumanDetailState, setHumanDetailState } from "./human-detail-state.svelte";
  import { onMount, tick } from "svelte";
  import { onStateChanged, type Settled } from "../ipc/client";
  import type { Review } from "./review.svelte";
  import Tools from "./Tools.svelte";
  import VerdictForm from "./VerdictForm.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, composing, editable, plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let {
    review,
    active = true,
    onback,
    onsubmitted,
  }: {
    review: Review;
    /** Its screen is shown: only then it offers palette commands. */
    active?: boolean;
    onback: () => void;
    /** A submission settled the request, published or saved locally only. */
    onsubmitted?: (settled: Settled) => void;
  } = $props();
  const h = new HumanDetailState();
  setHumanDetailState(h);

  /** Rows the instruction keeps once tools and fields take the body. */
  const SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
  
  
  

  const view = $derived(review.view);
  const stage = $derived(
    view?.stage === "completed" ? "completed" : view?.stage === "review" ? "REVIEW (yours)" : "CLAIM",
  );

  onMount(() => {
    void review.load();
    // Waiting time and the busy spinner redraw without reading the state.
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
   * focused field, so focus returns to the Detail and the next Esc still reaches it. */
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
    if (review.busy) return;
    if (chord(event, "g")) void review.choose("GREEN");
    else if (chord(event, "r")) void review.choose("RED");
    else if (chord(event, "u")) void review.release();
    else if (chord(event, "s")) void submit();
    else if (editable(event.target) && review.editing) return;
    else if (plain(event, "c")) void review.claim();
    else if (plain(event, "g")) void review.choose("GREEN");
    else if (plain(event, "r")) void review.choose("RED");
    else if (plain(event, "u")) void review.release();
    else if (plain(event, "t")) review.technical = !review.technical;
    else if (plain(event, "ArrowLeft") && !review.editing) onback();
    else return;
    handled();
  }

  useCommands(() => {
    if (!active) return [];
    const ready = () => !review.busy && !review.settled;
    return [
      { id: "human.claim", group: "Human review", label: "Claim", keys: "c", enabled: () => ready() && review.claimable, run: () => void review.claim() },
      { id: "human.green", group: "Human review", label: "GREEN form", keys: "g", enabled: () => ready() && review.owned, run: () => void review.choose("GREEN") },
      { id: "human.red", group: "Human review", label: "RED form", keys: "r", enabled: () => ready() && review.owned, run: () => void review.choose("RED") },
      { id: "human.submit", group: "Human review", label: "Submit", keys: "Ctrl+S", enabled: () => ready() && review.editing, run: () => void submit() },
      { id: "human.release", group: "Human review", label: "Release the claim", keys: "u", enabled: () => ready() && review.owned, run: () => void review.release() },
      { id: "human.run", group: "Human review", label: "Run the selected tool", keys: "Enter", enabled: () => ready() && review.owned && (view?.tools.length ?? 0) > 0, run: () => void review.run() },
      { id: "human.cancel", group: "Human review", label: "Cancel the running job", keys: "Esc", enabled: () => review.busy !== null, run: () => review.cancel() },
      { id: "human.technical", group: "Human review", label: "Toggle Technical", keys: "t", run: () => (review.technical = !review.technical) },
    ];
  });
</script>

<!-- The review takes focus so its letter keys work before any control is chosen; keys from
     its controls and fields bubble up here. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<section class="human" tabindex="0" aria-label="Human review" bind:this={h.root} onkeydown={keydown}>
  {#if !view}
    <p class="muted">{review.notice?.text ?? "Loading…"}</p>
  {:else}
    <header>
      <div class="title">
        <h3>{view.title}</h3>
        <span class="stage" data-stage={view.stage}>{stage}</span>
        <button class="link" aria-expanded={review.technical} onclick={() => (review.technical = !review.technical)}>
          Technical {review.technical ? "▾" : "▸"} <kbd>t</kbd>
        </button>
      </div>
      <p class="meta {tone(view.mark.status)}">{view.meta}</p>
      {#if view.shared}<p class="shared">{view.shared}</p>{/if}
    </header>

    <div class="body">
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
        {#if review.technical}
          <h4>Technical</h4>
          <dl>
            {#each view.technical as field, at (at)}
              <dt>{field.key}</dt>
              <dd>{field.value}</dd>
            {/each}
          </dl>
        {/if}
      </div>

      <Tools {review} />

      <section class="result" aria-label="Result" onfocusin={() => (review.area = "fields")}>
        <h4>{review.settled ? "Completed result" : "Result"}</h4>
        {#if review.settled}
          <pre>{view.result}</pre>
        {:else if review.owned && review.form}
          <VerdictForm {review} form={review.form} onsubmit={() => void submit()} />
        {:else if review.owned}
          <div class="verdicts">
            <button class="button" onclick={() => void review.choose("GREEN")}>GREEN</button>
            <button class="button" onclick={() => void review.choose("RED")}>RED</button>
          </div>
        {:else if view.claim}
          <p class="muted">Claimed by {view.claim.reviewer}. Read-only until released.</p>
        {:else}
          <button class="button primary" disabled={!review.claimable || review.busy !== null} onclick={() => void review.claim()}>Claim to review</button>
        {/if}
      </section>
    </div>

    <footer>
      {#if review.busy}
        <p class="busy" role="status">
          {SPINNER[Math.floor((h.now - review.busy.since) / 100) % SPINNER.length]}
          {review.busy.label}… {Math.floor((h.now - review.busy.since) / 1000)}s
          <button class="button small" onclick={() => review.cancel()}>Cancel <kbd>Esc</kbd></button>
        </p>
      {:else if review.notice}
        <p class="notice" class:error={review.notice.error} role="status">{review.notice.text}</p>
      {/if}
      <!-- Close belongs to the pane that shows the review, in its header. -->
      {#if !review.busy && !review.settled && review.owned}
        <div class="buttons">
          <button class="button" onclick={() => void review.release()}>Release</button>
        </div>
      {/if}
    </footer>
  {/if}
</section>

<style>
  /* Sizes flow from the parent down: every column is at most the width it is given, and long
   * text wraps inside it. */
  .human {
    display: grid;
    grid-template-rows: auto 1fr auto;
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
    min-height: 0;
    outline: none;
    padding: 0 var(--pad) 8px;
    gap: 10px;
  }
  header h3 {
    margin: 0;
    font-size: 14px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .title {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    align-items: center;
  }
  .stage {
    font-size: 11px;
    font-weight: 600;
    padding: 1px 8px;
    border-radius: 99px;
    background: var(--surface-2);
    color: var(--muted);
  }
  .stage[data-stage="review"] {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .meta {
    margin: 4px 0 0;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .shared {
    margin: 2px 0 0;
    color: var(--human);
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--muted);
    cursor: pointer;
    font: inherit;
    margin-left: auto;
  }
  /* One column that scrolls as a whole: instruction, tools, their log, the result. */
  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-content: start;
    gap: 16px;
    min-height: 0;
    overflow: auto;
  }
  .instruction {
    outline: none;
  }
  .result h4,
  .result :global(h4) {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .verdicts {
    display: flex;
    gap: 6px;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    margin: 0;
  }
  h4 {
    margin: 12px 0 4px;
  }
  dl {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 2px 12px;
    margin: 0;
  }
  dt {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  pre {
    margin: 0;
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  footer {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 6px;
  }
  footer p {
    margin: 0;
  }
  .busy {
    color: var(--human);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .notice {
    color: var(--green);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .notice.error {
    color: var(--red);
  }
  .buttons {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
</style>
