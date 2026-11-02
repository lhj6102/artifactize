<script lang="ts">
  // The shown worktree's terminal, floating over the graph's left side as the Detail floats over
  // its right; the graph never shrinks under it. Collapsed it is a tab at the left edge, and only
  // detaches: the terminal and its processes keep running, and expanding attaches again. Dragging
  // its right edge sets its width, kept like the Detail's. Collapse and expand leave the keyboard
  // where it was, unless it was on the button pressed: collapse hands it to the tab, expand into
  // the terminal.
  //
  // Above the screen it says what the person should know: that the terminal ends with the app,
  // that its agent moved to another worktree, that its process ended, or that it was gone and a
  // fresh shell runs, with what it can restore. Nothing of a restore runs until the person
  // presses its Run.
  import { tick } from "svelte";
  import { getAppState } from "../shell/app.svelte";
  import { HARNESS, commandLine, type RestoreChoice, type WorktreeKey } from "./backend";
  import { screen } from "./screen";

  let {
    worktree,
    name,
    width,
    onmoved,
    ongrab,
  }: {
    /** The worktree whose terminal shows. */
    worktree: WorktreeKey;
    /** The worktree as the Pane names it. */
    name: string;
    /** Its fitted width in pixels, or `null` before the stage is measured. */
    width: number | null;
    /** Open the worktree the agent moved to. */
    onmoved: (worktree: WorktreeKey) => void;
    /** A press on the right edge: the drag that sets the width. */
    ongrab: (event: MouseEvent) => void;
  } = $props();
  const app = getAppState();
  const { monitor: m, terminals } = app;

  const session = $derived(terminals.of(worktree));
  const offer = $derived(session.offer);
  /** The last directory, as the offer says it. */
  const where = $derived(offer?.cwd ? `in ${offer.cwd}` : "in the worktree (the last directory is not known)");
  /** The resume suggestion's words: a session known for certain, or only the latest one. */
  const resuming = $derived.by(() => {
    if (!offer?.resume) return null;
    const harness = offer.harness ? HARNESS[offer.harness] : offer.resume.program;
    return offer.latestSession || offer.sessionId === null ? `Resume the latest ${harness} session` : `Resume the ${harness} session`;
  });

  function restore(choice: RestoreChoice) {
    void terminals.restore(worktree, choice);
  }

  /** The collapsed tab, while it shows. */
  let tab = $state<HTMLButtonElement>();
  /** A press of the mouse on collapse or expand leaves the keyboard where it is. */
  const keep = (event: MouseEvent) => event.preventDefault();

  /** Collapse or expand. The keyboard stays with what had it; only when it was on the button
   * itself does it move on: from collapse to the tab, from the tab into the terminal. */
  async function fold(open: boolean, event: MouseEvent) {
    const had = document.activeElement;
    const pressed = had === event.currentTarget;
    m.terminal = open;
    await tick();
    if (pressed && open) m.panes.terminal?.querySelector<HTMLElement>("textarea")?.focus();
    else if (pressed) tab?.focus();
    else if (had instanceof HTMLElement && had.isConnected && document.activeElement !== had) had.focus();
  }

  /** Open the worktree the agent moved to; its terminal stays as it is. */
  function follow(to: WorktreeKey) {
    terminals.settled(worktree);
    onmoved(to);
  }
</script>

{#if m.terminal}
  <aside
    class="terminal"
    class:focused={m.focus === "terminal"}
    aria-label="Terminal"
    style:--terminal-width={width === null ? null : `${width}px`}
    bind:this={m.panes.terminal}
    onfocusin={() => (m.focus = "terminal")}
  >
    <header>
      <h2 title={worktree}>Terminal · {name}</h2>
      <button
        class="button small"
        aria-label="Collapse the terminal"
        title="Collapse; the terminal keeps running"
        onmousedown={keep}
        onclick={(event) => void fold(false, event)}>‹</button
      >
    </header>
    {#if session.fragile}
      <p class="state" role="status"><strong>Not persistent.</strong> This terminal ends when Artifactize closes.</p>
    {/if}
    {#if session.moved !== null}
      {@const moved = session.moved}
      <div class="offer" role="status">
        <p>The agent moved to another worktree: <code>{moved}</code>. This graph stays on its own worktree.</p>
        <div class="actions">
          <button class="button small primary" onclick={() => follow(moved)}>Open detected worktree</button>
          <button class="button small" onclick={() => terminals.settled(worktree)}>Dismiss</button>
        </div>
      </div>
    {/if}
    {#if offer}
      <section class="offer" aria-label="Restore">
        <p>This terminal was gone; a fresh shell runs in the worktree. Nothing runs until you choose.</p>
        <ul>
          {#if offer.command}
            <li>
              <span>Rerun the last command {where}: <code>{commandLine(offer.command)}</code></span>
              <button class="button small" aria-label="Run the last command again" onclick={() => restore("rerun")}>Run</button>
            </li>
          {/if}
          {#if offer.resume && resuming}
            <li>
              <span>{resuming}: <code>{commandLine(offer.resume)}</code></span>
              <button class="button small" aria-label={resuming} onclick={() => restore("resume")}>Run</button>
            </li>
          {/if}
        </ul>
        <div class="actions">
          <button class="button small" onclick={() => restore("freshShell")}>Keep the fresh shell</button>
        </div>
      </section>
    {/if}
    {#if session.ended}
      <div class="offer" role="status">
        <p>The terminal ended{session.ended.code === null ? "." : ` with exit status ${session.ended.code}.`}</p>
        <div class="actions"><button class="button small" onclick={() => terminals.fresh(worktree)}>Start a fresh shell</button></div>
      </div>
    {/if}
    {#if session.error}<p class="failure" role="status">{session.error}</p>{/if}
    {#key `${worktree}\n${session.attempt}`}
      <div class="screen" {@attach screen(worktree, terminals, app.workbench)}></div>
    {/key}
    <!-- The right edge takes a mouse drag, as the Detail's left edge does; hovering it shows
         only the resize cursor. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="edge" role="separator" aria-orientation="vertical" aria-label="Terminal width" onmousedown={ongrab}></div>
  </aside>
{:else}
  <button
    class="tab"
    aria-label="Expand the terminal"
    title="Terminal · {name}"
    bind:this={tab}
    onmousedown={keep}
    onclick={(event) => void fold(true, event)}>Terminal</button
  >
{/if}

<style>
  /* Docked toward the graph's left edge with a gap, mirroring the Detail on the right. */
  .terminal {
    position: absolute;
    z-index: 20;
    top: 12px;
    left: 12px;
    bottom: 12px;
    width: var(--terminal-width, min(40%, 560px));
    min-width: min(320px, calc(100% - 24px));
    display: grid;
    grid-template-rows: auto auto auto auto auto auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    contain: inline-size;
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 32px rgb(0 0 0 / 22%);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 6px 12px;
    border-top: 2px solid transparent;
  }
  .focused header {
    border-top-color: var(--accent);
  }
  h2 {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .focused h2 {
    color: var(--text);
  }
  .small {
    padding: 1px 8px;
    font-size: 12px;
  }
  .state,
  .offer,
  .failure {
    margin: 0 10px 6px;
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .state {
    background: var(--surface-2);
    color: var(--yellow);
  }
  .offer {
    background: var(--accent-soft);
  }
  .offer p {
    margin: 0 0 6px;
  }
  .offer ul {
    list-style: none;
    margin: 0 0 6px;
    padding: 0;
    display: grid;
    gap: 4px;
  }
  .offer li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
  }
  .offer code {
    font: 12px var(--mono);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .failure {
    color: var(--red);
    background: var(--surface-2);
  }
  .screen {
    min-height: 0;
    margin: 0 4px 4px 10px;
    overflow: hidden;
  }
  .screen :global(.xterm) {
    height: 100%;
  }
  .edge {
    position: absolute;
    z-index: 1;
    top: 0;
    bottom: 0;
    right: 0;
    width: 6px;
    cursor: col-resize;
    touch-action: none;
  }
  /* Collapsed: a tab on the graph's left edge, out of the legend's and the controls' way. */
  .tab {
    position: absolute;
    z-index: 20;
    left: 12px;
    top: 50%;
    transform: translateY(-50%);
    width: 28px;
    padding: 12px 0;
    writing-mode: vertical-rl;
    font: inherit;
    font-size: 12px;
    color: var(--muted);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 4px 12px rgb(0 0 0 / 15%);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
  }
</style>
