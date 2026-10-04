# artifactize.dev

The site for artifactize: the landing page at `/` and the docs at `/docs/`, both
deployed from this directory to Cloudflare Workers (static assets).

| Path | Contents |
|---|---|
| `landing/` | The landing page: static HTML, CSS and a little JS, copied to `dist/` as is. Also the shared brand fonts (`landing/fonts/`, with their OFL licenses), the promo media (`landing/media/`), favicons, `404.html` and `_headers`. |
| `docs/` | The user docs as an [mdBook](https://rust-lang.github.io/mdBook/): `book.toml`, `src/` (`SUMMARY.md` and the chapters) and `theme/`. Built into `dist/docs/`. |
| `build.sh` | Builds everything into `dist/` (gitignored) and checks every internal link. |
| `check-links.py` | The internal link check that `build.sh` runs over `dist/`. |
| `make-media.sh` | Re-creates `landing/media/` from the promo master. |
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

## Landing page media

The promo video is the owner's motion graphic (1920×1080, 60 fps). The master is
not committed; `make-media.sh MASTER.mp4` writes the web versions: AV1 WebM and
H.264 MP4 at 1600×900 and 30 fps, the poster (the "Reuse" frame) and the 1200×630
social card (`og.jpg`). `media/promo-v10.en.vtt` holds the video's on-screen text
as captions; keep it in step with the video.

## Deploy

`.github/workflows/website.yml` installs the pinned mdBook (checking the release
checksum), runs `build.sh` on pull requests and pushes that touch `website/`, and
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
