<script lang="ts">
  // An Agent request's live transcript. The backend's session reader lays it out in the
  // columns and rows this monospace pane fits; scrolling asks for another window.
  import { TranscriptState, setTranscriptState } from "./transcript-state.svelte";
  import { onDestroy, onMount } from "svelte";
  import {
    call,
    useIpc,
    message,
    onSessionChanged,
    onStateChanged,
    type Movement,
    
  } from "../ipc/client";
  import { plain } from "../shell/keys";

  let { id }: { id: string } = $props();
  const commands = useIpc();
  const t = new TranscriptState();
  setTranscriptState(t);

  /** How often a missed append or removal is checked, as in the terminal monitor. */
  const PROBE_MS = 5000;
  /** A wheel step moves three rows, like the terminal's. */
  const WHEEL_ROWS = 3;

  function measure(): boolean {
    if (!t.pane || !t.probe) return false;
    const cell = t.probe.getBoundingClientRect();
    // Before layout (or without one) a cell is assumed 7 × 18 pixels.
    const width = cell.width / 10 || 7;
    const height = cell.height || 18;
    // The rows fill the content box: the pane's width without its padding, never its border
    // box, so no laid-out row is wider than the room it is drawn in.
    const box = t.pane.getBoundingClientRect();
    const style = getComputedStyle(t.pane);
    const room = box.width - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0);
    const columns = Math.max(Math.floor(room / width) - 1, 10) || 10;
    const rows = Math.max(Math.floor(box.height / height), 3);
    const changed = columns !== t.size.columns || rows !== t.size.rows;
    t.size = { columns, rows };
    return changed;
  }

  /** Ask for the window; requests made meanwhile merge into one follow-up. */
  async function ask(movement: Movement | null = null, toggle: number | null = null, changed = false) {
    if (t.handle === null || t.closed) return;
    if (t.asking) {
      t.again = {
        movement: movement ?? t.again?.movement ?? null,
        toggle: toggle ?? t.again?.toggle ?? null,
        changed: changed || (t.again?.changed ?? false),
      };
      return;
    }
    t.asking = true;
    // A reader replaced while this request was pending makes its answer, rows or failure,
    // obsolete; only the current reader's answers are shown.
    const reader = t.handle;
    let loading = false;
    try {
      const next = await call(
        commands.transcriptWindow(reader, t.size.columns, t.size.rows, movement, toggle, changed),
      );
      if (reader === t.handle) {
        t.view = next;
        t.unavailable = null;
        loading = next.loading;
      }
    } catch (error) {
      if (reader === t.handle) t.unavailable = message(error);
    } finally {
      t.asking = false;
    }
    const next = t.again;
    t.again = null;
    if (next) void ask(next.movement, next.toggle, next.changed);
    // Bounded indexing continues in the next window.
    else if (loading) setTimeout(() => void ask(), 30);
  }

  /**
   * Resolve the request's session, again whenever its state may have changed: a queued request
   * gains a session once it runs, and the backend keeps an unchanged session's reader.
   */
  async function resolve() {
    if (t.closed || t.resolving) return;
    t.resolving = true;
    try {
      const opened = await call(commands.transcriptOpen(id, t.handle));
      if (t.closed) {
        if (opened.kind === "local") void commands.transcriptClose(opened.handle);
        return;
      }
      const next = opened.kind === "local" ? opened.handle : null;
      const replaced = next !== t.handle;
      t.handle = next;
      t.session = opened.kind === "local" ? opened.session : null;
      t.unavailable = opened.kind === "unavailable" ? opened.text : null;
      if (replaced) t.view = null;
      if (t.handle !== null) void ask(null, null, true);
    } catch (error) {
      t.unavailable = message(error);
    } finally {
      t.resolving = false;
    }
  }

  onMount(() => {
    measure();
    void resolve();
    const observer = new ResizeObserver(() => {
      if (measure()) void ask();
    });
    if (t.pane) observer.observe(t.pane);
    // Missed appends and removals show within the probe; without a session yet, resolve again.
    const timer = setInterval(() => (t.handle === null ? void resolve() : void ask(null, null, true)), PROBE_MS);
    const stopSession = onSessionChanged((changed) => {
      if (changed === t.session) void ask(null, null, true);
    });
    const stopState = onStateChanged(() => void resolve());
    return () => {
      observer.disconnect();
      clearInterval(timer);
      stopSession();
      stopState();
    };
  });

  onDestroy(() => {
    t.closed = true;
    if (t.handle !== null) void commands.transcriptClose(t.handle);
  });

  function keydown(event: KeyboardEvent) {
    const page = Math.max(t.size.rows, 1);
    const moves: Record<string, Movement> = {
      ArrowUp: { kind: "up", rows: 1 },
      k: { kind: "up", rows: 1 },
      ArrowDown: { kind: "down", rows: 1 },
      j: { kind: "down", rows: 1 },
      PageUp: { kind: "up", rows: page },
      PageDown: { kind: "down", rows: page },
      Home: { kind: "top" },
      End: { kind: "bottom" },
    };
    const movement = Object.entries(moves).find(([key]) => plain(event, key))?.[1];
    if (movement) {
      void ask(movement);
    } else if (plain(event, "Enter") || plain(event, " ")) {
      const first = t.view?.rows.findIndex((row) => row.group) ?? -1;
      if (first >= 0) void ask(null, first);
    } else return;
    event.preventDefault();
    event.stopPropagation();
  }

  /** The scrollbar: the shown rows' share of every laid-out row, and where they start. It is
   * drawn here because the pane holds only the visible rows. */
  const bar = $derived.by(() => {
    const view = t.view;
    const shown = t.size.rows;
    if (!view || view.total <= shown || shown === 0) return null;
    const size = Math.max(shown / view.total, 0.05);
    const start = (view.top / (view.total - shown)) * (1 - size);
    return { size, start: Math.min(Math.max(start, 0), 1 - size) };
  });

  /** Move the shown rows so they start at `row`. */
  function scrollTo(row: number) {
    const view = t.view;
    if (!view) return;
    const target = Math.round(Math.min(Math.max(row, 0), Math.max(view.total - t.size.rows, 0)));
    const delta = target - view.top;
    if (delta > 0) void ask({ kind: "down", rows: delta });
    else if (delta < 0) void ask({ kind: "up", rows: -delta });
  }

  /** Drag the thumb, or press the track to move there: the row under the pointer's share. */
  function drag(event: PointerEvent) {
    const track = event.currentTarget as HTMLElement;
    const view = t.view;
    if (!view || !bar) return;
    event.preventDefault();
    track.setPointerCapture(event.pointerId);
    const box = track.getBoundingClientRect();
    const thumb = bar.size * box.height;
    const onThumb = (event.target as HTMLElement).classList.contains("thumb");
    // A press on the thumb keeps its grip; one on the track centres the thumb there.
    const grip = onThumb ? event.clientY - box.top - bar.start * box.height : thumb / 2;
    const follow = (move: PointerEvent) => {
      const share = (move.clientY - box.top - grip) / Math.max(box.height - thumb, 1);
      scrollTo(share * (view.total - t.size.rows));
    };
    follow(event);
    const stop = () => {
      track.removeEventListener("pointermove", follow);
      track.removeEventListener("pointerup", stop);
      track.removeEventListener("pointercancel", stop);
    };
    track.addEventListener("pointermove", follow);
    track.addEventListener("pointerup", stop);
    track.addEventListener("pointercancel", stop);
  }

  function wheel(event: WheelEvent) {
    if (event.deltaY === 0) return;
    event.preventDefault();
    void ask({ kind: event.deltaY > 0 ? "down" : "up", rows: WHEEL_ROWS });
  }
</script>

<div class="transcript">
  <!-- A live log that scrolls by keys and wheel, since it holds only the visible rows. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="window"
    tabindex="0"
    role="log"
    aria-label="Agent transcript"
    bind:this={t.pane}
    onkeydown={keydown}
    onwheel={wheel}
  >
    <span class="probe" bind:this={t.probe} aria-hidden="true">MMMMMMMMMM</span>
    {#if t.unavailable}
      <p class="status">{t.unavailable}</p>
    {:else if t.view?.status && t.view.rows.length === 0}
      <p class="status">{t.view.status}</p>
    {:else if t.view}
      {#each t.view.rows as row, at (at)}
        {#if row.group}
          <button class="row group" onclick={() => void ask(null, at)}>{row.text || " "}</button>
        {:else}
          <div class="row" class:thinking={row.thinking}>{row.text || " "}</div>
        {/if}
      {/each}
    {:else}
      <p class="status">Loading session…</p>
    {/if}
    {#if bar}
      <div class="track" aria-hidden="true" onpointerdown={drag}>
        <div class="thumb" style:top="{bar.start * 100}%" style:height="{bar.size * 100}%"></div>
      </div>
    {/if}
  </div>
  <footer>
    <button class="button small" onclick={() => void ask({ kind: "top" })}>Top <kbd>Home</kbd></button>
    <button class="button small" onclick={() => void ask({ kind: "bottom" })}>Bottom <kbd>End</kbd></button>
    <span class="indicator">{t.view?.indicator ?? ""}</span>
  </footer>
</div>

<style>
  .transcript {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }
  .window {
    position: relative;
    overflow: hidden;
    padding: 0 var(--pad);
    font: 12px/18px var(--mono);
    outline: none;
    min-height: 0;
  }
  .window:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  .probe {
    position: absolute;
    visibility: hidden;
    font: 12px/18px var(--mono);
    white-space: pre;
  }
  /* Rows come laid out to the pane's columns; a row that still does not fit wraps inside the
   * pane rather than running past its edge. */
  .row {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    min-height: 18px;
  }
  .group {
    display: block;
    width: 100%;
    text-align: left;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    padding: 0;
    cursor: pointer;
  }
  .group:hover {
    color: var(--text);
  }
  .thinking {
    color: var(--muted);
    font-style: italic;
  }
  .status {
    color: var(--muted);
  }
  footer {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 6px var(--pad);
    border-top: 1px solid var(--border);
  }
  /* Thin like the other panes' scrollbars; the thumb darkens under the pointer. */
  .track {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 10px;
    cursor: default;
  }
  .thumb {
    position: absolute;
    left: 2px;
    right: 2px;
    border-radius: 4px;
    background: var(--border);
  }
  .track:hover .thumb {
    background: var(--muted);
  }
  .indicator {
    margin-left: auto;
    color: var(--muted);
    font: 11px var(--mono);
  }
</style>
