// @ts-check
// Entry point of the scroll story: checks the scenario against the page,
// builds the snapshots and connects the renderer to the scroll position.

import { scenario } from './scenario.js';
import { buildSnapshots, validateScenario } from './model.js';
import { StoryGraph } from './graph.js';
import { watchSteps } from './scroll.js';

/** @typedef {import('./types.js').Scenario} Scenario */

/**
 * Optional translations of the graphic's words: a
 * `<script type="application/json" id="story-strings">` holding
 * `{"nodes": {"spec": {"label": "…", "sub": "…"}}, "overlays": {"ci-lane": {"label": "…", "text": "…"}}}`.
 * @param {Scenario} sc
 * @returns {Scenario}
 */
function localize(sc) {
  const source = document.getElementById('story-strings');
  if (!source) return sc;
  /** @type {{ nodes?: Record<string, { label?: string, sub?: string }>, overlays?: Record<string, Record<string, string>> }} */
  let strings;
  try {
    strings = JSON.parse(source.textContent || '{}');
  } catch (err) {
    console.warn('[story] #story-strings is not valid JSON:', err);
    return sc;
  }
  const nodes = sc.nodes.map((n) => ({ ...n, ...(strings.nodes?.[n.id] || {}) }));
  /** @type {Scenario['overlays']} */
  const overlays = {};
  for (const [id, o] of Object.entries(sc.overlays)) {
    overlays[id] = /** @type {typeof o} */ ({ ...o, ...(strings.overlays?.[id] || {}) });
  }
  return { ...sc, nodes, overlays };
}

function start() {
  const root = document.querySelector('[data-story]');
  if (!(root instanceof HTMLElement)) return;
  const svg = root.querySelector('svg.stage-svg');
  const aside = root.querySelector('.story-aside');
  if (!(svg instanceof SVGSVGElement) || !(aside instanceof HTMLElement)) {
    console.warn('[story] the story needs an <svg class="stage-svg"> inside a .story-aside');
    return;
  }
  const sections = /** @type {HTMLElement[]} */ ([...root.querySelectorAll('section[data-step]')]);
  const sc = localize(scenario);
  for (const problem of validateScenario(sc, sections.map((s) => s.dataset.step || ''))) {
    console.warn(`[story] ${problem}`);
  }

  const snapshots = buildSnapshots(sc);
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const graph = new StoryGraph(svg, sc, snapshots, { reducedMotion, hud: root.querySelector('.stage') });

  const total = sections.length;
  const stepNo = root.querySelector('[data-hud-step]');
  const stepName = root.querySelector('[data-hud-title]');
  const links = /** @type {HTMLAnchorElement[]} */ ([...root.querySelectorAll('.story-rail a')]);

  watchSteps(sections, {
    stage: aside,
    onStep(id, { instant, index }) {
      graph.show(id, { instant });
      if (stepNo) stepNo.textContent = `${String(index + 1).padStart(2, '0')}/${String(total).padStart(2, '0')}`;
      const link = links.find((a) => a.hash === `#step-${id}`);
      if (stepName) stepName.textContent = link?.dataset.short || '';
      for (const a of links) {
        if (a === link) a.setAttribute('aria-current', 'step');
        else a.removeAttribute('aria-current');
      }
    },
  });
  document.documentElement.classList.add('story-ready');
}

start();
