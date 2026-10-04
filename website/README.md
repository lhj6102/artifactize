# artifactize.dev

The site for artifactize: the landing page at `/` (and, later, the docs at `/docs/`).

## Build and preview

```sh
website/build.sh                              # writes website/dist
python3 -m http.server -d website/dist 8080   # http://localhost:8080
```

## Deploy

`.github/workflows/website.yml` builds the site on pull requests that touch `website/`.
On pushes to `main` it also deploys `website/dist` to Cloudflare Workers (static assets)
with `wrangler deploy --config website/wrangler.toml`. The Worker serves the
`artifactize.dev` custom domain.

The deploy step needs two repository secrets, and is skipped with a notice without them:

- `CLOUDFLARE_API_TOKEN`: a user API token from the "Edit Cloudflare Workers" template,
  limited to this account and the `artifactize.dev` zone.
- `CLOUDFLARE_ACCOUNT_ID`: the Cloudflare account ID.

`workflow_dispatch` redeploys `main` by hand from the Actions tab.
