<script lang="ts">
  // Settings: what the CLI configures, through the same library calls, in two tabs. Providers
  // holds the Codex (ChatGPT) sign-in, the built-in providers' keys, the custom providers and
  // the models each lists; Remote holds the shared review store. Secrets are never shown; a
  // typed key or token leaves the state as soon as it is submitted.
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { useIpc } from "../ipc/client";
  import { useCommands } from "../shell/commands.svelte";
  import ProvidersTab from "./ProvidersTab.svelte";
  import RemoteStore from "./RemoteStore.svelte";
  import { SettingsState, setSettingsState } from "./settings-state.svelte";

  let { active, onclose }: { active: boolean; onclose?: () => void } = $props();
  const s = new SettingsState(useIpc());
  setSettingsState(s);

  const TABS = [
    { id: "providers", label: "Providers" },
    { id: "remote", label: "Remote" },
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
    s.tab = s.tab === "providers" ? "remote" : "providers";
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
  <p class="lead">The same settings as the CLI's <code>login</code>, <code>provider</code>, <code>models</code> and <code>remote</code> commands.</p>
  {#if s.tab === "providers"}
    <div id="settings-panel-providers" role="tabpanel" aria-labelledby="settings-tab-providers">
      <ProvidersTab />
    </div>
  {:else}
    <div id="settings-panel-remote" role="tabpanel" aria-labelledby="settings-tab-remote">
      <RemoteStore />
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
    font-size: 18px;
  }
  .tabs {
    display: flex;
    gap: 4px;
  }
  .tab {
    font: inherit;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 3px 12px;
    background: none;
    color: var(--text);
    cursor: pointer;
  }
  .tab[aria-selected="true"] {
    background: var(--text);
    color: var(--bg);
    border-color: var(--text);
  }
  .settings :global(.lead),
  .settings :global(.muted) {
    color: var(--muted);
    margin: 8px 0 10px;
  }
  .settings :global(section) {
    border-top: 1px solid var(--border);
    padding: 14px 0;
  }
  .settings :global(h2) {
    font-size: 14px;
    margin: 0 0 4px;
  }
  .settings :global(.facts) {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 8px 0 12px;
  }
  .settings :global(dt) {
    color: var(--muted);
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
  .settings :global(.input) {
    font: inherit;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    min-width: 0;
    flex: 1;
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
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    margin: 8px 0;
    background: var(--surface-2);
  }
  .settings :global(.pending p) {
    margin: 0 0 6px;
  }
  .settings :global(.url code) {
    overflow-wrap: anywhere;
    font-size: 11px;
  }
  .settings :global(.warn) {
    color: var(--human);
  }
  .settings :global(.notice) {
    color: var(--green);
  }
  .settings :global(.error) {
    color: var(--red);
  }
  .settings :global(table) {
    border-collapse: collapse;
    margin-top: 8px;
  }
  .settings :global(th),
  .settings :global(td) {
    text-align: left;
    padding: 3px 16px 3px 0;
  }
  .settings :global(th) {
    color: var(--muted);
    font-weight: 600;
    font-size: 12px;
  }
</style>
