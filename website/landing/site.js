// artifactize.dev: play/pause and captions for the promo video. The video
// pauses while it is off screen and never autoplays under reduced motion
// (see the inline script next to it in index.html).
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
