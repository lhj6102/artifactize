// Component tests render inside the window's context, as the app does; the test may read and
// change that context's state.
import { render } from "@testing-library/svelte";
import type { Component } from "svelte";
import TestHost from "./TestHost.svelte";
import { AppState } from "./shell/app.svelte";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function renderHosted(child: Component<any>, props: Record<string, unknown> = {}) {
  const app = new AppState();
  const view = render(TestHost, { props: { child, childProps: props, app } });
  return {
    ...view,
    app,
    async rerender(next: Record<string, unknown>) {
      props = { ...props, ...next };
      await view.rerender({ childProps: props });
    },
  };
}
