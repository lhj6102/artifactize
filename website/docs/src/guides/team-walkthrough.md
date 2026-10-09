# Walkthrough: two machines and CI share verdicts

This walkthrough reuses [examples/runtime-relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations).
Alice and Bob each have a checkout and their own state directory. CI has a third
checkout. One `artifactize server` holds the shared verdicts. You can run every
step on one computer. Each "machine" is just a `--state-dir`, and `USER` sets
the producer name.

```sh
export WALK=$(mktemp -d)            # outside any Git checkout
cp -r examples/runtime-relations "$WALK/alice"
cp -r examples/runtime-relations "$WALK/bob"
```

## 1. The server

On the host that runs the store, create one token per machine. `token add` prints
each token once; the store keeps only its SHA-256.

```sh
artifactize --state-dir "$WALK/server" server token add alice-laptop --scopes read,publish,human
artifactize --state-dir "$WALK/server" server token add bob-laptop --scopes read,publish
artifactize --state-dir "$WALK/server" server token add ci --scopes read
artifactize --state-dir "$WALK/server" server run      # Listening on http://127.0.0.1:8417/
```

Leave `server run` in its own terminal. A real deployment puts a TLS proxy or
tunnel in front, because clients accept plain HTTP only on loopback.

## 2. Alice reviews first

```sh
cd "$WALK/alice"
export ARTIFACTIZE_STATE_HOME="$WALK/state-alice"
artifactize remote login http://127.0.0.1:8417/   # paste alice-laptop's token; input is hidden when supported
artifactize verify --all
```

```text
  guide/terms [run-FBh4xF-1]: GREEN
  intro/heading [run-FBh4xF-2]: GREEN
  usage/heading [run-FBh4xF-3]: GREEN
Summary: executed 3 (runtime 3, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)
```

Each GREEN result passed its fingerprint recheck, so `verify` published its summary
record: verdict, owner fields, profile, usage counters, producer and timestamps.
It sent no argv, stdout/stderr or paths.

## 3. Bob reuses Alice's verdicts

```sh
cd "$WALK/bob"
export ARTIFACTIZE_STATE_HOME="$WALK/state-bob"
artifactize remote login http://127.0.0.1:8417/   # bob-laptop's token
artifactize status | tail -1
artifactize verify --all
```

```text
Verify actions: will execute 0, will reuse 3, wait 0, blocked 0
  guide/terms [run-MxxJJJ-1]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  intro/heading [run-MxxJJJ-2]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  usage/heading [run-MxxJJJ-3]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
Summary: executed 0 (runtime 0, agent 0, human 0), reused 3 (runtime 3, agent 0, human 0)
```

Bob's checkout has the same Artifacts and content, so the fingerprints, the Eval
definition hashes and therefore the reuse keys match. Paths do not matter. `status` asked the store without
changing anything. `verify` mirrored the three records into Bob's local cache:
`artifactize cache list` shows them with the store as their origin. A later
`verify` reuses them locally, even without the store.

## 4. Bob works offline, then pushes

```sh
printf 'Nothing unchanged is reviewed twice.\n' >> guide/usage/page.md
ARTIFACTIZE_REMOTE=off artifactize verify --all
artifactize remote push --dry-run
artifactize remote push
```

```text
  guide/terms [run-PzLGma-1]: GREEN
  intro/heading [run-PzLGma-2]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  usage/heading [run-PzLGma-3]: GREEN
Summary: executed 2 (runtime 2, agent 0, human 0), reused 1 (runtime 1, agent 0, human 0)
Would push 2, already in the store 0, skipped 0.
Pushed 2, already in the store 0, skipped 0.
```

Only `usage` and `guide`, whose `guide/terms` key covers its child `usage`, were
reviewed again. With the store unreachable instead of `off`, `verify` would warn
once and review locally in the same way. `remote push` sends each key's latest
local result that the store does not have yet. It never sends mirrors, and it skips
Human sign-offs unless the token has `human`.

## 5. CI reuses both

CI is configured through the environment only. It uses the read-only `ci` token
from step 1, here in `CI_TOKEN`, and `--reuse-only agent,human`, so that Agent
reviews and Human sign-offs only come from the store or the cache:

```sh
cp -r "$WALK/bob" "$WALK/ci" && cd "$WALK/ci"
ARTIFACTIZE_STATE_HOME="$WALK/state-ci" \
ARTIFACTIZE_REMOTE=http://127.0.0.1:8417/ \
ARTIFACTIZE_REMOTE_TOKEN="$CI_TOKEN" \
artifactize verify --all --reuse-only agent,human
```

```text
  guide/terms [run-GpqdSQ-1]: GREEN (reused from remote: bob@laptop, run-PzLGma)
  intro/heading [run-GpqdSQ-2]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  usage/heading [run-GpqdSQ-3]: GREEN (reused from remote: bob@laptop, run-PzLGma)
Summary: executed 0 (runtime 0, agent 0, human 0), reused 3 (runtime 3, agent 0, human 0)
```

CI executed nothing. A read token never publishes, so whatever CI does execute
stays in CI. Stop the server and run the same command with a fresh state
directory. The store is unreachable, so CI reviews locally and stays green:

```text
Remote store is unreachable: Connection refused (os error 111); continuing without the remote review store.
Summary: executed 3 (runtime 3, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)
```

A rejected token, by contrast, fails the job with exit 2.

## Human sign-offs

A Human verdict reuses by default through artifactsum; `fingerprint = false` disables it. Here
is a one-file project with a Human sign-off, with a copy for Bob:

```sh
mkdir "$WALK/brand" && cd "$WALK/brand"
echo 'logo v1' > logo.txt
cat > index.artf <<'TOML'
name = "brand"

[evals.signoff]
title = "Sign off"
profile = { kind = "human" }
payload.instruction = "Approve the logo."
TOML
cp -r "$WALK/brand" "$WALK/brand-bob"
```

Alice's `verify` records the request and waits for it. In a second terminal, in
the same directory and with the same `ARTIFACTIZE_STATE_HOME`, she claims it and
signs off. Her token has the `human` scope, so `request submit` publishes the
sign-off, and her `verify` finishes GREEN:

```sh
export ARTIFACTIZE_STATE_HOME="$WALK/state-alice"
artifactize verify --all                    # WAITING_HUMAN; waits for the sign-off
# In the second terminal:
artifactize request list                    # shows REQUEST_ID
artifactize request claim REQUEST_ID
artifactize request submit REQUEST_ID --verdict GREEN
```

In `$WALK/brand-bob`, with Bob's state, `verify --all` now reuses the sign-off
(so would CI with `--reuse-only agent,human`), and the output names both the
reviewer and the authenticated publisher:

```text
  brand/signoff [run-rWExIl-1]: GREEN (reused from remote: Human sign-off by alice, published by alice-laptop, run-FD9ny6)
Summary: executed 0 (runtime 0, agent 0, human 0), reused 1 (runtime 0, agent 0, human 1)
```

Had Bob run `verify --all` first, his `verify` would have waited, and his request
would settle as soon as Alice's sign-off reached the store. Bob's own token lacks `human`, so a
sign-off he submits stays local, with a warning.

To clean up, stop the server and `rm -rf "$WALK"`.
