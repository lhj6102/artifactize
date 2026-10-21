// A Tool log as a person reads it, a review's or an Artifact's: each run's start line (its time
// as HH:MM:SS), what it printed, its end line, and which lines are red. Read-only.

export type LogRow = { text: string; kind: "start" | "line" | "end"; error: boolean };

/** The rows of the Tool log inside `scope`, and the notice of the tools beside it. */
export function toolLog(browser: WebdriverIO.Browser, scope: string): Promise<{ notice: string; noticeError: boolean; rows: LogRow[] }> {
  return browser.execute((within) => {
    const root = document.querySelector(within);
    const notice = root?.querySelector(".notice");
    const log = root?.querySelector('[role="log"][aria-label="Tool log"]');
    return {
      notice: notice?.textContent?.trim() ?? "",
      noticeError: notice?.classList.contains("error") ?? false,
      rows: [...(log?.querySelectorAll("p:not(.muted)") ?? [])].map((row) => ({
        text: (row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS "),
        kind: row.classList.contains("start") ? "start" : row.classList.contains("end") ? "end" : "line",
        error: row.classList.contains("error"),
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
