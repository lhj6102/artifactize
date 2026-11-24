<script lang="ts">
  // Settings: what the CLI configures, through the same library calls, in three tabs. Providers
  // holds the Codex (ChatGPT) sign-in, the built-in providers' keys, the custom providers and
  // the models each lists; Remote holds the shared review store; Limits holds the sign-off idle
  // grace. Secrets are never shown; a typed key or token leaves the state as soon as it is
  // submitted.
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { useIpc } from "../ipc/client";
  import { useCommands } from "../shell/commands.svelte";
  import LimitsTab from "./LimitsTab.svelte";
  import ProvidersTab from "./ProvidersTab.svelte";
  import RemoteStore from "./RemoteStore.svelte";
  import { SettingsState, setSettingsState } from "./settings-state.svelte";
  import { getAppState } from "../shell/app.svelte";

  let { active, onclose }: { active: boolean; onclose?: () => void } = $props();
  const s = new SettingsState(useIpc());
  /** The window's reviewer and a newer release: Settings' header holds them (D-SET-1). */
  const app = getAppState();
  const { launch, release } = app;
  setSettingsState(s);

  const TABS = [
    { id: "providers", label: "Providers" },
    { id: "remote", label: "Remote" },
    { id: "limits", label: "Limits" },
  ] as const;

  /** Read every time Settings opens: another process may have signed in or out, or saved a
   * key. A sign-in in progress keeps what it shows. */
  const reload: Attachment = () => {
    if (active) untrack(() => !s.prompt && void s.load());
  };

  /** ← and → move between the tabs, as a tab list does. */
  function tabKey(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const at = TABS.findIndex((tab) => tab.id === app.settingsTab);
    app.settingsTab = TABS[(at + (event.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length].id;
    document.getElementById(`settings-tab-${app.settingsTab}`)?.focus();
  }

  const signedIn = $derived(s.codex?.state === "stored");

  useCommands(() =>
    active
      ? [
          { id: "settings.refresh", group: "Settings", label: "Refresh settings", run: () => void s.load() },
          {
            id: "settings.codex",
            group: "Settings",
            label: signedIn ? "Sign in to Codex again" : "Sign in to Codex",
            enabled: () => !s.busy && s.codex?.state !== "file" && s.codex?.state !== "refused",
            run: () => {
              app.settingsTab = "providers";
              s.choose({ kind: "builtin", provider: "codex" });
              void s.signIn();
            },
          },
        ]
      : [],
  );
</script>

<div class="settings" {@attach reload}>
  <header>
    <h1 id="settings-title">Settings</h1>
    <div class="tabs" role="tablist" aria-label="Settings">
      {#each TABS as tab (tab.id)}
        <button
          id="settings-tab-{tab.id}"
          role="tab"
          class="tab"
          aria-selected={app.settingsTab === tab.id}
          aria-controls="settings-panel-{tab.id}"
          tabindex={app.settingsTab === tab.id ? 0 : -1}
          onclick={() => (app.settingsTab = tab.id)}
          onkeydown={tabKey}>{tab.label}</button
        >
      {/each}
    </div>
    <div class="spacer"></div>
    {#if launch}<span class="reviewer">Reviewer {launch.reviewer}</span>{/if}
    {#if release.notice}
      <button class="button" onclick={() => (release.open = true)}>{release.notice.latest} available</button>
    {/if}
    {#if onclose}<button class="button" aria-keyshortcuts="Escape" onclick={onclose}>Close</button>{/if}
  </header>
  {#if app.settingsTab === "providers"}
    <div class="dialog-body" id="settings-panel-providers" role="tabpanel" aria-labelledby="settings-tab-providers">
      <ProvidersTab />
    </div>
  {:else if app.settingsTab === "remote"}
    <div class="dialog-body" id="settings-panel-remote" role="tabpanel" aria-labelledby="settings-tab-remote">
      <RemoteStore />
    </div>
  {:else}
    <div class="dialog-body" id="settings-panel-limits" role="tabpanel" aria-labelledby="settings-tab-limits">
      <LimitsTab />
    </div>
  {/if}
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  header {
    display: flex;
    flex: none;
    align-items: center;
    gap: 16px;
    flex-wrap: nowrap;
    margin-bottom: 12px;
  }
  .spacer {
    flex: 1;
  }
  /* The name the app claims reviews as, cut inside the header when long. */
  .reviewer {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  header > :global(button), .tabs { flex: none; }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }
  .tabs {
    display: flex;
    gap: 2px;
  }
  /* Text tabs: the active one in ink with the current mark beneath it. */
  .tab {
    font: inherit;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    height: var(--control);
    padding: 0 10px;
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  .tab:not([aria-selected="true"]):hover {
    background: var(--surface-group);
    color: var(--ink);
  }
  .tab[aria-selected="true"] {
    color: var(--ink);
    border-bottom-color: var(--current-mark);
  }
  .settings :global(.muted) {
    color: var(--ink-2);
    margin: 0 0 8px;
  }
  /* Sections under the inspector's header rows, without the fold icon: opened above by a group
     line, the name at 12px semibold in ink; a long name wraps inside its column. */
  .settings :global(section) {
    border-top: 1px solid var(--line-group);
    padding: 0 0 12px;
  }
  .settings :global(section > h2) {
    display: flex;
    align-items: center;
    min-height: var(--row);
    margin: 0 0 4px;
    font-size: var(--text-small);
    font-weight: 600;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .settings :global(.facts) {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 0 0 8px;
  }
  .settings :global(label) {
    color: var(--label);
  }
  .settings :global(dt) {
    color: var(--ink-2);
  }
  .settings :global(dd) {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .settings :global(.row) {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    margin: 6px 0;
  }
  /* Fields sit one step below the panel, outlined by the faintest line. */
  .settings :global(.input) {
    font: inherit;
    height: var(--control);
    padding: 0 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    min-width: 0;
    flex: 1;
  }
  .settings :global(.narrow) {
    flex: 0 0 auto;
  }
  .settings :global(.login) {
    display: grid;
    grid-template-columns: max-content minmax(0, 420px);
    gap: 8px 12px;
    align-items: center;
    margin-top: 12px;
  }
  .settings :global(.login .row) {
    grid-column: 2;
  }
  /* The URL a sign-in printed: wrapping, selectable text one step below the panel. */
  .settings :global(pre.url) {
    margin: 0 0 6px;
    padding: 6px 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    font: var(--text-small) / 1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .settings :global(.warn) {
    color: var(--yellow);
  }
  .settings :global(.notice) {
    color: var(--green);
  }
  .settings :global(.error) {
    color: var(--red);
  }
</style>
