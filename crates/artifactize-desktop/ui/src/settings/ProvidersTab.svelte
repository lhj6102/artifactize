<script lang="ts">
  // The Providers tab: the built-in providers, then the custom ones, then "+ Custom provider" on
  // the left; the chosen one's setup and Models on the right. Keys are typed, saved and never
  // read back.
  import type { EndpointApi, Provider } from "../ipc/client";
  import CodexAccount from "./CodexAccount.svelte";
  import { getSettingsState, type Choice } from "./settings-state.svelte";

  const s = getSettingsState();

  const BUILTIN: Provider[] = ["codex", "openai", "anthropic", "gemini"];
  const NAMES: Record<Provider, string> = { codex: "Codex (ChatGPT)", openai: "OpenAI", anthropic: "Anthropic", gemini: "Gemini" };
  const APIS: Record<EndpointApi, string> = {
    openaiResponses: "OpenAI Responses",
    openaiChat: "OpenAI Chat Completions",
    anthropic: "Anthropic Messages",
    gemini: "Gemini",
  };
  const ROOTS: Record<EndpointApi, string> = {
    openaiResponses: "https://api.openai.com/v1",
    openaiChat: "http://localhost:11434/v1",
    anthropic: "https://api.anthropic.com",
    gemini: "https://generativelanguage.googleapis.com",
  };

  const same = (a: Choice, b: Choice) =>
    a.kind === b.kind &&
    (a.kind !== "builtin" || (b.kind === "builtin" && a.provider === b.provider)) &&
    (a.kind !== "custom" || (b.kind === "custom" && a.name === b.name));

  /** The chosen built-in API-key provider's key status. */
  const keyStatus = $derived.by(() => {
    const choice = s.choice;
    return choice.kind === "builtin" ? (s.providers?.keys.find((key) => key.provider === choice.provider) ?? null) : null;
  });
  const canSave = $derived(
    !s.saving && s.draft.name.trim() !== "" && (s.draft.kind === "alias" || s.draft.url.trim() !== ""),
  );

  function saveKey(event: SubmitEvent, provider: Provider) {
    event.preventDefault();
    void s.saveKey(provider);
  }

  function saveCustom(event: SubmitEvent) {
    event.preventDefault();
    void s.saveCustom();
  }
</script>

{#snippet item(choice: Choice, label: string)}
  <li>
    <button class="choice" aria-current={same(s.choice, choice) ? "true" : undefined} onclick={() => s.choose(choice)}>{label}</button>
  </li>
{/snippet}

<div class="providers">
  <nav aria-label="Providers">
    <ul>
      {#each BUILTIN as provider (provider)}
        {@render item({ kind: "builtin", provider }, NAMES[provider])}
      {/each}
      {#each s.providers?.custom ?? [] as one (one.name)}
        {@render item({ kind: "custom", name: one.name }, one.name)}
      {/each}
      {@render item({ kind: "new" }, "+ Custom provider")}
    </ul>
  </nav>

  <div class="setup">
    {#if s.choice.kind === "builtin" && s.choice.provider === "codex"}
      <CodexAccount />
    {:else if s.choice.kind === "builtin"}
      {@const provider = s.choice.provider}
      <section aria-labelledby="key-title">
        <h2 id="key-title">{NAMES[provider]} API key</h2>
        <p class="muted">
          A key in <code>{keyStatus?.variable}</code> is used first; otherwise the key saved here, as <code>artifactize login {provider}</code> saves it. The key is never shown.
        </p>
        <dl class="facts" aria-label="{NAMES[provider]} key">
          <dt>Key</dt>
          <dd data-testid="key-{provider}">
            {#if !keyStatus}
              …
            {:else if keyStatus.source === "environment"}
              Set from {keyStatus.variable}
            {:else if keyStatus.source === "saved"}
              Saved
            {:else}
              Not set
            {/if}
          </dd>
        </dl>
        <form class="fields" onsubmit={(event) => saveKey(event, provider)}>
          <label for="provider-key">API key</label>
          <input id="provider-key" class="input" type="password" bind:value={s.key} autocomplete="off" spellcheck="false" />
          <div class="row">
            <button class="button primary" type="submit" disabled={s.saving || !s.key.trim()}>Save key</button>
            {#if keyStatus?.source === "saved"}
              <button class="button" type="button" disabled={s.saving} onclick={() => void s.removeKey(provider)}>Remove saved key</button>
            {/if}
          </div>
        </form>
      </section>
    {:else}
      <section aria-labelledby="custom-title">
        <h2 id="custom-title">{s.choice.kind === "custom" ? s.choice.name : "New custom provider"}</h2>
        <p class="muted">
          An eval uses a custom provider by its name in <code>backend</code>. It is set up on this machine only, as <code>artifactize provider add</code> does.
        </p>
        <form class="fields" onsubmit={saveCustom} aria-label="Custom provider">
          <label for="custom-name">Name</label>
          <input
            id="custom-name"
            class="input"
            bind:value={s.draft.name}
            readonly={s.choice.kind === "custom"}
            placeholder="my-provider"
            autocomplete="off"
            spellcheck="false"
          />
          <span class="label">Reaches</span>
          <div class="row" role="radiogroup" aria-label="Reaches">
            <label><input type="radio" name="custom-kind" value="alias" bind:group={s.draft.kind} /> a built-in provider</label>
            <label><input type="radio" name="custom-kind" value="endpoint" bind:group={s.draft.kind} /> an endpoint</label>
          </div>
          {#if s.draft.kind === "alias"}
            <label for="custom-provider">Provider</label>
            <select id="custom-provider" class="input" bind:value={s.draft.provider}>
              {#each BUILTIN as provider (provider)}
                <option value={provider}>{NAMES[provider]}</option>
              {/each}
            </select>
          {:else}
            <label for="custom-api">API</label>
            <select id="custom-api" class="input" bind:value={s.draft.api}>
              {#each Object.entries(APIS) as [value, name] (value)}
                <option {value}>{name}</option>
              {/each}
            </select>
            <label for="custom-url">URL</label>
            <input id="custom-url" class="input" bind:value={s.draft.url} placeholder={ROOTS[s.draft.api]} autocomplete="off" spellcheck="false" />
            <label for="custom-key">Key</label>
            <input
              id="custom-key"
              class="input"
              type="password"
              bind:value={s.draft.key}
              disabled={s.draft.noKey}
              placeholder={s.draft.saved ? "Saved; type to replace" : "Optional"}
              autocomplete="off"
              spellcheck="false"
            />
            <span></span>
            <label><input type="checkbox" bind:checked={s.draft.noKey} /> No key</label>
          {/if}
          <div class="row">
            <button class="button primary" type="submit" disabled={!canSave}>Save</button>
            {#if s.choice.kind === "custom"}
              {@const name = s.choice.name}
              <button class="button" type="button" disabled={s.saving} onclick={() => void s.removeCustom(name)}>Remove provider</button>
            {/if}
          </div>
        </form>
      </section>
    {/if}
    {#if s.providersNotice}<p class="notice" role="status">{s.providersNotice}</p>{/if}
    {#if s.providersError}<p class="error" role="alert">{s.providersError}</p>{/if}

    {#if s.backend}
      <section aria-labelledby="models-title">
        <h2 id="models-title">Models</h2>
        <p class="muted">What <code>{s.backend}</code> offers to your key or account, as <code>artifactize models {s.backend}</code> lists them.</p>
        <div class="row">
          <button class="button" disabled={s.listing} onclick={() => void s.listModels()}>{s.listing ? "Listing…" : "List models"}</button>
        </div>
        {#if s.modelsError}<p class="error" role="alert">{s.modelsError}</p>{/if}
        {#if s.models}
          {#if s.models.length === 0}
            <p class="muted">No models listed.</p>
          {:else}
            <table aria-label="Models">
              <thead><tr><th>Model</th><th>Name</th></tr></thead>
              <tbody>
                {#each s.models as model (model.slug)}
                  <tr><td><code>{model.slug}</code></td><td>{model.displayName}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  .providers {
    display: grid;
    grid-template-columns: minmax(140px, 180px) minmax(0, 1fr);
    gap: 16px;
    min-height: 0;
  }
  nav ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .choice {
    width: 100%;
    text-align: left;
    font: inherit;
    color: var(--text);
    background: none;
    border: 0;
    border-radius: 6px;
    padding: 6px 8px;
    cursor: pointer;
    overflow-wrap: anywhere;
  }
  .choice:hover {
    background: var(--surface-2);
  }
  .choice[aria-current="true"] {
    background: var(--surface-2);
    font-weight: 600;
  }
  .setup {
    min-width: 0;
  }
  .fields {
    display: grid;
    grid-template-columns: max-content minmax(0, 420px);
    gap: 8px 12px;
    align-items: center;
    margin-top: 8px;
  }
  .fields .row {
    grid-column: 2;
  }
</style>
