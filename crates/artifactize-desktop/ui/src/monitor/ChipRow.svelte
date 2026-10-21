<script lang="ts">
  // A branch's chip row: one chip per Run, oldest to newest left to right, ending with the Run
  // in progress or the Current chip. It has no scrollbar: the mouse held on it drags it
  // sideways, and chips fade out at whichever end has more. Reaching its oldest end asks for
  // older Runs, which arrive without moving what is shown. A drag chooses nothing.
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { BranchView, Chip } from "../ipc/client";
  import { tone } from "../shell/tone";

  let {
    branch,
    filled,
    onchoose,
    onolder,
  }: {
    branch: BranchView;
    /** The chip shown in the Main area, by its index in the row. */
    filled: number | null;
    onchoose: (chip: Chip) => void;
    /** The oldest end came into view and older Runs exist. */
    onolder: () => void;
  } = $props();

  /** Pixels the pointer moves before a press becomes a drag. */
  const DRAG = 4;
  /** Pixels from the oldest end within which older Runs are asked for. */
  const NEAR = 24;

  const label = (chip: Chip) => (chip.kind === "run" ? chip.text : "Current");

  /** The fades follow the scroll position; older Runs are asked for at the oldest end. Only
   * the DOM is touched: the row's own scroll offset and two data attributes. */
  const scroll: Attachment<HTMLElement> = (row) => {
    let width = row.scrollWidth;
    let anchored = false;
    const fade = () => {
      const right = row.scrollWidth - row.clientWidth - row.scrollLeft;
      row.dataset.fadeLeft = String(row.scrollLeft > 1);
      row.dataset.fadeRight = String(right > 1);
      if (row.scrollLeft < NEAR && branch.older && !anchored) {
        anchored = true;
        onolder();
      }
    };
    // Older chips arrive at the left: keep what is shown where it was.
    const grown = new MutationObserver(() => {
      const added = row.scrollWidth - width;
      width = row.scrollWidth;
      if (anchored && added > 0) row.scrollLeft += added;
      anchored = false;
      fade();
    });
    grown.observe(row, { childList: true, subtree: true, characterData: true });
    // A row starts at its newest end; later snapshots never move it.
    row.scrollLeft = row.scrollWidth;
    untrack(fade);
    row.addEventListener("scroll", fade);
    const resized = new ResizeObserver(fade);
    resized.observe(row);
    return () => {
      grown.disconnect();
      resized.disconnect();
      row.removeEventListener("scroll", fade);
    };
  };

  /** The filled chip scrolls into view when it changes. */
  const reveal: Attachment<HTMLElement> = (row) => {
    if (filled === null) return;
    row.querySelector<HTMLElement>(`[data-index="${filled}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  };

  /** Holding the mouse on the row drags it with the pointer until the button is released. */
  function press(event: MouseEvent) {
    if (event.button !== 0) return;
    const row = event.currentTarget as HTMLElement;
    const [start, offset] = [event.clientX, row.scrollLeft];
    let dragged = false;
    const drag = (moved: MouseEvent) => {
      const by = moved.clientX - start;
      if (!dragged && Math.abs(by) < DRAG) return;
      dragged = true;
      row.scrollLeft = offset - by;
    };
    const drop = () => {
      window.removeEventListener("mousemove", drag);
      window.removeEventListener("mouseup", drop);
      // The click that ends a drag chooses no chip.
      if (dragged) row.addEventListener("click", (click) => click.stopPropagation(), { capture: true, once: true });
    };
    window.addEventListener("mousemove", drag);
    window.addEventListener("mouseup", drop);
  }
</script>

<!-- The row takes the mouse to drag it; its chips are the buttons. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="chips"
  role="group"
  aria-label="Runs of {branch.project} {branch.name}"
  onmousedown={press}
  {@attach scroll}
  {@attach reveal}
>
  {#each branch.chips as chip, at (chip.kind === "run" ? chip.id : "current")}
    <button
      class="chip"
      class:filled={at === filled}
      class:running={chip.kind === "run" && chip.running}
      tabindex="-1"
      data-index={at}
      data-chip={chip.kind === "run" ? chip.id : "current"}
      aria-pressed={at === filled}
      onclick={(event) => {
        event.stopPropagation();
        onchoose(chip);
      }}
    >
      {#if chip.kind === "run"}<span class="glyph {tone(chip.mark.status)}">{chip.mark.glyph}</span>{/if}{label(chip)}
    </button>
  {/each}
</div>

<style>
  .chips {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    scrollbar-width: none;
    padding: 2px 0 4px;
    cursor: grab;
    user-select: none;
  }
  .chips::-webkit-scrollbar {
    display: none;
  }
  .chips:global([data-fade-left="true"]) {
    mask-image: linear-gradient(to right, transparent, black 32px);
  }
  .chips:global([data-fade-right="true"]) {
    mask-image: linear-gradient(to left, transparent, black 32px);
  }
  .chips:global([data-fade-left="true"][data-fade-right="true"]) {
    mask-image: linear-gradient(to right, transparent, black 32px, black calc(100% - 32px), transparent);
  }
  .chip {
    flex: none;
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: inherit;
    font: 11px var(--font);
    white-space: nowrap;
    cursor: pointer;
  }
  .chip:hover {
    background: var(--surface-2);
  }
  .chip.filled {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--bg);
  }
  .chip.filled .glyph {
    color: inherit;
  }
  .chip.running {
    font-weight: 600;
  }
</style>
