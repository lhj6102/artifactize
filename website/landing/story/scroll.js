// @ts-check
// Maps the scroll position to the active story step. A step is active while
// it crosses a trigger line: the middle of the viewport when the stage sits
// beside the steps, and just below the stage when it sits above them (phones).
// `?step=<id>` jumps to a step on load, and the step links (#step-<id>) jump
// by plain anchors, so neither needs a keyboard or a pointer gesture.
// Without IntersectionObserver, a throttled scroll handler does the same.

/**
 * @typedef {Object} WatchOptions
 * @property {HTMLElement} stage  The sticky graphic; its position sets the trigger line.
 * @property {(id: string, info: { instant: boolean, index: number }) => void} onStep
 */

/**
 * Watches the steps and reports the active one. Returns the id it started on.
 * @param {HTMLElement[]} sections  The `<section data-step>` elements, in order.
 * @param {WatchOptions} opts
 * @returns {string}
 */
export function watchSteps(sections, opts) {
  const ids = sections.map((s) => s.dataset.step || '');
  let active = '';
  let line = 0;
  let stacked = false;
  let stageBottom = 0;

  /** @param {string} id @param {boolean} instant */
  const activate = (id, instant) => {
    if (!id || id === active) return;
    active = id;
    const index = ids.indexOf(id);
    sections.forEach((s, i) => s.classList.toggle('is-active', i === index));
    opts.onStep(id, { instant, index });
  };

  /** The trigger line, in pixels from the top of the viewport. Read on resize only. */
  const measure = () => {
    const vh = window.innerHeight;
    const rect = opts.stage.getBoundingClientRect();
    stacked = rect.width > window.innerWidth * 0.7;
    if (stacked) {
      // The stage sticks at its CSS `top`; the line sits below it.
      stageBottom = (parseFloat(getComputedStyle(opts.stage).top) || 0) + rect.height;
      line = Math.min(vh - 40, stageBottom + (vh - stageBottom) * 0.3);
    } else {
      line = vh * 0.5;
    }
  };

  /** The step crossing the line now, or the nearest one. */
  const current = () => {
    let best = ids[0];
    for (const s of sections) {
      if (s.getBoundingClientRect().top <= line) best = s.dataset.step || best;
    }
    return best;
  };

  /** @type {IntersectionObserver | null} */
  let io = null;
  const observe = () => {
    io?.disconnect();
    if (typeof IntersectionObserver !== 'function') return;
    const vh = window.innerHeight;
    const top = -Math.round(line);
    const bottom = -Math.round(vh - line - 1);
    /** Steps touching the line now; at a boundary, two. */
    const inside = new Set();
    io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const id = /** @type {HTMLElement} */ (e.target).dataset.step || '';
          if (e.isIntersecting) inside.add(id);
          else inside.delete(id);
        }
        const hit = ids.filter((id) => inside.has(id));
        // Between steps (or past the last one), fall back to positions.
        activate(hit.length ? hit[hit.length - 1] : current(), false);
      },
      { rootMargin: `${top}px 0px ${bottom}px 0px`, threshold: 0 },
    );
    for (const s of sections) io.observe(s);
  };

  // Fallback: a scroll handler, at most once per frame.
  let queued = false;
  const onScroll = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      activate(current(), false);
    });
  };
  if (typeof IntersectionObserver !== 'function') {
    window.addEventListener('scroll', onScroll, { passive: true });
  }

  let resizeTimer = 0;
  window.addEventListener('resize', () => {
    window.clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(() => {
      measure();
      observe();
    }, 120);
  });

  measure();

  // ?step=<id> jumps there at once: for review links and screenshots.
  const wanted = new URLSearchParams(window.location.search).get('step');
  const target = wanted ? sections.find((s) => s.dataset.step === wanted) : null;
  if (wanted && !target) console.warn(`[story] ?step=${wanted}: no such step; the steps are ${ids.join(', ')}`);
  if (target) {
    // Beside the stage, centre the step; below it, start the step under it.
    const r = target.getBoundingClientRect();
    const y = stacked ? window.scrollY + r.top - stageBottom - 4 : window.scrollY + r.top + r.height / 2 - window.innerHeight / 2;
    window.scrollTo({ top: Math.max(0, y), behavior: 'instant' });
    activate(target.dataset.step || '', true);
  } else {
    activate(current(), true);
  }
  observe();
  return active;
}
