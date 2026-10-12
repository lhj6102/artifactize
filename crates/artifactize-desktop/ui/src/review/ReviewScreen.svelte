<script lang="ts">
  // The standalone review: the waiting Human sign-offs and the shared Human Detail of one.
  import { ContextMenu, Dialog } from "bits-ui";
  import { PaneGroup, Pane, PaneResizer } from "paneforge";
  import { onMount, tick } from "svelte";
  import { call, commands, message, onStateChanged, type HumanView, type LaunchView, type WaitingRow } from "../ipc/client";
  import { Review } from "./review.svelte";
  import HumanDetail from "./HumanDetail.svelte";
  import Panel from "../shell/Panel.svelte";
  import { useCommands } from "../shell/commands.svelte";
  import { chord, plain } from "../shell/keys";
  import { tone } from "../shell/tone";

  let { launch, active }: { launch: LaunchView; active: boolean } = $props();

  // The launch decides the first list; the toggle changes it afterwards.
  // svelte-ignore state_referenced_locally
  let all = $state(launch.repository === null);
  let rows = $state<WaitingRow[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let index = $state(0);
  let review = $state<Review | null>(null);
  let focus = $state<"list" | "detail">("list");
  let preview = $state<HumanView | null>(null);
  let asking = $state(false);
  let wanted = $state("");
  let list = $state<HTMLElement>();
  let notice = $state<string | null>(null);

  const selected = $derived(rows[index] ?? null);

  async function load(reload = true) {
    try {
      const next = await call(commands.humanWaiting(all, reload));
      const id = selected?.id;
      rows = next;
      loaded = true;
      error = null;
      const kept = id ? next.findIndex((row) => row.id === id) : -1;
      index = kept >= 0 ? kept : Math.min(index, Math.max(next.length - 1, 0));
    } catch (failure) {
      error = message(failure);
    }
  }

  onMount(() => {
    void load();
    if (launch.request) open(launch.request);
    const clock = setInterval(() => void load(false), 1000);
    const stop = onStateChanged(() => void load());
    return () => {
      clearInterval(clock);
      stop();
    };
  });

  $effect(() => {
    // The read-only Preview of the selected request; opening it claims nothing.
    const id = selected?.id;
    if (!id || focus !== "list") return;
    let current = true;
    call(commands.humanOpen(id, false)).then(
      (view) => current && (preview = view),
      () => current && (preview = null),
    );
    return () => {
      current = false;
    };
  });

  /** Open a request; another request than the open one starts from a clean review. */
  function open(id: string, tools = false) {
    if (review?.id !== id) review = new Review(id, true);
    if (tools) review.area = "tools";
    focus = "detail";
    notice = null;
  }

  async function back() {
    focus = "list";
    await tick();
    list?.focus();
    void load();
  }

  async function submitted() {
    await load();
    review = null;
    await back();
    if (rows.length === 0) notice = "No Human reviews are waiting.";
  }

  /** Screen-wide: Ctrl+O works wherever focus is while the Review screen shows. */
  function global(event: KeyboardEvent) {
    if (active && chord(event, "o")) {
      event.preventDefault();
      asking = true;
    }
  }

  $effect(() => {
    // Showing the screen puts focus on its list, so its keys work at once.
    if (active && focus === "list") void tick().then(() => list?.focus());
  });

  function keydown(event: KeyboardEvent) {
    if (focus !== "list") return;
    if (plain(event, "ArrowDown") || plain(event, "j")) index = Math.min(index + 1, rows.length - 1);
    else if (plain(event, "ArrowUp") || plain(event, "k")) index = Math.max(index - 1, 0);
    else if ((plain(event, "Enter") || plain(event, "ArrowRight")) && selected) open(selected.id);
    else if (plain(event, "r")) void load();
    else return;
    event.preventDefault();
  }

  $effect(() => {
    list?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  });

  useCommands(() =>
    active
      ? [
          { id: "review.open-id", group: "Review", label: "Open a request by ID", keys: "Ctrl+O", run: () => (asking = true) },
          { id: "review.refresh", group: "Review", label: "Refresh waiting reviews", keys: "r", run: () => void load() },
          {
            id: "review.scope",
            group: "Review",
            label: all ? "Show this repository's reviews" : "Show every repository's reviews",
            enabled: () => launch.repository !== null,
            run: () => {
              all = !all;
              void load();
            },
          },
          {
            id: "review.tools",
            group: "Review",
            label: "Open the selected request with Tools focused",
            enabled: () => selected !== null,
            run: () => selected && open(selected.id, true),
          },
        ]
      : [],
  );
</script>

<svelte:window onkeydown={global} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="review" onkeydown={keydown}>
  <nav class="crumbs" aria-label="Breadcrumb">
    <span class="root">artifactize review</span>
    <span class="sep">›</span>
    <span>{all ? "all repositories" : (launch.repository ?? "")}</span>
    {#if focus === "detail" && review?.view}
      <span class="sep">›</span><span>{review.view.title.split(" · ")[0]}</span>
    {/if}
    {#if error || notice}<span class="notice" class:error={!!error}>{error ?? notice}</span>{/if}
  </nav>
  <PaneGroup direction="horizontal" autoSaveId="review-columns">
    <Pane defaultSize={38} minSize={20}>
      <Panel title="Waiting Human reviews ({rows.length})" focused={focus === "list"} onfocus={() => (focus = "list")}>
        {#snippet actions()}
          {#if launch.repository !== null}
            <button class="button small" aria-pressed={all} onclick={() => ((all = !all), void load())}>
              {all ? "All repositories" : "This repository"}
            </button>
          {/if}
          <button class="button small" onclick={() => (asking = true)}>Open ID <kbd>Ctrl+O</kbd></button>
        {/snippet}
        {#if rows.length === 0}
          <p class="empty">{loaded ? "No Human reviews are waiting." : "Loading…"}</p>
        {:else}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              {#snippet child({ props })}
                <div {...props} class="waiting">
                  <div class="head" aria-hidden="true">
                    <span>Eval</span><span>Request</span><span>Claim</span><span>Waiting</span><span>Repository</span>
                  </div>
                  <ul role="listbox" aria-label="Waiting Human reviews" tabindex="0" bind:this={list}>
                    {#each rows as row, at (row.id)}
                      <li
                        role="option"
                        aria-selected={at === index}
                        class:selected={at === index}
                        data-index={at}
                        onclick={() => (at === index && focus === "list" ? open(row.id) : ((index = at), (focus = "list")))}
                        oncontextmenu={() => (index = at)}
                        ondblclick={() => open(row.id)}
                        onkeydown={() => {}}
                      >
                        <span><span class={tone("WAITING_HUMAN")}>?</span> {row.eval}</span>
                        <span class="mono">{row.id}</span>
                        <span>{row.claim}</span>
                        <span>{row.waiting}</span>
                        <span class="repo">{row.repository}</span>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/snippet}
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Content class="menu">
                <ContextMenu.Item class="menu-item" onSelect={() => selected && open(selected.id)}>Open review <kbd>Enter</kbd></ContextMenu.Item>
                <ContextMenu.Item class="menu-item" onSelect={() => selected && open(selected.id, true)}>Open with Tools focused</ContextMenu.Item>
              </ContextMenu.Content>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        {/if}
      </Panel>
    </Pane>
    <PaneResizer class="resizer" />
    <Pane defaultSize={62} minSize={30}>
      <Panel title={focus === "detail" ? "Review" : "Preview"} focused={focus === "detail"} onfocus={() => review && (focus = "detail")}>
        {#if focus === "detail" && review}
          {#key review}
            <HumanDetail {review} {active} onback={() => void back()} onsubmitted={() => void submitted()} />
          {/key}
        {:else if preview && selected}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="preview" role="button" tabindex="-1" onclick={() => open(selected.id)}>
            <h3>{preview.title}</h3>
            <p class="meta">{preview.meta}</p>
            {#if preview.shared}<p class="shared">{preview.shared}</p>{/if}
            <p class="instruction">{preview.instruction}</p>
            <p>
              <strong>Tools</strong>
              {preview.tools.length === 0
                ? "none declared in this eval's scope"
                : preview.tools.map((tool) => `${tool.name} (${tool.kind})`).join(" · ")}
            </p>
            <p class="hint">Enter or click: open review</p>
          </div>
        {:else}
          <p class="empty">Select a waiting review.</p>
        {/if}
      </Panel>
    </Pane>
  </PaneGroup>
</div>

<Dialog.Root bind:open={asking}>
  <Dialog.Portal>
    <Dialog.Overlay class="overlay" />
    <Dialog.Content class="dialog">
      <Dialog.Title class="dialog-title">Open a request by ID</Dialog.Title>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (wanted.trim()) {
            open(wanted.trim());
            asking = false;
            wanted = "";
          }
        }}
      >
        <input class="input" placeholder="req-…" bind:value={wanted} aria-label="Request ID" />
        <div class="dialog-actions">
          <Dialog.Close class="button">Cancel</Dialog.Close>
          <button class="button primary" type="submit">Open</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .review {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100%;
  }
  .crumbs {
    display: flex;
    gap: 8px;
    padding: 8px var(--pad);
    border-bottom: 1px solid var(--border);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
  }
  .root,
  .sep {
    color: var(--muted);
  }
  .notice {
    margin-left: auto;
    color: var(--muted);
    font-weight: 400;
  }
  .notice.error {
    color: var(--red);
  }
  .empty,
  .hint {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .waiting {
    --columns: minmax(120px, 1.2fr) minmax(120px, 1fr) minmax(80px, 0.8fr) 70px minmax(80px, 1fr);
  }
  .head,
  li {
    display: grid;
    grid-template-columns: var(--columns);
    gap: 8px;
    padding: 5px var(--pad);
    white-space: nowrap;
  }
  .head span,
  li span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .head {
    font-size: 11px;
    color: var(--muted);
    font-weight: 600;
    padding-top: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    outline: none;
  }
  li {
    cursor: pointer;
  }
  li:hover {
    background: var(--surface-2);
  }
  .selected {
    background: var(--selection);
  }
  ul:focus-visible .selected {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .mono {
    font: 11px var(--mono);
  }
  .repo {
    color: var(--muted);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .preview {
    padding: 0 var(--pad);
    cursor: pointer;
    outline: none;
  }
  .preview h3 {
    margin: 0 0 4px;
  }
  .meta {
    font-weight: 600;
    margin: 0 0 4px;
  }
  .shared {
    color: var(--human);
    margin: 0 0 4px;
  }
  .instruction {
    white-space: pre-wrap;
  }
  .input {
    width: 100%;
    font: inherit;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
  }
</style>
