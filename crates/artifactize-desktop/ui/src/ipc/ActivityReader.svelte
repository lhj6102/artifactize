<script lang="ts">
  import type { Attachment } from "svelte/attachments";
  import { getAppState } from "../shell/app.svelte";
  import { getActivity } from "./activity.svelte";
  import { call, message, useIpc } from "./client";

  let {
    label, query, onsubmit,
  }: { label: string; query: string; onsubmit: (completion: Promise<void>) => void } = $props();
  const app = getAppState();
  const activity = getActivity();
  const commands = useIpc();
  const snapshot = $derived(activity.snapshot());

  async function search() {
    try { await call(commands.browse({ run: null, close: false, query, older: null, star: null, full: false, resync: false })); }
    catch (error) { app.failure = message(error); }
  }

  function submit() {
    // Fixture observers receive only completion, after the normal UI catch has run.
    onsubmit(search());
  }

  // Like the real view attachments, dispatch must not depend on observation counters.
  const read: Attachment = () => { submit(); };
</script>

<section aria-label={label} {@attach read}>
  <output aria-label={`${label} activity`}>{JSON.stringify(snapshot)}</output>
  <button onclick={submit}>{label} search</button>
  <button onclick={() => void app.browse.navigate({ full: true })}>{label} browse</button>
  {#if app.failure}<p role="alert">{app.failure}</p>{/if}
</section>
