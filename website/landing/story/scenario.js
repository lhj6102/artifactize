// @ts-check
// The scroll story's graphic, as data: the single source for what the stage
// draws. The step copy lives in index.html (<section data-step="id">); keep the
// two in the same order. Shapes are documented in types.js; README.md says how
// to add a step, a node or a kind.

import { evalEnds, evalId } from './model.js';

/** @typedef {import('./types.js').Scenario} Scenario */
/** @typedef {import('./types.js').StoryNode} StoryNode */
/** @typedef {import('./types.js').StoryEval} StoryEval */
/** @typedef {import('./types.js').Overlay} Overlay */
/** @typedef {import('./types.js').LayoutPreset} LayoutPreset */
/** @typedef {import('./types.js').StoryStep} StoryStep */
/** @typedef {import('./types.js').Pos} Pos */

/** The project: one repository. People are not nodes; they appear as reviewer marks and labels. @type {StoryNode[]} */
const project = [
  { id: 'idea', label: 'Idea', kind: 'idea', parent: 'app' },
  { id: 'spec', label: 'Spec', kind: 'doc', parent: 'app' },
  { id: 'code', label: 'Code', kind: 'code', parent: 'app' },
  { id: 'src', label: 'src', kind: 'code', parent: 'code' },
  { id: 'tests', label: 'tests', kind: 'runtime', parent: 'code' },
  { id: 'core', label: 'core', kind: 'module', parent: 'code' },
  { id: 'api', label: 'api', kind: 'module', parent: 'code' },
  { id: 'cli', label: 'cli', kind: 'module', parent: 'code' },
  { id: 'sync', label: 'sync', kind: 'module', parent: 'code' },
  { id: 'style', label: 'Style sheet', kind: 'sheet', parent: 'app' },
  { id: 'docs', label: 'Docs pages', kind: 'docs', parent: 'app' },
  { id: 'site', label: 'Website', kind: 'website', parent: 'app' },
  { id: 'brand', label: 'Brand guide', kind: 'sheet', parent: 'app' },
];

/**
 * Evals. With `deps` an eval reviews `on` against those Artifacts and is drawn
 * as an edge; without, it reviews `on` alone and is drawn as a loop on it.
 * @type {StoryEval[]}
 */
const projectEvals = [
  { on: 'spec', kind: 'agent', side: 'top' },
  { on: 'spec', kind: 'agent', deps: ['idea'] },
  { on: 'spec', kind: 'human', by: 'Alice', side: 'right' },
  { on: 'code', kind: 'agent', deps: ['spec'], bend: 0 },
  { on: 'src', kind: 'runtime', deps: ['tests'] },
  { on: 'src', kind: 'agent', deps: ['style'] },
  { on: 'style', kind: 'human', by: 'Alice', side: 'top' },
  { on: 'core', kind: 'runtime', deps: ['tests'] },
  { on: 'api', kind: 'runtime', deps: ['tests'] },
  { on: 'cli', kind: 'runtime', deps: ['tests'] },
  { on: 'sync', kind: 'runtime', deps: ['tests'] },
  { on: 'core', kind: 'agent', deps: ['style'] },
  { on: 'api', kind: 'agent', deps: ['style'] },
  { on: 'cli', kind: 'agent', deps: ['style'] },
  { on: 'sync', kind: 'agent', deps: ['style'] },
  { on: 'api', kind: 'human', by: 'Alice', side: 'bottom', at: 0.25 },
  { on: 'docs', kind: 'agent', deps: ['api'] },
  { on: 'docs', kind: 'agent', deps: ['cli'] },
  { on: 'docs', kind: 'runtime', side: 'left' },
  { on: 'site', kind: 'runtime', deps: ['docs'], bend: 0 },
  { on: 'site', kind: 'runtime', deps: ['api'] },
  { on: 'site', kind: 'agent', deps: ['brand'], bend: 0 },
  { on: 'site', kind: 'human', by: 'Dana', side: 'top', at: -0.22 },
];

// Copies of the same project (clones or worktrees) where people work in parallel:
// the same Artifacts and evals, so their verdicts share reuse keys.
const COPIED = project.filter((n) => n.id !== 'src').map((n) => n.id);

/** @param {string} prefix @param {string} frame @returns {StoryNode[]} */
const copyNodes = (prefix, frame) =>
  project
    .filter((n) => COPIED.includes(n.id))
    .map((n) => ({ ...n, id: prefix + n.id, parent: n.parent === 'code' ? `${prefix}code` : frame, copyOf: n.id }));

/** @param {string} prefix @returns {StoryEval[]} */
const copyEvals = (prefix) =>
  projectEvals
    .filter((e) => evalEnds(e).every((n) => COPIED.includes(n)))
    .map((e) => ({ ...e, on: prefix + e.on, ...(e.deps ? { deps: e.deps.map((d) => prefix + d) } : {}) }));

const bob = { nodes: copyNodes('b-', 'app-b'), evals: copyEvals('b-') };
const carol = { nodes: copyNodes('c-', 'app-c'), evals: copyEvals('c-') };

/** @type {StoryNode[]} */
const nodes = [
  ...project,
  { id: 'app', label: 'app · main', kind: 'repo' },
  { id: 'app-b', label: 'app · bob/api', kind: 'repo' },
  ...bob.nodes,
  { id: 'app-c', label: 'app · carol/cli', kind: 'repo' },
  ...carol.nodes,
  { id: 'store', label: 'Review store', kind: 'store', sub: 'artifactize server' },
  { id: 'ci', label: 'CI', kind: 'ci', sub: 'read-only token' },
];

/** @type {StoryEval[]} */
const evals = [...projectEvals, ...bob.evals, ...carol.evals];

/** @type {Record<string, Overlay>} */
const overlays = {
  'flow-a': { type: 'flow', from: 'app', to: 'store', tone: 'publish', back: 'reuse' },
  'flow-b': { type: 'flow', from: 'app-b', to: 'store', tone: 'publish', back: 'reuse' },
  'flow-c': { type: 'flow', from: 'app-c', to: 'store', tone: 'publish', back: 'reuse', bend: 0 },
  'flow-ci': { type: 'flow', from: 'store', to: 'ci', tone: 'reuse', bend: 0.2 },
  'ci-lane': { type: 'lane', y: 93, label: 'verify --all', text: 'executed {executed} · reused {reused}' },
};

/** Where the grown project sits; the copies reuse it. @type {Record<string, Pos>} */
const BEYOND = {
  idea: [10, 9], spec: [50, 9], style: [90, 26], tests: [14, 45],
  core: [20, 63], cli: [50, 63], api: [80, 63], docs: [14, 94], site: [50, 94],
};
/** @type {Record<string, Pos>} */
const GROWN = { ...BEYOND, core: [14, 63], cli: [38, 63], sync: [62, 63], api: [86, 63], brand: [86, 94] };
/** @param {string} prefix */
const prefixed = (prefix) => Object.fromEntries(Object.entries(GROWN).map(([id, p]) => [prefix + id, p]));

/** Positions in percent of the stage. Frames (code, repos) wrap their children. @type {Record<string, LayoutPreset>} */
const layouts = {
  seed: { nodes: { idea: [50, 46, 1.3] } },
  artifacts: { nodes: { idea: [26, 50, 1.2], spec: [74, 50, 1.2] } },
  relations: { nodes: { idea: [20, 30], spec: [58, 30], code: [58, 74, 1.1] } },
  runtime: { nodes: { idea: [10, 12], spec: [50, 12], style: [90, 32], tests: [14, 52], src: [50, 72] } },
  modules: { base: 'runtime', nodes: { core: [20, 72], cli: [50, 72], api: [80, 72] } },
  beyond: { nodes: BEYOND },
  grown: { nodes: GROWN },
  team: {
    parts: [
      { nodes: GROWN, fit: [0, 3, 44, 38], scale: 0.5 },
      { nodes: prefixed('b-'), fit: [56, 3, 100, 38], scale: 0.5 },
      { nodes: prefixed('c-'), fit: [28, 66, 72, 99], scale: 0.5 },
      { nodes: { store: [50, 51] } },
    ],
  },
  merged: { parts: [{ nodes: GROWN, fit: [12, 4, 88, 54], scale: 0.5 }, { nodes: { store: [50, 72], ci: [10, 93] } }] },
};

const copyIds = [...bob.nodes, ...carol.nodes].map((n) => n.id);

/** One entry per <section data-step> in index.html, in the same order. @type {StoryStep[]} */
const steps = [
  { id: 'idea', layout: 'seed', add: ['idea'], sketch: ['idea'] },
  { id: 'artifacts', layout: 'artifacts', add: ['spec'], connect: ['spec#agent'] },
  { id: 'relations', layout: 'relations', add: ['code'], connect: ['spec->idea', 'code->spec', 'spec#human'] },
  {
    id: 'runtime', layout: 'runtime',
    add: ['src', 'tests', 'style'], emerge: { src: 'code', tests: 'code' },
    connect: ['src->tests', 'src->style', 'style#human'],
  },
  {
    id: 'modules', layout: 'modules',
    split: { from: 'src', into: ['core', 'api', 'cli'] },
    connect: ['core->tests', 'api->tests', 'cli->tests', 'core->style', 'api->style', 'cli->style', 'api#human'],
    // The code's children changed, so its review against the Spec runs again.
    mark: { reviewed: ['code->spec'] },
  },
  {
    id: 'beyond', layout: 'beyond',
    add: ['docs', 'site'],
    connect: ['docs->api', 'site->docs', 'site->api', 'docs#runtime', 'site#human'],
  },
  { id: 'ripple', layout: 'beyond', ripple: 'api' },
  {
    id: 'grows', layout: 'grown',
    add: ['sync', 'brand'], emerge: { sync: 'cli' },
    connect: ['sync->tests', 'sync->style', 'site->brand', 'docs->cli'],
  },
  {
    // Alice changes the docs in her copy, Bob api in his, Carol cli in hers.
    id: 'parallel', layout: 'team',
    add: ['app', 'store', 'app-b', 'app-c', ...copyIds],
    // A copy starts with the verdicts already in the store.
    connect: [...bob.evals, ...carol.evals].map(evalId), connectAs: 'reused',
    show: ['flow-a', 'flow-b', 'flow-c'],
    relabel: { app: 'app · alice/docs' },
    ripple: ['docs', 'b-api', 'c-cli'],
  },
  {
    id: 'merge', layout: 'merged',
    merge: { 'b-api': 'api', 'c-cli': 'cli' },
    remove: ['app-b', 'app-c', ...copyIds.filter((id) => id !== 'b-api' && id !== 'c-cli')],
    add: ['ci'], show: ['flow-ci', 'ci-lane'], hide: ['flow-a', 'flow-b', 'flow-c'],
    ripple: ['api', 'cli'],
    // The copies reviewed these with the same inputs, so CI reuses them from the
    // store. Only evals that see two people's changes at once run.
    mark: { reused: ['api->tests', 'api->style', 'api#human', 'cli->tests', 'cli->style', 'site->api'] },
  },
];

/** @type {Scenario} */
export const scenario = { nodes, evals, overlays, layouts, steps };
