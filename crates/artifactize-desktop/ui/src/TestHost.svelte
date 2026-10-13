<script lang="ts">
  // Renders a component inside the window's context, as App does, for the component tests.
  import type { Component } from "svelte";
  import { AppState, setAppState } from "./shell/app.svelte";

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let { child, props = {} }: { child: Component<any>; props?: Record<string, unknown> } = $props();
  setAppState(new AppState());
  const Child = $derived(child);
</script>

<Child {...props} />
