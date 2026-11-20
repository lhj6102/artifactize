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
  import ArtifactizeViewer from "./ArtifactizeViewer.svelte";

  let { session }: { session: number } = $props();
  // A window shows one viewer session for its whole life.
  const v = new ViewerState(untrack(() => session));
  setViewerState(v);

  const entry = $derived(v.entry);
  const context = $derived(v.context);
  /** The plugin page's frame: the one source the bridge answers. */
  let frame = $state<HTMLIFrameElement>();
  const CHANNEL = "artifactize-viewer/v1";

  /** Answer one bridge call of the plugin's page, from its own frame only. */
  async function bridge(event: MessageEvent) {
    if (!frame || event.source !== frame.contentWindow) return;
    const data = event.data as { channel?: string; id?: number; method?: string; args?: { path?: string; text?: string } } | null;
    if (data?.channel !== CHANNEL || typeof data.id !== "number") return;
    const reply = (body: { result: unknown } | { error: string }) => frame?.contentWindow?.postMessage({ channel: CHANNEL, id: data.id, ...body }, "*");
    try {
      const args = data.args ?? {};
      let result: unknown;
      if (data.method === "context") result = await call(v.ipc.viewerContext(session));
      else if (data.method === "files") result = await call(v.ipc.viewerFiles(session, args.path ?? ""));
      else if (data.method === "readText") result = await call(v.ipc.viewerReadText(session, args.path ?? ""));
      else if (data.method === "pointing") result = await call(v.ipc.viewerPoint(session, args.text ?? ""));
      else throw new Error(`The viewer bridge has no ${data.method}.`);
      reply({ result });
    } catch (failure) {
      reply({ error: failure instanceof Error ? failure.message : message(failure) });
    }
  }

  /** The plugin page's frame loads from the viewer scheme, under its own policy. */
  const page: Attachment<HTMLIFrameElement> = (element) => {
    frame = element;
    element.src = convertFileSrc(String(session), "viewer");
  };

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape" && v.pointing) {
      event.preventDefault();
      v.pointing = false;
    }
  }

  onMount(() => {
    void v.load();
    const unreview = onActiveReviewChanged(() => {
      void v.refresh();
      frame?.contentWindow?.postMessage({ channel: CHANNEL, event: "activeReviewChanged" }, "*");
    });
    // The pet overlay's Point points here, and asks which viewers are open.
    const unarm = subscribe<{ artifact: string }>((handler) => listen(ARM, handler), (asked) => {
      if (!v.pointable || asked.artifact !== context?.artifact) return;
      v.pointing = true;
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
    <span class="spacer"></span>
    {#if context?.review}
      <span class="review"><span class="tone-human"><Icon name="human" /></span>{context.review}</span>
    {/if}
    {#if v.pointable}
      <button class="button small" class:on={v.pointing} aria-pressed={v.pointing} onclick={() => (v.pointing = !v.pointing)}><Icon name="point" />Point</button>
    {/if}
  </header>
  {#if v.error}<p class="error" role="status">{v.error}</p>{/if}
  <div class="body">
    {#if entry?.kind === "web"}
      <iframe title="{context?.artifact ?? 'Artifact'} — {context?.title ?? 'viewer'}" sandbox="allow-scripts" {@attach page}></iframe>
    {:else if entry?.kind === "builtin" && context}
      {#if entry.viewer === "files"}
        <FilesViewer />
      {:else if entry.viewer === "markdown"}
        <MarkdownViewer />
      {:else}
        <ArtifactizeViewer />
      {/if}
    {/if}
  </div>
</div>

<style>
  .window {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    height: 100%;
    background: var(--surface-app);
  }
  .pointing {
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
  .name {
    font-weight: 600;
  }
  .kind {
    color: var(--ink-2);
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
  /* Pointing: Point reads as on, outlined in the accent (D-VIEW-2). */
  .on {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .error {
    margin: 0;
    padding: 4px var(--pad);
    color: var(--red);
    font-size: var(--text-small);
  }
  .body {
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
