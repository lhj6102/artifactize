<script lang="ts">
  // Renders a component inside the window's context, as App does, for the component tests.
  import type { Component } from "svelte";
  import { AppState, setAppState } from "./shell/app.svelte";

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let { child, childProps = {} }: { child: Component<any>; childProps?: Record<string, unknown> } = $props();
  import { setActivity } from "./ipc/activity.svelte";

  const app = new AppState();
  setAppState(app);
  setActivity(app.activity);
  const Child = $derived(child);
</script>

<Child {...childProps} />
