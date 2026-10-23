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
 * @typedef {'passed' | 'failed' | 'running' | 'none' | 'unknown'} CiState
 * @typedef {{
 *   number: number, title: string, url: string, branch: string, base: string,
 *   state: PrState, created: Date, ended: Date | null, mergeCommit: string | null,
 *   headSha: string, note: Note | null, ci: CiState, release: boolean
 * }} Pr
 * @typedef {{ tag: string, url: string, at: Date }} Release
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
 *   merge_commit_sha: string | null, head: { ref: string, sha: string }, base: { ref: string }
 * }} ApiPull
 * @typedef {{ tag_name: string, html_url: string, published_at: string | null, draft: boolean }} ApiRelease
 * @typedef {{ ref: string, object: { sha: string, type: string } }} ApiRef
 * @typedef {{ name: string, status: string, conclusion: string | null }} ApiCheck
 * @typedef {{
 *   pulls: ApiPull[], releases: ApiRelease[], tagRefs: ApiRef[], tagCommits: Record<string, { sha: string, at: string }>,
 *   devHead: string | null, checks: Record<string, ApiCheck[]>
 * }} Raw
 * `checks` holds the latest check runs (CI jobs) of the open pull requests whose CI was
 * fetched, by number. `tagCommits` holds the commit and date of each v* tag that neither
 * a published release nor a merged pull request dates.
 */

/** The repository the board follows. */
export const REPO = 'lhj6102/artifactize';

/** The note dev-master and dev-worker end their pull request bodies with. */
export const NOTE = /<!--\s*artifactize-board\b([\s\S]*?)-->/g;
const SHA = /^[0-9a-f]{7,40}$/i;
const RUN = /^run-[A-Za-z0-9_-]+$/;

/**
 * The last board note in a pull request body (the skills end the body with it), so a
 * quoted example earlier in the text never hides it.
 * @param {string | null} body
 * @returns {string | null}
 */
export function lastNote(body) {
  const notes = body ? [...body.matchAll(NOTE)] : [];
  return notes.length ? notes[notes.length - 1][0] : null;
}

/**
 * Reads the board note of a pull request body; null without one. Unknown or malformed
 * lines are skipped, so a hand-edited note never breaks the board.
 * @param {string | null} body
 * @returns {Note | null}
 */
export function parseNote(body) {
  const text = lastNote(body);
  const match = text ? /^<!--\s*artifactize-board\b([\s\S]*?)-->$/.exec(text) : null;
  if (!match) return null;
  /** @type {Note} */
  const note = { role: null, from: null, verify: null, green: [] };
  for (const line of match[1].split(/\r?\n/)) {
    const field = /^\s*([a-z]+)\s*:\s*(.*?)\s*$/.exec(line);
    if (!field) continue;
    const [, key, value] = field;
    const words = value.split(/\s+/);
    if (key === 'role' && (value === 'dev-worker' || value === 'dev-master')) {
      note.role = value;
    } else if (key === 'from' && SHA.test(value)) {
      note.from = value.toLowerCase();
    } else if (key === 'verify' && words.length === 3 && RUN.test(words[0])
      && (words[1] === 'GREEN' || words[1] === 'YELLOW' || words[1] === 'SIGNOFFS_PENDING')
      && SHA.test(words[2])) {
      // SIGNOFFS_PENDING is what notes written before 0.16 call YELLOW.
      const outcome = words[1] === 'GREEN' ? 'GREEN' : 'YELLOW';
      note.verify = { run: words[0], outcome, commit: words[2].toLowerCase() };
    } else if (key === 'green' && words.length === 2 && RUN.test(words[0])) {
      const at = new Date(isoExtended(words[1]));
      if (!Number.isNaN(at.getTime())) note.green.push({ run: words[0], at });
    }
  }
  return note;
}

/**
 * An ISO 8601 time in the extended form Date reads: a basic-form UTC time such as
 * `20260529T120000Z` gets its separators; any other text is returned unchanged.
 * @param {string} time
 */
function isoExtended(time) {
  const basic = /^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2}(?:[.,]\d+)?)Z$/.exec(time);
  if (!basic) return time;
  const [, year, month, day, hour, minute, second] = basic;
  return `${year}-${month}-${day}T${hour}:${minute}:${second.replace(',', '.')}Z`;
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
 * One CI state from the jobs of a pull request's head: passed only when every job that
 * ran succeeded. Skipped jobs did not run; Desktop E2E is informational on pull requests
 * into dev, so it does not count there. Without fetched jobs the state is unknown.
 * @param {ApiCheck[] | undefined} checks
 * @param {string} base
 * @returns {CiState}
 */
export function ciState(checks, base) {
  if (!checks) return 'unknown';
  const counted = checks.filter((check) => check.conclusion !== 'skipped'
    && !(base === 'dev' && check.name.startsWith('Desktop E2E')));
  if (counted.length === 0) return 'none';
  if (counted.some((check) => check.status === 'completed' && check.conclusion !== 'success')) return 'failed';
  if (counted.some((check) => check.status !== 'completed')) return 'running';
  return 'passed';
}

/**
 * The v* tags that need their commit looked up: no published release and no merged pull
 * request whose merge commit is the tag's (an annotated tag names a tag object, not a commit).
 * @param {ApiRef[]} tagRefs
 * @param {ApiRelease[]} releases
 * @param {ApiPull[]} pulls
 * @returns {ApiRef[]}
 */
export function undatedTags(tagRefs, releases, pulls) {
  return tagRefs.filter((ref) => ref.ref.startsWith('refs/tags/v')
    && !releases.some((release) => `refs/tags/${release.tag_name}` === ref.ref && !release.draft && release.published_at)
    && !(ref.object.type === 'commit' && pulls.some((pull) => pull.merged_at && sameCommit(pull.merge_commit_sha, ref.object.sha))));
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
        release: pull.head.ref === 'dev' && pull.base.ref === 'main',
      };
    })
    .sort((a, b) => a.created.getTime() - b.created.getTime() || a.number - b.number);

  // A release is a v* tag. Its date is its published GitHub release, otherwise the merge
  // of the pull request that made the tagged commit, otherwise the tag's own date.
  const releases = raw.tagRefs
    .filter((ref) => ref.ref.startsWith('refs/tags/v'))
    .flatMap((ref) => {
      const tag = ref.ref.slice('refs/tags/'.length);
      const published = raw.releases.find((release) => release.tag_name === tag && !release.draft && release.published_at);
      const looked = raw.tagCommits[tag];
      const commit = looked?.sha ?? ref.object.sha;
      const merged = prs.find((pr) => sameCommit(pr.mergeCommit, commit))?.ended;
      const at = published?.published_at ? new Date(published.published_at)
        : merged ?? (looked ? new Date(looked.at) : null);
      const url = published?.html_url ?? `https://github.com/${REPO}/tree/${tag}`;
      return at ? [{ tag, url, at }] : [];
    })
    .sort((a, b) => a.at.getTime() - b.at.getTime());

  const greenRef = raw.tagRefs.find((ref) => ref.ref === 'refs/tags/dev-green');
  const devGreen = greenRef
    ? { sha: greenRef.object.sha, pr: prs.find((pr) => sameCommit(pr.mergeCommit, greenRef.object.sha)) ?? null }
    : null;

  const greens = prs
    .filter((pr) => pr.state === 'merged' && pr.base === 'dev')
    .flatMap((pr) => (pr.note?.green ?? []).map((mark) => ({ pr, mark })))
    .sort((a, b) => a.mark.at.getTime() - b.mark.at.getTime());

  return { prs, releases, devGreen, devHead: raw.devHead, greens };
}

/**
 * The state of dev: GREEN when dev-green marks its latest commit, otherwise the
 * pull requests merged since dev-green.
 * @param {Model} model
 * @returns {{ kind: 'green' | 'converging' | 'unknown', since: Pr[] }}
 */
export function devState(model) {
  if (!model.devGreen) return { kind: 'unknown', since: [] };
  if (sameCommit(model.devGreen.sha, model.devHead)) return { kind: 'green', since: [] };
  const from = model.devGreen.pr?.ended ?? null;
  const since = model.prs.filter((pr) => pr.state === 'merged' && pr.base === 'dev' && pr.ended
    && (!from || pr.ended > from));
  return { kind: 'converging', since };
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
    const fork = pr.note?.from ? model.prs.find((other) => sameCommit(other.mergeCommit, pr.note?.from ?? null)) : undefined;
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
