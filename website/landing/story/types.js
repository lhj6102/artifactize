// @ts-check
// The data shapes of the scroll story. JSDoc only: nothing here runs, and no
// page loads this file. Other files import the types with
// `@typedef {import('./types.js').Name} Name`.

/**
 * What a node draws as. Every kind but person, repo, branch, store and ci is an
 * Artifact, and only Artifacts count in the stage's "Artifacts" meter. A
 * branch is a checkout (a worktree or a branch) drawn as a frame.
 * @typedef {'idea' | 'doc' | 'code' | 'module' | 'runtime' | 'sheet' | 'website' | 'docs'
 *   | 'repo' | 'branch' | 'store' | 'ci' | 'person'} NodeKind
 */

/** @typedef {'runtime' | 'agent' | 'human'} EvalKind */

/**
 * An eval's verdict state on the stage: `pending` (not reviewed yet), `stale`
 * (a fingerprint it depends on changed), `reviewed` (executed in this step) or
 * `reused` (its earlier verdict still holds).
 * @typedef {'pending' | 'stale' | 'reviewed' | 'reused'} EvalState
 */

/**
 * @typedef {Object} StoryNode
 * @property {string} id
 * @property {string} label   Text on the node.
 * @property {NodeKind} kind
 * @property {string} [parent] The group it sits in. A node with present children
 *   is drawn as a frame around them; its eval then depends on them too.
 * @property {string} [sub]    Second line. Artifacts default to their fingerprint.
 * @property {string} [fp]     Fingerprint to show. Defaults to a hash of the id.
 * @property {string} [copyOf] The Artifact this node copies in another checkout.
 *   It starts with that Artifact's fingerprint and is not counted again.
 */

/**
 * One eval: it reviews the Artifact `on`, and depends on the Artifacts in
 * `deps` (those its instruction or args name). With deps it is drawn as an
 * edge to each (id `${on}->${deps}`); without, as a chip on the node itself
 * (id `${on}#${kind}`). Give an `id` when a node has two of one kind.
 * @typedef {Object} StoryEval
 * @property {string} on
 * @property {EvalKind} kind
 * @property {string[]} [deps]
 * @property {string} [by]    Human evals: the reviewer's initial, shown on the chip.
 * @property {string} [id]
 * @property {number} [bend]  Edge curvature, about -0.4 to 0.4. Default 0.12.
 * @property {'top' | 'right' | 'bottom' | 'left'} [side] Without deps: the side of the
 *   node the loop leaves from. Default top.
 * @property {number} [at]    Without deps: offset along that side, -0.5 to 0.5. Default 0.
 */

/**
 * A position in percent of the stage's drawing area: [x, y] or [x, y, scale].
 * @typedef {[number, number] | [number, number, number]} Pos
 */

/**
 * @typedef {Object} LayoutPart
 * @property {string} [base]   Start from another preset's positions.
 * @property {Record<string, Pos>} [nodes] Positions to add or override.
 * @property {[number, number, number, number]} [fit] Map this part's 0–100 space
 *   into the box [x0, y0, x1, y1] (percent of the stage).
 * @property {number} [scale]  Multiply the scale of every node in this part.
 */

/**
 * A named layout. Frames take no position: they wrap their children.
 * @typedef {LayoutPart & { parts?: LayoutPart[] }} LayoutPreset
 */

/**
 * Particles moving between two nodes: `tone` from → to, `back` to → from.
 * @typedef {Object} FlowOverlay
 * @property {'flow'} type
 * @property {string} from
 * @property {string} to
 * @property {'publish' | 'reuse'} tone
 * @property {'publish' | 'reuse'} [back]
 * @property {number} [bend]  Curvature, as for edges. Default 0.08.
 */

/**
 * A text pill attached to a node.
 * @typedef {Object} BadgeOverlay
 * @property {'badge'} type
 * @property {string} node
 * @property {string} text
 * @property {'human' | 'reuse' | 'note'} [tone]
 * @property {'top' | 'bottom' | 'left' | 'right'} [side]
 */

/**
 * A full-width band at height `y` (percent). `text` may use {executed},
 * {reused}, {evals} and {artifacts}, filled from the step's counts.
 * @typedef {Object} LaneOverlay
 * @property {'lane'} type
 * @property {number} y
 * @property {string} label
 * @property {string} [text]
 * @property {'left' | 'right'} [align] Where the label and text sit. Default right.
 */

/** @typedef {FlowOverlay | BadgeOverlay | LaneOverlay} Overlay */

/**
 * One scroll step: only what changes. Nodes, edges and overlays carry over to
 * later steps; `sketch`, `ripple` and `mark` apply to this step only. A
 * connected edge is `reviewed` in its first step and `reused` afterwards.
 * @typedef {Object} StoryStep
 * @property {string} id       Matches `<section data-step="id">` in index.html.
 * @property {string} layout   A key of `layouts`.
 * @property {string[]} [add]  Nodes to add.
 * @property {Record<string, string>} [emerge] Added node → node it grows out of.
 * @property {{ from: string, into: string[] }} [split] Replace `from` with `into`.
 * @property {Record<string, string>} [merge] Node → node it merges into: the first
 *   is removed and the second takes its fingerprint (a branch merging back).
 * @property {string[]} [remove]  Nodes to remove (their edges go too).
 * @property {string[]} [connect] Eval ids to add.
 * @property {EvalState} [connectAs] State of the edges connected here. Default
 *   `reviewed`; `reused` for a copy whose verdicts are already in the store.
 * @property {string[]} [disconnect]
 * @property {string[]} [show]  Overlay ids to show.
 * @property {string[]} [hide]
 * @property {string[]} [sketch] Nodes drawn as not yet declared.
 * @property {string | string[]} [ripple] Nodes whose fingerprint changes in this
 *   step: every eval that depends on one goes stale and is reviewed again, unless
 *   `mark` says it is reused (a verdict from elsewhere, such as a merged branch).
 * @property {Partial<Record<EvalState, string[]>>} [mark] Set eval states.
 * @property {Record<string, string>} [relabel] Labels for this step only (frames).
 */

/**
 * @typedef {Object} Scenario
 * @property {StoryNode[]} nodes
 * @property {StoryEval[]} evals
 * @property {Record<string, Overlay>} overlays
 * @property {Record<string, LayoutPreset>} layouts
 * @property {StoryStep[]} steps
 */

/**
 * Per-step totals for the stage's meter. `cum*` add up every step so far;
 * `cumNaive` is what re-reviewing every eval in every step would execute.
 * @typedef {Object} Counts
 * @property {number} artifacts
 * @property {number} evals
 * @property {number} executed
 * @property {number} reused
 * @property {number} human      Human sign-offs executed so far.
 * @property {number} signoffs   Human sign-offs executed in this step.
 * @property {number} cumExecuted
 * @property {number} cumNaive
 */

/**
 * @typedef {Object} XYS
 * @property {number} x
 * @property {number} y
 * @property {number} s
 */

/**
 * The full state at the end of one step, computed by model.js.
 * @typedef {Object} Snapshot
 * @property {string} id
 * @property {number} index
 * @property {Set<string>} nodes      Present nodes.
 * @property {Set<string>} frames     Present nodes drawn as frames.
 * @property {Map<string, XYS>} pos   Layout position of each present node.
 * @property {Map<string, string>} emerge Node added here → node it grows from.
 * @property {Map<string, string>} merge  Node removed here → node it merges into.
 * @property {Map<string, string>} fp Fingerprint of each Artifact.
 * @property {Set<string>} sketch
 * @property {Map<string, string>} labels  Labels overridden in this step.
 * @property {Map<string, EvalState>} evals Present evals and their final state.
 * @property {Set<string>} fresh      Evals connected in this step.
 * @property {string[]} ripple  Nodes whose fingerprint changes in this step.
 * @property {string[]} wave1  Ripple: evals on the changed nodes themselves.
 * @property {string[]} wave2  Ripple: evals that depend on them.
 * @property {Map<string, string>} oldFp  Ripple: each changed node's previous fingerprint.
 * @property {Set<string>} overlays
 * @property {Counts} counts
 */

export {};
