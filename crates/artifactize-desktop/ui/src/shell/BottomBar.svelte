<script lang="ts">
  // The window's one bar, along its bottom in every mode (D-BAR-1), as the reference screens draw
  // it: the brand at its left, which opens the command palette; Chat, Verify and Review in its
  // middle, Review with the count of Human reviews waiting for a person; Settings at its right.
  // Showing a mode changes only the workspace above it.
  import brandMark from "@brand/artifactize-icon-mark.svg";
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
        <span class="mode-mark">
          <Icon name={ICON[item.mode]} />
          {#if count > 0}<span class="count" aria-hidden="true">{count}</span>{/if}
        </span>
        {item.label}
      </button>
    {/each}
  </nav>
  <div class="side end">
    <button class="mode settings" aria-pressed={m.settings} aria-label="Settings" title="Settings" aria-keyshortcuts="Control+," data-settings-toggle onclick={onsettings}>
      <Icon name="settings" />
    </button>
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
    align-items: stretch;
    min-width: 0;
  }
  .side:first-child {
    padding-left: var(--pad);
  }
  .side.end {
    justify-content: flex-end;
  }
  /* The brand, borderless; hovering only steps its ink up. */
  .brand {
    display: flex;
    align-items: center;
    align-self: center;
    flex: none;
    gap: 8px;
    height: var(--control);
    margin: 0;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--bar-ink);
    font: inherit;
    font-weight: 600;
    letter-spacing: 0.01em;
    cursor: pointer;
  }
  .brand:hover {
    color: var(--ink);
  }
  .modes {
    display: flex;
    align-items: stretch;
  }
  .mode {
    position: relative;
    flex: none;
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    width: 80px;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--bar-ink);
    font: inherit;
    font-size: var(--text-label);
    line-height: 1;
    cursor: pointer;
  }
  .mode-mark {
    position: relative;
    display: flex;
  }
  .mode :global(svg) {
    width: var(--icon-bar);
    height: var(--icon-bar);
  }
  .mode:not(.on):hover {
    color: var(--ink);
  }
  .mode.on {
    background: var(--bar-active);
    color: var(--bar-ink-on);
    box-shadow: inset 0 -2px 0 var(--bar-mark);
  }
  .settings {
    width: 44px;
  }
  /* The count sits beside Review's icon without moving the icon or the label. */
  .count {
    position: absolute;
    top: 0;
    left: calc(100% + 3px);
    min-width: 16px;
    height: 14px;
    padding: 0 4px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-chip);
    background: var(--bar-active);
    color: var(--human);
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 12px;
    text-align: center;
  }
</style>
