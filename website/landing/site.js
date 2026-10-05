// artifactize.dev: the theme toggle, and play/pause and captions for the
// promo video and the terminal recordings on pages that have them. The promo
// pauses while it is off screen and never autoplays under reduced motion.

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

// Terminal recordings: they load only when played, play while on screen (never
// under reduced motion), and a pause button stops them for good.
(function () {
  var reduced = window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches;
  var demos = document.querySelectorAll('.demo');
  Array.prototype.forEach.call(demos, function (demo) {
    var video = demo.querySelector('video');
    var button = demo.querySelector('.demo-play');
    var label = button.querySelector('.vc-label');
    var frame = demo.querySelector('.demo-frame');
    var userPaused = reduced;
    var visible = false;

    function play() {
      var p = video.play();
      if (p && p.catch) p.catch(function () {});
    }
    function sync() {
      var paused = video.paused;
      button.setAttribute('aria-pressed', paused ? 'true' : 'false');
      button.setAttribute('aria-label', paused ? 'Play the recording' : 'Pause the recording');
      label.textContent = paused ? 'Play' : 'Pause';
    }

    button.hidden = false;
    button.addEventListener('click', function () {
      if (video.paused) {
        userPaused = false;
        play();
      } else {
        userPaused = true;
        video.pause();
      }
    });
    video.addEventListener('play', sync);
    video.addEventListener('pause', sync);
    sync();

    Array.prototype.forEach.call(demo.querySelectorAll('.demo-tab'), function (tab) {
      tab.addEventListener('click', function () {
        Array.prototype.forEach.call(demo.querySelectorAll('.demo-tab'), function (t) {
          t.setAttribute('aria-pressed', t === tab ? 'true' : 'false');
        });
        var name = tab.getAttribute('data-name');
        frame.style.aspectRatio = '1000 / ' + tab.getAttribute('data-height');
        video.setAttribute('height', tab.getAttribute('data-height'));
        video.setAttribute('aria-label', tab.getAttribute('data-label'));
        video.poster = '/media/demo/' + name + '.webp';
        video.querySelector('source').src = '/media/demo/' + name + '.mp4';
        video.load();
        if (visible && !userPaused) play();
        sync();
      });
    });

    if ('IntersectionObserver' in window) {
      new IntersectionObserver(function (entries) {
        visible = entries[0].isIntersecting;
        if (!visible && !video.paused) {
          video.pause();
        } else if (visible && video.paused && !userPaused) {
          play();
        }
      }, { threshold: 0.35 }).observe(video);
    }
  });
})();
