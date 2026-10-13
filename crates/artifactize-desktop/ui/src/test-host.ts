// Component tests render inside the window's context, as the app does.
import { render } from "@testing-library/svelte";
import type { Component } from "svelte";
import TestHost from "./TestHost.svelte";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function renderHosted(child: Component<any>, props: Record<string, unknown> = {}) {
  return render(TestHost, { props: { child, props } });
}
