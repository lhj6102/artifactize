// Scales each figure's stage to the page's width: the screen is drawn at its stage size (--w
// by --h) and shrinks as one, callouts included.
const fit = () => {
  for (const stage of document.querySelectorAll(".stage")) {
    const width = Number.parseFloat(getComputedStyle(stage).getPropertyValue("--w"));
    const inner = stage.querySelector(".stage-inner");
    if (inner && width > 0) inner.style.setProperty("--scale", String(stage.clientWidth / width));
  }
};
addEventListener("resize", fit);
fit();
