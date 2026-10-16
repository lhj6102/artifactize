<script lang="ts">
  // Exercise the real window ownership boundary with two independent context consumers.
  import { onMount } from "svelte";
  import { AppState, setAppState } from "../shell/app.svelte";
  import { exposeActivity, setActivity } from "./activity.svelte";
  import ActivityReader from "./ActivityReader.svelte";

  let {
    label, query, onsubmit = () => {},
  }: { label: string; query: string; onsubmit?: (completion: Promise<void>) => void } = $props();
  const app = new AppState();
  setAppState(app);
  setActivity(app.activity);
  onMount(() => exposeActivity(app.activity));
</script>

<ActivityReader label={`${label} first`} {query} {onsubmit} />
<ActivityReader label={`${label} second`} {query} {onsubmit} />
