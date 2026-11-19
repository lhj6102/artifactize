// A Tool log as a person reads it, a review's or an Artifact's: each run's start line (its time
// as HH:MM:SS), what it printed, its end line, and which lines are red. Read-only.

export type LogRow = { text: string; kind: "start" | "line" | "end"; error: boolean };

/** The rows of the Tool log inside `scope`, and what its tools said: only a failure is said,
 * in red, beside them (`role="status"`); red is read as the painted colour. */
export function toolLog(browser: WebdriverIO.Browser, scope: string): Promise<{ notice: string; noticeError: boolean; rows: LogRow[] }> {
  return browser.execute((within) => {
    const root = document.querySelector(within);
    // Red is the painted colour, compared with the app's own red token.
    const token = getComputedStyle(document.documentElement).getPropertyValue("--red").trim();
    const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(token);
    const red = hex ? `rgb(${hex.slice(1).map((part) => Number.parseInt(part, 16)).join(", ")})` : token;
    const isRed = (element: Element | null | undefined) => !!element && getComputedStyle(element).color === red;
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
