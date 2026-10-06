// @ts-check
// The scroll story's renderer. It draws the snapshots that model.js builds from
// scenario.js as SVG, and animates from one step's state to the next: nodes
// move, appear and split, eval edges grow, ripples mark evals stale and then
// reviewed, and flows carry verdicts. It knows node kinds, eval kinds and
// states, never the story. One requestAnimationFrame loop writes SVG
// attributes only (no layout reads); state colours change through CSS
// transitions. It stops when nothing moves or the stage is off screen.

import { evalId, NON_ARTIFACT } from './model.js';

/** @typedef {import('./types.js').Scenario} Scenario */
/** @typedef {import('./types.js').Snapshot} Snapshot */
/** @typedef {import('./types.js').StoryNode} StoryNode */
/** @typedef {import('./types.js').StoryEval} StoryEval */
/** @typedef {import('./types.js').EvalState} EvalState */
/** @typedef {import('./types.js').NodeKind} NodeKind */
/** @typedef {import('./types.js').Overlay} Overlay */
/** @typedef {import('./types.js').Counts} Counts */

/** @typedef {{ x: number, y: number, s: number, o: number, fo: number }} Geo  Percent, scale, card and frame opacity. */
/** @typedef {{ x: number, y: number, w: number, h: number }} Box  Pixels; x and y are the centre. */
/** @typedef {{ ax: number, ay: number, cx: number, cy: number, bx: number, by: number }} Curve */

const NS = 'http://www.w3.org/2000/svg';

/** Card metrics in unscaled pixels. The labels use a monospaced font. */
const CARD = { padX: 9, icon: 16, gap: 6, label: 12, sub: 9.5, h1: 32, h2: 44, miniW: 52, miniH: 28 };
const CHAR = 0.6; // advance of the monospaced font, in em

/** Milliseconds. */
const T = {
  move: 820, // nodes move to their new places
  edgeDelay: 360, // a new edge starts growing
  edgeGrow: 560,
  verdict: 380, // a grown edge shows its verdict
  fade: 320,
  ripple: 560, // the ripple starts, after the move
  hold: 4200, // a finished ripple stays put before it replays
  crossfade: 180, // reduced motion: fade between steps
};

/** Icons on a 16×16 grid, stroked. One per node kind. @type {Record<NodeKind, string>} */
const ICONS = {
  idea: 'M8 1.8a4.3 4.3 0 0 0-2.5 7.8V11.4h5V9.6A4.3 4.3 0 0 0 8 1.8zM6.2 13.8h3.6',
  doc: 'M4 1.8h5.2L12 4.6v9.6H4zM9.2 1.8v2.8H12M6 8.2h4M6 10.8h4',
  code: 'M5.4 4.4 1.8 8l3.6 3.6M10.6 4.4 14.2 8l-3.6 3.6',
  module: 'M8 1.6l5.8 3.2v6.4L8 14.4l-5.8-3.2V4.8zM2.2 4.8 8 8l5.8-3.2M8 8v6.4',
  runtime: 'M1.8 2.8h12.4v10.4H1.8zM4.4 6.2l2.2 1.9-2.2 1.9M8.4 10.6h3.2',
  sheet: 'M10.6 1.9l3.5 3.5-6.8 6.8-3.5-3.5zM3.8 8.7 2 14l5.3-1.8',
  website: 'M1.6 2.8h12.8v10.4H1.6zM1.6 5.8h12.8M3.6 4.3h.1M5.4 4.3h.1',
  docs: 'M8 4.2C6.6 3 4.6 2.6 2 2.8v9.8c2.6-.2 4.6.2 6 1.4 1.4-1.2 3.4-1.6 6-1.4V2.8c-2.6-.2-4.6.2-6 1.4zM8 4.2V14',
  repo: 'M1.8 3.6h4.4l1.5 1.7h6.5v7.9H1.8z',
  branch: 'M5 5.2v5.6M3.4 3.6a1.6 1.6 0 1 0 3.2 0 1.6 1.6 0 1 0-3.2 0M3.4 12.4a1.6 1.6 0 1 0 3.2 0 1.6 1.6 0 1 0-3.2 0M9.4 5.2a1.6 1.6 0 1 0 3.2 0 1.6 1.6 0 1 0-3.2 0M11 6.8c0 2.8-6 1.8-6 4',
  store: 'M2.8 3.8C2.8 2.6 5.1 1.8 8 1.8s5.2.8 5.2 2v8.4c0 1.2-2.3 2-5.2 2s-5.2-.8-5.2-2zM2.8 3.8c0 1.2 2.3 2 5.2 2s5.2-.8 5.2-2M2.8 8c0 1.2 2.3 2 5.2 2s5.2-.8 5.2-2',
  ci: 'M13 8a5 5 0 0 1-8.7 3.4M3 8a5 5 0 0 1 8.7-3.4M11.8 1.8v2.8H9M4.2 14.2v-2.8H7',
  person: 'M8 7.4a2.7 2.7 0 1 0 0-5.4 2.7 2.7 0 0 0 0 5.4zM2.6 14.2c.4-3 2.7-4.8 5.4-4.8s5 1.8 5.4 4.8',
};

/** Glyphs inside an eval chip, centred on 0,0. */
const GLYPH = {
  runtime: 'M-1.6-2.8 1.6 0-1.6 2.8',
  agent: 'M0-3.4l.9 2.5 2.5.9-2.5.9L0 3.4l-.9-2.5-2.5-.9 2.5-.9z',
  human: 'M0-.6a1.6 1.6 0 1 0 0-3.2 1.6 1.6 0 0 0 0 3.2zM-2.8 3.4c.3-1.9 1.4-2.9 2.8-2.9s2.5 1 2.8 2.9',
  reviewed: 'M-2.8.2-.8 2.2 2.9-2',
  reused: 'M2.6-.9A2.8 2.8 0 1 0 2.4 1.6M2.9-3.1v2.3H.6',
  stale: 'M0-3.1V.6M0 2.8v.1',
};

/**
 * @param {string} tag
 * @param {Record<string, string | number>} [attrs]
 * @param {Element} [parent]
 * @returns {SVGElement}
 */
function el(tag, attrs, parent) {
  const node = /** @type {SVGElement} */ (document.createElementNS(NS, tag));
  for (const [k, v] of Object.entries(attrs || {})) node.setAttribute(k, String(v));
  if (parent) parent.appendChild(node);
  return node;
}

/** @param {number} a @param {number} b @param {number} t */
const lerp = (a, b, t) => a + (b - a) * t;
/** @param {number} v @param {number} lo @param {number} hi */
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
/** @param {number} t */
const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
/** @param {number} v */
const r1 = (v) => Math.round(v * 10) / 10;

/** Point on a quadratic curve. @param {Curve} c @param {number} t */
function at(c, t) {
  const u = 1 - t;
  return { x: u * u * c.ax + 2 * u * t * c.cx + t * t * c.bx, y: u * u * c.ay + 2 * u * t * c.cy + t * t * c.by };
}

/** The part of a curve from 0 to t, as a path. @param {Curve} c @param {number} t */
function partial(c, t) {
  const cx = lerp(c.ax, c.cx, t);
  const cy = lerp(c.ay, c.cy, t);
  const end = at(c, t);
  return `M${r1(c.ax)} ${r1(c.ay)}Q${r1(cx)} ${r1(cy)} ${r1(end.x)} ${r1(end.y)}`;
}

/**
 * The curve between two boxes, starting and ending at their borders.
 * @param {Box} a @param {Box} b @param {number} bend @param {number} gap
 * @returns {Curve | null}
 */
function curveBetween(a, b, bend, gap) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy);
  if (len < 1) return null;
  /** @param {Box} r */
  const exit = (r) => Math.min(dx ? r.w / 2 / Math.abs(dx) : Infinity, dy ? r.h / 2 / Math.abs(dy) : Infinity);
  const ta = exit(a) + gap / len;
  const tb = exit(b) + gap / len;
  if (ta + tb >= 1) return null;
  const ax = a.x + dx * ta;
  const ay = a.y + dy * ta;
  const bx = b.x - dx * tb;
  const by = b.y - dy * tb;
  const l = Math.hypot(bx - ax, by - ay);
  const cx = (ax + bx) / 2 + (-(by - ay) / l) * bend * l;
  const cy = (ay + by) / 2 + ((bx - ax) / l) * bend * l;
  return { ax, ay, cx, cy, bx, by };
}

export class StoryGraph {
  /**
   * @param {SVGSVGElement} svg  The empty <svg> to draw into.
   * @param {Scenario} scenario
   * @param {Snapshot[]} snapshots
   * @param {{ reducedMotion?: boolean, hud?: Element | null }} [options]
   */
  constructor(svg, scenario, snapshots, options = {}) {
    this.svg = svg;
    this.sc = scenario;
    this.snaps = snapshots;
    this.reduced = !!options.reducedMotion;
    this.hud = options.hud || null;
    /** @type {Map<string, StoryNode>} */
    this.nodeDef = new Map(scenario.nodes.map((n) => [n.id, n]));
    /** Each eval as drawn: an edge `from` → `to` (its first dependency), or a loop on `from`. */
    /** @type {Map<string, { from: string, to: string | null, kind: StoryEval['kind'], bend?: number, side: 'top' | 'right' | 'bottom' | 'left', at: number, by?: string }>} */
    this.edgeDef = new Map(
      scenario.evals.map((e) => [
        evalId(e),
        { from: e.on, to: e.deps?.[0] ?? null, kind: e.kind, bend: e.bend, side: e.side || 'top', at: e.at || 0, by: e.by },
      ]),
    );
    /** @type {Map<string, string[]>} */
    this.children = new Map();
    for (const n of scenario.nodes) {
      if (!n.parent) continue;
      const list = this.children.get(n.parent) || [];
      list.push(n.id);
      this.children.set(n.parent, list);
    }
    /** Deepest groups first, so a frame can wrap the frames inside it. */
    this.frameOrder = [...this.children.keys()].sort((a, b) => this.depth(b) - this.depth(a));
    this.maxNaive = Math.max(1, ...snapshots.map((s) => s.counts.cumNaive));

    svg.textContent = '';
    /** @param {string} name */
    const layer = (name) => el('g', { class: `layer-${name}` }, svg);
    this.layers = {
      lanes: layer('lanes'),
      frames: layer('frames'),
      flows: layer('flows'),
      edges: layer('edges'),
      chips: layer('chips'),
      nodes: layer('nodes'),
      fx: layer('fx'),
      badges: layer('badges'),
    };

    this.W = 0;
    this.H = 0;
    this.k = 1;
    this.padX = 58;
    this.padY = 26;
    /** @type {Map<string, Geo>} */ this.geo = new Map();
    /** @type {Map<string, Geo>} */ this.geoA = new Map();
    /** @type {Map<string, Geo>} */ this.geoB = new Map();
    /** @type {Map<string, Box>} */ this.box = new Map();
    /** @type {Map<string, { o: number, g: number, o0: number, o1: number, g0: number, g1: number, at: number, dur: number }>} */
    this.edgeAnim = new Map();
    /** @type {Map<string, EvalState>} */ this.edgeState = new Map();
    /** @type {Map<string, Curve>} */ this.curves = new Map();
    /** @type {Map<string, { o: number, o0: number, o1: number }>} */ this.overlayAnim = new Map();
    /** @type {Map<string, SVGElement & { _parts?: Record<string, SVGElement> }>} */ this.els = new Map();
    /** @type {{ at: number, run: () => void }[]} */ this.timeline = [];
    /** @type {{ at: number, dur: number, draw: (p: number) => void, end?: () => void }[]} */ this.tracks = [];
    /** @type {Snapshot | null} */ this.snap = null;
    /** @type {Set<string>} */ this.changed = new Set();
    this.t0 = 0;
    this.moving = false;
    this.visible = true;
    this.raf = 0;
    this.tick = this.tick.bind(this);

    if (typeof ResizeObserver === 'function') {
      new ResizeObserver(() => this.resize()).observe(svg);
    }
    if (typeof IntersectionObserver === 'function') {
      new IntersectionObserver((entries) => {
        this.visible = entries[entries.length - 1].isIntersecting;
        if (this.visible) this.wake();
      }).observe(svg);
    }
    this.resize();
  }

  /** @param {string} id */
  depth(id) {
    let d = 0;
    for (let p = this.nodeDef.get(id)?.parent; p; p = this.nodeDef.get(p)?.parent) d++;
    return d;
  }

  /** Reads the drawing area's size. Called on resize only. */
  resize() {
    const r = this.svg.getBoundingClientRect();
    if (!r.width || !r.height) return;
    this.W = r.width;
    this.H = r.height;
    this.k = clamp(Math.min(this.W, this.H * 1.15) / 640, 0.8, 1.06);
    this.svg.setAttribute('viewBox', `0 0 ${r1(this.W)} ${r1(this.H)}`);
    this.padX = 58 * this.k;
    this.padY = 26 * this.k;
    this.draw(true);
  }

  /** @param {number} x */
  px(x) {
    return this.padX + (x / 100) * (this.W - 2 * this.padX);
  }

  /** @param {number} y */
  py(y) {
    return this.padY + (y / 100) * (this.H - 2 * this.padY);
  }

  /**
   * Shows a step: animates from what is on the stage to the step's state.
   * @param {string} id  A step id.
   * @param {{ instant?: boolean }} [opts]
   */
  show(id, opts = {}) {
    const snap = this.snaps.find((s) => s.id === id);
    if (!snap || snap === this.snap) return;
    const instant = !!opts.instant || this.reduced || !this.snap;
    if (this.reduced && this.snap && !opts.instant) {
      this.svg.classList.add('is-fading');
      window.setTimeout(() => {
        this.apply(snap, true);
        this.svg.classList.remove('is-fading');
      }, T.crossfade);
      return;
    }
    this.apply(snap, instant);
  }

  /**
   * @param {Snapshot} snap
   * @param {boolean} instant
   */
  apply(snap, instant) {
    const prev = this.snap;
    this.snap = snap;
    this.timeline = [];
    this.tracks = [];
    this.t0 = performance.now();
    for (const ring of this.layers.fx.querySelectorAll('.fx-ring, .fx-dot')) ring.remove();

    // Nodes: where each one starts and where it goes.
    const ids = new Set([...this.geo.keys(), ...snap.nodes]);
    for (const id of ids) {
      this.ensureNode(id);
      const target = snap.pos.get(id);
      const isFrame = snap.frames.has(id);
      const present = snap.nodes.has(id);
      let cur = this.geo.get(id);
      if (!cur) {
        const src = snap.emerge.get(id);
        const from = (src && (this.geo.get(src) || snap.pos.get(src))) || target || { x: 50, y: 50, s: 1 };
        cur = { x: from.x, y: from.y, s: (target?.s ?? 1) * (src ? 0.5 : 0.82), o: 0, fo: 0 };
        this.geo.set(id, cur);
      }
      const goal = target || cur;
      this.geoA.set(id, { ...cur });
      this.geoB.set(id, {
        x: goal.x,
        y: goal.y,
        s: goal.s,
        o: present && !isFrame ? 1 : 0,
        fo: present && isFrame ? 1 : 0,
      });
      // A node that splits shrinks into its parts; one that merges moves into its target.
      if (!present && prev?.nodes.has(id)) {
        const into = [...snap.emerge].filter(([, src]) => src === id).map(([n]) => snap.pos.get(n));
        const mergedInto = snap.merge.get(id);
        const first = mergedInto ? snap.pos.get(mergedInto) : into.find(Boolean);
        if (first) Object.assign(/** @type {Geo} */ (this.geoB.get(id)), { x: first.x, y: first.y, s: first.s * 0.8 });
      }
      const g = this.els.get(id);
      if (g) {
        g.classList.toggle('is-sketch', snap.sketch.has(id));
        this.setFp(id, snap.fp.get(id));
        const label = snap.labels.get(id) || this.nodeDef.get(id)?.label || '';
        const fl = this.els.get(`f:${id}`)?._parts?.label;
        if (g._parts?.label) g._parts.label.textContent = label;
        if (fl) fl.textContent = label;
      }
    }
    this.moving = !instant;
    if (instant) {
      for (const [id, b] of this.geoB) this.geo.set(id, { ...b });
    }

    // Edges: grow the new ones, fade the old ones, and set verdicts.
    const edgeIds = new Set([...this.edgeAnim.keys(), ...snap.evals.keys()]);
    let order = 0;
    for (const id of edgeIds) {
      this.ensureEdge(id);
      const a = this.edgeAnim.get(id) || { o: 0, g: 0, o0: 0, o1: 0, g0: 0, g1: 0, at: 0, dur: 1 };
      const present = snap.evals.has(id);
      const shown = a.o > 0.5 && a.g > 0.99;
      if (present && !shown) {
        a.o0 = 1;
        a.o1 = 1;
        a.g0 = instant ? 1 : 0;
        a.g1 = 1;
        a.at = T.edgeDelay + order * 70;
        a.dur = T.edgeGrow;
        order++;
      } else {
        a.o0 = a.o;
        a.o1 = present ? 1 : 0;
        a.g0 = a.g;
        a.g1 = present ? 1 : a.g;
        a.at = 0;
        a.dur = T.fade;
      }
      if (instant) {
        a.o = a.o1;
        a.g = a.g1;
      }
      this.edgeAnim.set(id, a);
      if (!present) continue;
      const final = /** @type {EvalState} */ (snap.evals.get(id));
      const rippled = snap.wave1.includes(id) || snap.wave2.includes(id);
      if (instant) {
        this.setEdgeState(id, final);
      } else if (!shown) {
        this.setEdgeState(id, rippled ? 'reused' : 'pending');
        if (!rippled) this.later(a.at + a.dur + T.verdict, () => this.setEdgeState(id, final));
      } else if (!rippled) {
        this.setEdgeState(id, final);
      }
    }

    // Overlays fade in and out.
    for (const id of new Set([...this.overlayAnim.keys(), ...snap.overlays])) {
      this.ensureOverlay(id);
      const a = this.overlayAnim.get(id) || { o: 0, o0: 0, o1: 0 };
      a.o0 = a.o;
      a.o1 = snap.overlays.has(id) ? 1 : 0;
      if (instant) a.o = a.o1;
      this.overlayAnim.set(id, a);
    }

    this.changed.clear();
    for (const g of this.els.values()) g.classList.remove('is-changed');
    // Let new edges grow and merging nodes arrive before a ripple starts.
    const settle = snap.fresh.size || snap.merge.size ? T.edgeDelay + T.edgeGrow : 0;
    if (snap.ripple.length && !instant) this.ripple(snap, T.ripple + settle);
    this.syncNodeStates();
    this.updateHud(snap.counts);
    this.draw(true);
    this.wake();
  }

  /**
   * Schedules the ripple of a fingerprint change, `delay` ms after now.
   * @param {Snapshot} snap
   * @param {number} delay
   */
  ripple(snap, delay) {
    const ids = snap.ripple;
    const base = performance.now() - this.t0 + delay;
    const all = [...snap.wave1, ...snap.wave2];
    for (const id of ids) this.setFp(id, snap.oldFp.get(id));
    for (const e of all) this.setEdgeState(e, 'reused');
    this.syncNodeStates();
    this.later(base, () => {
      for (const id of ids) {
        this.changed.add(id);
        this.els.get(id)?.classList.add('is-changed');
        this.setFp(id, snap.fp.get(id));
        this.ring(id);
      }
    });
    snap.wave1.forEach((e, i) => this.later(base + 260 + i * 90, () => this.setEdgeState(e, 'stale')));
    snap.wave2.forEach((e, i) => {
      this.later(base + 420 + i * 90, () => this.pulse(e, ids));
      this.later(base + 760 + i * 90, () => this.setEdgeState(e, 'stale'));
    });
    // The changed Artifacts' own evals settle first; the evals that depend on them wait, then
    // settle: reviewed, or reused when the step marks a verdict found elsewhere.
    // Human reviews wait for a person, so they settle last.
    /** @param {string} e */
    const settle = (e) => this.setEdgeState(e, snap.evals.get(e) || 'reviewed');
    /** @param {string} e */
    const human = (e) => this.edgeDef.get(e)?.kind === 'human';
    // Order: the changed Artifacts' own evals, then any sign-off (a person takes a
    // moment), then the evals that depend on them, which waited for both.
    snap.wave1.filter((e) => !human(e)).forEach((e, i) => this.later(base + 1500 + i * 120, () => settle(e)));
    const second = base + 2300 + snap.wave1.length * 120;
    const signs = all.filter(human);
    signs.forEach((e, i) => this.later(second + i * 200, () => settle(e)));
    const third = second + signs.length * 200 + (signs.length ? 500 : 0);
    snap.wave2.filter((e) => !human(e)).forEach((e, i) => this.later(third + i * 140, () => settle(e)));
    const end = third + snap.wave2.length * 140;
    this.later(end + 200, () => {
      for (const id of ids) {
        this.changed.delete(id);
        this.els.get(id)?.classList.remove('is-changed');
      }
      this.syncNodeStates();
    });
    this.later(end + T.hold, () => {
      if (this.snap === snap) this.ripple(snap, 400);
    });
  }

  /** @param {number} atMs Milliseconds after the step started. @param {() => void} run */
  later(atMs, run) {
    this.timeline.push({ at: atMs, run });
  }

  /** Expanding rings around a changed node. @param {string} id */
  ring(id) {
    for (let i = 0; i < 2; i++) {
      const c = el('circle', { class: 'fx-ring', r: 0, opacity: 0 }, this.layers.fx);
      this.tracks.push({
        at: performance.now() - this.t0 + i * 260,
        dur: 1100,
        draw: (p) => {
          const b = this.box.get(id);
          if (!b) return;
          c.setAttribute('cx', String(r1(b.x)));
          c.setAttribute('cy', String(r1(b.y)));
          c.setAttribute('r', String(r1(Math.max(b.w, b.h) / 2 + ease(p) * 70 * this.k)));
          c.setAttribute('opacity', String(r1((1 - p) * 10) / 10));
        },
        end: () => c.remove(),
      });
    }
  }

  /** A dot that runs from a changed node along an eval reviewed against it. @param {string} edge @param {string[]} changed */
  pulse(edge, changed) {
    const to = this.edgeDef.get(edge)?.to;
    if (!to || !changed.includes(to)) return;
    const d = el('circle', { class: 'fx-dot', r: 3.4 * this.k, opacity: 0 }, this.layers.fx);
    this.tracks.push({
      at: performance.now() - this.t0,
      dur: 520,
      draw: (p) => {
        const c = this.curves.get(edge);
        if (!c) return;
        const pt = at(c, 1 - ease(p));
        d.setAttribute('cx', String(r1(pt.x)));
        d.setAttribute('cy', String(r1(pt.y)));
        d.setAttribute('opacity', '1');
      },
      end: () => d.remove(),
    });
  }

  /** @param {string} id @param {EvalState} state */
  setEdgeState(id, state) {
    if (this.edgeState.get(id) === state) return;
    this.edgeState.set(id, state);
    const line = this.els.get(`e:${id}`);
    const chip = this.els.get(`c:${id}`);
    for (const g of [line, chip]) {
      if (!g) continue;
      g.classList.remove('st-pending', 'st-stale', 'st-reviewed', 'st-reused');
      g.classList.add(`st-${state}`);
    }
    const inner = chip?.firstElementChild;
    if (inner && state === 'reviewed' && !this.reduced && typeof inner.animate === 'function') {
      inner.animate([{ transform: 'scale(1.7)' }, { transform: 'scale(1)' }], { duration: 420, easing: 'cubic-bezier(.2,.9,.3,1.25)' });
    }
    this.syncNodeStates();
  }

  /** A node shows the most urgent state of the evals that review it. */
  syncNodeStates() {
    if (!this.snap) return;
    /** @type {Map<string, string>} */
    const st = new Map();
    const rank = { pending: 1, reused: 2, reviewed: 3, stale: 4 };
    for (const [id, state] of this.edgeState) {
      if (!this.snap.evals.has(id)) continue;
      const from = this.edgeDef.get(id)?.from || '';
      const prev = st.get(from);
      if (!prev || rank[state] > rank[/** @type {EvalState} */ (prev)]) st.set(from, state);
    }
    for (const [id, g] of this.els) {
      if (id.includes(':')) continue;
      const state = this.changed.has(id) ? 'stale' : st.get(id) || 'idle';
      for (const target of [g, this.els.get(`f:${id}`)]) {
        if (!target) continue;
        const want = `ns-${state}`;
        if (target.classList.contains(want)) continue;
        target.classList.remove('ns-idle', 'ns-pending', 'ns-stale', 'ns-reviewed', 'ns-reused');
        target.classList.add(want);
      }
    }
  }

  /** @param {string} id @param {string | undefined} fp */
  setFp(id, fp) {
    if (!fp) return;
    const g = this.els.get(id);
    const def = this.nodeDef.get(id);
    if (!g || !def || def.sub || !fp) return;
    const sub = g._parts?.sub;
    if (sub) sub.textContent = g.classList.contains('is-sketch') ? '·····' : fp.slice(0, 5);
  }

  /** @param {string} id */
  ensureNode(id) {
    if (this.els.has(id)) return;
    const def = this.nodeDef.get(id);
    if (!def) return;
    const isArtifact = !NON_ARTIFACT.has(def.kind);
    const hasSub = !!def.sub || isArtifact;
    const labelW = def.label.length * CARD.label * CHAR;
    const subW = hasSub ? (def.sub ? def.sub.length : 5) * CARD.sub * CHAR : 0;
    const base = CARD.padX * 2 + CARD.icon + CARD.gap;
    const rx = def.kind === 'person' ? CARD.h2 / 2 : 9;
    const g = el('g', { class: `sn k-${def.kind} ns-idle`, opacity: 0 }, this.layers.nodes);
    const ring = el('rect', { class: 'sn-ring', rx: rx + 3 }, g);
    const card = el('rect', { class: 'sn-card', rx }, g);
    const icon = el('g', { class: 'sn-icon' }, g);
    el('path', { d: ICONS[def.kind] || ICONS.doc }, icon);
    const text = el('g', { class: 'sn-text' }, g);
    const label = el('text', { class: 'sn-label' }, text);
    label.textContent = def.label;
    const sub = el('text', { class: 'sn-sub' }, text);
    sub.textContent = def.sub || '';
    /** @type {SVGElement & { _parts?: Record<string, SVGElement>, _w?: number, _wShort?: number, _sub?: boolean }} */
    const gx = g;
    gx._parts = { ring, card, icon, text, label, sub };
    gx._w = base + Math.max(labelW, subW);
    gx._wShort = base + labelW;
    gx._sub = hasSub;
    this.els.set(id, gx);

    if (this.children.has(id)) {
      const f = el('g', { class: `sf k-${def.kind} ns-idle`, opacity: 0 }, this.layers.frames);
      const fr = el('rect', { class: 'sf-box', rx: 14 }, f);
      const ficon = el('g', { class: 'sf-icon' }, f);
      el('path', { d: ICONS[def.kind] || ICONS.repo }, ficon);
      const fl = el('text', { class: 'sf-label' }, f);
      fl.textContent = def.label;
      /** @type {SVGElement & { _parts?: Record<string, SVGElement> }} */
      const fx = f;
      fx._parts = { box: fr, icon: ficon, label: fl };
      this.els.set(`f:${id}`, fx);
      // Outer frames sit below inner ones.
      for (const fid of [...this.frameOrder].reverse()) {
        const fe = this.els.get(`f:${fid}`);
        if (fe) this.layers.frames.appendChild(fe);
      }
    }
  }

  /** @param {string} id */
  ensureEdge(id) {
    if (this.els.has(`e:${id}`)) return;
    const def = this.edgeDef.get(id);
    if (!def) return;
    const loop = !def.to;
    const g = el('g', { class: `se k-${def.kind} st-reused${loop ? ' is-loop' : ''}`, opacity: 0 }, this.layers.edges);
    const line = el('path', { class: 'se-line' }, g);
    // An edge ends in a dot at the dependency; a loop returns to its node with an arrowhead.
    const dot = el(loop ? 'path' : 'circle', loop ? { class: 'se-head' } : { class: 'se-end', r: 2.4 }, g);
    /** @type {SVGElement & { _parts?: Record<string, SVGElement> }} */
    const gx = g;
    gx._parts = { line, dot };
    if (loop && def.by) {
      // A Human review names its reviewer: people are marks, not nodes.
      const mark = el('g', { class: 'se-by' }, this.layers.badges);
      el('circle', { class: 'by-face', r: 6.5 }, mark);
      const initial = el('text', { class: 'by-initial' }, mark);
      initial.textContent = def.by.charAt(0);
      const name = el('text', { class: 'by-name' }, mark);
      name.textContent = def.by;
      gx._parts.mark = mark;
      gx._parts.initial = initial;
      gx._parts.name = name;
    }
    this.els.set(`e:${id}`, gx);

    const c = el('g', { class: `sc k-${def.kind} st-reused`, opacity: 0 }, this.layers.chips);
    const inner = el('g', { class: 'sc-in' }, c);
    el('circle', { class: 'sc-bg', r: 8.5 }, inner);
    for (const name of [def.kind, 'reviewed', 'reused', 'stale']) {
      el('path', { class: `sc-g g-${name === def.kind ? 'kind' : name}`, d: GLYPH[/** @type {keyof typeof GLYPH} */ (name)] }, inner);
    }
    this.els.set(`c:${id}`, c);
  }

  /** @param {string} id */
  ensureOverlay(id) {
    if (this.els.has(`o:${id}`)) return;
    const o = this.sc.overlays[id];
    if (!o) return;
    /** @type {SVGElement & { _parts?: Record<string, SVGElement> }} */
    let g;
    if (o.type === 'flow') {
      g = el('g', { class: `ov-flow`, opacity: 0 }, this.layers.flows);
      const line = el('path', { class: 'fl-line' }, g);
      /** @type {Record<string, SVGElement>} */
      const parts = { line };
      for (let i = 0; i < 3; i++) {
        parts[`a${i}`] = el('circle', { class: `fl-dot t-${o.tone}`, r: 3, opacity: 0 }, g);
        if (o.back) parts[`b${i}`] = el('circle', { class: `fl-dot t-${o.back}`, r: 3, opacity: 0 }, g);
      }
      g._parts = parts;
    } else if (o.type === 'badge') {
      g = el('g', { class: `ov-badge t-${o.tone || 'note'}`, opacity: 0 }, this.layers.badges);
      const box = el('rect', { class: 'bd-box', rx: 9 }, g);
      const text = el('text', { class: 'bd-text' }, g);
      text.textContent = o.text;
      g._parts = { box, text };
    } else {
      g = el('g', { class: 'ov-lane', opacity: 0 }, this.layers.lanes);
      const box = el('rect', { class: 'ln-box', rx: 12 }, g);
      const label = el('text', { class: 'ln-label' }, g);
      label.textContent = o.label;
      const pill = el('rect', { class: 'ln-pill', rx: 8 }, g);
      const text = el('text', { class: 'ln-text' }, g);
      g._parts = { box, label, pill, text };
    }
    this.els.set(`o:${id}`, g);
  }

  /** Starts the frame loop if anything needs drawing. */
  wake() {
    if (!this.raf && this.visible) this.raf = requestAnimationFrame(this.tick);
  }

  /** @param {number} now */
  tick(now) {
    this.raf = 0;
    const t = now - this.t0;
    let busy = false;

    if (this.moving) {
      const p = clamp(t / T.move, 0, 1);
      const e = ease(p);
      for (const [id, b] of this.geoB) {
        const a = this.geoA.get(id) || b;
        this.geo.set(id, {
          x: lerp(a.x, b.x, e),
          y: lerp(a.y, b.y, e),
          s: lerp(a.s, b.s, e),
          o: lerp(a.o, b.o, clamp(p * 1.6, 0, 1)),
          fo: lerp(a.fo, b.fo, clamp(p * 1.6, 0, 1)),
        });
      }
      if (p >= 1) this.moving = false;
      busy = true;
    }
    for (const a of this.edgeAnim.values()) {
      const p = clamp((t - a.at) / a.dur, 0, 1);
      a.o = lerp(a.o0, a.o1, p);
      a.g = lerp(a.g0, a.g1, ease(p));
      if (p < 1) busy = true;
    }
    for (const a of this.overlayAnim.values()) {
      const p = clamp((t - T.edgeDelay) / T.fade, 0, 1);
      a.o = lerp(a.o0, a.o1, p);
      if (p < 1) busy = true;
    }

    const due = this.timeline.filter((item) => item.at <= t);
    if (due.length) {
      this.timeline = this.timeline.filter((item) => item.at > t);
      for (const item of due) item.run();
    }
    this.draw(busy);
    if (this.timeline.length) busy = true;

    this.tracks = this.tracks.filter((tr) => {
      if (tr.at > t) return true;
      const p = clamp((t - tr.at) / tr.dur, 0, 1);
      tr.draw(p);
      if (p < 1) return true;
      tr.end?.();
      return false;
    });
    if (this.tracks.length) busy = true;

    const flows = this.drawFlows(now);
    if ((busy || flows) && this.visible) this.raf = requestAnimationFrame(this.tick);
  }

  /**
   * Writes every node, frame, edge and overlay. With `full` false, nothing
   * moved, so only what animates on its own is updated.
   * @param {boolean} full
   */
  draw(full) {
    if (!full || !this.W) return;
    const k = this.k;

    // Cards.
    for (const [id, geo] of this.geo) {
      const g = /** @type {SVGElement & { _parts: Record<string, SVGElement>, _w: number, _wShort: number, _sub: boolean } | undefined} */ (this.els.get(id));
      if (!g) continue;
      const mini = clamp((0.78 - geo.s) / 0.26, 0, 1);
      const sc = geo.s * k;
      // Cards drawn small drop their second line (the fingerprint) to save room.
      const two = g._sub ? clamp((sc - 0.8) / 0.1, 0, 1) : 0;
      const w = lerp(lerp(g._wShort, g._w, two), CARD.miniW, mini);
      const h = lerp(lerp(CARD.h1, CARD.h2, two), CARD.miniH, mini);
      const x = this.px(geo.x);
      const y = this.py(geo.y);
      g.setAttribute('transform', `translate(${r1(x)} ${r1(y)}) scale(${Math.round(sc * 1000) / 1000})`);
      g.setAttribute('opacity', String(Math.round(geo.o * 100) / 100));
      this.box.set(id, { x, y, w: w * sc, h: h * sc });
      const p = g._parts;
      for (const r of [p.card, p.ring]) {
        const grow = r === p.ring ? 4 : 0;
        r.setAttribute('x', String(r1(-w / 2 - grow)));
        r.setAttribute('y', String(r1(-h / 2 - grow)));
        r.setAttribute('width', String(r1(w + grow * 2)));
        r.setAttribute('height', String(r1(h + grow * 2)));
      }
      const left = -w / 2 + CARD.padX;
      const iconX = lerp(left, -CARD.icon / 2, mini);
      p.icon.setAttribute('transform', `translate(${r1(iconX)} -8)`);
      p.text.setAttribute('opacity', String(r1(1 - mini)));
      const tx = left + CARD.icon + CARD.gap;
      p.label.setAttribute('x', String(r1(tx)));
      p.label.setAttribute('y', String(r1(lerp(0.5, -6, two))));
      p.sub.setAttribute('opacity', String(r1(two)));
      p.sub.setAttribute('x', String(r1(tx)));
      p.sub.setAttribute('y', '9');
    }

    // Frames wrap their visible children, deepest first.
    for (const id of this.frameOrder) {
      const f = /** @type {SVGElement & { _parts: Record<string, SVGElement> } | undefined} */ (this.els.get(`f:${id}`));
      const geo = this.geo.get(id);
      if (!f || !geo) continue;
      const card = this.box.get(id);
      let x0 = Infinity;
      let y0 = Infinity;
      let x1 = -Infinity;
      let y1 = -Infinity;
      let s = 0;
      for (const c of this.children.get(id) || []) {
        const cg = this.geo.get(c);
        const cb = this.box.get(c);
        if (!cg || !cb || Math.max(cg.o, cg.fo) < 0.02) continue;
        x0 = Math.min(x0, cb.x - cb.w / 2);
        y0 = Math.min(y0, cb.y - cb.h / 2);
        x1 = Math.max(x1, cb.x + cb.w / 2);
        y1 = Math.max(y1, cb.y + cb.h / 2);
        s = Math.max(s, cg.s);
      }
      const def = this.nodeDef.get(id);
      const ls = def?.kind === 'repo' || def?.kind === 'branch' ? 1 : clamp(s, 0.5, 1);
      const pad = 12 * k * Math.max(s, 0.5);
      const top = (ls > 0.6 ? 20 : 4) * k * ls;
      let fb = card || { x: 0, y: 0, w: 0, h: 0 };
      if (x0 < Infinity) {
        fb = { x: (x0 + x1) / 2, y: (y0 + y1 - top) / 2, w: x1 - x0 + pad * 2, h: y1 - y0 + pad * 2 + top };
      }
      f.setAttribute('opacity', String(Math.round(geo.fo * 100) / 100));
      const p = f._parts;
      p.box.setAttribute('x', String(r1(fb.x - fb.w / 2)));
      p.box.setAttribute('y', String(r1(fb.y - fb.h / 2)));
      p.box.setAttribute('width', String(r1(fb.w)));
      p.box.setAttribute('height', String(r1(fb.h)));
      const lx = fb.x - fb.w / 2 + 9 * k * ls;
      const ly = fb.y - fb.h / 2 + 5 * k * ls;
      p.icon.setAttribute('transform', `translate(${r1(lx)} ${r1(ly)}) scale(${Math.round(0.78 * k * ls * 100) / 100})`);
      p.label.setAttribute('x', String(r1(lx + 17 * k * ls)));
      p.label.setAttribute('y', String(r1(ly + 6.5 * k * ls)));
      p.label.setAttribute('font-size', String(r1(11 * k * ls)));
      // A group drawn small (a frame inside a repository) keeps only its outline.
      const named = ls > 0.6 ? '1' : '0';
      p.label.setAttribute('opacity', named);
      p.icon.setAttribute('opacity', named);
      if (card && geo.fo > 0) {
        const t = geo.fo;
        this.box.set(id, { x: lerp(card.x, fb.x, t), y: lerp(card.y, fb.y, t), w: lerp(card.w, fb.w, t), h: lerp(card.h, fb.h, t) });
      }
    }

    // Edges and their verdict chips.
    for (const [id, a] of this.edgeAnim) {
      const g = /** @type {SVGElement & { _parts: Record<string, SVGElement> } | undefined} */ (this.els.get(`e:${id}`));
      const chip = this.els.get(`c:${id}`);
      const def = this.edgeDef.get(id);
      if (!g || !chip || !def) continue;
      if (!def.to) {
        this.drawLoop(id, g, chip, a);
        continue;
      }
      const ga = this.geo.get(def.from);
      const gb = this.geo.get(def.to);
      const ba = this.box.get(def.from);
      const bb = this.box.get(def.to);
      const vis = ga && gb ? Math.min(Math.max(ga.o, ga.fo), Math.max(gb.o, gb.fo)) * a.o : 0;
      const curve = ba && bb && vis > 0.01 ? curveBetween(ba, bb, def.bend ?? 0.12, 5 * k) : null;
      if (!curve || !ga || !gb) {
        g.setAttribute('opacity', '0');
        chip.setAttribute('opacity', '0');
        this.curves.delete(id);
        continue;
      }
      this.curves.set(id, curve);
      const mini = clamp((0.78 - Math.min(ga.s, gb.s)) / 0.26, 0, 1);
      g.setAttribute('opacity', String(Math.round(vis * 100) / 100));
      g.style.setProperty('--w', String(r1(lerp(1.7, 1.1, mini) * k)));
      g._parts.line.setAttribute('d', partial(curve, a.g));
      const end = at(curve, a.g);
      g._parts.dot.setAttribute('cx', String(r1(end.x)));
      g._parts.dot.setAttribute('cy', String(r1(end.y)));
      g._parts.dot.setAttribute('r', String(r1(2.4 * k * (1 - mini * 0.5))));
      // The chip sits near the Artifact the eval reviews, so chips of crossing edges stay apart.
      const span = Math.hypot(curve.bx - curve.ax, curve.by - curve.ay);
      const tc = clamp((46 * k) / Math.max(span, 1), 0.24, 0.5);
      const mid = at(curve, tc);
      const chipVis = vis * (1 - mini) * clamp((a.g - tc) / 0.2, 0, 1);
      chip.setAttribute('opacity', String(Math.round(chipVis * 100) / 100));
      chip.setAttribute('transform', `translate(${r1(mid.x)} ${r1(mid.y)}) scale(${Math.round(k * 100) / 100})`);
    }

    // Overlays: badges, lanes and the lines of flows.
    for (const [id, a] of this.overlayAnim) {
      const o = this.sc.overlays[id];
      const g = /** @type {SVGElement & { _parts: Record<string, SVGElement> } | undefined} */ (this.els.get(`o:${id}`));
      if (!o || !g) continue;
      g.setAttribute('opacity', String(Math.round(a.o * 100) / 100));
      if (o.type === 'badge') {
        const b = this.box.get(o.node);
        if (!b) continue;
        const fs = 10.5 * k;
        const w = o.text.length * fs * CHAR + 18 * k;
        const h = 19 * k;
        const side = o.side || 'bottom';
        const cx = side === 'left' ? b.x - b.w / 2 - 6 * k - w / 2 : side === 'right' ? b.x + b.w / 2 + 6 * k + w / 2 : b.x;
        const cy = side === 'top' ? b.y - b.h / 2 - 6 * k - h / 2 : side === 'bottom' ? b.y + b.h / 2 + 6 * k + h / 2 : b.y;
        g._parts.box.setAttribute('x', String(r1(cx - w / 2)));
        g._parts.box.setAttribute('y', String(r1(cy - h / 2)));
        g._parts.box.setAttribute('width', String(r1(w)));
        g._parts.box.setAttribute('height', String(r1(h)));
        g._parts.text.setAttribute('x', String(r1(cx)));
        g._parts.text.setAttribute('y', String(r1(cy + 0.5)));
        g._parts.text.setAttribute('font-size', String(r1(fs)));
      } else if (o.type === 'lane') {
        const h = 46 * k;
        const y = this.py(o.y);
        const x0 = 10;
        const x1 = this.W - 10;
        const p = g._parts;
        p.box.setAttribute('x', String(x0));
        p.box.setAttribute('y', String(r1(y - h / 2)));
        p.box.setAttribute('width', String(r1(x1 - x0)));
        p.box.setAttribute('height', String(r1(h)));
        const fs = 10.5 * k;
        const text = this.laneText(o.text || '');
        const pw = text.length * fs * CHAR + 18 * k;
        const ph = 22 * k;
        const lw = o.label.length * fs * CHAR;
        // Label then pill, at the lane's right end, or from its left end.
        const left = o.align === 'left';
        const pillX = left ? x0 + 14 * k + lw + 8 * k : x1 - 10 * k - pw;
        p.pill.setAttribute('x', String(r1(pillX)));
        p.pill.setAttribute('y', String(r1(y - ph / 2)));
        p.pill.setAttribute('width', String(r1(pw)));
        p.pill.setAttribute('height', String(r1(ph)));
        p.text.textContent = text;
        p.text.setAttribute('x', String(r1(pillX + pw / 2)));
        p.text.setAttribute('y', String(r1(y + 0.5)));
        p.text.setAttribute('font-size', String(r1(fs)));
        p.label.setAttribute('x', String(r1(pillX - 8 * k - lw)));
        p.label.setAttribute('y', String(r1(y + 0.5)));
        p.label.setAttribute('font-size', String(r1(fs)));
      } else {
        const ba = this.box.get(o.from);
        const bb = this.box.get(o.to);
        const curve = ba && bb ? curveBetween(ba, bb, o.bend ?? 0.08, 6 * k) : null;
        if (!curve) {
          this.curves.delete(`o:${id}`);
          continue;
        }
        this.curves.set(`o:${id}`, curve);
        g._parts.line.setAttribute('d', partial(curve, 1));
      }
    }
  }

  /**
   * An eval that reviews one Artifact alone: a loop that leaves the node on one
   * side and returns to it, with its verdict chip at the far end and, for a
   * Human review, the reviewer's mark beyond it.
   * @param {string} id
   * @param {SVGElement & { _parts: Record<string, SVGElement> }} g
   * @param {SVGElement} chip
   * @param {{ o: number, g: number }} a
   */
  drawLoop(id, g, chip, a) {
    const def = /** @type {NonNullable<ReturnType<typeof this.edgeDef.get>>} */ (this.edgeDef.get(id));
    const geo = this.geo.get(def.from);
    const b = this.box.get(def.from);
    const mark = g._parts.mark;
    const vis = geo && b ? Math.max(geo.o, geo.fo) * a.o * clamp(a.g * 1.6, 0, 1) : 0;
    if (!geo || !b || vis < 0.01) {
      g.setAttribute('opacity', '0');
      chip.setAttribute('opacity', '0');
      mark?.setAttribute('opacity', '0');
      return;
    }
    const mini = clamp((0.78 - geo.s) / 0.26, 0, 1);
    const sc = this.k * Math.max(geo.s, 0.5);
    // Outward normal n and tangent t of the side the loop sits on.
    const [nx, ny] = { top: [0, -1], right: [1, 0], bottom: [0, 1], left: [-1, 0] }[def.side];
    const tx = -ny;
    const ty = nx;
    const half = Math.abs(nx) * (b.w / 2) + Math.abs(ny) * (b.h / 2);
    const along = Math.abs(tx) * b.w + Math.abs(ty) * b.h;
    const bx = b.x + nx * half + tx * def.at * along;
    const by = b.y + ny * half + ty * def.at * along;
    const d = 6 * sc;
    const L = 30 * sc;
    const e = 9 * sc;
    const p1 = [bx - tx * d, by - ty * d];
    const p2 = [bx + tx * d, by + ty * d];
    const c1 = [p1[0] + nx * L - tx * e, p1[1] + ny * L - ty * e];
    const c2 = [p2[0] + nx * L + tx * e, p2[1] + ny * L + ty * e];
    g.setAttribute('opacity', String(Math.round(vis * 100) / 100));
    g.style.setProperty('--w', String(r1(lerp(1.7, 1.1, mini) * this.k)));
    g._parts.line.setAttribute('d', `M${r1(p1[0])} ${r1(p1[1])}C${r1(c1[0])} ${r1(c1[1])} ${r1(c2[0])} ${r1(c2[1])} ${r1(p2[0])} ${r1(p2[1])}`);
    // Arrowhead at the return point, along the curve's last direction.
    const ux = p2[0] - c2[0];
    const uy = p2[1] - c2[1];
    const ul = Math.hypot(ux, uy) || 1;
    const hx = (ux / ul) * 5 * sc;
    const hy = (uy / ul) * 5 * sc;
    g._parts.dot.setAttribute('d', `M${r1(p2[0])} ${r1(p2[1])}L${r1(p2[0] - hx - hy * 0.6)} ${r1(p2[1] - hy + hx * 0.6)}L${r1(p2[0] - hx + hy * 0.6)} ${r1(p2[1] - hy - hx * 0.6)}Z`);
    // The chip at the loop's far end (t = 0.5 of the cubic).
    const ax = 0.125 * p1[0] + 0.375 * c1[0] + 0.375 * c2[0] + 0.125 * p2[0];
    const ay = 0.125 * p1[1] + 0.375 * c1[1] + 0.375 * c2[1] + 0.125 * p2[1];
    chip.setAttribute('opacity', String(Math.round(vis * 100) / 100));
    chip.setAttribute('transform', `translate(${r1(ax)} ${r1(ay)}) scale(${Math.round(sc * lerp(0.82, 0.6, mini) * 100) / 100})`);
    if (mark && def.by) {
      // Avatar and name beyond the chip, in the same direction.
      const s = this.k;
      const w = (13 + 4 + def.by.length * 10 * CHAR) * s;
      const gap = 9 * sc + 5 * s;
      const mx = ax + nx * (gap + (Math.abs(nx) * w) / 2);
      const my = ay + ny * (gap + 8 * s);
      mark.setAttribute('transform', `translate(${r1(mx - w / 2)} ${r1(my)}) scale(${Math.round(s * 100) / 100})`);
      mark.setAttribute('opacity', String(Math.round(vis * (1 - mini) * 100) / 100));
      g._parts.initial.setAttribute('x', '6.5');
      g._parts.initial.setAttribute('y', '0.5');
      g._parts.name.setAttribute('x', '17');
      g._parts.name.setAttribute('y', '0.5');
      const face = mark.firstElementChild;
      face?.setAttribute('cx', '6.5');
    }
  }

  /** @param {string} template */
  laneText(template) {
    const c = /** @type {Record<string, number>} */ (/** @type {unknown} */ (this.snap?.counts || {}));
    return template.replace(/\{(\w+)\}/g, (_, key) => String(c[key] ?? ''));
  }

  /**
   * Moves the particles of every visible flow. Returns whether any flow is
   * running, which keeps the frame loop alive.
   * @param {number} now
   */
  drawFlows(now) {
    let running = false;
    for (const [id, a] of this.overlayAnim) {
      const o = this.sc.overlays[id];
      if (!o || o.type !== 'flow' || a.o < 0.01) continue;
      const g = /** @type {SVGElement & { _parts: Record<string, SVGElement> } | undefined} */ (this.els.get(`o:${id}`));
      const curve = this.curves.get(`o:${id}`);
      if (!g || !curve) continue;
      const still = this.reduced;
      running = running || !still;
      for (let i = 0; i < 3; i++) {
        for (const dir of ['a', 'b']) {
          const dot = g._parts[`${dir}${i}`];
          if (!dot) continue;
          let p = still ? (i + 0.5) / 3 : ((now / 2600 + i / 3 + (dir === 'b' ? 1 / 6 : 0)) % 1);
          if (dir === 'b') p = 1 - p;
          const pt = at(curve, p);
          dot.setAttribute('cx', String(r1(pt.x)));
          dot.setAttribute('cy', String(r1(pt.y)));
          dot.setAttribute('r', String(r1(2.8 * this.k)));
          dot.setAttribute('opacity', still ? '0.9' : String(Math.round(Math.sin(Math.PI * p) * 100) / 100));
        }
      }
    }
    return running;
  }

  /** @param {Counts} c */
  updateHud(c) {
    if (!this.hud) return;
    for (const node of this.hud.querySelectorAll('[data-hud]')) {
      const key = /** @type {keyof Counts} */ (node.getAttribute('data-hud'));
      if (key in c) node.textContent = String(c[key]);
    }
    for (const node of this.hud.querySelectorAll('[data-hud-bar]')) {
      const key = /** @type {keyof Counts} */ (node.getAttribute('data-hud-bar'));
      if (key in c) /** @type {HTMLElement} */ (node).style.transform = `scaleX(${c[key] / this.maxNaive})`;
    }
  }
}
