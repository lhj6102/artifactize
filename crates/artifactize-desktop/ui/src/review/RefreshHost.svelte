<script lang="ts">
  // The window's real observation boundary around the Review screen and the command palette,
  // for the manual-refresh witness tests: exposed as App exposes it, read as E2E reads it.
  import { onMount } from "svelte";
  import type { LaunchView } from "../ipc/client";
  import { exposeActivity, setActivity } from "../ipc/activity.svelte";
  import { AppState, setAppState } from "../shell/app.svelte";
  import Palette from "../shell/Palette.svelte";
  import ReviewScreen from "./ReviewScreen.svelte";

  let { launch }: { launch: LaunchView } = $props();
  const app = new AppState();
  setAppState(app);
  setActivity(app.activity);
  onMount(() => exposeActivity(app.activity));
</script>

<button onclick={() => (app.palette = true)}>Commands</button>
<ReviewScreen {launch} active />
<Palette bind:open={app.palette} />
