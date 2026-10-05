// @ts-check
// The scroll story's graphic, as data: the single source for what the stage
// draws. The step copy lives in index.html (<section data-step="id">); keep the
// two in the same order. Shapes are documented in types.js; README.md says how
// to add a step, a node or a kind.

/** @typedef {import('./types.js').Scenario} Scenario */
/** @typedef {import('./types.js').StoryNode} StoryNode */
/** @typedef {import('./types.js').StoryEdge} StoryEdge */
/** @typedef {import('./types.js').Overlay} Overlay */
/** @typedef {import('./types.js').LayoutPreset} LayoutPreset */
/** @typedef {import('./types.js').StoryStep} StoryStep */

/** @type {StoryNode[]} */
const nodes = [
  // One project, the "app" repository.
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
  { id: 'alice', label: 'Alice', kind: 'person', sub: 'reviewer', parent: 'app' },

  // More repositories, people, the review store and CI.
  { id: 'app', label: 'app', kind: 'repo' },
  { id: 'web', label: 'web', kind: 'repo' },
  { id: 'w-site', label: 'site', kind: 'website', parent: 'web' },
  { id: 'w-core', label: 'core', kind: 'module', parent: 'web' },
  { id: 'w-style', label: 'style', kind: 'sheet', parent: 'web' },
  { id: 'sdk', label: 'sdk', kind: 'repo' },
  { id: 'k-core', label: 'core', kind: 'module', parent: 'sdk' },
  { id: 'k-client', label: 'client', kind: 'module', parent: 'sdk' },
  { id: 'k-style', label: 'style', kind: 'sheet', parent: 'sdk' },
  { id: 'bob', label: 'Bob', kind: 'person', sub: 'developer', parent: 'web' },
  { id: 'store', label: 'Review store', kind: 'store', sub: 'artifactize server' },
  { id: 'ci', label: 'CI', kind: 'ci', sub: 'read-only token' },
];

/** Evals: `from` is reviewed against `to`. Ids are "from->to". @type {StoryEdge[]} */
const edges = [
  { from: 'spec', to: 'idea', kind: 'agent' },
  { from: 'spec', to: 'alice', kind: 'human' },
  { from: 'code', to: 'spec', kind: 'agent', bend: 0 },
  { from: 'src', to: 'tests', kind: 'runtime' },
  { from: 'src', to: 'style', kind: 'agent' },
  { from: 'core', to: 'tests', kind: 'runtime' },
  { from: 'api', to: 'tests', kind: 'runtime' },
  { from: 'cli', to: 'tests', kind: 'runtime' },
  { from: 'sync', to: 'tests', kind: 'runtime' },
  { from: 'core', to: 'style', kind: 'agent' },
  { from: 'api', to: 'style', kind: 'agent' },
  { from: 'cli', to: 'style', kind: 'agent' },
  { from: 'sync', to: 'style', kind: 'agent' },
  { from: 'docs', to: 'api', kind: 'agent' },
  { from: 'docs', to: 'cli', kind: 'agent' },
  { from: 'site', to: 'docs', kind: 'runtime', bend: 0 },
  { from: 'site', to: 'api', kind: 'runtime' },
  { from: 'site', to: 'brand', kind: 'agent', bend: 0 },
  { from: 'w-site', to: 'w-core', kind: 'runtime' },
  { from: 'w-core', to: 'w-style', kind: 'agent' },
  { from: 'k-core', to: 'k-style', kind: 'agent' },
  { from: 'k-client', to: 'k-style', kind: 'agent' },
  { from: 'k-client', to: 'k-core', kind: 'runtime' },
];

/** @type {Record<string, Overlay>} */
const overlays = {
  'signed-once': { type: 'badge', node: 'alice', text: 'signed off once', tone: 'human', side: 'top' },
  'flow-app': { type: 'flow', from: 'app', to: 'store', tone: 'publish', back: 'reuse' },
  'flow-web': { type: 'flow', from: 'web', to: 'store', tone: 'publish', back: 'reuse' },
  'flow-sdk': { type: 'flow', from: 'sdk', to: 'store', tone: 'publish', back: 'reuse' },
  'flow-ci': { type: 'flow', from: 'store', to: 'ci', tone: 'reuse', bend: 0.2 },
  'ci-lane': { type: 'lane', y: 93, label: 'verify --all', text: 'executed {executed} · reused {reused}' },
};

/** Positions in percent of the stage. Frames (code, repos) wrap their children. @type {Record<string, LayoutPreset>} */
const layouts = {
  seed: { nodes: { idea: [50, 46, 1.3] } },
  artifacts: { nodes: { idea: [26, 46, 1.2], spec: [74, 46, 1.2] } },
  relations: { nodes: { idea: [12, 24], spec: [50, 24], alice: [88, 24], code: [50, 70, 1.1] } },
  runtime: {
    nodes: { idea: [10, 10], spec: [50, 10], alice: [90, 10], style: [90, 27], tests: [14, 50], src: [50, 70] },
  },
  modules: { base: 'runtime', nodes: { core: [20, 70], api: [50, 70], cli: [80, 70] } },
  beyond: {
    nodes: {
      idea: [10, 6], spec: [50, 6], alice: [90, 6], style: [90, 25], tests: [14, 44],
      core: [20, 62], api: [50, 62], cli: [80, 62], docs: [14, 94], site: [50, 94],
    },
  },
  grown: { base: 'beyond', nodes: { core: [14, 62], api: [38, 62], cli: [62, 62], sync: [86, 62], brand: [86, 94] } },
  team: {
    parts: [
      { base: 'grown', fit: [0, 7, 52, 54], scale: 0.5 },
      { nodes: { alice: [47, 9, 0.85] } },
      { nodes: { 'w-site': [15, 25], 'w-core': [60, 25], 'w-style': [38, 80] }, fit: [70, 2, 98, 20], scale: 0.5 },
      { nodes: { bob: [86, 28, 0.85] } },
      { nodes: { 'k-core': [15, 25], 'k-client': [60, 25], 'k-style': [38, 80] }, fit: [70, 42, 98, 58], scale: 0.5 },
      { nodes: { store: [46, 76] } },
    ],
  },
  ci: { base: 'team', nodes: { store: [46, 69], ci: [10, 94] } },
};

/** One entry per <section data-step> in index.html, in the same order. @type {StoryStep[]} */
const steps = [
  { id: 'idea', layout: 'seed', add: ['idea'], sketch: ['idea'] },
  { id: 'artifacts', layout: 'artifacts', add: ['spec'] },
  {
    id: 'relations', layout: 'relations',
    add: ['code', 'alice'],
    connect: ['spec->idea', 'code->spec', 'spec->alice'],
  },
  {
    id: 'runtime', layout: 'runtime',
    add: ['src', 'tests', 'style'], emerge: { src: 'code', tests: 'code' },
    connect: ['src->tests', 'src->style'],
  },
  {
    id: 'modules', layout: 'modules',
    split: { from: 'src', into: ['core', 'api', 'cli'] },
    connect: ['core->tests', 'api->tests', 'cli->tests', 'core->style', 'api->style', 'cli->style'],
    // The code's children changed, so its review against the Spec runs again.
    mark: { reviewed: ['code->spec'] },
  },
  {
    id: 'beyond', layout: 'beyond',
    add: ['docs', 'site'],
    connect: ['docs->api', 'site->docs', 'site->api'],
  },
  { id: 'ripple', layout: 'beyond', ripple: 'api', show: ['signed-once'] },
  {
    id: 'grows', layout: 'grown',
    add: ['sync', 'brand'], emerge: { sync: 'cli' },
    connect: ['sync->tests', 'sync->style', 'site->brand', 'docs->cli'],
  },
  {
    id: 'team', layout: 'team',
    add: ['app', 'web', 'w-site', 'w-core', 'w-style', 'sdk', 'k-core', 'k-client', 'k-style', 'store', 'bob'],
    connect: ['w-site->w-core', 'w-core->w-style', 'k-core->k-style', 'k-client->k-style', 'k-client->k-core'],
    show: ['flow-app', 'flow-web', 'flow-sdk'],
    // The shared core and style sheet carry verdicts already in the store.
    mark: { reused: ['w-core->w-style', 'k-core->k-style'] },
  },
  {
    id: 'ci', layout: 'ci',
    add: ['ci'], show: ['flow-ci', 'ci-lane'],
    ripple: 'cli',
  },
];

/** @type {Scenario} */
export const scenario = { nodes, edges, overlays, layouts, steps };
