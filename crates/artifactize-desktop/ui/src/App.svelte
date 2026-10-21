<script lang="ts">
  // The window: its one screen over one state, the command palette, help, and what happens on
  // quit.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import brandMark from "../../../../assets/brand/artifactize-icon-mark.svg";
  import { call, message, onHumanToolLog, onStateChanged } from "./ipc/client";
  import Monitor from "./monitor/Monitor.svelte";
  import LeaveDialog from "./review/LeaveDialog.svelte";
  import Palette from "./shell/Palette.svelte";
  import Help from "./shell/Help.svelte";
  import { useCommands } from "./shell/commands.svelte";
  import { chord, composing, editable, plain } from "./shell/keys";
  import { HELP } from "./shell/shortcuts";
  import { AppState, setAppState } from "./shell/app.svelte";

  import { exposeActivity, refreshed, setActivity } from "./ipc/activity.svelte";

  const app = new AppState();
  setActivity(app.activity);
  const commands = app.ipc;
  setAppState(app);
  const { browse, current, leaving, monitor: m } = app;

  onMount(() => {
    const hideActivity = exposeActivity(app.activity);
    call(commands.launch()).then(
      (view) => {
        app.start(view);
      },
      (error) => (app.failure = message(error)),
    );
    void browse.navigate({ full: true });
    // Ages and running times redraw from the snapshot; nothing is read.
    const clock = setInterval(() => void browse.redraw(), 1000);
    const unchanged = onStateChanged((resync) => browse.refresh(resync));
    // What Human tools print, for whichever review or Artifact shows it.
    const unlogged = onHumanToolLog((logs) => app.logged(logs));
    // Closing the window asks about this session's review claims first, like `q`.
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested((event) => leaving.closeRequested(event))
      .then((stop) => (unlisten = stop), () => {});
    return () => {
      hideActivity();
      clearInterval(clock);
      unchanged();
      unlogged();
      unlisten?.();
    };
  });

  async function quit() {
    if (await leaving.allowed()) await getCurrentWindow().destroy();
  }

  /** Read the state afresh and scan the branches again; a shown Current is scanned again too. */
  async function refresh() {
    const shown = m.shown;
    if (shown.kind === "current") void current.scan(shown.branch);
    const read = await browse.reload();
    // E2E builds only: witness this refresh once its own read succeeded and settled on screen.
    if (import.meta.env.MODE === "e2e" && read) {
      await tick();
      refreshed(app.activity);
    }
  }

  function settings() {
    m.settings = !m.settings;
  }

  async function search() {
    m.settings = false;
    await tick();
    m.panes.search?.focus();
    m.panes.search?.select();
  }

  useCommands(() => [
    { id: "app.palette", group: "App", label: "Command palette", keys: "Ctrl+K", run: () => (app.palette = true) },
    { id: "app.help", group: "App", label: "Keyboard shortcuts", keys: "?", run: () => (app.help = true) },
    { id: "app.settings", group: "App", label: "Show Settings", keys: "Ctrl+,", run: () => (m.settings = true) },
    { id: "app.refresh", group: "App", label: "Refresh", keys: "Ctrl+R", run: () => void refresh() },
    { id: "app.quit", group: "App", label: "Quit", keys: "q", run: () => void quit() },
  ]);

  /** No dialog is open: window keys belong to the screen. */
  const free = () => !app.palette && !app.help && !app.asking && leaving.taken === null;

  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented || composing(event)) return;
    if (chord(event, "k") || (event.key === "F1" && !event.ctrlKey)) {
      event.preventDefault();
      app.palette = true;
    } else if ((event.ctrlKey || event.metaKey) && !event.altKey && (event.key === "," || event.code === "Comma")) {
      event.preventDefault();
      settings();
    } else if (chord(event, "o")) {
      event.preventDefault();
      app.asking = true;
    } else if (chord(event, "f") || (plain(event, "/") && free())) {
      event.preventDefault();
      void search();
    } else if (plain(event, "Escape") && m.settings && free() && !editable(event.target)) {
      event.preventDefault();
      m.settings = false;
    } else if (plain(event, "?")) {
      event.preventDefault();
      app.help = true;
    } else if (plain(event, "q")) {
      event.preventDefault();
      void quit();
    } else if (chord(event, "r") || event.key === "F5") {
      // Never reload the page: a refresh reads the state again.
      event.preventDefault();
      void refresh();
    }
  }

  /** The keys Help lists: of Settings, the Pane or the Main area, whichever has focus. */
  const help = $derived(m.settings ? HELP.settings : m.focus === "pane" ? HELP.pane : HELP.main);
</script>

<svelte:window onkeydown={keydown} />

<div class="app">
  <header class="bar">
    <div class="brand"><img src={brandMark} alt="" width="24" height="24" />Artifactize</div>
    <div class="spacer"></div>
    {#if browse.view && !browse.view.refreshed}<span class="muted">loading…</span>{/if}
    {#if app.launch}<span class="muted" aria-label="Reviewer {app.launch.reviewer}">{app.launch.reviewer}</span>{/if}
    <button class="button" aria-pressed={m.settings} onclick={settings}>Settings</button>
    <button class="button" onclick={() => (app.palette = true)}>
      Commands <kbd>Ctrl+K</kbd>
    </button>
  </header>
  {#if app.failure}
    <p class="failure">{app.failure}</p>
  {:else if app.launch}
    <main>
      <Monitor />
    </main>
  {/if}
</div>

<Palette bind:open={app.palette} />
<Help bind:open={app.help} sections={help} />
<LeaveDialog
  taken={leaving.taken}
  onstay={() => leaving.stay()}
  onleave={() => void getCurrentWindow().destroy()}
/>

<style>
  /* Nothing the screen shows widens the window's layout. */
  .app {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px var(--pad);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .brand {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    gap: 8px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .spacer {
    flex: 1;
  }
  .muted {
    color: var(--muted);
  }
  main {
    min-height: 0;
  }
  .failure {
    color: var(--red);
    padding: var(--pad);
  }
</style>
