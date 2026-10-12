<script lang="ts">
  // The window: Monitor and Review as tabs over one state, the command palette, help, and
  // what happens on quit.
  import { Tabs } from "bits-ui";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { call, commands, message, onStateChanged, type LaunchView } from "./ipc/client";
  import { Browse } from "./monitor/browse.svelte";
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
  import { Leaving } from "./shell/leaving.svelte";

  let launch = $state.raw<LaunchView | null>(null);
  let failure = $state<string | null>(null);
  let screen = $state<"monitor" | "review" | "settings">("monitor");
  let palette = $state(false);
  let help = $state(false);
  const leaving = new Leaving();
  const browse = new Browse();

  onMount(() => {
    call(commands.launch()).then(
      (view) => {
        launch = view;
        screen = view.mode;
      },
      (error) => (failure = message(error)),
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
    { id: "app.palette", group: "App", label: "Command palette", keys: "Ctrl+K", run: () => (palette = true) },
    { id: "app.help", group: "App", label: "Keyboard shortcuts", keys: "?", run: () => (help = true) },
    { id: "app.monitor", group: "App", label: "Show Monitor", keys: "Ctrl+1", run: () => (screen = "monitor") },
    { id: "app.review", group: "App", label: "Show Review", keys: "Ctrl+2", run: () => (screen = "review") },
    { id: "app.settings", group: "App", label: "Show Settings", keys: "Ctrl+,", run: () => (screen = "settings") },
    { id: "app.quit", group: "App", label: "Quit", keys: "q", run: () => void quit() },
  ]);

  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented || composing(event)) return;
    if (chord(event, "k") || (event.key === "F1" && !event.ctrlKey)) {
      event.preventDefault();
      palette = true;
    } else if (chord(event, "1") || chord(event, "2")) {
      event.preventDefault();
      screen = chord(event, "1") ? "monitor" : "review";
    } else if ((event.ctrlKey || event.metaKey) && !event.altKey && (event.key === "," || event.code === "Comma")) {
      event.preventDefault();
      screen = "settings";
    } else if (plain(event, "?")) {
      event.preventDefault();
      help = true;
    } else if (plain(event, "q")) {
      event.preventDefault();
      void quit();
    } else if (chord(event, "r") || event.key === "F5") {
      // Never reload the page: a refresh reads the state again.
      event.preventDefault();
      browse.refresh();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="app">
  <header class="bar">
    <div class="brand">artifactize</div>
    <Tabs.Root bind:value={screen}>
      <Tabs.List class="tabs" aria-label="Screens">
        <Tabs.Trigger class="tab" value="monitor">Monitor</Tabs.Trigger>
        <Tabs.Trigger class="tab" value="review">Review</Tabs.Trigger>
      </Tabs.List>
    </Tabs.Root>
    <div class="spacer"></div>
    {#if browse.view && !browse.view.refreshed}<span class="muted">loading…</span>{/if}
    <Badge counts={browse.view?.badge ?? []} />
    {#if launch}<span class="muted" title="Reviewer">{launch.reviewer}</span>{/if}
    <button
      class="button"
      aria-pressed={screen === "settings"}
      onclick={() => (screen = "settings")}
      title="Accounts, keys, models and the remote store (Ctrl+,)">Settings</button
    >
    <button class="button" onclick={() => (palette = true)} title="Command palette">
      Commands <kbd>Ctrl+K</kbd>
    </button>
  </header>
  {#if failure}
    <p class="failure">{failure}</p>
  {:else if launch}
    <main>
      <div class="screen" hidden={screen !== "monitor"}>
        <Monitor {browse} active={screen === "monitor"} />
      </div>
      <div class="screen" hidden={screen !== "review"}>
        <ReviewScreen {launch} active={screen === "review"} />
      </div>
      <div class="screen" hidden={screen !== "settings"}>
        <Settings active={screen === "settings"} />
      </div>
    </main>
  {/if}
</div>

<Palette bind:open={palette} />
<Help bind:open={help} sections={HELP[screen]} />
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
