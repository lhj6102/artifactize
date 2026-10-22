# 리뷰 저장소 서버와 클라이언트 {#review-store-server-and-client}

`artifactize server token add alice --scopes read,publish`(토큰을 한 번 출력합니다)와
`artifactize server run`으로 저장소를 시작하고,
`printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/`로 그 머신에서 로그인하면 `Signed in to … as alice`가 출력됩니다. 이 페이지는 서버와 클라이언트가
정확히 어떻게 동작하는지 설명합니다. 설정 과정은 [팀 리뷰 저장소](../features/team/index.md)
가이드에서 차례대로 따라 할 수 있습니다.

## 리뷰 저장소 서버 {#review-store-server}

```sh
artifactize server token add alice-laptop --scopes read,publish
artifactize server token add ci --scopes read
artifactize server run [--listen 127.0.0.1:8417]
artifactize server token list
artifactize server token revoke alice-laptop [--purge]
artifactize server rm KEY
```

`artifactize server`는 [공유 원격 리뷰 저장소](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md)를
`--state-dir` 아래 별도의 `review-store.sqlite`에 둡니다. `state.sqlite`와는 다른 파일입니다.
각 [재사용 키](../features/fingerprint.md#the-reuse-key)의 기록을 모두 보관하고, 조회하면
누가 게시했는지, 기록이 어떤 순서로 도착했는지와 상관없이 완료 시각이 가장 늦은 기록을
GREEN이든 RED이든 돌려줍니다. `server run`은 기본으로 루프백에서 일반 HTTP로 동작하고
(다른 주소에 바인딩하면 경고합니다) Ctrl-C로 멈춥니다. 클라이언트는 루프백이 아니면
HTTPS를 요구하므로 앞에 TLS 프록시나 터널을 두세요. 토큰 명령은 서버가 실행 중일 때도
같은 파일을 다루며 바로 적용됩니다.

토큰 이름은 비밀값이 아니라 길이가 제한된 ASCII 표시 이름입니다. 문법과 저장소를 열기 전에
하는 검사는 [CLI 식별자 규칙](index.md#identity-arguments)에 있습니다.

`token add`는 무작위 bearer 토큰을 한 번만 출력합니다. 저장소에서 다시 볼 수 없으니
그때 저장해 두세요. `--scopes`는 필수이고 기본값이 없어서, 빠뜨리면 종료 코드 2로 끝납니다.
범위는 `read`(조회), `publish`(runtime과 Agent 기록 게시), `human`(`publish`와 함께 쓰면
Human 승인도 게시)입니다. 신뢰할 수 없는 CI에는 `read`만 주세요. `token list`는 이름,
범위, 생성·폐기 시각을 보여 주고 토큰은 보여 주지 않습니다. `token revoke NAME`은 그 토큰을
바로 거부합니다. `--purge`를 붙이면 그 토큰이 게시한 기록도 모두 지우며, 나중에 다시 실행해도
됩니다. 폐기하면 `--json`이 없어도 항상 JSON 보고서를 출력합니다:
`{"name":"alice-laptop","revokedAt":"2026-10-06T09:14:02Z","purgedEntries":0}`.
`purgedEntries`는 `--purge`가 지운 기록 수입니다. 폐기된 토큰 이름은 다시 쓸 수 없습니다.
`rm KEY`는 그 키의 기록을 모두 지우고 `{"removed":true}`(없었으면 false)를 출력합니다.

저장소에 기록을 쓰는 것은 `artifactize` 클라이언트뿐입니다. `verify`, `request submit`,
`remote push`가 게시하고, `verify`와 `status`가 조회합니다. 서버는 아래 경로로 응답하므로
프록시나 방화벽 규칙을 정할 때 참고하세요. 모든 경로는 `Authorization: Bearer TOKEN`을 받고
JSON으로 응답합니다. 실패하면 `{"error":...}`를 돌려주며, 토큰이 없거나 모르는 토큰이거나
폐기된 토큰이면 401, 범위가 모자라면 403입니다.

| 경로 | 범위 | 쓰는 명령 |
|---|---|---|
| `GET /v1/whoami` | 아무 범위 | `remote login`, `remote status` |
| `POST /v1/lookup` | `read` | `verify`, `status`, `remote push` |
| `PUT /v1/entries/{key}` | `publish`(Human 승인이면 `human`도) | `verify`, `request submit`, `remote push` |

기록에는 eval, 판정, 포함한 fingerprint, 결과를 만든 Run, 만든 사람이 들어 있습니다.
요약 기록(공유 수준 `summary`)에는 eval에 선언된 프로필도 들어가므로 runtime eval의 명령과
인자도 함께 공유됩니다. 다만 캡처한 출력, 명령이 실제로 무엇을 실행했는지에 대한 기록,
저장소 경로는 들어가지 않습니다. 전체 기록(공유 수준 `full`)에는 저장된 실행과 그 출력이
더해집니다. 요약을 공유한다면 선언하는 인자에 비밀값을 넣지 마세요. 기록 크기는 요약이 최대
256 KiB, 전체 기록이 최대 16 MiB입니다. 서버는 형식이 잘못됐거나, 완료 시각이 없거나, 기록의
재사용 키와 보낸 키가 다른 기록을 거부하고, 게시한 토큰의 이름과 서버 시각을 기록에 남깁니다.
그다음 각 클라이언트는 받은 기록의 평가 기준과 fingerprint가 요청한 값과 일치하는지 확인하고, 다르면
경고한 뒤 무시합니다. 그래서 저장소가 클라이언트에게 다른 평가 기준이나 입력의 결과를
재사용하게 만들 수 없습니다. 무엇이 공유됐는지는 클라이언트에서 `artifactize cache list`로
보고, 키의 기록을 지우려면 `artifactize server rm KEY`를 쓰세요. 조회도 사용으로 치며,
저장소는 기록이 100,000개 또는 4 GiB를 넘으면 가장 오랫동안 쓰지 않은 기록부터 지웁니다.

**업그레이드.** 서버와 클라이언트는 함께 업그레이드하세요. 0.3이나 0.4의 저장소는 토큰은
그대로 두고 기록은 버리므로, 클라이언트가 한 번 더 리뷰하고 다시 게시합니다. 0.3과 0.4
클라이언트에는 업그레이드하라는 안내가 갑니다.

0.9부터는 재사용 키 v2를 쓰므로, 스크립트 fingerprint 결과와 Human 승인을 포함해 0.9 이전
기록은 하나도 재사용하지 않습니다. 기존 저장소 기록은 새 키에 맞지 않으므로 클라이언트는 한
번 리뷰하고 새 기록을 게시합니다. 로컬 상태는 따로 업그레이드됩니다.
[상태](../features/others/state.md#state)를 보세요.

## 원격 리뷰 저장소 클라이언트 {#remote-review-store-client}

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/ [--share full]
artifactize remote status [--json]
artifactize remote push [--dry-run] [--json]
artifactize remote logout
```

클라이언트는 `.artf` 선언이 아니라 상태 디렉터리와 환경 변수로만 설정합니다. 그래서 복제한
저장소가 토큰을 다른 곳으로 보내게 만들 수 없습니다. `remote login URL`은 토큰 한 줄을
argv가 아닌 stdin에서 읽습니다. 처음 8192바이트까지만 읽고 그 뒤는 무시한 다음 앞뒤 공백을
지웁니다. 토큰 앞의 공백도 이 8192바이트에 들어가며, 4096바이트가 넘는 토큰은 거부합니다.
터미널 입력을 가리려고 시도하며, Windows에서 이것이 실패하면 토큰이 화면에 보인다고 경고한
뒤 계속 읽습니다. 터미널이 아닌 stdin은 터미널 에코를 건드리지 않고 읽습니다.
`GET /v1/whoami`로 토큰을 확인한 다음, 내 계정만 읽을 수 있는
`$STATE/auth/remote-token.json`에 저장하고 `$STATE/remote.json`에
`{"url":"https://reviews.example/","share":"summary"}`를 씁니다. URL은 시스템 신뢰
저장소로 검증하는 HTTPS여야 하며, 일반 `http://`는 `localhost`, `127.0.0.0/8`, `[::1]`에서만
허용합니다. 자격 증명, 쿼리, 프래그먼트가 붙은 URL은 거부합니다. 저장된 토큰은 발급받은
출처에서만 씁니다.

`ARTIFACTIZE_REMOTE`(URL 또는 `off`), `ARTIFACTIZE_REMOTE_TOKEN`(CI용),
`ARTIFACTIZE_REMOTE_SHARE`(`summary` 또는 `full`)는 `remote.json`과 저장된 토큰보다
우선합니다. `remote status`는 URL, 공유 수준, 토큰 출처(`env`, `file`, `none`), 접속 가능
여부를 출력하고, 이어서 주체와 범위 또는 저장소가 돌려준 오류를 출력합니다. 토큰 인증에 성공하면 0, 실패하면 1로 종료합니다. JSON은
`{"configured":true,"url":…,"share":…,"tokenSource":…,"reachable":BOOL,"principal":…,"scopes":[…],"error":…}`
형태입니다. 로그인한 상태가 아니면 `principal`과 `scopes`가 null이고, 로그인한 상태면
`error`가 null입니다. 토큰이 거부되면 `reachable: true`에 오류가 함께 나오고, 저장소에 연결하지
못하면 `reachable: false`입니다. 설정된 저장소가 없으면 `No remote review store is
configured.`(JSON은 `{"configured":false}`)를 출력하고 1로 종료합니다. 요청은 연결 2초,
전체 5초 시간 제한을 쓰고 리디렉션은 따라가지 않습니다. `remote logout`은 저장된 토큰과
`remote.json`을 지웁니다. 서버에서는 `server token revoke`를 하기 전까지 그 토큰이 계속
유효합니다. 토큰은 출력하지도, 로그에 남기지도 않습니다.

저장소가 설정되어 있으면 `verify`는 리뷰를 실행하기 전에 모두 조회하고, 로컬 결과와 저장소
결과 중 더 나중에 완료된 쪽을 재사용합니다. 저장소 결과는 `origin`과 함께 로컬 상태로
복사되므로 `run show`, `cache`, monitor에 재사용으로 나옵니다. 아직 결과가 없거나 사람의
승인을 대기 중인 리뷰가 있으면 verify는 약 1초마다 저장소를 다시 확인합니다. 다른 곳에서
누군가 게시한 승인이 들어오면 대기 중인 요청은 그 승인을 재사용해 완료되고, 그 요청에 걸려 있던
Human 승인 실행은 ERROR(`SUPERSEDED`)로 끝납니다. 텍스트 출력에는 결과의 출처가 나옵니다.

```text
  app/check [run-Hq2b9X-1]: GREEN (reused from remote: alice@laptop, run-x05qFq)
  brand/signoff [run-Hq2b9X-2]: GREEN (reused from remote: Human sign-off by alice, published by alice-signoff, run-Ksl1Qr)
```

만든 사람(`user@host`)과 Human 리뷰어는 게시한 머신이 기록한 값이고, 게시자는 서버가 인증한
토큰 이름입니다. Agent 결과에는 만든 머신에 있는
[저장된 대화](../features/agent.md#saved-conversations)를 가리키는 참조도 들어 있습니다. 대화
자체는 게시하지 않으며, 재사용한 요청에 `session show`를 실행하면 대화가 어느 머신, 어느 상태에
있는지 알려 줍니다. 이전 버전 클라이언트도 업그레이드 없이 같은 저장소를 쓸 수 있습니다.

**게시되는 것.** 재사용 키가 있는 GREEN이나 RED 결과가 로컬에 저장되면 verify는 요약 기록을
게시하고, 공유 수준이 `full`이면 전체 기록을 게시합니다. 게시에 실패해도 로컬 결과는 남고 실패가
보고됩니다. `request submit`은 Human 승인을 게시합니다. `--speculate` Run은 스케줄링이 끝난 뒤에,
끝까지 유효한 결과만 게시합니다([추측 실행](../features/run.md#speculation-and-deferred-sign-offs)).
토큰은 부여된 [권한 범위](#review-store-server) 안에서만 동작합니다. `read`가 없으면 아무것도
조회하지 않고, `publish`가 없으면 아무것도 게시하지 않으므로 읽기 전용 CI 토큰은 재사용만 하며,
`human`이 없으면 Human 승인은 경고와 함께 로컬에만 남습니다. 재사용 키가 없는 eval은 아무것도
게시하지 않으며, [`--force`](../features/fingerprint.md#completed-result-reuse) 결과는 게시되어 가장
최근 기록이 됩니다. `status`도 읽기 전용 조회로 같은 비교를 하므로 `reuse` 예측에 더 새로운 원격
결과가 반영되며, `status --force`는 원격 호출을 하지 않습니다.

`remote push`는 키마다 이 머신이 만든 가장 최근 GREEN/RED 기록을 보냅니다. 저장소에 연결하지
못했을 때, `remote login` 전에, 또는 `ARTIFACTIZE_REMOTE=off`로 만든 결과가 대상입니다.
미러한 기록은 보내지 않습니다. `read`가 있는 토큰이면 먼저 각 키에 대해 저장소의 최신 기록을
조회하고, 이미 최신인 기록은 다시 보내지 않습니다. 저장소는 각 실행을 한 번만 보관하므로 이미
가진 기록에는 `created: false`로 응답합니다. 둘 다 `existing`으로 세므로 두 번 푸시해도
문제없습니다. `human` 범위 없이 보내는 Human 승인과 크기 제한을 넘는 기록은 stderr에 이유를
남기고 `skipped`가 됩니다. 출력은 `Pushed N, already in the store M, skipped K.` 또는 JSON
`{"dryRun":false,"pushed":N,"existing":M,"skipped":K}`입니다. `--dry-run`은 아무것도 보내지
않고 `Would push N, already in the store M, skipped K.`(JSON은 `"dryRun":true`)를
출력합니다. `verify`와 달리 `push`는 원격 실패가 하나라도 있으면 실패하며, `publish` 범위가
필요합니다.

연결 오류, 요청 시간 초과, 5xx 응답이 나면 경고를 한 번 하고 원격 없이 계속합니다. 각 키의
로컬 최신 기록을 재사용하고, 기록이 없는 것은 로컬에서 리뷰합니다. 401/403, TLS 실패, 그 밖의
오류 응답, 잘못된 설정, 그리고 성공 응답이지만 본문을 읽거나 해석할 수 없는 경우(본문 시간
초과 포함)는 오류로 처리하고 멈춥니다. 401/403에는 저장소가 토큰을 거부했다는 artifactize의
메시지가 붙고, 다른 4xx 응답에는 저장소가 오류 메시지를 보냈다면 그 메시지가 붙습니다. 검증은
Run 접수를 확실히 기록한 다음 원격 증거를 조회합니다. 그래서 조회에 실패해도 Run이 사라지지
않고 ERROR Run으로 남습니다. 제출만 하는 `verify`는 접수되면 0으로 종료하고, `verify
--wait`는 그 ERROR를 확인하고 2로 종료합니다. 설정이 잘못되었으면 접수 전에 제출이 거부될 수도
있습니다. `status` 같은 동기 명령은 오류로 처리된 조회를 바로 종료 코드 2로 알립니다. 검증을
통과하지 못한 기록은 경고와 함께 무시하고, 그 eval은 로컬에서 리뷰합니다.
