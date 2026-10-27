<script lang="ts">
  // Renders a component inside the window's context, as App does, for the component tests.
  import { untrack, type Component } from "svelte";
  import { AppState, setAppState } from "./shell/app.svelte";

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let { child, childProps = {}, app }: { child: Component<any>; childProps?: Record<string, unknown>; app: AppState } = $props();
  import { setActivity } from "./ipc/activity.svelte";

  // The window's state, the test's to read and change: the host's for its whole life, as App's.
  const state = untrack(() => app);
  setAppState(state);
  setActivity(state.activity);
  const Child = $derived(child);
</script>

<Child {...childProps} />
