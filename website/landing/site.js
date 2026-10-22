// artifactize.dev: the theme toggle, play/pause and captions for the promo video on
// pages that have it, the install downloads and tabs, the story's scroll cue and the desktop
// recordings. The promo pauses while it is off screen and never autoplays under
// reduced motion.

// Theme toggle: dark (the docs' "navy") or light. It writes the docs'
// localStorage key, so the choice carries across the site.
(function () {
  var button = document.querySelector('.theme-toggle');
  if (!button) return;
  var root = document.documentElement;
  function sync() {
    var light = root.getAttribute('data-theme') === 'light';
    button.setAttribute('aria-label', light ? 'Use the dark theme' : 'Use the light theme');
    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute('content', light ? '#FFFFFF' : '#090B0F');
  }
  button.hidden = false;
  button.addEventListener('click', function () {
    var light = root.getAttribute('data-theme') !== 'light';
    root.setAttribute('data-theme', light ? 'light' : 'dark');
    try {
      localStorage.setItem('mdbook-theme', light ? 'light' : 'navy');
    } catch (e) {}
    sync();
  });
  sync();
})();

(function () {
  var video = document.getElementById('promo');
  if (!video) return;
  var play = document.getElementById('promo-play');
  var cc = document.getElementById('promo-cc');
  var label = play.querySelector('.vc-label');
  var userPaused = document.documentElement.classList.contains('reduced-motion');
  var visible = true;

  function sync() {
    var paused = video.paused;
    play.setAttribute('aria-pressed', paused ? 'true' : 'false');
    play.setAttribute('aria-label', paused ? 'Play the video' : 'Pause the video');
    label.textContent = paused ? 'Play' : 'Pause';
  }

  play.hidden = false;
  play.addEventListener('click', function () {
    if (video.paused) {
      userPaused = false;
      var p = video.play();
      if (p && p.catch) p.catch(function () {});
    } else {
      userPaused = true;
      video.pause();
    }
  });
  video.addEventListener('play', sync);
  video.addEventListener('pause', sync);
  sync();

  var track = video.textTracks && video.textTracks[0];
  if (track) {
    track.mode = 'hidden';
    cc.hidden = false;
    cc.setAttribute('aria-label', 'Show captions');
    cc.addEventListener('click', function () {
      var on = track.mode !== 'showing';
      track.mode = on ? 'showing' : 'hidden';
      cc.setAttribute('aria-pressed', on ? 'true' : 'false');
      cc.setAttribute('aria-label', on ? 'Hide captions' : 'Show captions');
    });
  }

  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) {
      visible = entries[0].isIntersecting;
      if (!visible && !video.paused) {
        video.pause();
      } else if (visible && video.paused && !userPaused) {
        var p = video.play();
        if (p && p.catch) p.catch(function () {});
      }
    }, { threshold: 0.15 }).observe(video);
  }
})();

// Install: the visitor's OS gets the primary download button and its command tab
// first. Without JS no download stands out and every command panel shows; with it,
// the tabs pick one, and arrow keys, Home/End or a sideways swipe switch between them.
(function () {
  var install = document.querySelector('[data-install]');
  if (!install) return;

  // A phone gets no download highlight and the Linux commands.
  function detect() {
    var platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
    var agent = navigator.userAgent || '';
    if (/win/i.test(platform) || /Windows/.test(agent)) return 'windows';
    if (/iPhone|iPad|iPod|Android/.test(agent)) return '';
    if (/mac/i.test(platform) || /Macintosh|Mac OS X/.test(agent)) return 'macos';
    return 'linux';
  }
  var os = detect();

  Array.prototype.forEach.call(install.querySelectorAll('.download'), function (link) {
    if (link.getAttribute('data-os') !== os) return;
    link.classList.remove('btn-ghost');
    link.classList.add('btn-primary');
  });

  var list = install.querySelector('[role="tablist"]');
  var tabs = Array.prototype.slice.call(install.querySelectorAll('[role="tab"]'));
  if (!list || !tabs.length) return;

  function select(tab, focus) {
    tabs.forEach(function (t) {
      var on = t === tab;
      t.setAttribute('aria-selected', on ? 'true' : 'false');
      t.tabIndex = on ? 0 : -1;
      var panel = document.getElementById(t.getAttribute('aria-controls'));
      if (panel) panel.hidden = !on;
    });
    if (focus) tab.focus();
  }

  function step(by) {
    var current = tabs.indexOf(document.activeElement);
    if (current < 0) {
      current = tabs.findIndex(function (t) { return t.getAttribute('aria-selected') === 'true'; });
    }
    select(tabs[(current + by + tabs.length) % tabs.length], true);
  }

  tabs.forEach(function (tab) {
    tab.addEventListener('click', function () { select(tab, false); });
  });
  list.addEventListener('keydown', function (event) {
    if (event.key === 'ArrowRight') step(1);
    else if (event.key === 'ArrowLeft') step(-1);
    else if (event.key === 'Home') select(tabs[0], true);
    else if (event.key === 'End') select(tabs[tabs.length - 1], true);
    else return;
    event.preventDefault();
  });

  var startX = null;
  install.addEventListener('touchstart', function (event) {
    startX = event.touches.length === 1 ? event.touches[0].clientX : null;
  }, { passive: true });
  install.addEventListener('touchend', function (event) {
    if (startX === null) return;
    var dx = event.changedTouches[0].clientX - startX;
    startX = null;
    if (Math.abs(dx) < 50) return;
    var current = tabs.findIndex(function (t) { return t.getAttribute('aria-selected') === 'true'; });
    select(tabs[(current + (dx < 0 ? 1 : -1) + tabs.length) % tabs.length], false);
  }, { passive: true });

  list.hidden = false;
  select(tabs.filter(function (t) { return t.getAttribute('data-os') === (os || 'linux'); })[0] || tabs[0], false);
})();

// Copy buttons for the install commands. The panel's data-copy holds the exact command,
// without the prompt or comments. Without the Clipboard API, or when it refuses (an
// insecure context), the command is selected for Ctrl+C instead. Without JS the buttons
// stay hidden.
(function () {
  var keys = /Mac|iPhone|iPad/.test(navigator.platform || '') ? 'Cmd+C' : 'Ctrl+C';
  Array.prototype.forEach.call(document.querySelectorAll('[data-copy]'), function (panel) {
    var button = panel.querySelector('.copy');
    var status = panel.querySelector('[data-copy-status]');
    var command = panel.querySelector('[data-command]');
    var text = panel.getAttribute('data-copy');
    if (!button || !status || !text) return;
    var timer = 0;

    function show(message, state, ms) {
      clearTimeout(timer);
      button.textContent = message;
      button.setAttribute('data-state', state);
      // Clear first, so the same message is announced again.
      status.textContent = '';
      setTimeout(function () { status.textContent = message; }, 50);
      timer = setTimeout(function () {
        button.textContent = 'Copy';
        button.removeAttribute('data-state');
        status.textContent = '';
      }, ms);
    }

    function selectCommand() {
      var selection = window.getSelection();
      if (command && selection) {
        var range = document.createRange();
        range.selectNodeContents(command);
        selection.removeAllRanges();
        selection.addRange(range);
      }
      show('Press ' + keys + ' to copy', 'select', 4000);
    }

    button.addEventListener('click', function () {
      if (!navigator.clipboard || !navigator.clipboard.writeText) {
        selectCommand();
        return;
      }
      navigator.clipboard.writeText(text).then(function () {
        show('Copied', 'copied', 2000);
      }, selectCommand);
    });
    // The class makes room for the button.
    panel.classList.add('copyable');
    button.hidden = false;
  });
})();

// The story's scroll cue: shown while the story fills the middle of the screen, with a
// bar that fills as the steps advance and an arrow until the last step.
(function () {
  var cue = document.querySelector('[data-story-scroll]');
  var steps = document.querySelector('.story-steps');
  if (!cue || !steps) return;
  var frame = 0;

  function update() {
    frame = 0;
    var box = steps.getBoundingClientRect();
    var middle = window.innerHeight / 2;
    var progress = Math.min(1, Math.max(0, (middle - box.top) / Math.max(1, box.height - middle)));
    cue.style.setProperty('--story-progress', progress.toFixed(3));
    cue.classList.toggle('is-visible', box.top < middle && box.bottom > middle);
    cue.classList.toggle('is-end', progress > 0.97);
  }

  function schedule() {
    if (!frame) frame = requestAnimationFrame(update);
  }

  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', schedule);
  update();
})();

// The desktop app recordings: each tab swaps the GIF, its description and caption.
(function () {
  var demo = document.querySelector('[data-app-demo]');
  if (!demo) return;
  var img = demo.querySelector('img');
  var caption = demo.querySelector('.app-demo-caption');
  var tabs = Array.prototype.slice.call(demo.querySelectorAll('.demo-tab'));
  tabs.forEach(function (tab) {
    tab.addEventListener('click', function () {
      tabs.forEach(function (t) { t.setAttribute('aria-pressed', t === tab ? 'true' : 'false'); });
      img.src = tab.getAttribute('data-src');
      img.alt = tab.getAttribute('data-alt');
      if (caption) caption.textContent = tab.getAttribute('data-label');
    });
  });
})();
