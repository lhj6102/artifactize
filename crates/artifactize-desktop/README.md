# artifactize-desktop

The desktop app (Tauri 2 and Svelte 5) over the `artifactize` library.

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
