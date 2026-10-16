<script lang="ts">
  // The Detail summary of one node of the Run, floating beside it: its one-line outcome, what it
  // waits for, and a running Agent's conversation as it goes.
  import type { Attachment } from "svelte/attachments";
  import { call, useIpc, message } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { SummaryState, setSummaryState } from "./summary-state.svelte";
  import Sections from "./Sections.svelte";
  import Transcript from "./Transcript.svelte";
  import { beside } from "./float";
  import { tone } from "../shell/tone";

  let {
    run,
    node,
    anchor,
    hint,
  }: {
    run: string;
    node: string;
    /** Finds the element the summary floats beside, in the summary's offset parent. */
    anchor: string;
    /** What opens more of the node. */
    hint: string;
  } = $props();
  const commands = useIpc();
  const s = new SummaryState();
  setSummaryState(s);
  const { browse } = getAppState();

  /** Read again for every accepted snapshot; the Detail comes from the backend's copy. */
  const read: Attachment<HTMLElement> = () => {
    void browse.revision;
    const [at, target] = [run, node];
    let current = true;
    call(commands.detail(at, target)).then(
      (view) => {
        if (!current) return;
        s.detail = view;
        s.error = null;
      },
      (failure) => {
        if (current) s.error = message(failure);
      },
    );
    return () => {
      current = false;
    };
  };

  /** Only this node's Detail: another node's, still shown while this one is read, is not. */
  const detail = $derived(s.detail?.run === run && s.detail.node === node ? s.detail : null);
  const agent = $derived(detail?.profile === "agent" && detail.request !== null);
</script>

<aside class="summary" aria-label="Summary" {@attach read} {@attach beside(anchor)}>
  {#if s.error}
    <p class="error">{s.error}</p>
  {:else if detail}
    <strong class="title">{detail.title}</strong>
    {#if detail.summary}<p class="line {tone(detail.mark?.status)}">{detail.summary}</p>{/if}
    <Sections sections={detail.sections} peek />
    {#if agent && detail.running && detail.request}
      {#key detail.request}
        <Transcript id={detail.request} peek />
      {/key}
    {/if}
    <p class="hint">{hint}</p>
  {:else}
    <p class="hint">Loading…</p>
  {/if}
</aside>

<style>
  .summary {
    position: absolute;
    z-index: 15;
    width: min(380px, calc(100% - 16px));
    max-height: 50%;
    overflow: hidden;
    padding: 10px 0 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 24px rgb(0 0 0 / 18%);
    /* It describes what is under the pointer; it never takes the pointer from it. */
    pointer-events: none;
  }
  .title,
  .line,
  .hint,
  .error {
    display: block;
    margin: 0 0 8px;
    padding: 0 var(--pad);
  }
  .line {
    font-weight: 500;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
  }
  .error {
    color: var(--red);
  }
</style>
