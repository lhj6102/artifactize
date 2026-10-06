# The scroll story

A sticky graphic beside the steps of `index.html` (above them on phones): native ES modules, JSDoc types, no build.

| File | Role |
|---|---|
| `scenario.js` | The graphic as data: nodes, evals, overlays, layout presets and, per step, only what changes. |
| `types.js` | The JSDoc types of that data. |
| `model.js` | Builds one snapshot per step and checks the scenario; problems go to `console.warn` on load. |
| `graph.js` | Draws snapshots as SVG and animates between them. It knows kinds and states, not the story. |
| `scroll.js` | Maps the scroll position to the active step; handles `?step=<id>`. |
| `main.js`, `story.css` | Wire it up; style it with the colour tokens and themes of `/site.css`. |

## Change the story

- **Copy**: edit the step's `<section data-step="id">` in `index.html` (heading, text, and the `.sr` line that
  describes the graphic). A translation copies the HTML; a `#story-strings` JSON block renames the graphic's
  words, such as node labels and badge text (format in `main.js`).
- **Add a step**: add a `<section data-step="new" id="step-new">` and a rail link to `#step-new` in `index.html`,
  and `{ id: 'new', layout: '…' }` at the same position in `steps` in `scenario.js`. List only what changes:
  `add`, `emerge`, `split`, `merge`, `remove`, `connect`, `connectAs`, `show`, `hide`, `relabel`, `ripple`, `mark`.
  A connected eval executes in its first step, then is reused; `ripple` changes fingerprints. To reorder,
  move the section, its rail link and its `steps` entry together.
- **Add a node**: add it to `nodes` (`id`, `label`, `kind`, optional `parent`), place it in the layouts of the
  steps that show it, and `add` it in a step. People are never nodes: they are reviewer marks and labels.
- **Add an eval**: `{ on, kind, deps?, by? }`. With `deps` it is an edge to each (id `on->dep`); without, a loop
  on `on` (id `on#kind`), placed by `side` and `at`. A Human eval's `by` names its reviewer.
- **Add a node kind**: add it to `NodeKind` (`types.js`), `ICONS` (`graph.js`) and, if not an Artifact, `NON_ARTIFACT`.
- **Layouts**: positions are percent of the stage, `[x, y]` or `[x, y, scale]`. A preset can start from
  another (`base`) and move a few nodes, or combine `parts`, each fitted into a box (`fit`, `scale`). A node
  with children is drawn as a frame around them and takes no position. Check phones (390×844) too.

## Check and preview

```sh
npx -y -p typescript@5 tsc -p website/landing/jsconfig.json   # the type check: prints nothing when clean
python3 -m http.server -d website/landing 8080                # then open http://localhost:8080/?step=ripple
````?step=<id>` jumps to a step and shows its end state at once, for review and screenshots. Watch the console:
the scenario check names unknown ids, steps missing from the page or out of order, and nodes without a position.
