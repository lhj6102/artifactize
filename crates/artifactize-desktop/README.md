# artifactize-desktop

The desktop app (Tauri 2 and Svelte 5) over the `artifactize` library. `pnpm e2e` in `ui/`
drives the built app on its own Xvfb display; Korean input needs the desktop's real input
method, so it is a manual test.

## Manual test: Korean input

1. Start the app with fcitx5 running in Hangul mode.
2. Open a waiting Human review, claim it (`c`) and open its GREEN form (`g`).
3. In the comment field, type `dkssud`.

Expected: the field shows `안녕`, and no key typed during the composition (for example `g`,
`r`, `q` or Esc) triggers a shortcut: the form stays open and nothing is claimed, released or
submitted.
