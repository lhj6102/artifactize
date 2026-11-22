<script lang="ts">
  // An Artifact Viewer window (J-VIEWER, D-VIEW-1): the app's strip along its top (the Artifact,
  // the viewer, the active review and Point) and under it the viewer: a built-in one (File
  // Explorer, Markdown, Artifactize Viewer) or a plugin's page in a sandboxed frame that reaches
  // only the viewer bridge (J-VIEWER-PLUGIN). Esc, or Point again, stops pointing.
  import { listen } from "@tauri-apps/api/event";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { call, message, onActiveReviewChanged, subscribe } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import { ARM, ASK, ViewerState, setViewerState } from "./viewer-state.svelte";
  import FilesViewer from "./FilesViewer.svelte";
  import MarkdownViewer from "./MarkdownViewer.svelte";
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import ArtifactizeViewer from "./ArtifactizeViewer.svelte";

  let { session }: { session: number } = $props();
  // A window shows one viewer session for its whole life.
  const v = new ViewerState(untrack(() => session));
  setViewerState(v);

  const entry = $derived(v.entry);
  const context = $derived(v.context);
  /** The plugin page's frame: the one source the bridge answers. */
  const frame = $derived(v.frame);
  const CHANNEL = "artifactize-viewer/v1";

  /** Answer one bridge call of the plugin's page, from its own frame only. */
  async function bridge(event: MessageEvent) {
    if (!frame || event.source !== frame.contentWindow) return;
    const data = event.data as { channel?: string; id?: number; method?: string; args?: { path?: string; text?: string } } | null;
    if (data?.channel !== CHANNEL || typeof data.id !== "number") return;
    // The page reached the bridge: it runs, whatever it asks first.
    v.page = "connected";
    const reply = (body: { result: unknown } | { error: string }) => frame?.contentWindow?.postMessage({ channel: CHANNEL, id: data.id, ...body }, "*");
    try {
      const args = data.args ?? {};
      let result: unknown;
      if (data.method === "context") result = await call(v.ipc.viewerContext(session));
      else if (data.method === "files") result = await call(v.ipc.viewerFiles(session, args.path ?? ""));
      else if (data.method === "readText") result = await call(v.ipc.viewerReadText(session, args.path ?? ""));
      else if (data.method === "pointing") {
        // A place pointed at in the page ends pointing, as a built-in viewer's click does, and
        // goes to the review pointing was started for; a refusal shows in the strip too.
        try {
          result = await v.append(args.text ?? "");
          v.error = null;
        } catch (failure) {
          v.error = message(failure);
          throw failure;
        } finally {
          tellPoint();
        }
      } else if (data.method === "cancelPoint") {
        point(false);
        result = null;
      } else if (data.method === "status") {
        v.status = args.text?.trim() || null;
        result = null;
      } else throw new Error(`The viewer bridge has no ${data.method}.`);
      reply({ result });
    } catch (failure) {
      reply({ error: failure instanceof Error ? failure.message : message(failure) });
    }
  }

  /** The plugin page's frame loads from the viewer scheme, under its own policy. */
  const page: Attachment<HTMLIFrameElement> = (element) => {
    v.frame = element;
    element.src = convertFileSrc(String(session), "viewer");
    return () => { if (v.frame === element) v.frame = undefined; };
  };

  /** The page finished loading: one that has not asked the bridge for anything yet did not
   * connect, until it does. */
  function loaded() {
    if (v.page === "loading") v.page = "silent";
  }

  /** Start or stop pointing; a plugin's page is told, and points in its own document. */
  function point(on: boolean) {
    v.arm(on);
    void v.announce();
    tellPoint();
  }

  function tellPoint() {
    if (entry?.kind === "web") frame?.contentWindow?.postMessage({ channel: CHANNEL, event: "point", armed: v.pointing }, "*");
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape" && v.pointing) {
      event.preventDefault();
      point(false);
    }
  }

  onMount(() => {
    void v.load();
    const unreview = onActiveReviewChanged(() => {
      void v.refresh().then(tellPoint);
      frame?.contentWindow?.postMessage({ channel: CHANNEL, event: "activeReviewChanged" }, "*");
    });
    // The pet overlay's Point points in this session only (the library matched it to the active
    // review's original request), and asks which viewers are open.
    const unarm = subscribe<{ session: number }>((handler) => listen(ARM, handler), (asked) => {
      if (!v.pointable || asked.session !== session || !context?.petMatch) return;
      point(!v.pointing);
      void getCurrentWindow().setFocus().catch(() => {});
    });
    const unask = subscribe<unknown>((handler) => listen(ASK, handler), () => void v.announce());
    const gone = () => void v.announce(false);
    window.addEventListener("beforeunload", gone);
    return () => {
      gone();
      window.removeEventListener("beforeunload", gone);
      unreview();
      unarm();
      unask();
    };
  });
</script>

<svelte:window onmessage={(event) => void bridge(event)} onkeydown={keydown} />

<div class="window" class:pointing={v.pointing}>
  <header class="strip">
    <span class="name">{context?.artifact ?? ""}</span>
    <span class="kind">{context?.title ?? ""}</span>
    {#if v.error}
      <span class="page silent" role="status">{v.error}</span>
    {:else if entry?.kind === "web" && v.page !== "connected"}
      <span class="page" class:silent={v.page === "silent"} role="status">
        {v.page === "loading" ? `Loading ${context?.title ?? "the viewer"}…` : `${context?.title ?? "The viewer"} did not connect: its page never asked the viewer for its context.`}
      </span>
    {:else if entry?.kind === "web" && v.status}
      <!-- The page's own latest word on what it shows, replaced by the next. -->
      <span class="page" role="status">{v.status}</span>
    {/if}
    <span class="spacer"></span>
    {#if context?.review}
      <span class="review"><span class="tone-human"><Icon name="human" /></span>{v.evalId ?? context.review}</span>
    {/if}
    {#if v.pointable}
      <button class="button small point" class:on={v.pointing} aria-pressed={v.pointing} onclick={() => point(!v.pointing)}><Icon name="point" />Point</button>
    {/if}
  </header>
  <div class="body">
    {#if entry?.kind === "web"}
      <iframe title="{context?.artifact ?? 'Artifact'} — {context?.title ?? 'viewer'}" sandbox="allow-scripts" onload={loaded} {@attach page}></iframe>
    {:else if entry?.kind === "builtin" && context}
      {#if entry.viewer === "files"}
        <FilesViewer />
      {:else if entry.viewer === "markdown"}
        <MarkdownViewer />
      {:else}
        <SvelteFlowProvider><ArtifactizeViewer /></SvelteFlowProvider>
      {/if}
    {/if}
  </div>
</div>

<style>
  .window {
    display: grid;
    grid-template-rows: var(--row-head) minmax(0, 1fr);
    height: 100%;
    background: var(--surface-app);
  }
  .pointing .body {
    cursor: crosshair;
  }
  /* The app's strip: the same for every viewer (D-VIEW-1). */
  .strip {
    display: flex;
    align-items: center;
    gap: var(--gap);
    min-height: var(--row-head);
    padding: 0 var(--pad);
    background: var(--surface-panel);
    border-bottom: 1px solid var(--line-panel);
    font-size: var(--text-small);
  }
  /* A long name is cut with an ellipsis; Point and the review keep their place (D-SYS-9). */
  .name,
  .kind {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    flex: 0 1 auto;
    font-weight: 600;
  }
  .kind {
    flex: 0 2 auto;
    color: var(--ink-2);
  }
  .strip > :global(button) {
    flex: none;
  }
  /* The plugin page's state until it reaches the bridge (D-VIEW-6). */
  .page {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  .page.silent {
    color: var(--red);
  }
  .spacer {
    flex: 1;
  }
  .review {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
    font-family: var(--mono);
  }
  .tone-human {
    display: inline-flex;
    color: var(--human);
  }
  /* Point is an on/off toggle: only its ink changes. */
  .point {
    color: var(--ink-2);
  }
  .point.on {
    color: var(--ink-bright);
  }
  .body {
    grid-row: 2;
    min-height: 0;
    min-width: 0;
  }
  iframe {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
  }
</style>
