# Backends and sign-in

Agent evals send their reviews to a model service; Runtime and Human evals need none.
Set up only the backends your Agent evals use: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
or a ChatGPT plan that includes Codex.

An Agent eval names exactly one backend and one exact model ID. There is no model
catalog and no fallback to another backend or model.

A **backend** is the connection Artifactize uses to send an Agent review to a
model service. An **API key** is that service's secret account credential. For the
API-key backends, create a key in your [OpenAI account](https://platform.openai.com/api-keys)
or [Anthropic Console](https://console.anthropic.com/settings/keys), then replace
`...` in the matching command with that key. Never commit it or put it in an
Artifact declaration. Codex uses the separate browser sign-in below instead.

| Backend | Setup | List models |
|---|---|---|
| `openai` | `export OPENAI_API_KEY=...` | `artifactize models openai` |
| `anthropic` | `export ANTHROPIC_API_KEY=...` | `artifactize models anthropic` |
| `codex` | `artifactize login codex` | `artifactize models codex` |

In PowerShell, set a key with `$env:OPENAI_API_KEY = "..."`.

`login codex` signs in to your ChatGPT account the way the Codex CLI does. It prints
a sign-in URL and opens it in the default browser (`xdg-open` on Linux and WSL, `open`
on macOS, the default browser on Windows). Finish in a browser on the same machine:
the browser returns to `localhost:1455`. On WSL 2, a Windows browser works too; copy
the printed URL into it if nothing opens. If another program, such as a Codex CLI or app sign-in, is using port 1455,
artifactize asks you to paste the address instead. On another machine, use the same fallback.
Paste the full URL starting `http://localhost:1455/auth/callback?code=` into the
terminal, or the desktop sign-in's paste field. Sign-in must finish within five minutes;
after that, run `login codex` again. Find it in the browser's address bar
after the failed load, or in DevTools → Network if the page keeps loading.
`logout codex` deletes artifactize's tokens
and asks the server to revoke them; it warns when the revocation is not confirmed. To use the tokens of an existing Codex sign-in instead, set
`ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json`; artifactize only reads that file.
See [Codex](../features/agent.md#codex).

```sh
artifactize doctor                 # state and its schema, API-key presence, Codex sign-in, review store, desktop app
artifactize doctor --repo PROJECT  # also validates PROJECT's declarations
```

`doctor` calls no provider and creates no Run. Missing keys or sign-ins are
warnings, because every backend is optional; so is a missing desktop app, with a link to
install it. It exits 1 only for hard errors such as unusable state
or invalid configuration.
