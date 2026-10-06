// @ts-check
// Turns the scenario's per-step changes into one full snapshot per step, and
// checks the scenario for mistakes the types cannot catch (an edge naming a
// node that does not exist, a step without copy, a node without a position).
// It knows node kinds and eval rules, never the story itself.

/** @typedef {import('./types.js').Scenario} Scenario */
/** @typedef {import('./types.js').StoryNode} StoryNode */
/** @typedef {import('./types.js').StoryEdge} StoryEdge */
/** @typedef {import('./types.js').EvalState} EvalState */
/** @typedef {import('./types.js').LayoutPart} LayoutPart */
/** @typedef {import('./types.js').Snapshot} Snapshot */
/** @typedef {import('./types.js').XYS} XYS */

/** Kinds that are not Artifacts: they never count as one and have no fingerprint. */
export const NON_ARTIFACT = new Set(['person', 'repo', 'branch', 'store', 'ci']);

/** @param {string | string[] | undefined} r */
const rippleList = (r) => (r === undefined ? [] : Array.isArray(r) ? r : [r]);

/** @type {readonly EvalState[]} */
const EVAL_STATES = ['pending', 'stale', 'reviewed', 'reused'];

/** @param {StoryEdge} e */
export const edgeId = (e) => `${e.from}->${e.to}`;

/**
 * A short, stable stand-in for a fingerprint.
 * @param {string} text
 */
export function hashHex(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, '0').slice(0, 6);
}

/**
 * Every problem found in the scenario, as readable sentences. An empty list
 * means the scenario is consistent with itself and with the page's sections.
 * @param {Scenario} sc
 * @param {string[]} [sectionIds] The page's `data-step` ids, in page order.
 * @returns {string[]}
 */
export function validateScenario(sc, sectionIds) {
  /** @type {string[]} */
  const out = [];
  const nodeIds = new Set();
  for (const n of sc.nodes) {
    if (nodeIds.has(n.id)) out.push(`node "${n.id}" is declared twice`);
    nodeIds.add(n.id);
  }
  for (const n of sc.nodes) {
    if (n.parent && !nodeIds.has(n.parent)) out.push(`node "${n.id}" has an unknown parent "${n.parent}"`);
    if (n.copyOf && !nodeIds.has(n.copyOf)) out.push(`node "${n.id}" copies an unknown node "${n.copyOf}"`);
  }
  const edgeIds = new Set();
  for (const e of sc.edges) {
    const id = edgeId(e);
    if (edgeIds.has(id)) out.push(`edge "${id}" is declared twice`);
    edgeIds.add(id);
    for (const end of [e.from, e.to]) {
      if (!nodeIds.has(end)) out.push(`edge "${id}" names an unknown node "${end}"`);
    }
  }
  for (const [id, o] of Object.entries(sc.overlays)) {
    const ends = o.type === 'flow' ? [o.from, o.to] : o.type === 'badge' ? [o.node] : [];
    for (const end of ends) {
      if (!nodeIds.has(end)) out.push(`overlay "${id}" names an unknown node "${end}"`);
    }
  }
  for (const [name, preset] of Object.entries(sc.layouts)) {
    const parts = [preset, ...(preset.parts || [])];
    for (const part of parts) {
      if (part.base && !sc.layouts[part.base]) out.push(`layout "${name}" builds on an unknown layout "${part.base}"`);
      for (const id of Object.keys(part.nodes || {})) {
        if (!nodeIds.has(id)) out.push(`layout "${name}" places an unknown node "${id}"`);
      }
    }
  }

  // Walk the steps the way buildSnapshots does, checking each reference.
  const present = new Set();
  const connected = new Set();
  const seenSteps = new Set();
  /** @param {string} stepId @param {string} what @param {string[] | undefined} ids @param {Set<string>} known */
  const known = (stepId, what, ids, known) => {
    for (const id of ids || []) {
      if (!known.has(id)) out.push(`step "${stepId}": ${what} "${id}" does not exist`);
    }
  };
  for (const step of sc.steps) {
    const s = step.id;
    if (seenSteps.has(s)) out.push(`step "${s}" appears twice`);
    seenSteps.add(s);
    if (!sc.layouts[step.layout]) out.push(`step "${s}" uses an unknown layout "${step.layout}"`);
    known(s, 'added node', step.add, nodeIds);
    known(s, 'removed node', step.remove, nodeIds);
    known(s, 'sketched node', step.sketch, nodeIds);
    known(s, 'edge', step.connect, edgeIds);
    known(s, 'edge', step.disconnect, edgeIds);
    known(s, 'overlay', step.show, new Set(Object.keys(sc.overlays)));
    known(s, 'overlay', step.hide, new Set(Object.keys(sc.overlays)));
    for (const id of step.add || []) present.add(id);
    if (step.split) {
      known(s, 'split node', [step.split.from, ...step.split.into], nodeIds);
      if (!present.has(step.split.from)) out.push(`step "${s}" splits "${step.split.from}", which is not on the stage`);
      for (const id of step.split.into) present.add(id);
      present.delete(step.split.from);
    }
    for (const [id, src] of Object.entries(step.emerge || {})) {
      if (!present.has(id)) out.push(`step "${s}": "${id}" emerges but is not added`);
      if (!present.has(src)) out.push(`step "${s}": "${id}" emerges from "${src}", which is not on the stage`);
    }
    for (const [id, into] of Object.entries(step.merge || {})) {
      if (!present.has(id)) out.push(`step "${s}" merges "${id}", which is not on the stage`);
      if (!present.has(into)) out.push(`step "${s}" merges "${id}" into "${into}", which is not on the stage`);
      present.delete(id);
    }
    for (const id of step.remove || []) present.delete(id);
    for (const id of [...connected]) {
      const [from, to] = id.split('->');
      if (!present.has(from) || !present.has(to)) connected.delete(id);
    }
    for (const id of step.disconnect || []) connected.delete(id);
    for (const id of step.connect || []) {
      const [from, to] = id.split('->');
      for (const end of [from, to]) {
        if (edgeIds.has(id) && !present.has(end)) out.push(`step "${s}" connects "${id}", but "${end}" is not on the stage`);
      }
      connected.add(id);
    }
    for (const [state, ids] of Object.entries(step.mark || {})) {
      if (!EVAL_STATES.includes(/** @type {EvalState} */ (state))) out.push(`step "${s}" marks an unknown state "${state}"`);
      for (const id of ids || []) {
        if (!connected.has(id)) out.push(`step "${s}" marks "${id}", which is not connected`);
      }
    }
    for (const id of rippleList(step.ripple)) {
      if (!present.has(id)) out.push(`step "${s}" ripples from "${id}", which is not on the stage`);
    }
    if (sc.layouts[step.layout]) {
      const pos = resolveLayout(sc, step.layout);
      const frames = framesOf(sc, present);
      for (const id of present) {
        if (!pos.has(id) && !frames.has(id)) out.push(`step "${s}": layout "${step.layout}" has no position for "${id}"`);
      }
    }
  }

  if (sectionIds) {
    const stepIds = sc.steps.map((x) => x.id);
    for (const id of sectionIds) {
      if (!seenSteps.has(id)) out.push(`index.html has <section data-step="${id}">, but scenario.js has no such step`);
    }
    for (const id of stepIds) {
      if (!sectionIds.includes(id)) out.push(`scenario.js has step "${id}", but index.html has no <section data-step="${id}">`);
    }
    const shared = sectionIds.filter((id) => seenSteps.has(id));
    const order = stepIds.filter((id) => sectionIds.includes(id));
    if (shared.join() !== order.join()) out.push(`the steps in index.html and scenario.js are in a different order`);
  }
  return out;
}

/**
 * Present nodes that have at least one present child.
 * @param {Scenario} sc
 * @param {Set<string>} present
 */
function framesOf(sc, present) {
  const frames = new Set();
  for (const n of sc.nodes) {
    if (n.parent && present.has(n.id) && present.has(n.parent)) frames.add(n.parent);
  }
  return frames;
}

/**
 * The positions a layout preset gives, after its `base`, `parts`, `fit` and `scale`.
 * @param {Scenario} sc
 * @param {string} name
 * @param {Set<string>} [seen] Guards against a preset that builds on itself.
 * @returns {Map<string, XYS>}
 */
export function resolveLayout(sc, name, seen = new Set()) {
  const preset = sc.layouts[name];
  /** @type {Map<string, XYS>} */
  const out = new Map();
  if (!preset || seen.has(name)) return out;
  seen.add(name);
  for (const part of [preset, ...(preset.parts || [])]) {
    for (const [id, p] of resolvePart(sc, part, seen)) out.set(id, p);
  }
  seen.delete(name);
  return out;
}

/**
 * @param {Scenario} sc
 * @param {LayoutPart} part
 * @param {Set<string>} seen
 * @returns {Map<string, XYS>}
 */
function resolvePart(sc, part, seen) {
  /** @type {Map<string, XYS>} */
  const out = new Map();
  if (part.base) {
    for (const [id, p] of resolveLayout(sc, part.base, seen)) out.set(id, { ...p });
  }
  for (const [id, pos] of Object.entries(part.nodes || {})) {
    out.set(id, { x: pos[0], y: pos[1], s: pos[2] ?? 1 });
  }
  if (part.fit) {
    const [x0, y0, x1, y1] = part.fit;
    for (const p of out.values()) {
      p.x = x0 + (p.x / 100) * (x1 - x0);
      p.y = y0 + (p.y / 100) * (y1 - y0);
    }
  }
  if (part.scale) {
    for (const p of out.values()) p.s *= part.scale;
  }
  return out;
}

/**
 * The evals a fingerprint change of the `changed` nodes makes stale: those
 * reviewing one, those reviewed against one, and those of a parent (a group's
 * evals depend on its children). Like artifactize's reuse key, it looks one
 * connection away.
 * @param {Scenario} sc
 * @param {Iterable<string>} edgeIds
 * @param {string[]} changed
 */
export function affectedBy(sc, edgeIds, changed) {
  const parents = new Set(changed.map((c) => sc.nodes.find((n) => n.id === c)?.parent).filter(Boolean));
  /** @type {string[]} */
  const wave1 = [];
  /** @type {string[]} */
  const wave2 = [];
  for (const id of edgeIds) {
    const [from, to] = id.split('->');
    if (changed.includes(from)) wave1.push(id);
    else if (changed.includes(to) || parents.has(from)) wave2.push(id);
  }
  return { wave1, wave2 };
}

/**
 * One full snapshot per step.
 * @param {Scenario} sc
 * @returns {Snapshot[]}
 */
export function buildSnapshots(sc) {
  const kind = new Map(sc.nodes.map((n) => [n.id, n.kind]));
  const def = new Map(sc.nodes.map((n) => [n.id, n]));
  const edgeKind = new Map(sc.edges.map((e) => [edgeId(e), e.kind]));
  /** @type {Map<string, string>} */
  const fp = new Map();
  for (const n of sc.nodes) {
    if (!NON_ARTIFACT.has(n.kind)) fp.set(n.id, n.fp || hashHex(n.id));
  }
  const nodes = new Set();
  const edges = new Set();
  const overlays = new Set();
  let cumExecuted = 0;
  let cumNaive = 0;
  let human = 0;

  return sc.steps.map((step, index) => {
    /** @type {Map<string, string>} */
    const emerge = new Map();
    /** @type {Map<string, string>} */
    const merge = new Map();
    for (const id of step.add || []) {
      nodes.add(id);
      // A copy in another checkout starts with the original's fingerprint.
      const copyOf = def.get(id)?.copyOf;
      const orig = copyOf ? fp.get(copyOf) : undefined;
      if (orig && !def.get(id)?.fp) fp.set(id, orig);
    }
    if (step.split) {
      for (const id of step.split.into) {
        nodes.add(id);
        emerge.set(id, step.split.from);
      }
      nodes.delete(step.split.from);
    }
    for (const [id, src] of Object.entries(step.emerge || {})) emerge.set(id, src);
    const before = new Map(fp);
    for (const [id, into] of Object.entries(step.merge || {})) {
      if (!nodes.has(id) || !nodes.has(into)) continue;
      const f = fp.get(id);
      if (f) fp.set(into, f);
      merge.set(id, into);
      nodes.delete(id);
    }
    for (const id of step.remove || []) nodes.delete(id);
    for (const id of [...edges]) {
      const [from, to] = id.split('->');
      if (!nodes.has(from) || !nodes.has(to)) edges.delete(id);
    }
    for (const id of step.disconnect || []) edges.delete(id);
    const fresh = new Set();
    for (const id of step.connect || []) {
      if (!edgeKind.has(id)) continue;
      const [from, to] = id.split('->');
      if (!nodes.has(from) || !nodes.has(to)) continue;
      if (!edges.has(id)) fresh.add(id);
      edges.add(id);
    }
    for (const id of step.show || []) overlays.add(id);
    for (const id of step.hide || []) overlays.delete(id);

    /** @type {Map<string, EvalState>} */
    const state = new Map();
    for (const id of edges) state.set(id, fresh.has(id) ? 'reviewed' : 'reused');
    const ripple = rippleList(step.ripple).filter((id) => nodes.has(id));
    const { wave1, wave2 } = affectedBy(sc, edges, ripple);
    for (const id of [...wave1, ...wave2]) state.set(id, 'reviewed');
    /** @type {Map<string, string>} */
    const oldFp = new Map();
    for (const id of ripple) {
      const old = before.get(id);
      if (!old) continue;
      oldFp.set(id, old);
      // A node a branch merged into already took that branch's fingerprint.
      if (![...merge.values()].includes(id)) fp.set(id, hashHex(`${id}:${step.id}:${old}`));
    }
    for (const [st, ids] of Object.entries(step.mark || {})) {
      for (const id of ids || []) {
        if (edges.has(id)) state.set(id, /** @type {EvalState} */ (st));
      }
    }

    const sketch = new Set((step.sketch || []).filter((id) => nodes.has(id)));
    const layout = resolveLayout(sc, step.layout);
    const frames = framesOf(sc, nodes);
    /** @type {Map<string, XYS>} */
    const pos = new Map();
    for (const id of nodes) {
      const p = layout.get(id);
      if (p) pos.set(id, p);
      else if (!frames.has(id)) pos.set(id, { x: 50, y: 50, s: 1 });
    }

    let executed = 0;
    let reused = 0;
    for (const [id, st] of state) {
      if (st === 'reviewed') {
        executed++;
        if (edgeKind.get(id) === 'human') human++;
      } else if (st === 'reused') {
        reused++;
      }
    }
    cumExecuted += executed;
    cumNaive += edges.size;
    const artifacts = [...nodes].filter(
      (id) => !NON_ARTIFACT.has(kind.get(id) || 'person') && !sketch.has(id) && !def.get(id)?.copyOf,
    ).length;

    return {
      id: step.id,
      index,
      nodes: new Set(nodes),
      frames,
      pos,
      emerge,
      merge,
      fp: new Map(fp),
      sketch,
      edges: state,
      fresh,
      ripple,
      wave1,
      wave2,
      oldFp,
      overlays: new Set([...overlays].filter((id) => overlayFits(sc, id, nodes))),
      counts: { artifacts, evals: edges.size, executed, reused, human, cumExecuted, cumNaive },
    };
  });
}

/**
 * An overlay shows only while the nodes it names are on the stage.
 * @param {Scenario} sc
 * @param {string} id
 * @param {Set<string>} nodes
 */
function overlayFits(sc, id, nodes) {
  const o = sc.overlays[id];
  if (!o) return false;
  if (o.type === 'flow') return nodes.has(o.from) && nodes.has(o.to);
  if (o.type === 'badge') return nodes.has(o.node);
  return true;
}
