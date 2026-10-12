<script lang="ts">
  // An Agent request's live transcript. The backend's session reader lays it out in the
  // columns and rows this monospace pane fits; scrolling asks for another window.
  import { TranscriptState, setTranscriptState } from "./transcript.svelte";
  import { onDestroy, onMount } from "svelte";
  import {
    call,
    commands,
    message,
    onSessionChanged,
    onStateChanged,
    type Movement,
    
  } from "../ipc/client";
  import { plain } from "../shell/keys";

  let { id, peek = false }: { id: string; peek?: boolean } = $props();
  const t = new TranscriptState();
  setTranscriptState(t);

  /** Rows of a peek: enough to find the last line of a running Agent. */
  const PEEK_ROWS = 6;
  /** How often a missed append or removal is checked, as in the terminal monitor. */
  const PROBE_MS = 5000;
  /** A wheel step moves three rows, like the terminal's. */
  const WHEEL_ROWS = 3;

  
  
  
  
  let asking = false;
  let again: { movement: Movement | null; toggle: number | null; changed: boolean } | null = null;
  let closed = false;

  function measure(): boolean {
    if (!t.pane || !t.probe) return false;
    const cell = t.probe.getBoundingClientRect();
    // Before layout (or without one) a cell is assumed 7 × 18 pixels.
    const width = cell.width / 10 || 7;
    const height = cell.height || 18;
    const box = t.pane.getBoundingClientRect();
    const columns = Math.max(Math.floor(box.width / width) - 1, 10) || 10;
    const rows = peek ? PEEK_ROWS : Math.max(Math.floor(box.height / height), 3);
    const changed = columns !== t.size.columns || rows !== t.size.rows;
    t.size = { columns, rows };
    return changed;
  }

  /** Ask for the window; requests made meanwhile merge into one follow-up. */
  async function ask(movement: Movement | null = null, toggle: number | null = null, changed = false) {
    if (t.handle === null || closed) return;
    if (asking) {
      again = {
        movement: movement ?? again?.movement ?? null,
        toggle: toggle ?? again?.toggle ?? null,
        changed: changed || (again?.changed ?? false),
      };
      return;
    }
    asking = true;
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
      asking = false;
    }
    const next = again;
    again = null;
    if (next) void ask(next.movement, next.toggle, next.changed);
    // Bounded indexing continues in the next window.
    else if (loading) setTimeout(() => void ask(), 30);
  }

  let resolving = false;

  /**
   * Resolve the request's session, again whenever its state may have changed: a queued request
   * gains a session once it runs, and the backend keeps an unchanged session's reader.
   */
  async function resolve() {
    if (closed || resolving) return;
    resolving = true;
    try {
      const opened = await call(commands.transcriptOpen(id, t.handle));
      if (closed) {
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
      resolving = false;
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
    closed = true;
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

  function wheel(event: WheelEvent) {
    if (peek || event.deltaY === 0) return;
    event.preventDefault();
    void ask({ kind: event.deltaY > 0 ? "down" : "up", rows: WHEEL_ROWS });
  }

  const last = $derived(
    t.view?.status ?? t.view?.rows.map((row) => row.text.trim()).findLast((text) => text) ?? "reading the session…",
  );
</script>

{#if peek}
  <p class="last" bind:this={t.pane}>
    <span class="probe" bind:this={t.probe} aria-hidden="true">MMMMMMMMMM</span>
    <strong>Last:</strong> {t.unavailable ?? last}
  </p>
{:else}
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
    </div>
    <footer>
      <button class="button small" onclick={() => void ask({ kind: "top" })}>Top <kbd>Home</kbd></button>
      <button class="button small" onclick={() => void ask({ kind: "bottom" })}>Bottom <kbd>End</kbd></button>
      <span class="indicator">{t.view?.indicator ?? ""}</span>
    </footer>
  </div>
{/if}

<style>
  .transcript {
    display: grid;
    grid-template-rows: 1fr auto;
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
  .row {
    white-space: pre;
    height: 18px;
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
  .indicator {
    margin-left: auto;
    color: var(--muted);
    font: 11px var(--mono);
  }
  .last {
    position: relative;
    margin: 0;
    padding: 0 var(--pad) 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
