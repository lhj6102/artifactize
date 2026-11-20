<script lang="ts">
  // The window's one bar, along its bottom in every mode (D-BAR-1): the brand and the Pane switch
  // at its left, Chat, Verify and Review in its middle, Review with the count of Human reviews
  // waiting for a person, and the reviewer, the release notice, Commands and Settings at its
  // right. Showing a mode changes only the workspace above it.
  import brandMark from "../../../../../assets/brand/artifactize-icon-mark.svg";
  import { getAppState, MODES, type WindowMode } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import { label } from "./keys";

  let { ontogglepane, onsettings }: { ontogglepane: () => void; onsettings: () => void } = $props();
  const app = getAppState();
  const { browse, monitor: m, release } = app;

  /** The Human reviews waiting for a person, whatever a filter shows. */
  const waiting = $derived(browse.view?.waiting.length ?? 0);
  const ICON: Record<WindowMode, "chat" | "verify" | "review"> = { chat: "chat", verify: "verify", review: "review" };
</script>

<footer class="bar" class:raised={m.settings} aria-label="Bottom bar">
  <div class="side">
    <div class="brand"><img src={brandMark} alt="" width="16" height="16" />Artifactize</div>
    <!-- The Pane switch: on (a line along its top) while the Panes are shown. -->
    <button class="switch" class:on={app.paneShown} aria-pressed={app.paneShown} aria-label="Pane (Ctrl+B)" aria-keyshortcuts="Control+B" onclick={ontogglepane}>
      <Icon name="pane" />Pane
    </button>
  </div>
  <nav class="modes" aria-label="Modes">
    {#each MODES as item (item.mode)}
      {@const on = app.mode === item.mode}
      <button
        class="mode"
        class:on
        aria-current={on ? "page" : undefined}
        aria-label={item.mode === "review" && waiting > 0 ? `Review, ${waiting} waiting` : item.label}
        aria-keyshortcuts={item.keys.replace("Ctrl", "Control")}
        onclick={() => (app.mode = item.mode)}
      >
        <Icon name={ICON[item.mode]} />{item.label}
        {#if item.mode === "review" && waiting > 0}<span class="count" aria-hidden="true">{waiting}</span>{/if}
      </button>
    {/each}
  </nav>
  <div class="side end">
    {#if app.launch}<span class="reviewer" aria-label="Reviewer {app.launch.reviewer}">{app.launch.reviewer}</span>{/if}
    {#if release.notice}
      <button class="button" onclick={() => (release.open = true)}>{release.notice.latest} available</button>
    {/if}
    <button class="button" aria-keyshortcuts="Control+K" onclick={() => (app.palette = true)}>
      Commands <kbd>{label("Ctrl+K")}</kbd>
    </button>
    <button class="button" aria-pressed={m.settings} aria-label="Settings (Ctrl+,)" aria-keyshortcuts="Control+," data-settings-toggle onclick={onsettings}>Settings</button>
  </div>
</footer>

<style>
  /* The darkest surface, opened above by a panel line; the modes centred on the window. */
  .bar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: stretch;
    gap: var(--gap);
    height: var(--bar-height);
    padding: 0 var(--pad);
    border-top: 1px solid var(--line-panel);
    background: var(--surface-bar);
  }
  /* Above the Settings overlay and dialog, so its Settings button closes it again. */
  .raised {
    position: relative;
    z-index: 52;
  }
  .side {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-width: 0;
  }
  .side.end {
    justify-content: flex-end;
  }
  .brand {
    display: flex;
    align-items: center;
    flex: none;
    gap: 8px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  /* Borderless as tall as the bar: ink under a 2px accent line along its top while on. */
  .switch,
  .mode {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: stretch;
    margin: 0;
    padding: 0 10px;
    border: 0;
    border-radius: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    cursor: pointer;
    box-shadow: inset 0 2px 0 transparent;
  }
  .switch {
    margin-left: 4px;
  }
  .switch.on {
    color: var(--ink);
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .switch:hover,
  .mode:not(.on):hover {
    background: var(--surface-group);
  }
  .modes {
    display: flex;
    align-items: stretch;
    gap: 2px;
  }
  .mode {
    padding: 0 12px;
  }
  /* The shown mode: on the group surface, in ink, semibold, under the accent line. */
  .mode.on {
    background: var(--surface-group);
    color: var(--ink);
    font-weight: 600;
    box-shadow: inset 0 2px 0 var(--accent);
  }
  /* The count of Human reviews waiting for a person, in their colour. */
  .count {
    min-width: 18px;
    height: 16px;
    padding: 0 5px;
    box-sizing: border-box;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
    color: var(--human);
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 14px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  /* A long reviewer name is cut inside the bar; the buttons after it keep their place. */
  .reviewer {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  .side :global(.button) {
    flex: none;
  }
</style>
