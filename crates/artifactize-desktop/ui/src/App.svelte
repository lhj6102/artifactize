<script lang="ts">
  // The window: its one screen over one state, the command palette, help, and what happens on
  // quit.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Dialog } from "bits-ui";
  import { onMount, tick } from "svelte";
  import brandMark from "../../../../assets/brand/artifactize-icon-mark.svg";
  import { call, message, onCurrentChanged, onHumanToolLog, onStateChanged } from "./ipc/client";
  import Monitor from "./monitor/Monitor.svelte";
  import LeaveDialog from "./review/LeaveDialog.svelte";
  import Palette from "./shell/Palette.svelte";
  import Help from "./shell/Help.svelte";
  import Settings from "./settings/Settings.svelte";
  import ReleaseDialog from "./shell/ReleaseDialog.svelte";
  import { useCommands } from "./shell/commands.svelte";
  import { chord, composing, plain } from "./shell/keys";
  import { HELP } from "./shell/shortcuts";
  import { AppState, setAppState } from "./shell/app.svelte";
  import { sourceOf } from "./monitor/monitor-state.svelte";

  import { exposeActivity, refreshed, setActivity } from "./ipc/activity.svelte";

  const app = new AppState();
  setActivity(app.activity);
  const commands = app.ipc;
  setAppState(app);
  const { browse, current, leaving, monitor: m, release } = app;

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
    const unwatched = release.watch();
    const unchanged = onStateChanged((resync) => browse.refresh(resync));
    // What Human tools print, for whichever review or Artifact shows it.
    const unlogged = onHumanToolLog((logs) => app.logged(logs));
    // Each step of the shown Current's scan.
    const unscanned = onCurrentChanged((branch) => void current.changed(branch));
    // Closing the window asks about this session's review claims first, like `q`.
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested((event) => leaving.closeRequested(event))
      .then((stop) => (unlisten = stop), () => {});
    return () => {
      hideActivity();
      clearInterval(clock);
      unwatched();
      unchanged();
      unlogged();
      unscanned();
      unlisten?.();
    };
  });

  async function quit() {
    if (await leaving.allowed()) await getCurrentWindow().destroy();
  }

  /** Read the state afresh and scan the branches again; a shown Current is scanned again too. */
  async function refresh() {
    const shown = m.shown;
    if (shown.kind === "current") void current.scan(shown.branch, true);
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

  /** A press on the bar's Settings button is its own toggle, not a click outside Settings that
   * would close it only for the button to open it again. */
  function keepToggle(event: PointerEvent) {
    if (event.target instanceof Element && event.target.closest("[data-settings-toggle]")) event.preventDefault();
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
  const free = () => !app.palette && !app.help && !app.asking && !m.settings && !release.open && leaving.taken === null;

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
    } else if (chord(event, "f") && sourceOf(m.shown) !== null && m.focus !== "pane" && free()) {
      // Over a graph, Ctrl+F finds in it; in the Pane, or with no graph, it searches the Pane.
      event.preventDefault();
      m.finding = true;
    } else if (chord(event, "f") || (plain(event, "/") && free())) {
      event.preventDefault();
      void search();
    } else if (plain(event, "?")) {
      event.preventDefault();
      app.help = true;
    } else if (plain(event, "q") && !m.settings) {
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
  <header class="bar" class:raised={m.settings}>
    <div class="brand"><img src={brandMark} alt="" width="24" height="24" />Artifactize</div>
    <div class="spacer"></div>
    {#if browse.view && !browse.view.refreshed}<span class="muted">loading…</span>{/if}
    {#if app.launch}<span class="muted" aria-label="Reviewer {app.launch.reviewer}">{app.launch.reviewer}</span>{/if}
    {#if release.notice}
      <button class="button" onclick={() => (release.open = true)}>{release.notice.latest} available</button>
    {/if}
    <button class="button" aria-pressed={m.settings} data-settings-toggle onclick={settings}>Settings</button>
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

<!-- Settings opens over the screen; closing it leaves the screen beneath as it was. The bar
     stays above it, so its Settings button closes it again. -->
<Dialog.Root bind:open={m.settings}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog settings-dialog" aria-labelledby="settings-title" onInteractOutside={keepToggle}>
      <Settings active={m.settings} onclose={() => (m.settings = false)} />
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
<Palette bind:open={app.palette} />
<Help bind:open={app.help} sections={help} />
<ReleaseDialog {release} />
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
  /* Above the Settings overlay and dialog. */
  .raised {
    position: relative;
    z-index: 52;
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
