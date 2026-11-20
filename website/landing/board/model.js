// The development board's data model: what GitHub's public REST API returns, turned into
// pull requests with states, a time-ordered event list and the lanes of the branch flow.
// Pure functions only; main.js fetches and draws.

/**
 * @typedef {'dev-worker' | 'dev-master'} Role
 * @typedef {'GREEN' | 'YELLOW'} VerifyOutcome
 * @typedef {{ run: string, outcome: VerifyOutcome, commit: string }} Verify
 * @typedef {{ run: string, at: Date }} GreenMark
 * @typedef {{ role: Role | null, from: string | null, verify: Verify | null, green: GreenMark[] }} Note
 * @typedef {'open' | 'draft' | 'merged' | 'closed'} PrState
 * @typedef {'passed' | 'failed' | 'running' | 'skipped' | 'none' | 'unknown'} CiState
 * @typedef {{
 *   number: number, title: string, url: string, branch: string, base: string,
 *   state: PrState, created: Date, ended: Date | null, mergeCommit: string | null,
 *   headSha: string, note: Note | null, ci: CiState, release: boolean
 * }} Pr
 * @typedef {{ tag: string, url: string, at: Date, fromDevMain: boolean }} Release
 * @typedef {{ sha: string, pr: Pr | null }} DevGreen
 * @typedef {{
 *   prs: Pr[], releases: Release[], devGreen: DevGreen | null, devHead: string | null,
 *   greens: { pr: Pr, mark: GreenMark }[]
 * }} Model
 */

/**
 * The parts of GitHub's REST responses the board reads.
 * @typedef {{
 *   number: number, title: string, html_url: string, state: string, draft?: boolean,
 *   body: string | null, created_at: string, closed_at: string | null, merged_at: string | null,
 *   merge_commit_sha: string | null, head: { ref: string, sha: string, repo?: { full_name: string } | null }, base: { ref: string }
 * }} ApiPull
 * @typedef {{ tag_name: string, html_url: string, published_at: string | null, draft: boolean }} ApiRelease
 * @typedef {{ ref: string, object: { sha: string, type: string } }} ApiRef
 * @typedef {{ name: string, status: string, conclusion: string | null }} ApiCheck
 * @typedef {{ checks: ApiCheck[], complete: boolean, suitePending?: boolean }} ApiCheckSet
 * @typedef {{
 *   pulls: ApiPull[], releases: ApiRelease[], tagRefs: ApiRef[], tagCommits: Record<string, { sha: string, at: string, kind?: 'tag' | 'commit' }>,
 *   devHead: string | null, checks: Record<string, ApiCheck[] | ApiCheckSet>, openTruncated?: boolean, historyTruncated?: boolean
 * }} Raw
 * `checks` holds the latest check runs and commit statuses (CI jobs) of the open pull requests
 * whose CI was fetched, by number. `tagCommits` holds the commit, timestamp, and object kind of
 * each resolved v* tag.
 */

/** The repository the board follows. */
export const REPO = 'lhj6102/artifactize';

/** The trailing board note dev-master and dev-worker end their pull request bodies with. */
export const NOTE = /<!--\s*artifactize-board(?=\s|$)((?:(?!-->|<!--)[\s\S])*?)-->\s*$/;
const FULL_SHA = /^[0-9a-f]{40}$/i;
const RUN = /^\S+$/;

/**
 * The trailing board note at the end of a pull request body (the skills end the body with it),
 * so a quoted example earlier in the description never produces a false note and an earlier
 * unterminated comment cannot mix fields into the real footer.
 * @param {string | null} body
 * @returns {string | null}
 */
export function lastNote(body) {
  if (!body) return null;
  const match = NOTE.exec(body);
  return match ? match[0].trimEnd() : null;
}

/**
 * Reads the trailing board note of a pull request body; null without one. Unknown or malformed
 * lines are skipped in linear time, so a hand-edited or very long note never breaks the board.
 * @param {string | null} body
 * @returns {Note | null}
 */
export function parseNote(body) {
  const text = lastNote(body);
  const match = text ? NOTE.exec(text) : null;
  if (!match) return null;
  /** @type {Note} */
  const note = { role: null, from: null, verify: null, green: [] };
  for (const line of match[1].split(/\r?\n/)) {
    const colon = line.indexOf(':');
    if (colon < 0) continue;
    const key = line.slice(0, colon).trim();
    if (!/^[a-z]+$/.test(key)) continue;
    const value = line.slice(colon + 1).trim();
    if (!value) continue;
    const words = value.split(/\s+/);
    if (key === 'role' && (value === 'dev-worker' || value === 'dev-master')) {
      note.role = value;
      note.green = [];
    } else if (key === 'from' && FULL_SHA.test(value)) {
      note.from = value.toLowerCase();
      note.green = [];
    } else if (key === 'verify' && words.length === 3 && RUN.test(words[0])
      && (words[1] === 'GREEN' || words[1] === 'YELLOW')
      && FULL_SHA.test(words[2])) {
      note.verify = { run: words[0], outcome: words[1], commit: words[2].toLowerCase() };
      note.green = [];
    } else if (key === 'green' && words.length === 2 && RUN.test(words[0])) {
      const at = parseStrictUtc(words[1]);
      if (at) note.green.push({ run: words[0], at });
    }
  }
  return note;
}

/**
 * Parses a strict UTC timestamp as printed by `date -u +%Y-%m-%dT%H:%M:%SZ`,
 * rejecting calendar rollovers such as `2026-02-30T12:00:00Z`.
 * @param {string} time
 * @returns {Date | null}
 */
function parseStrictUtc(time) {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})Z$/.exec(time);
  if (!m) return null;
  const year = Number(m[1]);
  const month = Number(m[2]);
  const day = Number(m[3]);
  const hour = Number(m[4]);
  const minute = Number(m[5]);
  const second = Number(m[6]);
  if (month < 1 || month > 12 || day < 1 || day > 31 || hour > 23 || minute > 59 || second > 59) return null;
  const date = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
  if (Number.isNaN(date.getTime())) return null;
  if (date.getUTCFullYear() !== year || date.getUTCMonth() + 1 !== month || date.getUTCDate() !== day
    || date.getUTCHours() !== hour || date.getUTCMinutes() !== minute || date.getUTCSeconds() !== second) {
    return null;
  }
  return date;
}

/**
 * True when two commit ids name the same commit; either may be abbreviated.
 * @param {string | null} a
 * @param {string | null} b
 */
export function sameCommit(a, b) {
  if (!a || !b) return false;
  const x = a.toLowerCase();
  const y = b.toLowerCase();
  return x.length <= y.length ? y.startsWith(x) : x.startsWith(y);
}

/**
 * One CI state from the jobs and check suites of a pull request's head: passed only when
 * every check suite has completed, the fetched check set is complete, and every blocking job
 * succeeded on its own conclusion. Desktop E2E is informational on pull requests into dev,
 * so it does not count there. Unexpectedly skipped blocking jobs prevent reporting passed
 * (returning skipped only if every blocking job was skipped, otherwise unknown).
 * @param {ApiCheck[] | ApiCheckSet | undefined} input
 * @param {string} base
 * @returns {CiState}
 */
export function ciState(input, base) {
  if (!input) return 'unknown';
  const checks = Array.isArray(input) ? input : input.checks;
  const complete = Array.isArray(input) ? true : input.complete;
  const suitePending = Array.isArray(input) ? false : Boolean(input.suitePending);
  const relevant = checks.filter((check) => !(base === 'dev' && check.name.startsWith('Desktop E2E')));
  const nonSkipped = relevant.filter((check) => check.conclusion !== 'skipped');
  if (nonSkipped.some((check) => check.status === 'completed' && check.conclusion !== 'success')) return 'failed';
  if (suitePending || nonSkipped.some((check) => check.status !== 'completed')) return 'running';
  if (!complete) return 'unknown';
  if (relevant.length === 0) return 'none';
  if (nonSkipped.length === 0) return 'skipped';
  // If any blocking job was skipped alongside passing jobs, do not call CI passed.
  if (relevant.some((check) => check.conclusion === 'skipped')) return 'unknown';
  return 'passed';
}

/**
 * The v* tags that need their tag/commit object looked up: any annotated tag (whose ref points
 * to a tag object rather than a commit) and any lightweight tag lacking a published release and
 * matching merged pull request.
 * @param {ApiRef[]} tagRefs
 * @param {ApiRelease[]} releases
 * @param {ApiPull[]} pulls
 * @returns {ApiRef[]}
 */
export function undatedTags(tagRefs, releases, pulls) {
  return tagRefs.filter((ref) => {
    if (!ref.ref.startsWith('refs/tags/v')) return false;
    if (ref.object.type === 'tag') return true;
    const hasRelease = releases.some((release) => `refs/tags/${release.tag_name}` === ref.ref && !release.draft && release.published_at);
    const hasMerge = pulls.some((pull) => pull.merged_at && sameCommit(pull.merge_commit_sha, ref.object.sha));
    return !hasRelease && !hasMerge;
  });
}

/**
 * @param {Raw} raw
 * @returns {Model}
 */
export function buildModel(raw) {
  const prs = raw.pulls
    .filter((pull) => pull.base.ref === 'dev' || pull.base.ref === 'main')
    .map((pull) => {
      const merged = pull.merged_at !== null;
      /** @type {PrState} */
      const state = merged ? 'merged' : pull.state === 'closed' ? 'closed' : pull.draft ? 'draft' : 'open';
      const ended = pull.merged_at ?? pull.closed_at;
      const sameRepo = pull.head.repo?.full_name?.toLowerCase() === REPO.toLowerCase();
      return {
        number: pull.number,
        title: pull.title,
        url: pull.html_url,
        branch: pull.head.ref,
        base: pull.base.ref,
        state,
        created: new Date(pull.created_at),
        ended: ended ? new Date(ended) : null,
        mergeCommit: merged ? pull.merge_commit_sha : null,
        headSha: pull.head.sha,
        note: parseNote(pull.body),
        ci: ciState(raw.checks[String(pull.number)], pull.base.ref),
        release: sameRepo && pull.head.ref === 'dev' && pull.base.ref === 'main',
      };
    })
    .sort((a, b) => a.created.getTime() - b.created.getTime() || a.number - b.number);

  // A release is a v* tag on a verified same-repo dev -> main merge PR. Its date prefers the
  // annotated tag's own creation time, then a published GitHub release time, then the merge
  // time of the dev -> main release PR.
  const releases = raw.tagRefs
    .filter((ref) => ref.ref.startsWith('refs/tags/v'))
    .flatMap((ref) => {
      const tag = ref.ref.slice('refs/tags/'.length);
      const published = raw.releases.find((release) => release.tag_name === tag && !release.draft && release.published_at);
      const looked = raw.tagCommits[tag];
      const commit = looked?.sha ?? (ref.object.type === 'commit' ? ref.object.sha : null);
      const mergedPr = commit ? prs.find((pr) => pr.release && pr.state === 'merged' && sameCommit(pr.mergeCommit, commit)) ?? null : null;
      if (!mergedPr) return [];
      const tagDate = looked?.kind === 'tag' && looked.at ? new Date(looked.at) : null;
      const at = (tagDate && !Number.isNaN(tagDate.getTime()) ? tagDate : null)
        ?? (published?.published_at ? new Date(published.published_at) : null)
        ?? mergedPr.ended;
      const url = published?.html_url ?? `https://github.com/${REPO}/tree/${tag}`;
      return at ? [{ tag, url, at, fromDevMain: true }] : [];
    })
    .sort((a, b) => a.at.getTime() - b.at.getTime());

  const greenRef = raw.tagRefs.find((ref) => ref.ref === 'refs/tags/dev-green');
  const devGreen = greenRef
    ? { sha: greenRef.object.sha, pr: prs.find((pr) => pr.base === 'dev' && pr.state === 'merged' && sameCommit(pr.mergeCommit, greenRef.object.sha)) ?? null }
    : null;

  const greens = prs
    .filter((pr) => pr.state === 'merged' && pr.base === 'dev')
    .flatMap((pr) => (pr.note?.green ?? []).map((mark) => ({ pr, mark })))
    .sort((a, b) => a.mark.at.getTime() - b.mark.at.getTime());

  return { prs, releases, devGreen, devHead: raw.devHead, greens };
}

/**
 * The state of dev: GREEN when dev-green marks its latest commit, ahead when dev has moved
 * past dev-green (dev-master may be checking, fixing, or waiting at YELLOW for sign-offs),
 * or unknown when dev-green or dev's head is unavailable. When the dev-green commit is not
 * matched to a fetched pull request, `since` is null rather than an inferred count.
 * @param {Model} model
 * @returns {{ kind: 'green' | 'ahead' | 'unknown', since: Pr[] | null }}
 */
export function devState(model) {
  if (!model.devGreen || !model.devHead) return { kind: 'unknown', since: null };
  if (sameCommit(model.devGreen.sha, model.devHead)) return { kind: 'green', since: [] };
  const from = model.devGreen.pr?.ended ?? null;
  if (!from) return { kind: 'ahead', since: null };
  const since = model.prs.filter((pr) => pr.state === 'merged' && pr.base === 'dev' && pr.ended && pr.ended > from);
  return { kind: 'ahead', since };
}

/**
 * @typedef {{ at: Date, kind: 'opened' | 'merged' | 'closed' | 'release' | 'green', pr: Pr | null, release: Release | null, mark: GreenMark | null }} Event
 */

/**
 * Every dated event, oldest first.
 * @param {Model} model
 * @returns {Event[]}
 */
export function events(model) {
  /** @type {Event[]} */
  const list = [];
  for (const pr of model.prs) {
    if (!pr.release) list.push({ at: pr.created, kind: 'opened', pr, release: null, mark: null });
    // A release pull request matters only once merged: it is the release itself.
    if (pr.ended && (pr.state === 'merged' || (pr.state === 'closed' && !pr.release))) {
      list.push({ at: pr.ended, kind: pr.state === 'merged' ? 'merged' : 'closed', pr, release: null, mark: null });
    }
  }
  for (const release of model.releases) list.push({ at: release.at, kind: 'release', pr: null, release, mark: null });
  for (const { pr, mark } of model.greens) list.push({ at: mark.at, kind: 'green', pr, release: null, mark });
  return list.sort((a, b) => a.at.getTime() - b.at.getTime());
}

/**
 * @typedef {{ pr: Pr, lane: number, base: number, start: number, opened: number, end: number }} Segment
 * @typedef {{ columns: Date[], segments: Segment[], releases: { pr: Pr, column: number }[],
 *   tags: { release: Release, column: number }[], merges: Map<number, number>, lanes: number, devFrom: number }} Flow
 */

/** Lanes 0 and 1 are main and dev; development branches take the lanes below. */
export const MAIN = 0;
export const DEV = 1;

/**
 * Lays the branch flow out on a grid, like GitHub's network graph: one column per
 * event in time order (the last column is now) and one lane per branch at a time.
 * A branch starts where its note's `from` commit was merged into dev, otherwise at
 * its pull request.
 * @param {Model} model
 * @returns {Flow}
 */
export function layout(model) {
  const dated = events(model).filter((event) => event.kind !== 'green');
  /** @type {Map<string, number>} */
  const column = new Map();
  dated.forEach((event, index) => column.set(eventKey(event), index));
  const now = dated.length;
  const columns = [...dated.map((event) => event.at), new Date()];

  /** @type {Map<number, number>} merge column of each merged pull request */
  const merges = new Map();
  for (const pr of model.prs) {
    if (pr.state === 'merged') merges.set(pr.number, column.get(`merged:${pr.number}`) ?? now);
  }

  const branches = model.prs.filter((pr) => !pr.release);
  const segments = branches.map((pr) => {
    const opened = column.get(`opened:${pr.number}`) ?? now;
    const fork = pr.note?.from
      ? model.prs.find((other) => other.base === pr.base && other.state === 'merged' && sameCommit(other.mergeCommit, pr.note?.from ?? null))
      : undefined;
    const forked = fork ? merges.get(fork.number) : undefined;
    const start = forked !== undefined && forked < opened ? forked : opened;
    const end = pr.state === 'open' || pr.state === 'draft'
      ? now
      : column.get(`${pr.state}:${pr.number}`) ?? now;
    return { pr, lane: 0, base: pr.base === 'dev' ? DEV : MAIN, start, opened, end: Math.max(end, start + 1) };
  });

  // Greedy lanes: each branch takes the first lane that is free before its start column.
  /** @type {number[]} last occupied column per lane */
  const busy = [];
  for (const segment of [...segments].sort((a, b) => a.start - b.start || a.pr.number - b.pr.number)) {
    let lane = busy.findIndex((last) => last < segment.start);
    if (lane < 0) lane = busy.push(-1) - 1;
    busy[lane] = segment.end;
    segment.lane = DEV + 1 + lane;
  }

  const devPrs = model.prs.filter((pr) => pr.base === 'dev' || pr.release);
  const devFrom = Math.min(now, ...segments.filter((s) => s.base === DEV).map((s) => s.start),
    ...devPrs.filter((pr) => pr.release).map((pr) => merges.get(pr.number) ?? now));

  return {
    columns,
    segments,
    releases: model.prs.filter((pr) => pr.release && pr.state === 'merged').map((pr) => ({ pr, column: merges.get(pr.number) ?? now })),
    tags: model.releases.map((release) => ({ release, column: column.get(`release:${release.tag}`) ?? now })),
    merges,
    lanes: DEV + 1 + busy.length,
    devFrom,
  };
}

/** @param {Event} event */
function eventKey(event) {
  if (event.kind === 'release') return `release:${event.release?.tag}`;
  return `${event.kind}:${event.pr?.number}`;
}
