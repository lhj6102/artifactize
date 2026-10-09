# artifactize.dev

The site for artifactize: the landing page at `/` and the docs at `/docs/`, both
deployed from this directory to Cloudflare Workers (static assets).

| Path | Contents |
|---|---|
| `landing/` | The landing page, copied to `dist/` as is: a short hero, the scroll story (`landing/story/`, native ES modules with JSDoc types; see its `README.md`), the terminal recordings and the install block. Also the shared brand fonts (`landing/fonts/`, with their OFL licenses), the promo media (`landing/media/`), `404.html`, `_headers`, the install scripts `install.sh` and `install.ps1` (served at `/install.sh` and `/install.ps1`, tested by `.github/workflows/binaries.yml`) and `.assetsignore`, which keeps `jsconfig.json` and `story/README.md` out of the deployed site. The brand images and favicons are copied from `../assets/brand/` at build time. |
| `docs/` | The user docs as an [mdBook](https://rust-lang.github.io/mdBook/): `book.toml`, `src/` (`SUMMARY.md` and the chapters) and `theme/`. Built into `dist/docs/`. |
| `build.sh` | Builds everything into `dist/` (gitignored) and checks every internal link. |
| `check-links.py` | The internal link check that `build.sh` runs over `dist/`. |
| `../assets/brand/` | The sole brand source, generated images, generator, minimum tool requirements, Artifact declarations and social card template. |
| `make-media.sh` | Re-creates the promo media in `landing/media/` from the promo master (not the social card). |
| `demo/` | The VHS tapes, demo projects and scripts behind the README GIFs and the landing page's terminal videos (see [Recordings](#recordings)). |
| `wrangler.toml` | The Worker that serves `dist/` on `artifactize.dev`. |

## Build and preview

The build needs mdBook 0.5.4 (the version `build.sh` and the workflow pin) and
`python3`:

```sh
cargo install mdbook --version 0.5.4 --locked --root ~/.local   # or a release binary
MDBOOK=~/.local/bin/mdbook website/build.sh                      # writes website/dist
python3 -m http.server -d website/dist 8080                      # http://localhost:8080
```

`build.sh` fails when an internal link or `#anchor` in `dist/` does not resolve.
`MDBOOK` defaults to `mdbook` on `PATH`.

## Docs

The docs are the single source for the user guide and the reference; the README
links into them at `https://artifactize.dev/docs/`. Write chapters in
`docs/src/` and list each one in `docs/src/SUMMARY.md`.

`docs/theme/` restyles mdBook in the brand palette:

- `index.hbs` is mdBook 0.5.4's default template with a few marked
  (`artifactize:`) changes: the wordmark linking to `/`, links to the landing
  page and GitHub, the Dark and Light themes only, previous/next cards and the
  shared fonts. After an mdBook upgrade, re-apply these marks to the new default
  template (`mdbook init --theme` writes it).
- `css/variables.css` defines the dark (`navy`, the default) and light themes;
  `artifactize.css` styles the chrome and content; `highlight.css` and
  `tomorrow-night.css` color code from the same variables.
- The fonts come from `/fonts/fonts.css`, shared with the landing page;
  `theme/fonts/fonts.css` only keeps mdBook from bundling its default fonts.

## Brand assets

All brand files live in the top-level `assets/brand/`, outside `website/`.
`assets/brand/artifactize-icon.svg` is the owner's original and the single source
of truth. Edit only that artwork or `assets/brand/og-template.svg`, never their
generated outputs. The generator derives the cropped header mark, README PNG,
standalone square favicons, opaque Apple icon and 1200×630 social card in that
same folder. The template's text is outlined from the bundled fonts; rendering
uses 8× supersampling and LANCZOS with no timestamps or image metadata. `build.sh`
copies the assets to their existing served URLs and injects the favicons into a temporary mdBook
source; no generated copies are committed under `landing/` or `docs/theme/`.

```sh
python3 assets/brand/build-icons.py
python3 assets/brand/build-icons.py --check
artifactize verify brand --recursive
```

Install the tools in the user's `python3` environment first: CairoSVG >=2.8,
Pillow >=12.1 and system Cairo (`python3-cairosvg`, `python3-pil` and `libcairo2`
on Ubuntu; or `python3 -m pip install -r assets/brand/requirements.txt`). No
project environment or automatic installation is used. Runtime preflight reports
missing tools clearly; artifactize's private HOME may hide `pip --user` modules.
The committed images use CairoSVG 2.8.2 and Pillow 12.1.1; after a renderer upgrade,
regenerate and commit the outputs if the byte check reports drift.

The `brand-icon/approved` Human eval owns artwork approval; `brand/generated`
waits for it and checks every output in memory without modifying files. The folder
uses its default artifactsum, including the source artwork and all outputs.
This is the brand drift check; there is no separate brand CI. The Website workflow
still builds and deploys committed assets. The logos in `demo/`, including
`demo/brand/logo/logo.svg`, are recording fixtures and intentionally unchanged.

## Landing page media

The promo video is the owner's motion graphic (1920×1080, 60 fps). The landing page
no longer shows it; the README's GIF is rendered from the video (see Recordings).
The master is not committed; `make-media.sh MASTER.mp4` writes the web versions:
AV1 WebM and H.264 MP4 at 1600×900 and 30 fps, plus the poster (the "Reuse" frame).
The social card (`og.jpg`) is generated separately from `assets/brand/og-template.svg`.
`media/promo-v10.en.vtt` holds the video's on-screen text as captions; keep it in
step with the video.

## Recordings

The README's GIFs and the landing page's terminal videos are real sessions
recorded with [VHS](https://github.com/charmbracelet/vhs) from the tapes in
`demo/`:

| Tape | Shows | Project |
|---|---|---|
| `reuse.tape` | `status`, then `verify` executes everything and the next `verify` reuses it all | `demo/shop` |
| `change.tape` | one changed file: `status` predicts one execution and names the file, `verify` re-reviews only that | `demo/shop` |
| `human.tape` | `verify` waits by default; in `monitor`, Enter drills from the Run to the waiting eval and opens its Detail pane beside the tree, `c` claims the request, Tab reaches the tools, an output tool runs, `g` opens the GREEN schema form and Ctrl-S submits | `demo/brand` |
| `team.tape` | Alice's `verify` publishes to an `artifactize server`; Bob's `status` and `verify` reuse it | `demo/shop` |

```sh
cargo build --release
website/demo/record.sh               # or: website/demo/record.sh human
```

`record.sh` needs `vhs` (with `ttyd`), `ffmpeg`, `tmux` and `unshare`, and records
`target/release/artifactize` (set `ARTIFACTIZE_BIN` for another build). It writes
`demo/media/NAME.gif` for the README and `landing/media/demo/NAME.{mp4,webp}` for the
landing page. Each tape starts `demo/session.sh`, which runs the shell in private
user, mount, UTS, network and PID namespaces: user alice on host `laptop` with a
throwaway home, state and `/tmp`, a loopback-only network for the review store, and
no provider calls or GUI programs in the fixture. VHS itself needs a Chromium
browser. `DEMO_WORK` must be empty; `record.sh` creates it under `TMPDIR` and cleans
it on exit, and closing the fixture also stops its private tmux and server children.
The tapes use JetBrains Mono, so make it visible
to fontconfig (for example through `FONTCONFIG_FILE`) or change `FontFamily` in
`demo/settings.tape`.

The README's promo GIF, `demo/media/promo.gif`, is rendered from the landing
page's promo video by `website/demo/promo-gif.sh`: 640 px, 25 fps and 64 colours,
with a progress bar along the bottom edge. It needs `ffmpeg`.

## Deploy

`.github/workflows/website.yml` installs the pinned mdBook (checking the release
checksum), runs `build.sh` on pull requests and pushes that touch `website/` or
`assets/`, and
on pushes to `main` deploys `website/dist` with
`wrangler deploy --config website/wrangler.toml`. The Worker serves the
`artifactize.dev` custom domain; unknown paths get `dist/docs/404.html` under
`/docs/` and `dist/404.html` elsewhere. `workflow_dispatch` redeploys `main` by
hand from the Actions tab. The Rust CI workflow skips changes that touch only
`website/`.

The deploy step is skipped with a notice when its secrets are missing.

### Owner actions

These are done for the current deployment; repeat them for a new account or zone.

- [ ] Create a Cloudflare user API token from the "Edit Cloudflare Workers"
      template, limited to this account and the `artifactize.dev` zone.
- [ ] Add the repository secrets `CLOUDFLARE_API_TOKEN` (that token) and
      `CLOUDFLARE_ACCOUNT_ID` (the Cloudflare account ID).
- [ ] Let the first deploy from `main` attach `artifactize.dev` as the Worker's
      custom domain. To serve `www.artifactize.dev` as well, add
      `{ pattern = "www.artifactize.dev", custom_domain = true }` to `routes` in
      `wrangler.toml`.
