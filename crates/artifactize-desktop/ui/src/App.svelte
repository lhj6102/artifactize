<script lang="ts">
  // The window: three modes over one state, each a workspace of its own above the bottom bar,
  // the command palette, help, Open by ID, and what happens on quit. Every mode stays mounted,
  // the hidden ones out of sight and out of reach, so each keeps its place.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Dialog } from "bits-ui";
  import { onMount, tick, untrack } from "svelte";
  import { Coalesced } from "./ipc/coalesced";
  import { listen } from "@tauri-apps/api/event";
  import type { Attachment } from "svelte/attachments";
  import { call, message, subscribe, onActiveReviewChanged, onCurrentChanged, onHumanDraftChanged, onHumanToolLog, onStateChanged } from "./ipc/client";
  import { PET_OPEN } from "./pet/pet";
  import { followPointed } from "./review/pointed";
  import Monitor from "./monitor/Monitor.svelte";
  import ReviewMode from "./review/ReviewMode.svelte";
  import ChatMode from "./chat/ChatMode.svelte";
  import BottomBar from "./shell/BottomBar.svelte";
  import LeaveDialog from "./review/LeaveDialog.svelte";
  import Palette from "./shell/Palette.svelte";
  import Help from "./shell/Help.svelte";
  import Settings from "./settings/Settings.svelte";
  import ReleaseDialog from "./shell/ReleaseDialog.svelte";
  import { useCommands } from "./shell/commands.svelte";
  import { chord, composing, editable, plain } from "./shell/keys";
  import { help, type Focused } from "./shell/shortcuts";
  import { AppState, MODES, setAppState, type WindowMode } from "./shell/app.svelte";
  import { sourceOf } from "./monitor/monitor-state.svelte";

  import { exposeActivity, refreshed, setActivity } from "./ipc/activity.svelte";

  const app = new AppState();
  setActivity(app.activity);
  const commands = app.ipc;
  setAppState(app);
  const { browse, current, leaving, monitor: m, release, reviewMode: r } = app;
  /** The ID typed into the Open by ID dialog. */
  let wanted = $state("");

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
    void app.countWaiting();
    // Each change reads again at most once at a time: the bar's count always, Verify's branches
    // and Run only while Verify is on screen (once more when it is shown again).
    const unchanged = onStateChanged((resync) => {
      resyncing ||= resync;
      browsing.request();
      counting.request();
    });
    // What Human tools print, for whichever review or Artifact shows it.
    const unlogged = onHumanToolLog((logs) => app.logged(logs));
    // Each step of the shown Current's scan.
    const unscanned = onCurrentChanged((branch) => void current.changed(branch));
    // A review's draft written elsewhere (another window, the pet overlay, a viewer's Point).
    const undrafted = onHumanDraftChanged((id) => void app.reviews[id]?.readDraft());
    // The active review settled, was released or was replaced.
    const unactive = onActiveReviewChanged((request) => (app.active = request));
    // What a viewer's Point appends stands out in the review's Notes until they are next edited.
    const unpointed = followPointed(
      ({ review, text }) => {
        const shown = app.reviews[review];
        if (shown) shown.pointed = [...shown.pointed, text];
      },
      (review) => {
        const shown = app.reviews[review];
        if (shown) shown.pointed = [];
      },
    );
    // The pet overlay leads back to its review here.
    const unpet = subscribe<string>((handler) => listen(PET_OPEN, handler), (id) => void app.showReview(id));
    // Whether this window is in front: the pet overlay stands aside while the review is.
    const focus = () => (app.focused = true);
    const blur = () => (app.focused = false);
    window.addEventListener("focus", focus);
    window.addEventListener("blur", blur);
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
      undrafted();
      unactive();
      unpointed();
      unpet();
      window.removeEventListener("focus", focus);
      window.removeEventListener("blur", blur);
      unlisten?.();
    };
  });

  /** The active review follows the claimed review shown in Review, and the pet overlay shows it
   * while its evaluation space is not in front (J-POINT, J-PET). Re-runs as those change. */
  /** Reads the backend's changes ask for, one at a time (see `Coalesced`). */
  let resyncing = false;
  const browsing = new Coalesced(
    async () => {
      const resync = resyncing;
      resyncing = false;
      browse.refresh(resync);
    },
    () => app.mode === "verify",
  );
  const counting = new Coalesced(() => app.countWaiting());
  /** Verify shown again reads what changed while it was not. */
  const verifying: Attachment = () => {
    if (app.mode === "verify") untrack(() => browsing.showing());
  };

  const following: Attachment = () => {
    const shown = r.shown.kind === "review" ? app.reviews[r.shown.id] : undefined;
    const owned = shown?.owned ? shown.id : null;
    if (owned) void app.activate(owned);
    const inFront = app.focused && app.mode === "review" && owned !== null && owned === app.active;
    void app.pet(app.active && !inFront ? app.active : null);
  };

  async function quit() {
    if (await leaving.allowed()) await leaving.close();
  }

  /** Read the state afresh and scan the branches again; a shown Current is scanned again too. */
  async function refresh() {
    const shown = m.shown;
    if (shown.kind === "current") void current.scan(shown.branch, true);
    const read = await browse.reload();
    // E2E builds only: witness this refresh once its own read succeeded and settled on screen.
    if (import.meta.env.MODE === "e2e" && read) {
      await tick();
      refreshed(app.activity, read);
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
    app.mode = "verify";
    app.paneShown = true;
    await tick();
    m.panes.search?.focus();
    m.panes.search?.select();
  }

  /** Hide every mode's Pane and its divider, or show them again as they were; focus in a Pane
   * that hides moves to its mode's main area. */
  async function togglePane() {
    const verify = app.mode === "verify" && (m.panes.pane?.contains(document.activeElement) || m.panes.search === document.activeElement || m.focus === "pane");
    const review = app.mode === "review" && (r.panes.pane?.contains(document.activeElement) || r.focus === "pane");
    app.paneShown = !app.paneShown;
    if (app.paneShown) return;
    if (verify) m.focus = "main";
    if (review) r.focus = "main";
    if (!verify && !review) return;
    await tick();
    (verify ? m.panes.main : r.panes.main)?.focus();
  }

  /** Focus Review's filter, showing Review and its Pane first. */
  async function filter() {
    m.settings = false;
    app.mode = "review";
    app.paneShown = true;
    await tick();
    r.panes.filter?.focus();
    r.panes.filter?.select();
  }

  /** Show a mode: the workspace above the bar changes, each mode as it was left. */
  function show(mode: WindowMode) {
    m.settings = false;
    app.mode = mode;
  }

  /** Open the request typed into Open by ID in Review. */
  function openWanted(event: SubmitEvent) {
    event.preventDefault();
    const id = wanted.trim();
    if (!id) return;
    app.asking = false;
    wanted = "";
    void app.openRequest(id).then(async () => {
      r.focus = "main";
      await tick();
      r.panes.main?.focus();
    });
  }

  useCommands(() => [
    { id: "app.help", group: "App", label: "Keyboard shortcuts", keys: "?", run: () => (app.help = true) },
    { id: "app.settings", group: "App", label: "Show Settings", keys: "Ctrl+,", run: () => (m.settings = true) },
    ...MODES.map((item) => ({ id: `app.mode.${item.mode}`, group: "App", label: `Show ${item.label}`, keys: item.keys, run: () => show(item.mode) })),
    { id: "app.pane", group: "App", label: "Show or hide the Pane", keys: "Ctrl+B", run: () => void togglePane() },
    { id: "app.search", group: "App", label: "Search the branches", keys: "/", run: () => void search() },
    { id: "app.filter", group: "App", label: "Filter the reviews", run: () => void filter() },
    { id: "app.open-id", group: "App", label: "Open a request by ID", keys: "Ctrl+O", run: () => (app.asking = true) },
    { id: "app.refresh", group: "App", label: "Refresh", keys: "Ctrl+R", run: () => void refresh() },
    { id: "app.quit", group: "App", label: "Quit", keys: "q", run: () => void quit() },
  ]);

  /** No dialog is open: window keys belong to the screen. */
  const free = () => !app.palette && !app.help && !app.asking && !m.settings && !release.open && leaving.held === null;

  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented || composing(event)) return;
    const mode = MODES.find((item) => chord(event, item.keys.slice(-1)));
    if (chord(event, "k") || (event.key === "F1" && !event.ctrlKey)) {
      event.preventDefault();
      app.palette = true;
    } else if (mode && free()) {
      event.preventDefault();
      show(mode.mode);
    } else if ((event.ctrlKey || event.metaKey) && !event.altKey && (event.key === "," || event.code === "Comma")) {
      event.preventDefault();
      settings();
    } else if (chord(event, "o")) {
      event.preventDefault();
      app.asking = true;
    } else if (chord(event, "b") && free()) {
      event.preventDefault();
      void togglePane();
    } else if (chord(event, "f") && app.mode === "verify" && sourceOf(m.shown) !== null && m.focus !== "pane" && !editable(event.target) && free()) {
      // Outside fields and the Pane, Ctrl+F finds in a graph. Otherwise it searches the Pane.
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

  /** What has focus, as Help lists its keys: Settings, Verify's Pane, a Human review (in
   * Review, or on the Detail's tab), or a graph with its Detail. */
  const focused = $derived.by((): Focused => {
    if (m.settings) return "settings";
    if (app.mode === "review") return r.focus === "pane" ? "pane" : "review";
    if (m.focus === "pane") return "pane";
    if (m.focus === "detail" && m.opened?.request !== undefined) return "review";
    return m.shown.kind === "current" ? "current" : "graph";
  });
  const keys = $derived(help(focused));
</script>

<svelte:window onkeydown={keydown} />

<div class="app" {@attach following} {@attach verifying}>
  {#if app.failure}
    <p class="failure">{app.failure}</p>
  {:else if app.launch}
    <!-- The three workspaces share one place; the hidden ones keep their size and their place,
         out of sight and out of reach. -->
    <div class="workspaces">
      <main class="workspace" aria-label="Chat" hidden={app.mode !== "chat"} inert={app.mode !== "chat"} style:opacity={app.mode === "chat" ? null : "0"}><ChatMode /></main>
      <main class="workspace" aria-label="Verify" hidden={app.mode !== "verify"} inert={app.mode !== "verify"} style:opacity={app.mode === "verify" ? null : "0"}><Monitor /></main>
      <main class="workspace" aria-label="Review" hidden={app.mode !== "review"} inert={app.mode !== "review"} style:opacity={app.mode === "review" ? null : "0"}><ReviewMode /></main>
    </div>
  {:else}
    <div></div>
  {/if}
  <BottomBar onsettings={settings} />
</div>

<Dialog.Root bind:open={app.asking}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog open-id">
      <Dialog.Title class="dialog-title">Open a request by ID</Dialog.Title>
      <form onsubmit={openWanted}>
        <input class="input" placeholder="req-…" bind:value={wanted} aria-label="Request ID" />
        <div class="dialog-actions">
          <Dialog.Close class="button" aria-keyshortcuts="Escape">Cancel</Dialog.Close>
          <button class="button primary" type="submit" aria-keyshortcuts="Enter">Open</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

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
<Help bind:open={app.help} sections={keys.sections} marks={keys.marks} />
<ReleaseDialog {release} />
<LeaveDialog
  held={leaving.held}
  onstay={() => leaving.stay()}
  onleave={() => void leaving.close()}
/>

<style>
  /* Nothing the screen shows widens the window's layout: the workspace above, the bar below. */
  .app {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
  }
  /* The modes stack in one place; only the shown one is visible. A hidden one keeps its size,
   * so its graph's camera and its scroll stay as they were. */
  .workspaces {
    display: grid;
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    min-height: 0;
  }
  .workspace {
    grid-area: 1 / 1;
    min-height: 0;
    min-width: 0;
  }
  /* A hidden mode paints nothing: no descendant can show itself again (a graph's nodes set
   * their own visibility), and its composited layers are transparent with it. */
  .workspace[hidden] {
    display: block;
    visibility: hidden;
    pointer-events: none;
  }
  .workspace[hidden] :global(*) {
    visibility: hidden !important;
  }
  :global(.dialog.open-id) {
    width: min(480px, 92vw);
  }
  .input::placeholder {
    color: var(--ink-3);
  }
  .input {
    width: 100%;
    height: var(--control);
    font: inherit;
    padding: 0 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
  }
  main {
    min-height: 0;
  }
  .failure {
    color: var(--red);
    padding: var(--pad);
  }
</style>
