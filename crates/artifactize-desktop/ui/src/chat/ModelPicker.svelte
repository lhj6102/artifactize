<script lang="ts">
  // Target & Config Picker (J-CHAT-MODEL, D-CHAT-7): groups ready ACP Chat agents (with live
  // configOptions: model, mode with bypass warning, thought_level) above direct HTTP API
  // providers, ending with Manage chat agents and Connect provider.
  import type { Attachment } from "svelte/attachments";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";
  import { raisedRoom } from "../shell/raised-room";
  import Icon from "../shell/Icon.svelte";
  import { providerName } from "../settings/provider-names";
  import type { ChatConfigOption, ChatConfigSelection, ChatDirectBackend, ChatSession } from "../ipc/client";
  import { directBackendName, triggerSummary, type MenuAgent, type MenuProvider } from "./model-providers";

  let { session }: { session: ChatSession } = $props();
  const app = getAppState();
  const c = app.chat;
  const target = $derived(session.target);
  const liveOptions = $derived(c.view?.session.id === session.id ? c.view.liveConfigOptions : []);
  const activeAgentOptions = $derived.by(() => {
    if (target?.engine !== "acp") return [];
    if (liveOptions.length > 0) return liveOptions;
    const found = c.agents.find((item) => item.entry.id === target.agent);
    return found?.availability.kind === "ready" ? found.availability.configOptions : [];
  });
  const triggerLabel = $derived(
    target ? triggerSummary(target, c.agents.map((item) => item.entry), activeAgentOptions) : null,
  );
  const query = $derived(c.modelQuery.trim().toLowerCase());

  const filteredAgents = $derived.by(() =>
    c.agents.filter((item: MenuAgent) => {
      if (item.availability.kind === "missing" || item.availability.kind === "notSetUp") return false;
      if (!query) return true;
      if (
        item.entry.name.toLowerCase().includes(query) ||
        item.entry.id.toLowerCase().includes(query)
      ) {
        return true;
      }
      if (item.availability.kind === "ready") {
        return item.availability.configOptions.some((opt) =>
          opt.choices.some(
            (choice) =>
              choice.name.toLowerCase().includes(query) || choice.value.toLowerCase().includes(query),
          ),
        );
      }
      return false;
    }),
  );

  const unavailableAgents = $derived(
    c.agents
      .filter((item) => item.availability.kind === "missing" || item.availability.kind === "notSetUp")
      .map((item) => item.entry.name),
  );

  const groups = $derived.by(() =>
    c.providers.flatMap<MenuProvider & { name: string }>(({ provider, directBackend, listing }) => {
      if (listing.kind === "notSetUp") return [];
      const name = providerName(provider);
      const matchesProvider = name.toLowerCase().includes(query) || provider.toLowerCase().includes(query);
      if (listing.kind !== "models") return query ? [] : [{ provider, directBackend, name, listing }];
      const models = matchesProvider
        ? listing.models
        : listing.models.filter(({ slug, displayName }) =>
            [slug, displayName].some((value) => value.toLowerCase().includes(query)),
          );
      return models.length || (!query && matchesProvider)
        ? [{ provider, directBackend, name, listing: { kind: "models" as const, models } }]
        : [];
    }),
  );

  const efforts = $derived([
    ...new Set(
      c.providers.flatMap(({ listing }) =>
        listing.kind === "models" ? listing.models.flatMap((item) => item.efforts) : [],
      ),
    ),
  ]);

  const unconfigured = $derived(
    c.providers.filter(({ listing }) => listing.kind === "notSetUp").map(({ provider }) => providerName(provider)),
  );

  let trigger: HTMLButtonElement;

  const room = raisedRoom("above");
  const focus: Attachment<HTMLInputElement> = (input) => input.focus({ preventScroll: true });
  const dismiss: Attachment<HTMLElement> = (picker) => {
    if (!c.picking || app.mode !== "chat") return;
    const outside = (event: PointerEvent) => {
      if (!event.composedPath().includes(picker)) c.picking = false;
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || composing(event)) return;
      event.preventDefault();
      event.stopPropagation();
      c.picking = false;
      trigger.focus({ preventScroll: true });
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape, true);
    };
  };

  function agentOptionsFor(item: MenuAgent): ChatConfigOption[] {
    if (target?.engine === "acp" && target.agent === item.entry.id && liveOptions.length > 0) {
      return liveOptions;
    }
    return item.availability.kind === "ready" ? item.availability.configOptions : [];
  }

  function pickDirect(directBackend: ChatDirectBackend, model: string, effort: string | null) {
    trigger.focus({ preventScroll: true });
    void c.setTarget(session.id, { engine: "direct", backend: directBackend, model, effort });
  }

  function pickAgent(agentId: string, options: ChatConfigOption[]) {
    trigger.focus({ preventScroll: true });
    const existing = target?.engine === "acp" && target.agent === agentId ? (target.config ?? []) : [];
    const config: ChatConfigSelection[] =
      existing.length > 0
        ? existing
        : options
            .filter((opt) => opt.currentValue !== "")
            .map((opt) => ({ id: opt.id, value: opt.currentValue, category: opt.category }));
    void c.setTarget(session.id, { engine: "acp", agent: agentId, config });
  }

  function pickAgentOption(agentId: string, options: ChatConfigOption[], option: ChatConfigOption, value: string) {
    trigger.focus({ preventScroll: true });
    const base =
      target?.engine === "acp" && target.agent === agentId
        ? [...(target.config ?? [])]
        : options
            .filter((opt) => opt.currentValue !== "")
            .map((opt) => ({ id: opt.id, value: opt.currentValue, category: opt.category }));
    const next = base.filter((item) => item.id !== option.id);
    next.push({ id: option.id, value, category: option.category });
    void c.setTarget(session.id, { engine: "acp", agent: agentId, config: next });
  }

  function currentOptionValue(agentId: string, option: ChatConfigOption): string {
    if (target?.engine === "acp" && target.agent === agentId) {
      const saved = (target.config ?? []).find((item) => item.id === option.id);
      if (saved) return saved.value;
    }
    return option.currentValue;
  }
</script>

<div class="picker" {@attach dismiss}>
  <button
    class="model"
    bind:this={trigger}
    disabled={session.unavailable !== null}
    aria-haspopup="menu"
    aria-expanded={c.picking}
    aria-busy={c.choosing.has(session.id)}
    onclick={() => (c.picking ? (c.picking = false) : void c.openPicker())}
  >
    <span class="model-name" class:empty={!triggerLabel}>{triggerLabel ?? "Choose a model"}</span><Icon name="unfold" />
  </button>
  {#if c.picking && session.unavailable === null}
    <div
      class="menu"
      {@attach room}
      role="menu"
      aria-label="Chat agents and models"
      style:--effort-width={`${efforts.length * 52}px`}
    >
      <input
        class="filter"
        type="search"
        aria-label="Filter agents and models"
        placeholder="Filter agents and models"
        value={c.modelQuery}
        oninput={(event) => (c.modelQuery = event.currentTarget.value)}
        {@attach focus}
      />
      {#if c.providerError}<p class="note failed">{c.providerError}</p>{/if}

      {#if filteredAgents.length > 0}
        <div role="group" aria-label="Chat Agents">
          <p class="label">Chat Agents</p>
          {#each filteredAgents as item (item.entry.id)}
            {@const options = agentOptionsFor(item)}
            {@const selected = target?.engine === "acp" && target.agent === item.entry.id}
            {#if item.availability.kind === "loading"}
              <p class="note">{item.entry.name}: Probing…</p>
            {:else if item.availability.kind === "failed"}
              <p class="note failed">{item.entry.name}: {item.availability.error}</p>
            {:else if item.availability.kind === "tooOld"}
              <p class="note failed">{item.entry.name}: {item.availability.reason}</p>
            {:else}
              <div class="item-row" class:selected role="presentation">
                <button
                  class="item name"
                  role="menuitemradio"
                  aria-checked={selected}
                  disabled={c.choosing.has(session.id)}
                  onclick={() => pickAgent(item.entry.id, options)}
                >{item.entry.name}</button>
              </div>
              {#if options.length > 0}
                {#each options as option (option.id)}
                  {@const current = currentOptionValue(item.entry.id, option)}
                  <div class="config-row" role="group" aria-label="{item.entry.name} {option.name}">
                    <span class="config-label">{option.name}</span>
                    <div class="config-choices">
                      {#each option.choices as choice (choice.value)}
                        <button
                          class="item choice-chip"
                          class:bypass={choice.bypassWarning}
                          role="menuitemradio"
                          aria-checked={selected && current === choice.value}
                          disabled={c.choosing.has(session.id)}
                          onclick={() => pickAgentOption(item.entry.id, options, option, choice.value)}
                        >
                          {choice.name}{#if choice.bypassWarning} ⚠{/if}
                        </button>
                      {/each}
                    </div>
                  </div>
                {/each}
              {/if}
            {/if}
          {/each}
        </div>
      {/if}

      {#each groups as { provider, directBackend, name, listing } (provider)}
        <div role="group" aria-label={name}>
          <p class="label">{name}</p>
          {#if listing.kind === "failed"}
            <p class="note failed">{listing.error}</p>
          {:else if listing.kind === "cliMissing"}
            <p class="note">{listing.reason}</p>
          {:else if listing.kind === "cliTooOld"}
            <p class="note failed">{listing.error}</p>
          {:else if listing.kind === "loading"}
            <p class="note">Reading models…</p>
          {:else if listing.kind === "models"}
            {#each listing.models as item (item.slug)}
              {@const selected =
                target?.engine === "direct" &&
                directBackendName(target.backend) === provider &&
                target.model === item.slug}
              <div class="item-row" class:selected role="presentation">
                <button
                  class="item name"
                  role="menuitemradio"
                  aria-checked={selected}
                  disabled={c.choosing.has(session.id)}
                  onclick={() => pickDirect(directBackend, item.slug, null)}
                >{item.displayName}</button>
                {#each efforts as effort (effort)}
                  {#if item.efforts.includes(effort)}
                    <button
                      class="item effort"
                      role="menuitemradio"
                      aria-label={`${item.displayName} · ${effort}`}
                      aria-checked={selected && target?.engine === "direct" && target.effort === effort}
                      disabled={c.choosing.has(session.id)}
                      onclick={() => pickDirect(directBackend, item.slug, effort)}
                    >{effort}</button>
                  {:else}
                    <span class="effort" aria-hidden="true"></span>
                  {/if}
                {/each}
              </div>
            {:else}
              <p class="note">No models listed.</p>
            {/each}
          {/if}
        </div>
      {:else}
        {#if query && filteredAgents.length === 0}<p class="note">No agent or model matches.</p>{/if}
      {/each}

      <hr />
      <button
        class="item connect"
        role="menuitem"
        onclick={() => {
          c.picking = false;
          app.openSettings("chat-agents");
        }}
      ><Icon name="settings" />Manage chat agents</button>
      <button
        class="item connect"
        role="menuitem"
        onclick={() => {
          c.picking = false;
          app.openSettings("providers");
        }}
      ><Icon name="add" />Connect provider</button>
      {#if unavailableAgents.length}<p class="note setup">Agents not set up: {unavailableAgents.join(", ")}</p>{/if}
      {#if unconfigured.length}<p class="note setup">Providers not set up: {unconfigured.join(", ")}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .picker { position: relative; min-width: 0; }
  .model {
    display: flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    height: var(--control);
    padding: 0 8px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .model-name, .name { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .model-name.empty { color: var(--ink-2); }
  .model:hover { background: var(--surface-group); }
  /* D-CHAT-7: the menu opens into the space above the selector. */
  .menu {
    position: absolute;
    inset: auto auto 100% 0;
    z-index: 30;
    box-sizing: border-box;
    width: min(max(280px, calc(180px + var(--effort-width))), var(--raised-width, calc(100vw - 32px)));
    max-height: var(--raised-height, 65vh);
    overflow: auto;
    overscroll-behavior: contain;
    padding: 4px;
    background: var(--surface-raised);
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .filter {
    position: sticky;
    top: 0;
    z-index: 1;
    box-sizing: border-box;
    width: 100%;
    height: var(--control);
    padding: 0 8px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface-input);
    color: var(--ink);
    font: inherit;
  }
  .filter::placeholder { color: var(--ink-2); }
  .label, .note { margin: 0; padding: 4px 8px; color: var(--ink-2); }
  .label { padding-top: 8px; font-size: var(--text-label); font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; }
  .note { font-size: var(--text-small); overflow-wrap: anywhere; }
  .setup { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .failed { color: var(--red); }
  .item-row { display: flex; border-radius: var(--radius-control); }
  .item-row.selected { background: var(--selection); }
  .item {
    min-height: var(--row);
    margin: 0;
    border: 0;
    background: none;
    color: var(--ink-2);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:focus-visible { outline-offset: -2px; }
  .name { flex: 1; min-width: 0; padding: 0 8px; }
  .selected .name { color: var(--ink); }
  .effort { flex: 0 1 52px; min-width: 0; overflow: hidden; text-overflow: ellipsis; padding: 0; text-align: center; font-size: var(--text-small); }
  .effort[aria-checked="true"], .choice-chip[aria-checked="true"] { color: var(--ink); box-shadow: inset 0 -2px 0 var(--current-mark); }
  .item:hover:not(.selected .name) { background: var(--surface-group); }
  .config-row {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 2px 8px 4px 16px;
  }
  .config-label { font-size: var(--text-label); color: var(--ink-3); }
  .config-choices { display: flex; flex-wrap: wrap; gap: 4px; }
  .choice-chip {
    min-height: var(--row-dense);
    padding: 0 6px;
    border-radius: var(--radius-control);
    font-size: var(--text-small);
  }
  .choice-chip.bypass { color: var(--yellow); }
  .connect { display: flex; align-items: center; gap: 8px; width: 100%; padding: 0 8px; }
  hr { margin: 4px 0; border: 0; border-top: 1px solid var(--line-group); }
</style>
