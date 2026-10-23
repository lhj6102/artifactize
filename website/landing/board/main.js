// The development board: reads the repository from GitHub's public REST API in the
// visitor's browser, then draws dev's state, the branch flow, the pull requests and
// the recent work. Without a token GitHub allows 60 requests an hour per network
// address, so a fresh snapshot is kept in localStorage and reused for a few minutes.

import { DEV, MAIN, REPO, buildModel, devState, events, lastNote, layout, sameCommit, undatedTags } from './model.js';

/** @typedef {import('./model.js').Raw} Raw */
/** @typedef {import('./model.js').Model} Model */
/** @typedef {import('./model.js').Pr} Pr */
/** @typedef {import('./model.js').Event} Event */

const API = `https://api.github.com/repos/${REPO}`;
const CACHE_KEY = 'artifactize-board-4';
/** One load costs six requests plus one per open pull request; reusing a snapshot this young keeps reloads within the hourly limit. */
const FRESH_MS = 5 * 60 * 1000;
/** Open pull requests whose CI jobs are fetched, newest first; the rest link to GitHub for CI. */
const CHECKED_PRS = 10;
/** Pages of open pull requests followed (100 each). */
const OPEN_PAGES = 5;
/** Undated v* tags whose commit is looked up, newest first. */
const TAG_LOOKUPS = 10;
/** Closed and merged pull requests listed under the open ones. */
const RECENT_ROWS = 30;
/** Entries in the recent work list. */
const ACTIVITY_ROWS = 25;

class LimitError extends Error {
  /** @param {Date | null} reset */
  constructor(reset) {
    super('GitHub rate limit reached');
    this.reset = reset;
  }
}

/**
 * @param {string} path a path under the repository, or a full API URL
 * @returns {Promise<Response>}
 */
async function request(path) {
  const response = await fetch(path.startsWith('https://') ? path : API + path, { headers: { Accept: 'application/vnd.github+json' } });
  if ((response.status === 403 || response.status === 429) && response.headers.get('x-ratelimit-remaining') === '0') {
    const reset = Number(response.headers.get('x-ratelimit-reset'));
    throw new LimitError(reset ? new Date(reset * 1000) : null);
  }
  return response;
}

/**
 * Every page of a list, following GitHub's Link header up to `pages` pages.
 * @param {string} path
 * @param {number} pages
 * @returns {Promise<any[]>}
 */
async function getAll(path, pages) {
  /** @type {any[]} */
  const items = [];
  /** @type {string | null} */
  let next = path;
  for (let page = 0; next && page < pages; page += 1) {
    const response = await request(next);
    if (!response.ok) throw new Error(`GitHub answered ${response.status} for ${path}`);
    items.push(...await response.json());
    next = /<([^>]+)>;\s*rel="next"/.exec(response.headers.get('link') ?? '')?.[1] ?? null;
  }
  return items;
}

/**
 * @param {string} path
 * @param {boolean} [optional] a 404 returns null instead of failing
 * @returns {Promise<any>}
 */
async function get(path, optional = false) {
  const response = await request(path);
  if (optional && response.status === 404) return null;
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for ${path}`);
  return response.json();
}

/**
 * Fetches everything the board shows and keeps only the fields it reads.
 * @returns {Promise<Raw>}
 */
async function fetchRaw() {
  const [recent, updated, open, releases, tagRefs, dev] = await Promise.all([
    get('/pulls?state=all&per_page=100&sort=created&direction=desc'),
    // Recently merged or edited ones too: a long-lived branch merges late, and dev-master
    // edits a merged pull request when it adds a green: line.
    get('/pulls?state=closed&per_page=100&sort=updated&direction=desc'),
    // Open pull requests separately, so an old one still waiting is never cut off.
    getAll('/pulls?state=open&per_page=100', OPEN_PAGES),
    get('/releases?per_page=100'),
    get('/git/matching-refs/tags'),
    get('/branches/dev', true),
  ]);
  /** @type {Map<number, import('./model.js').ApiPull>} */
  const pulls = new Map();
  for (const pull of [...recent, ...updated, ...open]) pulls.set(pull.number, pull);
  // The pull request that made the dev-green commit, should both lists have missed it.
  const green = tagRefs.find((/** @type {import('./model.js').ApiRef} */ ref) => ref.ref === 'refs/tags/dev-green');
  if (green && ![...pulls.values()].some((pull) => sameCommit(pull.merge_commit_sha, green.object.sha))) {
    const made = await get(`/commits/${green.object.sha}/pulls`);
    for (const pull of made) pulls.set(pull.number, pull);
  }
  const waiting = [...open].sort((a, b) => b.number - a.number).slice(0, CHECKED_PRS);
  const lookups = undatedTags(tagRefs, releases, [...pulls.values()])
    .sort((a, b) => b.ref.localeCompare(a.ref, 'en', { numeric: true }))
    .slice(0, TAG_LOOKUPS);
  const [checks, tagged] = await Promise.all([
    Promise.all(waiting.map((pull) => get(`/commits/${pull.head.sha}/check-runs?per_page=100`))),
    Promise.all(lookups.map(async (ref) => {
      // An annotated tag is its own object with a date; a lightweight one is the commit.
      if (ref.object.type === 'tag') {
        const tag = await get(`/git/tags/${ref.object.sha}`);
        return { sha: tag.object.sha, at: tag.tagger.date };
      }
      const commit = await get(`/git/commits/${ref.object.sha}`);
      return { sha: commit.sha, at: commit.committer.date };
    })),
  ]);
  return {
    pulls: [...pulls.values()].map((pull) => ({
      number: pull.number,
      title: pull.title,
      html_url: pull.html_url,
      state: pull.state,
      draft: pull.draft,
      // Only the note is read; the rest of the body would only fill the cache.
      body: lastNote(pull.body),
      created_at: pull.created_at,
      closed_at: pull.closed_at,
      merged_at: pull.merged_at,
      merge_commit_sha: pull.merge_commit_sha,
      head: { ref: pull.head.ref, sha: pull.head.sha },
      base: { ref: pull.base.ref },
    })),
    releases: releases.map((/** @type {import('./model.js').ApiRelease} */ r) => ({
      tag_name: r.tag_name, html_url: r.html_url, published_at: r.published_at, draft: r.draft,
    })),
    tagRefs: tagRefs.map((/** @type {import('./model.js').ApiRef} */ ref) => ({ ref: ref.ref, object: { sha: ref.object.sha, type: ref.object.type } })),
    tagCommits: Object.fromEntries(lookups.map((ref, index) => [ref.ref.slice('refs/tags/'.length), tagged[index]])),
    devHead: dev ? dev.commit.sha : null,
    checks: Object.fromEntries(waiting.map((pull, index) => [String(pull.number), checks[index].check_runs.map(
      (/** @type {import('./model.js').ApiCheck} */ check) => ({ name: check.name, status: check.status, conclusion: check.conclusion }),
    )])),
  };
}

/** @returns {{ at: number, raw: Raw } | null} */
function readCache() {
  try {
    const saved = JSON.parse(localStorage.getItem(CACHE_KEY) ?? 'null');
    return saved && typeof saved.at === 'number' && saved.raw ? saved : null;
  } catch {
    return null;
  }
}

/** @param {Raw} raw */
function writeCache(raw) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), raw }));
  } catch {
    // Storage full or blocked: the board still works, it just asks GitHub every time.
  }
}

// ---------- Formatting ----------

const dayFormat = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });
const timeFormat = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
const clockFormat = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' });

/** @param {Date} date */
function ago(date) {
  const minutes = Math.round((Date.now() - date.getTime()) / 60000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return `${Math.round(hours / 24)} days ago`;
}

/** @param {string} sha */
const short = (sha) => sha.slice(0, 7);

/**
 * @template {keyof HTMLElementTagNameMap} K
 * @param {K} tag
 * @param {Record<string, string>} [attrs]
 * @param {(Node | string | null)[]} [children]
 * @returns {HTMLElementTagNameMap[K]}
 */
function h(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) node.setAttribute(name, value);
  for (const child of children) if (child !== null) node.append(child);
  return node;
}

/** @param {Date} date */
function time(date) {
  return h('time', { datetime: date.toISOString(), title: timeFormat.format(date) }, [ago(date)]);
}

/** @param {Pr} pr */
function prLink(pr) {
  return h('a', { href: pr.url }, [`#${pr.number}`]);
}

/** @param {string} sha */
function commitLink(sha) {
  return h('a', { href: `https://github.com/${REPO}/commit/${sha}` }, [h('code', {}, [short(sha)])]);
}

/**
 * A state badge: an icon and a word, never colour alone.
 * @param {'good' | 'bad' | 'wait' | 'none'} tone
 * @param {string} label
 */
function badge(tone, label) {
  const icon = { good: '✓', bad: '✕', wait: '●', none: '–' }[tone];
  return h('span', { class: `badge badge-${tone}` }, [h('span', { 'aria-hidden': 'true' }, [icon]), ` ${label}`]);
}

/** @param {Pr} pr */
function roleOf(pr) {
  if (pr.release) return 'release';
  return pr.note?.role ?? null;
}

// ---------- Summary ----------

/** @param {Model} model */
function renderStatus(model) {
  const dev = devState(model);
  const green = model.devGreen;
  /** @type {(Node | string | null)[]} */
  let devBody;
  if (dev.kind === 'green' && green) {
    devBody = [badge('good', 'GREEN'), h('p', {}, ['Its latest commit ', commitLink(green.sha), ' passed every check and is marked ', h('code', {}, ['dev-green']), '.'])];
  } else if (dev.kind === 'converging' && green) {
    const count = dev.since.length;
    devBody = [badge('wait', 'Checking'), h('p', {}, [
      `${count === 1 ? 'One change' : `${count} changes`} joined since the last GREEN commit `, commitLink(green.sha),
      green.pr?.ended ? ` (${ago(green.pr.ended)})` : '', '. dev-master checks dev again and fixes what fails.',
    ])];
  } else {
    devBody = [badge('none', 'Not marked yet'), h('p', {}, ['No commit is marked ', h('code', {}, ['dev-green']), ' yet.'])];
  }

  const waiting = model.prs.filter((pr) => pr.base === 'dev' && (pr.state === 'open' || pr.state === 'draft'));
  const count = (/** @type {string} */ ci) => waiting.filter((pr) => pr.ci === ci).length;
  const ciParts = [
    count('passed') ? `${count('passed')} passing CI` : '',
    count('failed') ? `${count('failed')} failing CI` : '',
    count('running') ? `${count('running')} running CI` : '',
  ].filter(Boolean);

  const latest = model.releases.at(-1);

  const cards = document.getElementById('board-status');
  if (!cards) return;
  cards.replaceChildren(
    h('div', { class: 'card' }, [h('h3', {}, [h('code', {}, ['dev'])]), ...devBody]),
    h('div', { class: 'card' }, [
      h('h3', {}, ['Waiting to join dev']),
      h('p', { class: 'card-figure' }, [String(waiting.length)]),
      h('p', {}, [waiting.length ? `pull request${waiting.length === 1 ? '' : 's'}${ciParts.length ? `: ${ciParts.join(', ')}` : ''}.` : 'Nothing is waiting.']),
    ]),
    h('div', { class: 'card' }, [
      h('h3', {}, ['Latest release']),
      latest ? h('p', { class: 'card-figure' }, [h('a', { href: latest.url }, [latest.tag])]) : h('p', { class: 'card-figure' }, ['–']),
      latest ? h('p', {}, ['Released ', time(latest.at), ' from ', h('code', {}, ['dev']), ' through ', h('code', {}, ['main']), '.']) : null,
    ]),
  );
}

// ---------- Branch flow ----------

const SVG_NS = 'http://www.w3.org/2000/svg';
/** Column width, lane height, the room above the lanes for dates and tags, and the room the sticky lane labels cover. */
const COL = 24;
const LANE = 26;
const TOP = 62;
const LEFT = 60;

/**
 * @param {string} name
 * @param {Record<string, string | number>} attrs
 * @param {Element} parent
 * @param {string} [text]
 */
function svg(name, attrs, parent, text) {
  const node = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
  if (text !== undefined) node.textContent = text;
  parent.append(node);
  return node;
}

/** @param {number} column */
const x = (column) => LEFT + column * COL + COL / 2;
/** @param {number} lane */
const y = (lane) => TOP + lane * LANE + LANE / 2;

/** @param {Pr} pr */
function laneClass(pr) {
  const role = roleOf(pr);
  return role === 'dev-worker' ? 'role-worker' : role === 'dev-master' ? 'role-master' : 'role-other';
}

/** @param {Pr} pr */
function describePr(pr) {
  const into = `into ${pr.base}`;
  const by = pr.note?.role ? ` by ${pr.note.role}` : '';
  const when = pr.ended ? timeFormat.format(pr.ended) : '';
  const state = pr.state === 'merged' ? `merged ${into} ${when}`
    : pr.state === 'closed' ? `closed ${when}`
      : `${pr.state === 'draft' ? 'draft' : 'open'} ${into}, CI ${pr.ci}`;
  return `#${pr.number} ${pr.branch}${by}: ${state}. ${pr.title}`;
}

/** @param {Model} model */
function renderFlow(model) {
  const host = document.getElementById('board-flow');
  if (!host) return;
  const flow = layout(model);
  const width = LEFT + flow.columns.length * COL + 48;
  const height = TOP + flow.lanes * LANE + 8;
  const root = document.createElementNS(SVG_NS, 'svg');
  root.setAttribute('viewBox', `0 0 ${width} ${height}`);
  root.setAttribute('width', String(width));
  root.setAttribute('height', String(height));
  root.setAttribute('class', 'flow-svg');
  root.setAttribute('role', 'img');
  root.setAttribute('aria-label', 'Branch flow over time: main, dev and the development branches. The table below lists the same pull requests.');

  // Day gridlines and labels; a label is skipped when it would touch the previous one.
  const grid = svg('g', { class: 'flow-grid' }, root);
  let lastLabel = -Infinity;
  flow.columns.forEach((date, column) => {
    const previous = flow.columns[column - 1];
    if (previous && previous.toDateString() === date.toDateString()) return;
    const left = x(column) - COL / 2;
    svg('line', { x1: left, x2: left, y1: 20, y2: height }, grid);
    if (left - lastLabel < 52) return;
    lastLabel = left;
    svg('text', { x: left + 4, y: 14 }, grid, column === flow.columns.length - 1 ? 'now' : dayFormat.format(date));
  });

  const now = flow.columns.length - 1;
  svg('line', { class: 'lane-main', x1: x(0), x2: x(now), y1: y(MAIN), y2: y(MAIN) }, root);
  if (flow.devFrom < now) svg('line', { class: 'lane-dev', x1: x(flow.devFrom), x2: x(now), y1: y(DEV), y2: y(DEV) }, root);

  // Release tags in two staggered rows above main.
  const tags = svg('g', { class: 'flow-tags' }, root);
  /** @type {number[]} */
  const rowEnd = [-Infinity, -Infinity];
  for (const { release, column } of flow.tags) {
    const cx = x(column);
    const w = release.tag.length * 6.4 + 10;
    const row = rowEnd[0] < cx - w / 2 ? 0 : 1;
    rowEnd[row] = cx + w / 2 + 4;
    const ty = 26 + row * 16;
    const link = svg('a', { href: release.url }, tags);
    svg('title', {}, link, `${release.tag}, released ${timeFormat.format(release.at)}`);
    svg('line', { x1: cx, x2: cx, y1: ty + 6, y2: y(MAIN) - 4 }, link);
    svg('rect', { x: cx - w / 2, y: ty - 6, width: w, height: 13, rx: 6.5 }, link);
    svg('text', { x: cx, y: ty + 3.5, 'text-anchor': 'middle' }, link, release.tag);
  }

  const half = COL / 2;
  for (const segment of flow.segments) {
    const { pr, lane, base, start, opened, end } = segment;
    const link = svg('a', { href: pr.url, class: `flow-branch ${laneClass(pr)}`, 'aria-label': describePr(pr) }, root);
    svg('title', {}, link, describePr(pr));
    const xs = x(start);
    const xe = x(end);
    const ys = y(base);
    const yl = y(lane);
    let d = `M${xs} ${ys} C${xs + half / 2} ${ys} ${xs + half / 2} ${yl} ${xs + half} ${yl}`;
    if (pr.state === 'merged') {
      d += ` H${xe - half} C${xe - half / 2} ${yl} ${xe - half / 2} ${ys} ${xe} ${ys}`;
    } else {
      d += ` H${xe}`;
    }
    svg('path', { d, class: 'flow-path' }, link);
    const mark = opened === start ? xs + half : x(opened);
    svg('circle', { cx: mark, cy: yl, r: 3.5, class: 'flow-open' }, link);
    svg('text', { x: mark + 6, y: yl - 6, class: 'flow-label' }, link, `#${pr.number}`);
    if (pr.state === 'merged') {
      svg('circle', { cx: xe, cy: ys, r: 4, class: `flow-merge ${base === DEV ? 'on-dev' : 'on-main'}` }, link);
    } else if (pr.state === 'closed') {
      svg('path', { d: `M${xe - 4} ${yl - 4}l8 8m0-8l-8 8`, class: 'flow-closed' }, link);
    } else {
      svg('circle', { cx: xe, cy: yl, r: 5.5, class: `flow-tip ci-${pr.ci}` }, link);
    }
  }

  for (const { pr, column } of flow.releases) {
    const link = svg('a', { href: pr.url, class: 'flow-release', 'aria-label': describePr(pr) }, root);
    svg('title', {}, link, describePr(pr));
    const xe = x(column);
    svg('path', { d: `M${xe - COL} ${y(DEV)} C${xe - half} ${y(DEV)} ${xe - half} ${y(MAIN)} ${xe} ${y(MAIN)}`, class: 'flow-path' }, link);
    svg('circle', { cx: xe, cy: y(MAIN), r: 4, class: 'flow-merge on-main' }, link);
  }

  // GREEN dev commits: a ring around the merge that made them; the current one is labeled.
  for (const { pr, mark } of model.greens) {
    const column = flow.merges.get(pr.number);
    if (column === undefined) continue;
    const ring = svg('circle', { cx: x(column), cy: y(DEV), r: 7.5, class: 'flow-green' }, root);
    svg('title', {}, ring, `GREEN at ${pr.mergeCommit ? short(pr.mergeCommit) : `#${pr.number}`}, verify ${mark.run}, ${timeFormat.format(mark.at)}`);
  }
  const current = model.devGreen?.pr;
  const currentColumn = current ? flow.merges.get(current.number) : undefined;
  if (currentColumn !== undefined) {
    svg('circle', { cx: x(currentColumn), cy: y(DEV), r: 7.5, class: 'flow-green' }, root);
    svg('text', { x: x(currentColumn) + 10, y: (y(MAIN) + y(DEV)) / 2 + 4, class: 'flow-green-label' }, root, 'dev-green');
  }

  const scroller = h('div', { class: 'flow-scroll', tabindex: '0', 'aria-label': 'Branch flow, scrolls sideways' }, [root]);
  const labels = h('div', { class: 'flow-lanes', 'aria-hidden': 'true' }, [
    h('span', { class: 'lane-label lane-label-main', style: `top:${y(MAIN)}px` }, ['main']),
    flow.devFrom < now ? h('span', { class: 'lane-label lane-label-dev', style: `top:${y(DEV)}px` }, ['dev']) : null,
  ]);
  host.replaceChildren(labels, scroller);
  scroller.scrollLeft = scroller.scrollWidth;
}

// ---------- Pull requests ----------

/** @param {Pr} pr */
function verifyCell(pr) {
  const verify = pr.note?.verify;
  if (!verify) return h('td', {}, [badge('none', 'not recorded')]);
  const tone = verify.outcome === 'GREEN' ? 'good' : 'wait';
  const label = verify.outcome === 'GREEN' ? 'GREEN' : 'YELLOW: sign-offs pending';
  const stale = (pr.state === 'open' || pr.state === 'draft') && !sameCommit(verify.commit, pr.headSha);
  return h('td', { title: `Run ${verify.run} on ${short(verify.commit)}` }, [
    badge(tone, label),
    stale ? h('span', { class: 'cell-note' }, [' older commit']) : null,
  ]);
}

/** @param {Pr} pr */
function ciCell(pr) {
  if (pr.ci === 'passed') return h('td', {}, [badge('good', 'passed')]);
  if (pr.ci === 'failed') return h('td', {}, [badge('bad', 'failed')]);
  if (pr.ci === 'running') return h('td', {}, [badge('wait', 'running')]);
  if (pr.ci === 'unknown' && (pr.state === 'open' || pr.state === 'draft')) {
    return h('td', {}, [h('a', { href: `${pr.url}/checks` }, ['see GitHub'])]);
  }
  if (pr.state === 'open' || pr.state === 'draft') return h('td', {}, [badge('none', 'no results yet')]);
  return h('td', {}, [h('span', { class: 'cell-note' }, ['–'])]);
}

/** @param {Pr} pr */
function stateCell(pr) {
  if (pr.state === 'merged') return h('td', {}, [`merged into ${pr.base} `, pr.ended ? time(pr.ended) : null]);
  if (pr.state === 'closed') return h('td', {}, ['closed ', pr.ended ? time(pr.ended) : null]);
  return h('td', {}, [`${pr.state === 'draft' ? 'draft' : 'open'} for ${pr.base}, `, time(pr.created)]);
}

/** @param {Model} model */
function renderPrs(model) {
  const body = document.getElementById('board-prs');
  if (!body) return;
  const open = model.prs.filter((pr) => pr.state === 'open' || pr.state === 'draft').reverse();
  const done = model.prs.filter((pr) => pr.ended && pr.state !== 'open' && pr.state !== 'draft')
    .sort((a, b) => (b.ended?.getTime() ?? 0) - (a.ended?.getTime() ?? 0))
    .slice(0, RECENT_ROWS);
  body.replaceChildren(...[...open, ...done].map((pr) => {
    const role = roleOf(pr);
    return h('tr', { class: pr.state === 'open' || pr.state === 'draft' ? 'is-open' : '' }, [
      h('td', { class: 'cell-pr' }, [
        prLink(pr), ' ', h('span', { class: 'cell-title' }, [pr.title]), h('br'), h('code', { class: 'cell-branch' }, [pr.branch]),
      ]),
      h('td', {}, [role ? h('span', { class: `role role-${role}` }, [role]) : h('span', { class: 'cell-note' }, ['–'])]),
      verifyCell(pr),
      ciCell(pr),
      stateCell(pr),
    ]);
  }));
}

// ---------- Recent work ----------

/** @param {Event} event */
function describe(event) {
  const pr = event.pr;
  if (event.kind === 'release' && event.release) {
    return ['Released ', h('a', { href: event.release.url }, [event.release.tag]), '.'];
  }
  if (!pr) return [];
  if (event.kind === 'green') {
    return ['dev-master marked ', pr.mergeCommit ? commitLink(pr.mergeCommit) : prLink(pr), ' GREEN after verify ', h('code', {}, [event.mark?.run ?? '']), '.'];
  }
  const who = pr.note?.role ?? null;
  if (event.kind === 'opened') {
    return [who ? `${who} opened ` : 'Opened ', prLink(pr), ` for ${pr.base}: ${pr.title}`];
  }
  if (event.kind === 'merged') {
    return pr.release ? [prLink(pr), ' brought dev into main for a release.'] : [prLink(pr), ` joined ${pr.base}: ${pr.title}`];
  }
  return [prLink(pr), ' was closed without joining.'];
}

/** @param {Model} model */
function renderActivity(model) {
  const list = document.getElementById('board-activity');
  if (!list) return;
  const recent = events(model).reverse().slice(0, ACTIVITY_ROWS);
  list.replaceChildren(...recent.map((event) => h('li', {}, [time(event.at), h('span', {}, describe(event))])));
}

// ---------- Loading ----------

/**
 * @param {Raw} raw
 * @param {number} at when the data was fetched
 * @param {string} [problem]
 */
function render(raw, at, problem) {
  const model = buildModel(raw);
  renderStatus(model);
  renderFlow(model);
  renderPrs(model);
  renderActivity(model);
  const meta = document.getElementById('board-updated');
  if (meta) meta.textContent = `Updated ${clockFormat.format(new Date(at))}.`;
  showProblem(problem);
}

/** @param {string} [message] */
function showProblem(message) {
  const box = document.getElementById('board-problem');
  if (!box) return;
  box.textContent = message ?? '';
  box.hidden = !message;
}

/** @param {unknown} error */
function explain(error) {
  if (error instanceof LimitError) {
    const reset = error.reset ? ` until ${clockFormat.format(error.reset)}` : ' for up to an hour';
    return `GitHub limits how often one network address can ask for data, and that limit is used up${reset}.`;
  }
  return 'GitHub could not be reached just now.';
}

/** @param {boolean} force skip the fresh snapshot */
async function load(force) {
  const cached = readCache();
  if (cached && !force && Date.now() - cached.at < FRESH_MS) {
    render(cached.raw, cached.at);
    return;
  }
  const button = /** @type {HTMLButtonElement | null} */ (document.getElementById('board-refresh'));
  if (button) button.disabled = true;
  try {
    const raw = await fetchRaw();
    writeCache(raw);
    render(raw, Date.now());
  } catch (error) {
    if (cached) {
      render(cached.raw, cached.at, `${explain(error)} Showing the data from ${timeFormat.format(new Date(cached.at))}.`);
    } else {
      showProblem(`${explain(error)} Try again later, or see the pull requests on GitHub.`);
    }
  } finally {
    if (button) button.disabled = false;
  }
}

document.getElementById('board-refresh')?.addEventListener('click', () => load(true));
load(false);
