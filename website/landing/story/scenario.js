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
  // One project: the "app" repository.
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

  // The same repository as a frame, two worktrees where people change one module
  // each in parallel (copies of the Artifacts they touch), the review store and CI.
  { id: 'app', label: 'app · main', kind: 'repo' },
  { id: 'wt-api', label: 'bob/api', kind: 'branch' },
  { id: 'bob', label: 'Bob', kind: 'person', sub: 'developer', parent: 'wt-api' },
  { id: 'bob-api', label: 'api', kind: 'module', parent: 'wt-api', copyOf: 'api' },
  { id: 'bob-tests', label: 'tests', kind: 'runtime', parent: 'wt-api', copyOf: 'tests' },
  { id: 'bob-style', label: 'Style sheet', kind: 'sheet', parent: 'wt-api', copyOf: 'style' },
  { id: 'wt-cli', label: 'carol/cli', kind: 'branch' },
  { id: 'carol', label: 'Carol', kind: 'person', sub: 'developer', parent: 'wt-cli' },
  { id: 'carol-cli', label: 'cli', kind: 'module', parent: 'wt-cli', copyOf: 'cli' },
  { id: 'carol-tests', label: 'tests', kind: 'runtime', parent: 'wt-cli', copyOf: 'tests' },
  { id: 'carol-style', label: 'Style sheet', kind: 'sheet', parent: 'wt-cli', copyOf: 'style' },
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
  { from: 'bob-api', to: 'bob-tests', kind: 'runtime' },
  { from: 'bob-api', to: 'bob-style', kind: 'agent' },
  { from: 'carol-cli', to: 'carol-tests', kind: 'runtime' },
  { from: 'carol-cli', to: 'carol-style', kind: 'agent' },
];

/** @type {Record<string, Overlay>} */
const overlays = {
  'signed-once': { type: 'badge', node: 'alice', text: 'signed off once', tone: 'human', side: 'top' },
  'flow-bob': { type: 'flow', from: 'wt-api', to: 'store', tone: 'publish', back: 'reuse' },
  'flow-carol': { type: 'flow', from: 'wt-cli', to: 'store', tone: 'publish', back: 'reuse', bend: 0 },
  'flow-ci': { type: 'flow', from: 'store', to: 'ci', tone: 'reuse', bend: 0 },
  'ci-lane': { type: 'lane', y: 92, label: 'verify --all', text: 'executed {executed} · reused {reused}', align: 'left' },
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
  parallel: {
    parts: [
      { base: 'grown', fit: [0, 4, 52, 52], scale: 0.5 },
      { nodes: { alice: [46.8, 6.9, 0.85] } },
      {
        nodes: { bob: [20, 32, 0.85], 'bob-api': [66, 32, 0.85], 'bob-tests': [52, 86, 0.5], 'bob-style': [80, 86, 0.5] },
        fit: [0, 66, 44, 92],
      },
      {
        nodes: { carol: [20, 32, 0.85], 'carol-cli': [66, 32, 0.85], 'carol-tests': [52, 86, 0.5], 'carol-style': [80, 86, 0.5] },
        fit: [56, 66, 100, 92],
      },
      { nodes: { store: [84, 30] } },
    ],
  },
  merged: { base: 'parallel', nodes: { ci: [84, 92] } },
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
    id: 'parallel', layout: 'parallel',
    add: ['app', 'store', 'wt-api', 'bob', 'bob-api', 'bob-tests', 'bob-style', 'wt-cli', 'carol', 'carol-cli', 'carol-tests', 'carol-style'],
    connect: ['bob-api->bob-tests', 'bob-api->bob-style', 'carol-cli->carol-tests', 'carol-cli->carol-style'],
    show: ['flow-bob', 'flow-carol'],
    // Bob changes api and Carol changes cli, each in their own worktree; their evals run there.
    ripple: ['bob-api', 'carol-cli'],
  },
  {
    id: 'merge', layout: 'merged',
    merge: { 'bob-api': 'api', 'carol-cli': 'cli', 'bob-tests': 'tests', 'bob-style': 'style', 'carol-tests': 'tests', 'carol-style': 'style' },
    remove: ['wt-api', 'wt-cli', 'bob', 'carol'],
    add: ['ci'], show: ['flow-ci', 'ci-lane'], hide: ['flow-bob', 'flow-carol'],
    ripple: ['api', 'cli'],
    // The branches reviewed these with the same inputs, so CI reuses them from the store.
    // Only the code's review against the Spec sees both changes at once, and runs.
    mark: { reused: ['api->tests', 'api->style', 'cli->tests', 'cli->style', 'docs->api', 'docs->cli', 'site->api'] },
  },
];

/** @type {Scenario} */
export const scenario = { nodes, edges, overlays, layouts, steps };
