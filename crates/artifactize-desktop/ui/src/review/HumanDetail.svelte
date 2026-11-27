<script lang="ts">
  // A Human review as a conversation (D-REV-1, J-HUMAN-LAYOUT), the same in Review's main area
  // and on a Human eval's tab in the Detail: the head line; the request, unboxed as Chat's agent
  // text is (its instruction, its tools and Debug folded); each tool's run and each point, newest last; the reviewer's issues as
  // bubbles at the bottom, or the submitted result as the last bubble; then, where Chat's composer
  // stands, the issues box and under it the decision bar.
  import { HumanDetailState, setHumanDetailState } from "./human-detail-state.svelte";
  import { onMount, tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { onStateChanged, type Settled } from "../ipc/client";
  import { Coalesced } from "../ipc/coalesced";
  import type { Review } from "./review.svelte";
  import Tools from "./Tools.svelte";
  import ToolRuns from "./ToolRuns.svelte";
  import ReviewPoints from "./ReviewPoints.svelte";
  import Issues from "./Issues.svelte";
  import IssuesBox from "./IssuesBox.svelte";
  import DecisionBar from "./DecisionBar.svelte";
  import { IssuesCaret } from "./issues-caret.svelte";
  import ResultEntries from "../monitor/ResultEntries.svelte";
  import { getAppState } from "../shell/app.svelte";
  import Reference from "../shell/Reference.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, composing, editable, plain } from "../shell/keys";
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
    /** The eval's title, said by the head line (in the Detail, its title says it). */
    title?: string | null;
    onback: () => void;
    /** A submission settled the request, published or saved locally only. */
    onsubmitted?: (settled: Settled) => void;
    /** Shown outside Review (a Human eval's tab in the Detail): open it there (D-REV-11). */
    onopenreview?: () => void;
  } = $props();
  const app = getAppState();
  const { sections } = app;
  const h = new HumanDetailState();
  setHumanDetailState(h);
  const caret = new IssuesCaret();

  const view = $derived(review.view);
  const debugOpen = $derived(sections.open("Debug"));
  const pointed = $derived(review.owned && (review.draft?.points.length ?? 0) > 0);

  onMount(() => {
    void review.load();
    const clock = setInterval(() => {
      if (!review.busy && active) void review.load(false);
    }, 1000);
    // A settlement or claim elsewhere shows on the next read, one read at a time and only while
    // the review is on screen.
    const reading = new Coalesced(() => review.load(), () => active);
    const stop = onStateChanged(() => reading.request());
    // Tools that already took focus (a reopened request with Tools focused) keep it.
    if (h.root && !h.root.contains(document.activeElement)) h.root.focus();
    return () => {
      clearInterval(clock);
      stop();
    };
  });

  /** The conversation follows its newest line while it is read at its end (D-CHAT-2). */
  const following: Attachment<HTMLElement> = (log) => {
    let atEnd = true;
    const scrolled = () => {
      atEnd = log.scrollHeight - log.scrollTop - log.clientHeight < 24;
    };
    const grown = new MutationObserver(() => {
      if (atEnd) log.scrollTop = log.scrollHeight;
    });
    grown.observe(log, { childList: true, subtree: true, characterData: true });
    log.addEventListener("scroll", scrolled);
    return () => {
      grown.disconnect();
      log.removeEventListener("scroll", scrolled);
    };
  };

  async function submit() {
    const settled = await review.submit(h.typed);
    if (!settled) return;
    onsubmitted?.(settled);
    await tick();
    // Submission removes the issues box and the bar. Keep Esc reachable, without taking focus
    // back if the person moved to another control or screen while it ran.
    if (active && h.root?.isConnected && document.activeElement === document.body) h.root.focus();
  }

  /** The keyboard back on the review, after Esc left a field or an edit. */
  function refocus() {
    h.root?.focus({ preventScroll: true });
  }

  /** Esc: cancel the running work, then leave. */
  function back() {
    if (review.escape()) onback();
  }

  function keydown(event: KeyboardEvent) {
    // Keys of an input method's composition, Escape included, belong to the composition.
    if (event.defaultPrevented || composing(event)) return;
    const handled = () => {
      event.preventDefault();
      event.stopPropagation();
    };
    if ((event.ctrlKey || event.metaKey) && (event.key === "PageUp" || event.key === "PageDown")) {
      h.convo?.scrollBy({ top: (event.key === "PageDown" ? 10 : -10) * 20 });
      return handled();
    }
    if (event.key === "Escape") {
      back();
      return handled();
    }
    // `q` quits only outside a field or work, as in the terminal.
    if (review.busy && plain(event, "q")) return handled();
    // Submit is open while a tool runs: it cancels the tool first (Review.submit).
    if (chord(event, "s")) void submit();
    else if (editable(event.target)) return;
    else if (plain(event, "g")) void review.choose("GREEN");
    else if (plain(event, "r")) void review.choose("RED");
    else if (review.busy) return;
    else if (plain(event, "c")) void review.claim();
    else if (plain(event, "u")) void review.release();
    else if (plain(event, "t")) sections.toggle("Debug");
    else if (plain(event, "ArrowLeft")) onback();
    else return;
    handled();
  }

  useCommands(() => {
    if (!active) return [];
    const ready = () => !review.busy && !review.settled;
    const choosable = () => review.owned && (review.busy === null || review.busy.tool);
    return [
      { id: "human.claim", group: "Human review", label: "Claim to review", keys: "c", enabled: () => ready() && review.claimable, run: () => void review.claim() },
      { id: "human.green", group: "Human review", label: "Choose GREEN", keys: "g", enabled: choosable, run: () => void review.choose("GREEN") },
      { id: "human.red", group: "Human review", label: "Choose RED", keys: "r", enabled: choosable, run: () => void review.choose("RED") },
      { id: "human.submit", group: "Human review", label: "Submit", keys: "Ctrl+S", enabled: () => review.submittable(h.typed), run: () => void submit() },
      { id: "human.release", group: "Human review", label: "Release the claim", keys: "u", enabled: () => ready() && review.owned, run: () => void review.release() },
      { id: "human.cancel", group: "Human review", label: "Cancel the running work", keys: "Esc", enabled: () => review.busy !== null, run: () => review.cancel() },
      { id: "human.debug", group: "Human review", label: "Show or hide Debug", keys: "t", run: () => sections.toggle("Debug") },
    ];
  });
</script>

<!-- The review takes focus so its letter keys work before any control is chosen; keys from
     its controls and fields bubble up here. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<section class="human" class:wide tabindex="0" aria-label="Human review" bind:this={h.root} onkeydown={keydown}>
  {#if !view}
    <p class="loading">{review.notice?.text ?? "Loading…"}</p>
  {:else}
    <div class="convo" bind:this={h.convo} {@attach following}>
      <div class="column">
        <!-- The head line: the eval's title; in the Detail only Open in Review (D-REV-11). -->
        {#if title || onopenreview}
          <div class="rhead">
            {#if title}<span class="rtitle">{title}</span>{:else}<span class="spacer"></span>{/if}
            {#if onopenreview}<Reference onclick={onopenreview}>Open in Review</Reference>{/if}
          </div>
        {/if}
        <!-- The request asks, unboxed: the instruction, the tools, Debug. -->
        <div class="request" role="region" aria-label="Instruction" onfocusin={() => (review.area = "tools")}>
          <p class="text">{view.instruction}</p>
          {#if view.shared}<p class="shared">{view.shared}</p>{/if}
          {#if view.tools.length > 0}<Tools {review} />{/if}
          <div class="debug">
            <button class="fold" aria-expanded={debugOpen} aria-label="Debug (t)" aria-keyshortcuts="t" onclick={() => sections.toggle("Debug")}>
              <span class="icon"><Icon name={debugOpen ? "unfold" : "fold"} /></span>Debug
            </button>
            {#if debugOpen}
              <dl>
                <dt>State</dt>
                <dd>{view.meta}</dd>
                {#each view.technical as field, at (at)}
                  <dt>{field.key}</dt>
                  <dd>{field.value}</dd>
                {/each}
              </dl>
            {/if}
          </div>
        </div>
        <!-- Newest last as they happen: each tool's run, then each point. -->
        <ToolRuns {review} />
        {#if pointed}
          <ReviewPoints {review} ready={caret.ready} insert={(text) => caret.insert(text)} />
        {/if}
        <!-- The issues, pushed to the conversation's bottom; once submitted, the result as the
             reviewer's last bubble. -->
        <div class="answers">
          {#if review.settled}
            <div class="message result" role="group" aria-label="Result">
              {#if view.completed}<ResultEntries entries={view.completed} />{:else}<pre>{view.result}</pre>{/if}
            </div>
            <!-- Settled here but not published: what failed, so it is pushed again, not resubmitted. -->
            {#if review.notice?.error}<p class="error" role="status">{review.notice.text}</p>{/if}
          {:else}
            <Issues {review} onleave={refocus} />
          {/if}
        </div>
      </div>
    </div>
    {#if !review.settled}
      <div class="bottom">
        {#if review.owned}
          <IssuesBox {review} {caret} bind:typed={h.typed} onleave={refocus} />
        {:else if review.claimable}
          <p class="quiet">Claim it to write issues and a verdict</p>
        {/if}
        <DecisionBar {review} typed={h.typed} onsubmit={() => void submit()} />
      </div>
    {/if}
  {/if}
</section>

<style>
  /* The conversation scrolls on its own; the issues box and the bar stand under it. */
  .human {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
    outline: none;
  }
  .human:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .convo {
    flex: 1;
    min-height: 0;
    overflow: auto;
    container-type: size;
  }
  .column,
  .bottom {
    box-sizing: border-box;
    width: 100%;
    max-width: var(--measure);
    padding: 0 var(--pad);
  }
  .column {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 100%;
    padding-top: 8px;
    padding-bottom: 8px;
  }
  .wide .column,
  .wide .bottom {
    margin: 0 auto;
  }
  /* In the Detail: the Detail's width. */
  .human:not(.wide) .column,
  .human:not(.wide) .bottom {
    max-width: none;
  }
  .bottom {
    flex: none;
    padding-bottom: 12px;
  }
  .rhead {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-height: var(--row);
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
  }
  .request {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .text {
    margin: 0;
    color: var(--ink);
    font-size: var(--text-body);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .shared {
    margin: 0;
    color: var(--ink-2);
    overflow-wrap: anywhere;
  }
  /* Debug: one folded line of Chat's tool-line anatomy (D-CHAT-4). */
  .fold {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: var(--row-dense);
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .fold:hover {
    background: var(--surface-group);
  }
  .fold .icon {
    display: inline-flex;
    color: var(--ink-3);
  }
  dl {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 4px 16px;
    margin: 2px 0 4px 22px;
    font-size: var(--text-small);
  }
  dt {
    color: var(--ink-2);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .answers {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: auto;
  }
  /* The submitted result as the reviewer's last bubble, each field by name, read-only. */
  .message {
    align-self: flex-end;
    max-width: 80%;
    min-width: 0;
    padding: 6px 10px;
    border-radius: var(--radius-chip);
    background: var(--surface-group);
    overflow-wrap: anywhere;
  }
  .message pre {
    margin: 0;
    white-space: pre-wrap;
    font: inherit;
  }
  .quiet {
    margin: 0;
    padding: 8px 0 4px;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .loading {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
  .error {
    margin: 0;
    color: var(--red);
    overflow-wrap: anywhere;
  }
</style>
