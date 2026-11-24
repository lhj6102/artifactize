<script lang="ts">
  // The pet overlay (J-PET, D-PET-1): a small frameless window above every window while this
  // reviewer works away from the active review's evaluation space. It holds the review's eval and
  // Run (a link to the review in Review) and the same library draft, split as Review's is,
  // compactly: its Points with Insert, its reasons with Remove, the reasons field with Add and
  // Point (pointing in an open viewer of the review's Artifact), then apart the GREEN and RED
  // choice and Submit. The main window shows and hides it; it follows the active review.
  import { emit, listen } from "@tauri-apps/api/event";
  import { createContext, onMount, untrack } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { commands, onActiveReviewChanged, onHumanDraftChanged, onStateChanged, subscribe } from "../ipc/client";
  import { Jobs } from "../ipc/jobs.svelte";
  import { Review } from "../review/review.svelte";
  import { ReasonsCaret } from "../review/reasons-caret.svelte";
  import ReviewPoints from "../review/ReviewPoints.svelte";
  import Reasons from "../review/Reasons.svelte";
  import ReasonsBox from "../review/ReasonsBox.svelte";
  import DecisionBar from "../review/DecisionBar.svelte";
  import Icon from "../shell/Icon.svelte";
  import { ARM, ASK, PRESENCE, type Presence } from "../viewers/viewer-state.svelte";
  import { PET_OPEN } from "./pet";
  import petImage from "../shell/pet.svg";

  let { request }: { request: string } = $props();
  class PetState {
    readonly jobs = new Jobs();
    readonly viewers = new SvelteMap<number, Presence>();
    review: Review;
    constructor(id: string) { this.review = $state(new Review(id, this.jobs)); }
  }
  const [getPetState, setPetState] = createContext<PetState>();
  setPetState(new PetState(untrack(() => request)));
  const pet = getPetState();
  const caret = new ReasonsCaret();
  /** What the overlay's reasons field holds and has not added: its own until Add. */
  let typed = $state("");
  const review = $derived(pet.review);
  const view = $derived(review.view);
  const { viewers } = pet;
  /** The open viewer Point arms: the one the library matches to this review's original request. */
  const pointer = $derived(view === null ? null : ([...viewers.values()].find((each) => each.open && each.pointable && each.petMatch && each.review === review.id) ?? null));
  const run = $derived(view?.target.run ?? "");
  const evalId = $derived(view?.target.node.replace(/^e:/, "") ?? "");

  /** Read review `id`, its draft with it. */
  async function show(id: string) {
    if (pet.review.id !== id) {
      pet.review = new Review(id, pet.jobs);
      typed = "";
    }
    await pet.review.load();
  }

  async function submit() {
    await review.submit(typed);
  }

  function open() {
    void emit(PET_OPEN, review.id);
    void commands.mainFocus();
  }

  onMount(() => {
    void show(request);
    const undrafted = onHumanDraftChanged((id) => {
      if (id === review.view?.owner) void review.readDraft();
    });
    const unstate = onStateChanged((resync) => {
      if (resync && review.owned) void review.readDraft();
    });
    const unactive = onActiveReviewChanged((next) => {
      if (next) void show(next);
    });
    const unpresent = subscribe<Presence>((handler) => listen(PRESENCE, handler), (presence) => {
      if (presence.open) viewers.set(presence.session, presence);
      else viewers.delete(presence.session);
    });
    void emit(ASK);
    return () => {
      unstate();
      undrafted();
      unactive();
      unpresent();
    };
  });
</script>

<div class="pet-window">
  <div class="card">
    <button class="head" onclick={open} aria-label="Show {evalId} in Review">
      <img class="pet" src={petImage} alt="" width="40" height="40" />
      <span class="tone-human"><Icon name="human" /></span><span class="eval">{evalId}</span><span class="run">· {run}</span>
    </button>
    {#if view && review.owned}
      {#if review.draft?.points.length}
        <ReviewPoints {review} compact ready={caret.ready} insert={(text) => caret.insert(text)} />
      {/if}
      <Reasons {review} compact />
      <ReasonsBox {review} {caret} compact bind:typed>
        {#snippet after()}
          <button class="button small point" class:off={pointer === null} aria-pressed={pointer?.pointing ?? false} disabled={pointer === null} onclick={() => pointer && void emit(ARM, { session: pointer.session })}><Icon name="point" />Point</button>
        {/snippet}
      </ReasonsBox>
      {#if pointer === null}<p class="why">Open {view.artifact} in a viewer to point.</p>{/if}
      {#if review.notice?.error}<p class="error" role="status">{review.notice.text}</p>{/if}
      <div class="decision">
        <DecisionBar {review} {typed} compact onsubmit={() => void submit()} />
      </div>
    {:else if view}
      <p class="why">{review.settled ? "Submitted." : "This review is no longer claimed here."}</p>
    {/if}
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    background: transparent;
  }
  .pet-window {
    position: relative;
    height: 100%;
    padding-top: 20px;
    box-sizing: border-box;
  }
  .pet {
    position: absolute;
    top: 0;
    left: 12px;
    z-index: 1;
    filter: saturate(60%);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: calc(100% - 4px);
    padding: 12px;

    box-sizing: border-box;
    background: var(--surface-raised);
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-chip);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin: 0 0 0 46px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink);
    font: var(--text-small) var(--mono);
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    background: var(--surface-group);
  }
  .tone-human {
    display: inline-flex;
    color: var(--human);
  }
  .eval,
  .run {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .eval { min-width: 0; }
  .tone-human, .run { flex: none; }
  .run {
    color: var(--ink-2);
  }
  /* The decision row stands apart from the reasons row (D-PET-1). */
  .decision {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 4px;
  }
  .point { color: var(--ink-2); }
  .point[aria-pressed="true"] { color: var(--ink-bright); }
  .point:disabled {
    background: none;
    border-color: var(--line-section);
    color: var(--ink-3);
  }
  .why, .error {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .why {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .error {
    margin: 0;
    color: var(--red);
    font-size: var(--text-small);
  }
</style>
