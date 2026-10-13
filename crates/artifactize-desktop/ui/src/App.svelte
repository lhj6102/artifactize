<script lang="ts">
  // The window: Monitor and Review as tabs over one state, the command palette, help, and
  // what happens on quit.
  import { Tabs } from "bits-ui";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import brandMark from "../../../../assets/brand/artifactize-icon-mark.svg";
  import { call, commands, message, onStateChanged } from "./ipc/client";
  import CurrentGraph from "./monitor/CurrentGraph.svelte";
  import Monitor from "./monitor/Monitor.svelte";
  import ReviewScreen from "./review/ReviewScreen.svelte";
  import Settings from "./settings/Settings.svelte";
  import LeaveDialog from "./review/LeaveDialog.svelte";
  import Palette from "./shell/Palette.svelte";
  import Help from "./shell/Help.svelte";
  import Badge from "./shell/Badge.svelte";
  import { useCommands } from "./shell/commands.svelte";
  import { chord, composing, plain } from "./shell/keys";
  import { HELP } from "./shell/help";
  import { AppState, setAppState } from "./shell/app.svelte";

  const app = new AppState();
  setAppState(app);
  const { browse, leaving } = app;

  onMount(() => {
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
    // Closing the window asks about this session's review claims first, like `q`.
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested((event) => leaving.closeRequested(event))
      .then((stop) => (unlisten = stop), () => {});
    return () => {
      clearInterval(clock);
      unchanged();
      unlisten?.();
    };
  });

  async function quit() {
    if (await leaving.allowed()) await getCurrentWindow().destroy();
  }

  useCommands(() => [
    { id: "app.palette", group: "App", label: "Command palette", keys: "Ctrl+K", run: () => (app.palette = true) },
    { id: "app.help", group: "App", label: "Keyboard shortcuts", keys: "?", run: () => (app.help = true) },
    { id: "app.monitor", group: "App", label: "Show Monitor", keys: "Ctrl+1", run: () => app.show("monitor") },
    { id: "app.review", group: "App", label: "Show Review", keys: "Ctrl+2", run: () => app.show("review") },
    { id: "app.settings", group: "App", label: "Show Settings", keys: "Ctrl+,", run: () => app.show("settings") },
    { id: "app.quit", group: "App", label: "Quit", keys: "q", run: () => void quit() },
  ]);

  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented || composing(event)) return;
    if (chord(event, "k") || (event.key === "F1" && !event.ctrlKey)) {
      event.preventDefault();
      app.palette = true;
    } else if (chord(event, "1") || chord(event, "2")) {
      event.preventDefault();
      app.show(chord(event, "1") ? "monitor" : "review");
    } else if ((event.ctrlKey || event.metaKey) && !event.altKey && (event.key === "," || event.code === "Comma")) {
      event.preventDefault();
      app.show("settings");
    } else if (plain(event, "?")) {
      event.preventDefault();
      app.help = true;
    } else if (plain(event, "q")) {
      event.preventDefault();
      void quit();
    } else if (chord(event, "r") || event.key === "F5") {
      // The current graph owns its declaration reload; leave the event for that screen.
      if (app.screen === "graph") return;
      // Never reload the page: a refresh reads the state again.
      event.preventDefault();
      browse.refresh();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="app">
  <header class="bar">
    <div class="brand"><img src={brandMark} alt="" width="24" height="24" />Artifactize</div>
    <Tabs.Root bind:value={app.screen}>
      <Tabs.List class="tabs" aria-label="Screens">
        {#if app.launch?.mode === "graph"}<Tabs.Trigger class="tab" value="graph">Graph</Tabs.Trigger>{/if}
        <Tabs.Trigger class="tab" value="monitor">Monitor</Tabs.Trigger>
        <Tabs.Trigger class="tab" value="review">Review</Tabs.Trigger>
      </Tabs.List>
    </Tabs.Root>
    <div class="spacer"></div>
    {#if browse.view && !browse.view.refreshed}<span class="muted">loading…</span>{/if}
    <Badge counts={browse.view?.badge ?? []} />
    {#if app.launch}<span class="muted" title="Reviewer">{app.launch.reviewer}</span>{/if}
    <button
      class="button"
      aria-pressed={app.screen === "settings"}
      onclick={() => app.show("settings")}
      title="Accounts, keys, models and the remote store (Ctrl+,)">Settings</button
    >
    <button class="button" onclick={() => (app.palette = true)} title="Command palette">
      Commands <kbd>Ctrl+K</kbd>
    </button>
  </header>
  {#if app.failure}
    <p class="failure">{app.failure}</p>
  {:else if app.launch}
    <main>
      {#if app.screen === "graph"}<CurrentGraph />{/if}
      <div class="screen" hidden={app.screen !== "monitor"}>
        <Monitor active={app.screen === "monitor"} />
      </div>
      <div class="screen" hidden={app.screen !== "review"}>
        <ReviewScreen launch={app.launch} active={app.screen === "review"} />
      </div>
      <div class="screen" hidden={app.screen !== "settings"}>
        <Settings active={app.screen === "settings"} />
      </div>
    </main>
  {/if}
</div>

<Palette bind:open={app.palette} />
<Help bind:open={app.help} sections={HELP[app.screen]} />
<LeaveDialog
  taken={leaving.taken}
  onstay={() => leaving.stay()}
  onleave={() => void getCurrentWindow().destroy()}
/>

<style>
  .app {
    display: grid;
    grid-template-rows: auto 1fr;
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
  :global(.tabs) {
    display: flex;
    gap: 4px;
    background: var(--surface-2);
    padding: 3px;
    border-radius: 8px;
  }
  :global(.tab) {
    font: inherit;
    border: 0;
    background: transparent;
    color: var(--muted);
    padding: 3px 12px;
    border-radius: 6px;
    cursor: pointer;
  }
  :global(.tab[data-state="active"]) {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 10%);
  }
  main {
    min-height: 0;
  }
  .screen {
    height: 100%;
  }
  .screen[hidden] {
    display: none;
  }
  .failure {
    color: var(--red);
    padding: var(--pad);
  }
</style>
