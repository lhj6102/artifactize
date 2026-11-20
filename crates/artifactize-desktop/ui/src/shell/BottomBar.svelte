<script lang="ts">
  // The window's one bar, along its bottom in every mode (D-BAR-1), as the reference screens draw
  // it: the brand at its left, which opens the command palette; Chat, Verify and Review in its
  // middle, Review with the count of Human reviews waiting for a person; Settings at its right.
  // Showing a mode changes only the workspace above it.
  import brandMark from "../../../../../assets/brand/artifactize-icon-mark.svg";
  import { getAppState, MODES, type WindowMode } from "./app.svelte";
  import Icon from "./Icon.svelte";

  let { onsettings }: { onsettings: () => void } = $props();
  const app = getAppState();
  const { monitor: m } = app;

  const ICON: Record<WindowMode, "chat" | "verify" | "review"> = { chat: "chat", verify: "verify", review: "review" };
</script>

<footer class="bar" class:raised={m.settings} aria-label="Bottom bar">
  <div class="side">
    <button class="brand" aria-label="Artifactize: commands (Ctrl+K)" aria-keyshortcuts="Control+K" onclick={() => (app.palette = true)}>
      <img src={brandMark} alt="" width="16" height="16" />Artifactize
    </button>
  </div>
  <nav class="modes" aria-label="Modes">
    {#each MODES as item (item.mode)}
      {@const on = app.mode === item.mode}
      {@const count = item.mode === "review" ? app.waiting : 0}
      <button
        class="mode"
        class:on
        aria-current={on ? "page" : undefined}
        aria-label={count > 0 ? `${item.label}, ${count} waiting` : item.label}
        aria-keyshortcuts={item.keys.replace("Ctrl", "Control")}
        onclick={() => (app.mode = item.mode)}
      >
        <Icon name={ICON[item.mode]} />{item.label}
        {#if count > 0}<span class="count" aria-hidden="true">{count}</span>{/if}
      </button>
    {/each}
  </nav>
  <div class="side end">
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
  /* The brand, borderless; hovering only steps its face up. */
  .brand {
    display: flex;
    align-items: center;
    flex: none;
    gap: 8px;
    height: var(--control);
    margin: 0;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink);
    font: inherit;
    font-weight: 600;
    letter-spacing: 0.01em;
    cursor: pointer;
  }
  .brand:hover {
    background: var(--surface-group);
  }
  /* Borderless as tall as the bar: the shown one on the group surface, in ink, semibold, under
     a 2px accent line along its top. */
  .modes {
    display: flex;
    align-items: stretch;
    gap: 2px;
  }
  .mode {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    padding: 0 12px;
    border: 0;
    border-radius: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    cursor: pointer;
    box-shadow: inset 0 2px 0 transparent;
  }
  .mode:not(.on):hover {
    background: var(--surface-group);
  }
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
  .side :global(.button) {
    flex: none;
  }
</style>
