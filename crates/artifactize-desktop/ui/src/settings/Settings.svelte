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

  let { active, onclose }: { active: boolean; onclose?: () => void } = $props();
  const s = new SettingsState(useIpc());
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
    const at = TABS.findIndex((tab) => tab.id === s.tab);
    s.tab = TABS[(at + (event.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length].id;
    document.getElementById(`settings-tab-${s.tab}`)?.focus();
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
              s.tab = "providers";
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
          aria-selected={s.tab === tab.id}
          aria-controls="settings-panel-{tab.id}"
          tabindex={s.tab === tab.id ? 0 : -1}
          onclick={() => (s.tab = tab.id)}
          onkeydown={tabKey}>{tab.label}</button
        >
      {/each}
    </div>
    <div class="spacer"></div>
    {#if onclose}<button class="button" onclick={onclose}>Close <kbd>Esc</kbd></button>{/if}
  </header>
  <p class="lead">The same settings as the CLI's <code>login</code>, <code>provider</code>, <code>models</code>, <code>remote</code> and <code>limits</code> commands.</p>
  {#if s.tab === "providers"}
    <div id="settings-panel-providers" role="tabpanel" aria-labelledby="settings-tab-providers">
      <ProvidersTab />
    </div>
  {:else if s.tab === "remote"}
    <div id="settings-panel-remote" role="tabpanel" aria-labelledby="settings-tab-remote">
      <RemoteStore />
    </div>
  {:else}
    <div id="settings-panel-limits" role="tabpanel" aria-labelledby="settings-tab-limits">
      <LimitsTab />
    </div>
  {/if}
</div>

<style>
  .settings {
    height: 100%;
    overflow: auto;
    padding: 4px 0 8px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }
  .spacer {
    flex: 1;
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }
  .tabs {
    display: flex;
    gap: 2px;
  }
  /* Text tabs: the active one in ink with an accent line under it. */
  .tab {
    font: inherit;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    padding: 4px 10px;
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  .tab:hover {
    background: var(--surface-group);
    color: var(--ink);
  }
  .tab[aria-selected="true"] {
    color: var(--ink);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }
  .settings :global(.lead),
  .settings :global(.muted) {
    color: var(--ink-2);
    margin: 8px 0 10px;
  }
  .settings :global(section) {
    border-top: 1px solid var(--line-group);
    padding: 10px 0;
  }
  /* A long name wraps inside its column. */
  .settings :global(h2),
  .settings :global(h3) {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .settings :global(h2) {
    font-size: var(--text-title);
    margin: 0 0 4px;
  }
  .settings :global(.facts) {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 8px 0 12px;
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
  .settings :global(.pending) {
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    padding: 8px 10px;
    margin: 8px 0;
    background: var(--surface-group);
  }
  .settings :global(.pending p) {
    margin: 0 0 6px;
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
  /* A table stays within its column: its cells share the width and wrap what is long. */
  .settings :global(table) {
    border-collapse: collapse;
    margin-top: 8px;
    width: 100%;
    table-layout: fixed;
  }
  .settings :global(th),
  .settings :global(td) {
    text-align: left;
    padding: 3px 16px 3px 0;
    overflow-wrap: anywhere;
  }
  .settings :global(th) {
    color: var(--ink-2);
    font-weight: 600;
    font-size: var(--text-label);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
</style>
