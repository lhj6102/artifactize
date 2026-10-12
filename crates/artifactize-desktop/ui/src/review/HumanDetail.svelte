<script lang="ts">
  // The one Human review component, shared by the monitor's Detail and the Review screen:
  // instruction first, then CLAIM → REVIEW with tools, output and the GREEN/RED fields.
  import { HumanDetailState, setHumanDetailState } from "./humanDetail.svelte";
  import { onMount, tick } from "svelte";
  import { onStateChanged, type Settled } from "../ipc/client";
  import type { Review } from "./review.svelte";
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import Tools from "./Tools.svelte";
  import VerdictForm from "./VerdictForm.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, composing, editable, label, plain } from "../shell/keys";
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
  const expanded = $derived(review.expanded || review.technical);
  /** Before a claim the instruction is the body, with the tools in one line below it. */
  const summary = $derived(view?.stage === "claim" && !expanded && review.area !== "tools");
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
    if (settled) onsubmitted?.(settled);
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
    else if (chord(event, "e")) void review.editJson();
    else if (editable(event.target) && review.editing) return;
    else if (plain(event, "c")) void review.claim();
    else if (plain(event, "g")) void review.choose("GREEN");
    else if (plain(event, "r")) void review.choose("RED");
    else if (plain(event, "u")) void review.release();
    else if (plain(event, "i")) review.expanded = !review.expanded;
    else if (plain(event, "t")) review.technical = !review.technical;
    else if (plain(event, "ArrowLeft") && !review.editing) onback();
    else return;
    handled();
  }

  const hints = $derived.by(() => {
    if (review.busy) return ["Esc cancel"];
    if (review.settled) return ["i instruction", "t technical", "Esc back"];
    const tools = (view?.tools.length ?? 0) > 0;
    const list: string[] = [];
    if (review.area === "tools") {
      if (tools) list.push("↑↓ tool", review.owned ? "Enter run" : review.claimable ? "c claim, then Enter run" : "");
      else if (review.claimable) list.push("c claim");
    } else if (review.editing) {
      list.push("Ctrl+S submit", "Ctrl+G/R verdict", "Ctrl+E JSON", "Esc stop editing");
    } else if (review.owned) {
      list.push("g GREEN", "r RED", "u release");
    } else if (review.claimable) {
      list.push("c claim");
    }
    if (!review.editing) list.push("i instruction", "t technical", "Esc back");
    return list.filter(Boolean).map(label);
  });

  useCommands(() => {
    if (!active) return [];
    const ready = () => !review.busy && !review.settled;
    return [
      { id: "human.claim", group: "Human review", label: "Claim", keys: "c", enabled: () => ready() && review.claimable, run: () => void review.claim() },
      { id: "human.green", group: "Human review", label: "GREEN form", keys: "g", enabled: () => ready() && review.owned, run: () => void review.choose("GREEN") },
      { id: "human.red", group: "Human review", label: "RED form", keys: "r", enabled: () => ready() && review.owned, run: () => void review.choose("RED") },
      { id: "human.submit", group: "Human review", label: "Submit", keys: "Ctrl+S", enabled: () => ready() && review.editing, run: () => void submit() },
      { id: "human.json", group: "Human review", label: "Edit the form as JSON", keys: "Ctrl+E", enabled: () => ready() && review.editing && review.form?.json === null, run: () => void review.editJson() },
      { id: "human.release", group: "Human review", label: "Release the claim", keys: "u", enabled: () => ready() && review.owned, run: () => void review.release() },
      { id: "human.run", group: "Human review", label: "Run the selected tool", keys: "Enter", enabled: () => ready() && review.owned && (view?.tools.length ?? 0) > 0, run: () => void review.run() },
      { id: "human.cancel", group: "Human review", label: "Cancel the running job", keys: "Esc", enabled: () => review.busy !== null, run: () => review.cancel() },
      { id: "human.technical", group: "Human review", label: "Toggle Technical", keys: "t", run: () => (review.technical = !review.technical) },
      { id: "human.instruction", group: "Human review", label: "Expand or fold the instruction", keys: "i", run: () => (review.expanded = !review.expanded) },
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

    <div class="body" class:summary class:expanded>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="instruction"
        class:focused={review.area === "instruction"}
        tabindex="0"
        role="region"
        aria-label="Instruction"
        bind:this={h.instruction}
        onfocusin={() => {
          if (!review.editing) review.area = "instruction";
        }}
      >
        <div class="box-title">
          Instruction
          <button class="link" onclick={() => (review.expanded = !review.expanded)}>
            {expanded ? "fold" : "expand"} <kbd>i</kbd>
          </button>
        </div>
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

      {#if summary}
        <div class="oneline">
          <button class="link tools-line" onclick={() => (review.area = "tools")}>
            <strong>Tools</strong>
            {#if view.tools.length === 0}
              none declared in this eval's scope
            {:else}
              {view.tools.map((tool) => `${tool.name} (${tool.kind})`).join(" · ")} · Tab or click: tools
            {/if}
          </button>
          <p class="muted">
            {view.claim
              ? `Claimed by ${view.claim.reviewer}. Read-only until released.`
              : `Unclaimed. c claims it for ${view.reviewer} and starts REVIEW.`}
          </p>
        </div>
      {:else}
        <PaneGroup direction="horizontal" autoSaveId="human-split" class="split">
          <Pane defaultSize={45} minSize={25}><Tools {review} /></Pane>
          <PaneResizer class="resizer gap" />
          <Pane defaultSize={55} minSize={25}>
          <div
            class="fields"
            class:focused={review.area === "fields"}
            onfocusin={() => (review.area = "fields")}
          >
            {#if review.settled}
              <div class="box-title">Completed result</div>
              <pre>{view.result}</pre>
            {:else if review.owned && review.form}
              <VerdictForm {review} form={review.form} />
            {:else if review.owned}
              <div class="box-title">Result fields</div>
              <p>REVIEW — press <kbd>g</kbd> for GREEN or <kbd>r</kbd> for RED, fill its fields, then <kbd>{label("Ctrl+S")}</kbd> submits.</p>
            {:else}
              <div class="box-title">Human sign-off</div>
              <p>
                CLAIM —
                {view.claim
                  ? `Claimed by ${view.claim.reviewer}. Read-only until released.`
                  : `Unclaimed. c claims it for ${view.reviewer} and starts REVIEW.`}
              </p>
            {/if}
          </div>
          </Pane>
        </PaneGroup>
      {/if}
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
      <div class="buttons">
        {#if !review.busy && !review.settled}
          {#if review.owned}
            <button class="button" onclick={() => void review.release()}>Release <kbd>{review.editing ? label("Ctrl+U") : "u"}</kbd></button>
            <button class="button" aria-pressed={review.form?.verdict === "GREEN"} onclick={() => void review.choose("GREEN")}>
              GREEN <kbd>{review.editing ? label("Ctrl+G") : "g"}</kbd>
            </button>
            <button class="button" aria-pressed={review.form?.verdict === "RED"} onclick={() => void review.choose("RED")}>
              RED <kbd>{review.editing ? label("Ctrl+R") : "r"}</kbd>
            </button>
            <button class="button primary" disabled={!review.form} onclick={() => void submit()}>Submit <kbd>{label("Ctrl+S")}</kbd></button>
            <button class="button" disabled={view.tools.length === 0} onclick={() => void review.run()}>Run tool <kbd>Enter</kbd></button>
          {:else}
            <button class="button primary" disabled={!review.claimable} onclick={() => void review.claim()}>Claim <kbd>c</kbd></button>
          {/if}
        {/if}
        <span class="spacer"></span>
        <button class="button" onclick={() => void back()}>Close <kbd>Esc</kbd></button>
      </div>
      <p class="hints">{hints.join(" · ")}</p>
    </footer>
  {/if}
</section>

<style>
  .human {
    display: grid;
    grid-template-rows: auto 1fr auto;
    height: 100%;
    min-height: 0;
    outline: none;
    padding: 0 var(--pad) 8px;
    gap: 10px;
  }
  header h3 {
    margin: 0;
    font-size: 14px;
  }
  .title {
    display: flex;
    gap: 10px;
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
  }
  .shared {
    margin: 2px 0 0;
    color: var(--human);
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
  .body {
    display: grid;
    grid-template-rows: minmax(0, 72px) minmax(0, 1fr);
    gap: 10px;
    min-height: 0;
  }
  .body.summary {
    grid-template-rows: minmax(0, 1fr) auto;
  }
  .body.expanded {
    grid-template-rows: minmax(0, 7fr) minmax(0, 3fr);
  }
  .instruction,
  .fields {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 12px;
    overflow: auto;
    min-height: 0;
    outline: none;
  }
  .focused {
    border-color: var(--accent);
  }
  .box-title {
    display: flex;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    margin-bottom: 4px;
  }
  .text {
    white-space: pre-wrap;
    margin: 0;
  }
  h4 {
    margin: 12px 0 4px;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 12px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .oneline .tools-line {
    margin: 0;
    color: var(--text);
    text-align: left;
  }
  .oneline p {
    margin: 4px 0 0;
  }
  :global(.split) {
    min-height: 0;
  }
  :global(.resizer.gap) {
    margin: 0 5px;
    background: transparent;
  }
  .fields {
    height: 100%;
  }
  pre {
    margin: 0;
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
  }
  footer {
    display: grid;
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
  }
  .notice.error {
    color: var(--red);
  }
  .buttons {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .spacer {
    flex: 1;
  }
  .hints {
    color: var(--muted);
    font-size: 12px;
  }
  .button[aria-pressed="true"] {
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
