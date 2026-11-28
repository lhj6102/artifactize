<script lang="ts">
  // Settings → Chat Agents (J-SET-CHAT-AGENTS, D-SET-9): built-in ACP presets (including
  // Antigravity's native Google sign-in), official ACP Registry discovery & pinned install
  // consent, machine-local custom ACP agents (write-only secrets), and saved Workspace Trust.
  import { Dialog } from "bits-ui";
  import type { ChatAgentCatalogEntry, ChatRegistryInstallSpec } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import AntigravityAccount from "./AntigravityAccount.svelte";
  import { getSettingsState, type ChatAgentChoice } from "./settings-state.svelte";

  const s = getSettingsState();

  const presets = $derived(s.chatAgents.filter((agent) => agent.tier === "preset"));
  const installed = $derived(s.chatAgents.filter((agent) => agent.tier !== "preset"));
  const selectedAgent = $derived(
    s.chatAgentChoice.kind === "agent"
      ? (s.chatAgents.find((agent) => agent.id === (s.chatAgentChoice as { kind: "agent"; id: string }).id) ?? null)
      : null,
  );
  const selectedProbe = $derived(selectedAgent ? (s.chatAgentProbes.get(selectedAgent.id) ?? null) : null);
  const selectedTrust = $derived(
    selectedAgent ? s.trustRecords.filter((record) => record.agentId === selectedAgent.id) : [],
  );

  function sameChoice(a: ChatAgentChoice, b: ChatAgentChoice): boolean {
    return a.kind === b.kind && (a.kind !== "agent" || (b.kind === "agent" && a.id === b.id));
  }

  function submitCustom(event: SubmitEvent) {
    event.preventDefault();
    void s.saveCustomAgent();
  }

  function openInstallConsent(spec: ChatRegistryInstallSpec) {
    s.registryConfirm = { ...spec };
    s.registryError = null;
  }

  function toggleRemoveSecret(key: string) {
    const existing = s.customAgentDraft.removeSecretKeys;
    if (existing.includes(key)) {
      s.customAgentDraft.removeSecretKeys = existing.filter((item) => item !== key);
    } else {
      s.customAgentDraft.removeSecretKeys = [...existing, key];
    }
  }

  function tierBadge(entry: ChatAgentCatalogEntry): string {
    if (entry.tier === "registry") return "Registry";
    if (entry.tier === "custom") return "Custom";
    return "Preset";
  }
</script>

{#snippet navItem(choice: ChatAgentChoice, label: string, icon?: "add" | "open")}
  <li>
    <button
      class="choice"
      aria-current={sameChoice(s.chatAgentChoice, choice) ? "true" : undefined}
      onclick={() => s.chooseChatAgent(choice)}
    >
      {#if icon}<Icon name={icon} />{/if}{label}
    </button>
  </li>
{/snippet}

<div class="agents">
  <nav aria-label="Chat agents">
    <ul>
      {#each presets as agent (agent.id)}
        {@render navItem({ kind: "agent", id: agent.id }, agent.name)}
      {/each}
      {#each installed as agent (agent.id)}
        {@render navItem({ kind: "agent", id: agent.id }, agent.name)}
      {/each}
      {@render navItem({ kind: "registry" }, "Browse Registry", "open")}
      {@render navItem({ kind: "new" }, "Custom agent", "add")}
    </ul>
  </nav>

  <div class="setup">
    {#if s.chatAgentChoice.kind === "agent" && selectedAgent}
      {#if selectedAgent.id === "antigravity"}
        <AntigravityAccount />
      {:else}
        <section aria-labelledby="agent-title">
          <div class="head-row">
            <h2 id="agent-title">{selectedAgent.name}</h2>
            <span class="tier">{tierBadge(selectedAgent)}</span>
          </div>
          <dl class="facts" aria-label="{selectedAgent.name} details">
            <dt>ID</dt>
            <dd class="mono">{selectedAgent.id}</dd>
            <dt>Command</dt>
            <dd class="mono">{[selectedAgent.command, ...selectedAgent.args].join(" ")}</dd>
            <dt>Verify bridge</dt>
            <dd>{selectedAgent.mcpStdio ? "One-turn Verify MCP tools enabled" : "Disabled"}</dd>
            <dt>Isolation</dt>
            <dd>{selectedAgent.nativeSandboxNote}</dd>
            {#if selectedAgent.secretEnvKeys.length > 0}
              <dt>Secret env keys</dt>
              <dd class="mono">{selectedAgent.secretEnvKeys.join(", ")}</dd>
            {/if}
            {#if selectedAgent.registry}
              <dt>Pinned version</dt>
              <dd class="mono">{selectedAgent.registry.exactVersion} ({selectedAgent.registry.distributionKind})</dd>
            {/if}
          </dl>
          {#if selectedAgent.tier !== "preset"}
            <div class="row">
              {#if selectedAgent.tier === "custom"}
                <button class="button" type="button" onclick={() => s.editCustomAgent(selectedAgent)}>Edit</button>
              {/if}
              <button
                class="button"
                type="button"
                disabled={s.customAgentSaving}
                onclick={() => void s.removeCustomAgent(selectedAgent.id)}
              >Remove agent</button>
            </div>
          {/if}
        </section>
      {/if}

      <section aria-labelledby="probe-title">
        <h2 id="probe-title">Status &amp; capabilities</h2>
        <div class="row">
          <button
            class="button"
            type="button"
            disabled={selectedProbe?.kind === "loading"}
            onclick={() => void s.probeChatAgent(selectedAgent.id, true)}
          >
            {selectedProbe?.kind === "loading" ? "Probing…" : "Probe agent"}
          </button>
        </div>
        {#if selectedProbe?.kind === "loading"}
          <p class="muted">Probing in an empty temporary directory…</p>
        {:else if selectedProbe?.kind === "ready"}
          <dl class="facts">
            <dt>Status</dt>
            <dd class="ready">Ready</dd>
            <dt>Executable</dt>
            <dd class="mono">{selectedProbe.executable}</dd>
            {#if selectedProbe.version}
              <dt>Version</dt>
              <dd class="mono">{selectedProbe.version}</dd>
            {/if}
          </dl>
          {#if selectedProbe.configOptions.length > 0}
            <ul class="configs" aria-label="Agent configuration options">
              {#each selectedProbe.configOptions as opt (opt.id)}
                <li class="config-group">
                  <div class="config-head">
                    <span class="config-name">{opt.name}</span>
                    <span class="mono">{opt.currentValue}</span>
                    <span class="muted">({opt.choices.length} choices)</span>
                  </div>
                  {#if opt.choices.length > 0}
                    <ul class="choices" aria-label={opt.category === "model" || opt.id === "model" ? "Models" : `${opt.name} choices`}>
                      {#each opt.choices as choice (choice.value)}
                        <li>{choice.name} <code>{choice.value}</code></li>
                      {/each}
                    </ul>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        {:else if selectedProbe?.kind === "missing"}
          <p class="muted">{selectedProbe.reason}</p>
        {:else if selectedProbe?.kind === "notSetUp"}
          <p class="muted">{selectedProbe.reason}</p>
        {:else if selectedProbe?.kind === "tooOld"}
          <p class="error" role="alert">{selectedProbe.reason}</p>
        {:else if selectedProbe?.kind === "failed"}
          <p class="error" role="alert">{selectedProbe.error}</p>
        {/if}
      </section>

      <section aria-labelledby="trust-title">
        <h2 id="trust-title">Trusted workspaces</h2>
        {#if selectedTrust.length === 0}
          <p class="muted">No workspaces trusted for {selectedAgent.name} yet. Artifactize asks before launching an ACP agent in a repository.</p>
        {:else}
          <ul class="trust-list">
            {#each selectedTrust as record (`${record.agentId}:${record.scopeLabel}`)}
              <li>
                <div class="trust-info">
                  <span class="mono">{record.scopeLabel}</span>
                  <span class="muted mono">{record.fingerprint.resolvedExecutable} · sha256:{record.fingerprint.executableSha256.slice(0, 12)}</span>
                </div>
                <button
                  class="button small"
                  type="button"
                  onclick={() => void s.revokeTrust(record.agentId, record.scope)}
                >Revoke</button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {:else if s.chatAgentChoice.kind === "registry"}
      <section aria-labelledby="registry-title">
        <h2 id="registry-title">Official ACP Registry</h2>
        <p class="muted">Browse verified ACP agents. Nothing is downloaded or executed until you confirm an exact pinned version.</p>
        <div class="row">
          <button class="button" type="button" disabled={s.registryLoading} onclick={() => void s.loadRegistry()}>
            {s.registryLoading ? "Loading…" : "Refresh catalog"}
          </button>
        </div>
        {#if s.registryError}<p class="error" role="alert">{s.registryError}</p>{/if}
        {#if s.registryEntries && s.registryEntries.length > 0}
          <ul class="registry-list">
            {#each s.registryEntries as entry (entry.id)}
              <li>
                <div class="registry-info">
                  <strong>{entry.name}</strong> <span class="mono muted">{entry.version}</span>
                  {#if entry.description}<p class="muted">{entry.description}</p>{/if}
                </div>
                <button class="button" type="button" onclick={() => openInstallConsent(entry.spec)}>Install…</button>
              </li>
            {/each}
          </ul>
        {:else if s.registryEntries && s.registryEntries.length === 0}
          <p class="muted">No Registry entries matched this platform.</p>
        {/if}
      </section>
    {:else}
      <section aria-labelledby="custom-agent-title">
        <h2 id="custom-agent-title">{s.customAgentDraft.editing ? `Edit ${s.customAgentDraft.id}` : "Custom ACP agent"}</h2>
        <form class="fields" onsubmit={submitCustom} aria-label="Custom ACP agent">
          <label for="agent-id">ID</label>
          <input
            id="agent-id"
            class="input"
            bind:value={s.customAgentDraft.id}
            readonly={s.customAgentDraft.editing}
            placeholder="my-acp-agent"
            autocomplete="off"
            spellcheck="false"
          />
          <label for="agent-name">Display name</label>
          <input
            id="agent-name"
            class="input"
            bind:value={s.customAgentDraft.name}
            placeholder="My ACP Agent"
            autocomplete="off"
            spellcheck="false"
          />
          <label for="agent-command">Command</label>
          <input
            id="agent-command"
            class="input mono"
            bind:value={s.customAgentDraft.command}
            placeholder="my-agent-acp"
            autocomplete="off"
            spellcheck="false"
          />
          <label for="agent-args">Arguments (one per line)</label>
          <textarea
            id="agent-args"
            class="input mono"
            rows="2"
            bind:value={s.customAgentDraft.argsText}
            placeholder="acp"
            spellcheck="false"
          ></textarea>
          <label for="agent-env">Env (KEY=VALUE per line)</label>
          <textarea
            id="agent-env"
            class="input mono"
            rows="2"
            bind:value={s.customAgentDraft.envText}
            placeholder="FOO=bar"
            spellcheck="false"
          ></textarea>
          <label for="agent-secret-key">Add secret env</label>
          <div class="secret-row">
            <input
              id="agent-secret-key"
              class="input mono"
              bind:value={s.customAgentDraft.secretKey}
              placeholder="API_TOKEN"
              autocomplete="off"
              spellcheck="false"
            />
            <input
              id="agent-secret-val"
              class="input"
              type="password"
              bind:value={s.customAgentDraft.secretValue}
              placeholder="Secret value (write-only)"
              autocomplete="off"
              spellcheck="false"
            />
          </div>
          {#if s.customAgentDraft.savedSecretKeys.length > 0}
            <span class="label">Saved secrets</span>
            <div class="saved-secrets">
              {#each s.customAgentDraft.savedSecretKeys as key (key)}
                <label class="mono">
                  <input
                    type="checkbox"
                    checked={s.customAgentDraft.removeSecretKeys.includes(key)}
                    onchange={() => toggleRemoveSecret(key)}
                  /> Remove {key}
                </label>
              {/each}
            </div>
          {/if}
          <span></span>
          <label><input type="checkbox" bind:checked={s.customAgentDraft.mcpStdio} /> Connect one-turn Verify MCP bridge</label>
          <div class="row">
            <button class="button primary" type="submit" disabled={s.customAgentSaving}>Save agent</button>
          </div>
        </form>
      </section>
    {/if}
    {#if s.chatAgentsError}<p class="error" role="alert">{s.chatAgentsError}</p>{/if}
  </div>
</div>

<!-- Full-spec Registry install consent dialog (D-OVL-8). -->
<Dialog.Root open={s.registryConfirm !== null} onOpenChange={(open) => { if (!open) s.registryConfirm = null; }}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog registry-consent">
      <Dialog.Title class="dialog-title">Install ACP agent from Registry</Dialog.Title>
      {#if s.registryConfirm}
        {@const spec = s.registryConfirm}
        <!-- A download is installed only against the SHA-256 shown here (D-SET-9, D-OVL-8). -->
        {@const undigested = !spec.artifactSha256 && (spec.distributionKind === "binary" || spec.distributionKind === "archive")}
        <form
          class="dialog-layout"
          onsubmit={(event) => {
            event.preventDefault();
            if (s.registryConfirm && !undigested) void s.installRegistry(s.registryConfirm);
          }}
        >
          <div class="dialog-body">
            <dl class="facts">
              <dt>Agent</dt>
              <dd>{spec.displayName} (<span class="mono">{spec.registryId}</span>)</dd>
              <dt>Pinned version</dt>
              <dd class="mono">{spec.exactVersion}</dd>
              <dt>Platform</dt>
              <dd class="mono">{spec.platform}</dd>
              <dt>Distribution</dt>
              <dd class="mono">{spec.distributionKind}</dd>
              <dt>Source</dt>
              <dd class="mono">{spec.packageOrUrl}</dd>
              {#if (spec.args ?? []).length > 0}
                <dt>Arguments</dt>
                <dd class="mono">{(spec.args ?? []).join(" ")}</dd>
              {/if}
              <dt>Environment</dt>
              <dd class="mono">
                {#if Object.keys(spec.envPairs ?? {}).length > 0}
                  {Object.entries(spec.envPairs ?? {})
                    .map(([key, value]) => `${key}=${value}`)
                    .join(", ")}
                {:else}
                  None
                {/if}
              </dd>
              <dt>SHA-256</dt>
              <dd class="mono">{spec.artifactSha256 ?? "Not published by Registry"}</dd>
            </dl>
            {#if spec.distributionKind === "npx" || spec.distributionKind === "uvx"}
              <label class="consent-opt">
                <input
                  type="checkbox"
                  checked={spec.buildScriptsAllowed}
                  onchange={(event) => {
                    if (s.registryConfirm) s.registryConfirm = { ...s.registryConfirm, buildScriptsAllowed: event.currentTarget.checked };
                  }}
                /> Allow package build scripts (off by default)
              </label>
            {/if}
            {#if undigested}
              <p class="consent-opt">The Registry publishes no SHA-256 for this download, so it cannot be installed.</p>
            {/if}
            {#if s.registryError}<p class="error" role="alert">{s.registryError}</p>{/if}
          </div>
          <div class="dialog-actions">
            <Dialog.Close class="button">Cancel</Dialog.Close>
            <button class="button primary" type="submit" disabled={s.registryInstalling || undigested}>
              {s.registryInstalling ? "Installing…" : "Install"}
            </button>
          </div>
        </form>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .agents {
    display: grid;
    grid-template-columns: 180px minmax(0, 1fr);
    gap: 20px;
    min-height: 0;
  }
  nav {
    border-right: 1px solid var(--line-group);
    padding-right: 12px;
  }
  nav ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: var(--row);
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .choice:hover { background: var(--surface-group); color: var(--ink); }
  .choice[aria-current="true"] { background: var(--selection); color: var(--ink); }
  .setup { display: flex; flex-direction: column; gap: 18px; min-width: 0; }
  section { display: flex; flex-direction: column; gap: 8px; }
  .head-row { display: flex; align-items: center; gap: 8px; }
  h2 { margin: 0; font-size: var(--text-body); font-weight: 600; }
  .tier {
    padding: 1px 6px;
    border-radius: var(--radius-control);
    background: var(--surface-group);
    color: var(--ink-2);
    font-size: var(--text-small);
  }
  .facts {
    display: grid;
    grid-template-columns: 112px minmax(0, 1fr);
    gap: 6px 12px;
    margin: 0;
    font-size: var(--text-small);
  }
  .facts dt { color: var(--ink-2); }
  .facts dd { margin: 0; overflow-wrap: anywhere; }
  .mono { font-family: var(--mono); }
  .ready { color: var(--green); font-weight: 500; }
  .muted { margin: 0; color: var(--ink-2); font-size: var(--text-small); }
  .error { margin: 0; color: var(--red); font-size: var(--text-small); }
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .configs, .trust-list, .registry-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .config-group { display: flex; flex-direction: column; gap: 4px; font-size: var(--text-small); }
  .config-head { display: flex; gap: 8px; }
  .choices {
    list-style: none;
    margin: 0;
    padding: 0 0 0 12px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .choices li { display: flex; gap: 8px; }
  .choices code { color: var(--ink-2); font-family: var(--mono); }
  .config-name { font-weight: 500; }
  .trust-list li, .registry-list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line-group);
    font-size: var(--text-small);
  }
  .trust-list li:last-child, .registry-list li:last-child { border-bottom: 0; }
  .trust-info, .registry-info { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .fields {
    display: grid;
    grid-template-columns: 128px minmax(0, 1fr);
    gap: 8px 12px;
    align-items: start;
    font-size: var(--text-small);
  }
  .fields label, .fields .label { padding-top: 6px; color: var(--ink-2); }
  .secret-row { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .saved-secrets { display: flex; flex-wrap: wrap; gap: 10px; }
  .consent-opt { display: flex; align-items: center; gap: 6px; margin-top: 8px; font-size: var(--text-small); }
</style>
