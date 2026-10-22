# 워크스루: 두 머신과 CI가 판정을 공유하기 {#walkthrough-two-machines-and-ci-share-verdicts}

이 워크스루는 [examples/runtime-relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations)를 사용합니다.
Alice와 Bob은 서로 다른 체크아웃과 상태 디렉터리를 씁니다. CI에는 세 번째
체크아웃이 있습니다. `artifactize server` 하나가 공유 판정을 보관합니다. 모든
단계를 컴퓨터 한 대에서 실행할 수 있습니다. 이 실습에서는 `--state-dir`로 상태 디렉터리를
달리 지정해 각 "머신"을 구분하고, `USER`로 결과를 만든 사람의 이름을 정합니다. 명령은 `examples/`가 들어 있는 artifactize 체크아웃
([`git clone https://github.com/lhj6102/artifactize`](https://github.com/lhj6102/artifactize))의
루트에서 연 POSIX 셸(Linux, macOS, WSL)용입니다.

```sh
export WALK=$(mktemp -d)            # outside any Git checkout
cp -r examples/runtime-relations "$WALK/alice"
cp -r examples/runtime-relations "$WALK/bob"
printf 'WALK directory: %s\n' "$WALK"
```

`mktemp -d`는 체크아웃 밖에 새 임시 폴더를 만들고, `WALK`에 그
경로가 담기며 마지막 줄이 이를 출력합니다. 그 값을 보관하세요.

서버, Alice, Bob, 로컬 CI 시연에는 각각 별도의 터미널을 쓰세요.
**새 터미널마다** 다음을 실행하되, `COPIED_WALK_PATH`를 방금 출력된
절대 경로로 바꾸세요.

```sh
export WALK="COPIED_WALK_PATH"
```

한 터미널에서 export해도 다른 터미널의 변수는 설정되지 않습니다.
`mktemp`를 다시 실행하지 마세요. 이 로컬 예제는 모두 같은 폴더를 써야 합니다. 아래
출력은 Alice/Bob 역할을 보여 주는 예이며, 실제 Run ID와 user/host 라벨은
다릅니다.

## 1. 서버 {#1-the-server}

저장소를 실행하는 호스트에서 머신마다 토큰을 하나씩 만드세요. `token add`는
각 토큰을 한 번만 출력하며 다시 볼 수 없으니, 그때 저장해 두세요.

```sh
artifactize --state-dir "$WALK/server" server token add alice-laptop --scopes read,publish,human
artifactize --state-dir "$WALK/server" server token add bob-laptop --scopes read,publish
artifactize --state-dir "$WALK/server" server token add ci --scopes read
artifactize --state-dir "$WALK/server" server run      # Listening on http://127.0.0.1:8417/
```

`server run`은 별도 터미널에서 계속 실행해 두세요. 이 워크스루는 같은 컴퓨터의
프로그램만 연결합니다. 서로 다른 컴퓨터라면 먼저
[안전한 연결 설정](index.md#connect-teammates-securely)을 따르세요.

## 2. Alice가 먼저 리뷰하기 {#2-alice-reviews-first}

Alice를 위한 두 번째 터미널을 여세요. 첫 번째 터미널에서는 서버가 계속 실행됩니다.

```sh
export WALK='/path/from/setup'
cd "$WALK/alice"
export ARTIFACTIZE_STATE_HOME="$WALK/state-alice"
artifactize remote login http://127.0.0.1:8417/   # paste alice-laptop's token; input is hidden when supported
artifactize verify --all --wait
```

```text
  guide/terms [run-FBh4xF-1]: GREEN
  intro/heading [run-FBh4xF-2]: GREEN
  usage/heading [run-FBh4xF-3]: GREEN
Summary: executed 3 (runtime 3, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)
```

각 GREEN 결과가 fingerprint 재확인을 통과했으므로, `verify`는 요약
기록을 게시했습니다. 판정, 소유자 필드, 프로필, 사용량 카운터, 만든 사람, 타임스탬프입니다.
요약에는 캡처한 stdout/stderr, 별도의 호출 감사 기록,
로컬 저장소 출처가 빠졌습니다. 선언된 Runtime 명령과 인자는
프로필의 일부로 공유되었으며, 그 인자 안의 경로와 민감한 리터럴은
가려지지 않습니다.

## 3. Bob이 Alice의 판정 재사용하기 {#3-bob-reuses-alices-verdicts}

Bob을 위한 세 번째 터미널을 열고, 앞에서 복사해 둔 것과 같은 디렉터리 경로를 설정하세요.

```sh
export WALK='/path/from/setup'
cd "$WALK/bob"
export ARTIFACTIZE_STATE_HOME="$WALK/state-bob"
artifactize remote login http://127.0.0.1:8417/   # bob-laptop's token
artifactize status | tail -1
artifactize verify --all --wait
```

```text
Verify actions: will execute 0, will reuse 3, wait 0, blocked 0
  guide/terms [run-MxxJJJ-1]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  intro/heading [run-MxxJJJ-2]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  usage/heading [run-MxxJJJ-3]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
Summary: executed 0 (runtime 0, agent 0, human 0), reused 3 (runtime 3, agent 0, human 0)
```

Bob의 체크아웃은 Artifact 이름, 종류, 내용이 같아 fingerprint와
eval 정의 해시가 같고, 따라서 재사용 키도 일치합니다. 경로는 상관없습니다. `status`는 Bob의 로컬 상태를 바꾸지 않고
저장소를 조회했으며, 저장소는 조회된 기록을 사용한 것으로 표시합니다. `verify`는 세 기록을 Bob의 로컬 캐시에 미러링했습니다.
`artifactize cache list`가 저장소를 출처로 하는 그 기록을 보여 줍니다. 이후의
`verify`는 저장소가 없어도 그 기록을 로컬에서 재사용합니다.

## 4. Bob이 오프라인으로 작업한 뒤 푸시하기 {#4-bob-works-offline-then-pushes}

Bob의 터미널에서 `$WALK/bob`에 있는 채로 계속하세요.

```sh
printf 'Nothing unchanged is reviewed twice.\n' >> guide/usage/page.md
ARTIFACTIZE_REMOTE=off artifactize verify --all --wait
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

`guide/terms`의 재사용 키에 자식 `usage`가 들어 있으므로, 다시 리뷰한 Artifact는 `usage`와 `guide`뿐입니다.
`off` 대신 저장소에 연결할 수 없는 경우였다면 `verify`는 한 번 경고하고
같은 방식으로 로컬에서 리뷰했을 것입니다. `remote push`는 각 키에 대해 저장소에
아직 없는 가장 최근 로컬 결과를 보냅니다. 미러링한 기록은 보내지 않으며, 토큰에 `human`이 없으면
Human 승인은 건너뜁니다.

## 5. CI가 둘 다 재사용하기 {#5-ci-reuses-both}

CI는 환경 변수로만 설정합니다. 1단계의 읽기 전용 `ci` 토큰을 쓰며,
여기서는 `CI_TOKEN` 변수로 전달합니다. 토큰은 커밋한 파일이 아니라 CI 시크릿으로 등록하세요. `--reuse-only
agent,human`은 Agent 리뷰와 Human 승인이 저장소나 캐시에서만 오도록 보장합니다.
로컬 시연에서는 먼저 이 터미널에도 `WALK`를 복사하고,
1단계에서 `server token add ci`가 출력한 토큰을 `CI_TOKEN`으로 붙여 넣으세요.

```sh
export WALK='/path/from/setup'
export CI_TOKEN='token-printed-for-ci'
cp -r "$WALK/bob" "$WALK/ci" && cd "$WALK/ci"
ARTIFACTIZE_STATE_HOME="$WALK/state-ci" \
ARTIFACTIZE_REMOTE=http://127.0.0.1:8417/ \
ARTIFACTIZE_REMOTE_TOKEN="$CI_TOKEN" \
artifactize verify --all --wait --reuse-only agent,human
```

```text
  guide/terms [run-GpqdSQ-1]: GREEN (reused from remote: bob@laptop, run-PzLGma)
  intro/heading [run-GpqdSQ-2]: GREEN (reused from remote: alice@laptop, run-FBh4xF)
  usage/heading [run-GpqdSQ-3]: GREEN (reused from remote: bob@laptop, run-PzLGma)
Summary: executed 0 (runtime 0, agent 0, human 0), reused 3 (runtime 3, agent 0, human 0)
```

CI는 아무것도 실행하지 않았습니다. 읽기 토큰으로는 결과를 게시할 수 없으므로, CI에서 만든 결과는 CI의 로컬 상태에만 남습니다. 서버를 멈추고 새 상태 디렉터리로
같은 명령을 실행해 보세요. 저장소에 연결할 수 없으므로 CI는 로컬에서 리뷰하고 계속 통과합니다.

```text
Remote store is unreachable: Connection refused (os error 111); continuing without the remote review store.
Summary: executed 3 (runtime 3, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)
```

반면 토큰이 거부되면 작업은 종료 코드 2로 실패합니다.

## Human 승인 {#human-sign-offs}

Human 판정은 기본적으로 artifactsum을 통해 재사용되며, `fingerprint = false`면 재사용되지 않습니다. 다음은
Human 승인이 있는 파일 하나짜리 프로젝트와, Bob을 위한 그 사본입니다. `WALK`가 아직 공유 디렉터리를
가리키고 있는 Alice의 터미널로 돌아가세요.

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

Alice의 `verify --wait`는 요청을 기록할 Run을 제출한 뒤, 포그라운드에서 진행 상황을 보여 주며 Run이 끝날 때까지 대기합니다. 두 번째 터미널에서 같은 디렉터리와 같은
`ARTIFACTIZE_STATE_HOME`으로 Alice는 요청을 맡고
승인합니다. Alice의 토큰에는 `human` 범위가 있으므로 `request submit`이
승인을 게시하고, Alice의 `verify --wait`는 GREEN으로 끝납니다.

첫 번째 터미널에서는 지켜보기를 계속 실행해 두세요.

```sh
export ARTIFACTIZE_STATE_HOME="$WALK/state-alice"
artifactize verify --all --wait
```

새 터미널에서도 앞에서 복사한 경로로 `WALK`를 설정하고, 같은 프로젝트 디렉터리로 이동한 뒤
Alice가 쓰는 상태 디렉터리를 지정하세요. `REQUEST_ID`는 `request list`가 출력한
대기 중인 요청의 식별자로 바꾸세요.

```sh
cd "$WALK/brand"
export ARTIFACTIZE_STATE_HOME="$WALK/state-alice"
artifactize request list
artifactize request claim REQUEST_ID
artifactize request submit REQUEST_ID --verdict GREEN
```

이제 `$WALK/brand-bob`에서 Bob의 상태로 `verify --all --wait`를 실행하면 그 승인을 재사용하며
(`--reuse-only agent,human`을 쓰는 CI도 마찬가지), 출력에
리뷰어와 인증된 게시자가 모두 나옵니다.

```text
  brand/signoff [run-rWExIl-1]: GREEN (reused from remote: Human sign-off by alice, published by alice-laptop, run-FD9ny6)
Summary: executed 0 (runtime 0, agent 0, human 0), reused 1 (runtime 0, agent 0, human 1)
```

Bob이 먼저 `verify --all --wait`를 실행했다면, Bob의 `verify --wait`는
Run의 진행 상황을 보여 주며 끝날 때까지 대기했을 것이고, Alice의 승인이 저장소에 도착하는 즉시 Bob의 요청도
확정되었을 것입니다. Bob의 토큰에는 `human`이 없으므로, Bob이 제출한 승인은
경고와 함께 로컬에만 남습니다.

정리하려면 서버를 멈추고 `rm -rf "$WALK"`를 실행하세요.
