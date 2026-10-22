# artifactize-desktop

The desktop app (Tauri 2 and Svelte 5) over the `artifactize` library.

## Try this branch's app

In the desktop Artifact's Detail, run the **app** Human tool. It builds this tree's
production frontend and desktop app (without WebDriver), then opens this worktree's branch
on your display with your own artifactize state and complete environment. Build output streams
to the Tool log. Closing the app removes its private build output; it never writes `ui/dist`
or the workspace's `target/`. The CLI and `artifactize-tools` are built beside the app, without
registering or replacing commands in your installation.

Install the UI dependencies once from the workspace root before using the tool:

```sh
pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile
```

The tool needs the ordinary desktop build prerequisites (Rust, Node, pnpm and the platform's
Tauri system dependencies). To run the same command directly from `crates/artifactize-desktop/`:

```sh
node harness/app.mts
```

The app is yours to close, even if you close artifactize first. Ctrl+C or SIGTERM to the
launcher is forwarded to the app; cleanup waits for it to exit.

## Automated tests

`pnpm e2e` in `ui/` builds the CLI and the test app into invocation-owned output and runs
WebdriverIO with `@wdio/tauri-service` and its embedded WebDriver provider. The `e2e` Cargo
feature, separate test capabilities and matching frontend build include the WebDriver plugins;
the normal app and production frontend do not. No external Tauri or WebKit WebDriver is needed.

The service owns each capability's app. Independent scenarios have private repositories,
state and environment inputs; domain fixtures are released after the runner finishes. The
Agent-tool harness uses the same service's public standalone API. On headless Linux the whole
runner uses standard `dbus-run-session` and `xvfb-run -a` wrappers with X11, not the user's
window display. Windows and macOS E2E use the release gate's runner desktops. Vitest component
tests remain the IPC-mocking layer.

This follows [Tauri's WebDriver guide](https://v2.tauri.app/develop/tests/webdriver/) and the
[WebdriverIO Tauri service](https://webdriver.io/docs/desktop-testing/tauri/). Korean input
needs the desktop's real input method, so it also has a manual test.

## Manual test: Korean input

1. Start the app with fcitx5 running in Hangul mode.
2. Open a waiting Human review, claim it (`c`) and open its GREEN form (`g`).
3. In the comment field, type `dkssud`.

Expected: the field shows `안녕`, and no key typed during the composition (for example `g`,
`r`, `q` or Esc) triggers a shortcut: the form stays open and nothing is claimed, released or
submitted.
