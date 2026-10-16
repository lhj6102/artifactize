// The Monitor's breadcrumb, which follows the focused pane and node in a later render.

/** Wait until the breadcrumb names `name`. A timeout reports the breadcrumb, the Monitor's
 * focused pane and the graph's selected and focused nodes. */
export async function breadcrumbNames(browser: WebdriverIO.Browser, name: string): Promise<void> {
  const crumbs = () => browser.$('nav[aria-label="Breadcrumb"]').getText();
  try {
    await browser.waitUntil(async () => (await crumbs()).includes(name), { timeout: 15000, timeoutMsg: `the breadcrumb names ${name}` });
  } catch (error) {
    const state = await browser.execute(() => ({
      panes: [...document.querySelectorAll(".monitor section.panel")].map((pane) => `${pane.getAttribute("aria-label")}${pane.classList.contains("focused") ? " (focused)" : ""}`),
      selected: [...document.querySelectorAll(".svelte-flow__node.selected")].map((node) => node.getAttribute("data-id")),
      active: document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? document.activeElement?.tagName ?? null,
    }));
    throw new Error(`the breadcrumb names ${name}: ${await crumbs()}; ${JSON.stringify(state)}`, { cause: error });
  }
}
