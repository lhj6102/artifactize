// A Tool log as a person reads it, a review's or an Artifact's: each run's start line (its time
// as HH:MM:SS), what it printed, its end line, and which lines are red. Read-only.

export type LogRow = { text: string; kind: "start" | "line" | "end"; error: boolean };

/** The rows of the Tool log inside `scope`, and what its tools said: only a failure is said,
 * in red, beside them (`role="status"`); red is read as the painted colour. */
export function toolLog(browser: WebdriverIO.Browser, scope: string): Promise<{ notice: string; noticeError: boolean; rows: LogRow[] }> {
  return browser.execute((within) => {
    const root = document.querySelector(within);
    // Red as a person sees it: the painted colour's red clearly outweighs its green and blue,
    // whatever the theme's token says.
    const isRed = (element: Element | null | undefined) => {
      if (!element) return false;
      const [r, g, b] = (getComputedStyle(element).color.replace("srgb", "").match(/[\d.]+/g) ?? []).slice(0, 3)
        .map((part) => Number(part) <= 1 && part.includes(".") ? Number(part) * 255 : Number(part));
      return r > 150 && r > g * 1.6 && r > b * 1.6;
    };
    const notice = root?.querySelector('p[role="status"]:not(.busy)');
    const log = root?.querySelector('[role="log"][aria-label="Tool log"]');
    return {
      notice: notice?.textContent?.trim() ?? "",
      noticeError: isRed(notice),
      rows: [...(log?.querySelectorAll("p:not(.muted)") ?? [])].map((row) => ({
        text: (row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS "),
        kind: row.classList.contains("start") ? "start" : row.classList.contains("end") ? "end" : "line",
        error: isRed(row),
      })),
    };
  }, scope) as Promise<{ notice: string; noticeError: boolean; rows: LogRow[] }>;
}

/** The rows of the last run of `artifact`'s tool `operation`, from its start line on. */
export async function runOf(browser: WebdriverIO.Browser, scope: string, operation: string, artifact = "release"): Promise<LogRow[] | null> {
  const { rows } = await toolLog(browser, scope);
  const at = rows.findLastIndex((row) => row.kind === "start" && row.text === `▶ HH:MM:SS · ${operation} · ${artifact}`);
  if (at < 0) return null;
  const next = rows.findIndex((row, index) => index > at && row.kind === "start");
  return rows.slice(at, next < 0 ? rows.length : next);
}
