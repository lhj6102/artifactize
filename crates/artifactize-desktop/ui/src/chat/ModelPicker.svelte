<script lang="ts">
  import type { Attachment } from "svelte/attachments";
  import { getAppState } from "../shell/app.svelte";
  import { composing } from "../shell/keys";
  import Icon from "../shell/Icon.svelte";
  import { providerName } from "../settings/provider-names";
  import type { ChatSession } from "../ipc/client";
  import type { MenuProvider } from "./model-providers";

  let { session }: { session: ChatSession } = $props();
  const app = getAppState();
  const c = app.chat;
  const model = $derived(session.selection);
  const label = $derived(c.providers.find(({ provider }) => provider === model.backend)?.listing);
  const modelName = $derived(label?.kind === "models" ? label.models.find((item) => item.slug === model.model)?.displayName ?? model.model : model.model);
  const query = $derived(c.modelQuery.trim().toLowerCase());
  const groups = $derived.by(() => c.providers.flatMap<MenuProvider & { name: string }>(({ provider, listing }) => {
    if (listing.kind === "unconfigured") return [];
    const name = providerName(provider);
    const matchesProvider = name.toLowerCase().includes(query) || provider.toLowerCase().includes(query);
    if (listing.kind !== "models") return matchesProvider ? [{ provider, name, listing }] : [];
    const models = matchesProvider ? listing.models : listing.models.filter(({ slug, displayName }) =>
      [slug, displayName].some((value) => value.toLowerCase().includes(query)));
    return models.length || (!query && matchesProvider) ? [{ provider, name, listing: { kind: "models" as const, models } }] : [];
  }));
  const efforts = $derived([...new Set(c.providers.flatMap(({ listing }) =>
    listing.kind === "models" ? listing.models.flatMap((item) => item.efforts) : []))]);
  const unconfigured = $derived(c.providers.filter(({ listing }) => listing.kind === "unconfigured").map(({ provider }) => providerName(provider)));
  let trigger: HTMLButtonElement;

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
    const available = () => {
      const { top, left } = picker.getBoundingClientRect();
      picker.style.setProperty("--picker-room", `${Math.max(0, top - 8)}px`);
      picker.style.setProperty("--picker-width", `${Math.max(0, window.innerWidth - left - 8)}px`);
    };
    available();
    window.addEventListener("resize", available);
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
    return () => {
      window.removeEventListener("resize", available);
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape, true);
    };
  };

  function pick(backend: string, model: string, effort: string | null) {
    trigger.focus({ preventScroll: true });
    void c.setModel(session.id, { backend, model, effort });
  }
</script>

<div class="picker" {@attach dismiss}>
  <button class="model" bind:this={trigger} disabled={session.unavailable !== null} aria-haspopup="menu" aria-expanded={c.picking} aria-busy={c.choosing.has(session.id)} onclick={() => c.picking ? c.picking = false : void c.openPicker()}>
    <span class="model-name">{modelName}</span>{#if model.effort}<span class="effort-label"> · {model.effort}</span>{/if}<Icon name="unfold" />
  </button>
  {#if c.picking && session.unavailable === null}
    <div class="menu" role="menu" aria-label="Models" style:--effort-width={`${efforts.length * 52}px`}>
      <input class="filter" type="search" aria-label="Filter models" placeholder="Filter models" value={c.modelQuery} oninput={(event) => c.modelQuery = event.currentTarget.value} {@attach focus} />
      {#if c.providerError}<p class="note failed">{c.providerError}</p>{/if}
      {#each groups as { provider, name, listing } (provider)}
        <div role="group" aria-label={name}>
          <p class="label">{name}</p>
          {#if listing.kind === "failed"}
            <p class="note failed">{listing.error}</p>
          {:else if listing.kind === "loading"}
            <p class="note">Reading models…</p>
          {:else if listing.kind === "models"}
            {#each listing.models as item (item.slug)}
              {@const selected = model.backend === provider && model.model === item.slug}
              <div class="item-row" class:selected role="presentation">
                <button class="item name" role="menuitemradio" aria-checked={selected} disabled={c.choosing.has(session.id)} onclick={() => pick(provider, item.slug, null)}>{item.displayName}</button>
                {#each efforts as effort (effort)}
                  {#if item.efforts.includes(effort)}
                    <button class="item effort" role="menuitemradio" aria-label={`${item.displayName} · ${effort}`} aria-checked={selected && model.effort === effort} disabled={c.choosing.has(session.id)} onclick={() => pick(provider, item.slug, effort)}>{effort}</button>
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
        {#if query}<p class="note">No model matches.</p>{/if}
      {/each}
      <hr />
      <button class="item connect" role="menuitem" onclick={() => { c.picking = false; app.openSettings(); }}><Icon name="add" />Connect provider</button>
      {#if unconfigured.length}<p class="note">Not set up: {unconfigured.join(", ")}</p>{/if}
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
  .effort-label { flex: none; color: var(--ink-2); }
  .model:hover { background: var(--surface-group); }
  /* D-CHAT-7: the menu opens into the space above the selector. */
  .menu {
    position: absolute;
    inset: auto auto 100% 0;
    z-index: 30;
    box-sizing: border-box;
    width: min(max(264px, calc(170px + var(--effort-width))), var(--picker-width, calc(100vw - 32px)));
    max-height: var(--picker-room, 65vh);
    overflow: auto;
    padding: 4px;
    background: var(--surface-raised);
    border: 1px solid var(--line-panel);
    border-radius: var(--radius-control);
    box-shadow: 0 2px 8px var(--scrim);
  }
  .filter {
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
  .effort { flex: 0 0 52px; padding: 0; text-align: center; font-size: var(--text-small); }
  .effort[aria-checked="true"] { color: var(--ink); box-shadow: inset 0 -2px 0 var(--current-mark); }
  .item:hover:not(.selected .name) { background: var(--surface-group); }
  .connect { display: flex; align-items: center; gap: 8px; width: 100%; padding: 0 8px; }
  hr { margin: 4px 0; border: 0; border-top: 1px solid var(--line-group); }
</style>
