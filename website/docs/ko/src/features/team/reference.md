# 리뷰 저장소 서버와 클라이언트 {#review-store-server-and-client}

`artifactize server`와 공유 리뷰 저장소 클라이언트 쪽의 정확한
동작입니다. 설정 방법은 [팀 리뷰 저장소](index.md) 가이드에
있습니다.

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
`--state-dir` 아래의 자기 `review-store.sqlite`에 보관하며, 이는 `state.sqlite`와 별개입니다.
각 [재사용 키](../fingerprint.md#the-reuse-key)의 모든 기록을 보관하고,
조회에는 누가 게시했든, 기록이 어떤 순서로 도착했든, GREEN이든 RED이든
완료 시각 기준으로 가장 최근 기록으로 답합니다. `server run`은 기본으로 루프백에서 일반 HTTP를 제공하며
(다른 곳에 바인딩하면 경고), Ctrl-C/SIGTERM에 멈춥니다.
클라이언트는 루프백이 아니면 HTTPS를 요구하므로 앞에 TLS 프록시나 터널을 두세요.
토큰 명령은 서버가 실행 중일 때도 같은 파일에 대해 동작하며 즉시
적용됩니다.

토큰 이름은 비밀이 아니라 길이가 제한된 ASCII 표시 라벨입니다.
[CLI 식별자 규칙](../../cli/index.md#identity-arguments)에 문법과
저장 전 검증이 있습니다.

`token add`는 무작위 bearer 토큰을 한 번 출력하며, 저장소는 그 SHA-256만 보관합니다.
범위는 `read`(조회), `publish`(runtime과 Agent 기록 게시),
`human`(`publish`와 함께 Human 승인도 게시)입니다. 신뢰할 수 없는
CI에는 `read`만 주세요. `token list`는 이름, 범위, 생성/폐기
시각을 보여 주며, 토큰은 결코 보여 주지 않습니다. `token revoke NAME`은 그 토큰을 즉시 거부하고, `--purge`는
그 토큰이 게시한 모든 기록도 삭제하며, 나중에 반복할 수도 있습니다. 폐기는
`--json`이 없어도 언제나 JSON 보고서를 출력합니다. 폐기된 토큰의 이름은 결코 재사용되지 않습니다. `rm KEY`는 키의 모든 기록을 삭제하고
`{"removed":true}`(없으면 false)를 출력합니다.

API는 `Authorization: Bearer TOKEN`을 받고 JSON으로 답합니다(실패하면 `{"error":...}`.
토큰이 없거나 알 수 없거나 폐기되었으면 401, 범위가 없으면 403).

| 경로 | 범위 | 결과 |
|---|---|---|
| `GET /v1/whoami` | 아무 범위 | `{"principal":NAME,"scopes":[...]}` |
| `{"keys":[KEY,...]}`(최대 1000개)를 담은 `POST /v1/lookup` | `read` | `{"entries":[record,...]}`: 찾은 각 키의 가장 최근 기록 |
| 기록을 담은 `PUT /v1/entries/{key}` | `publish`(Human 기록이면 + `human`) | 201 `{"created":true}`, 또는 저장소에 이미 그 키의 이 실행이 있으면 200 `{"created":false}` |

서버는 기록의 봉투(스키마 2, 경로의 키, GREEN/RED
판정, 프로필 kind, 실행 ID, RFC 3339 `completedAt`)와 크기를 확인합니다.
요약은 256 KiB, `execution`을 담은 전체 기록은 16 MiB입니다. 서버는
`publisher`(토큰 이름)와 `publishedAt`(서버 시계)을 찍으며, 클라이언트가 준 값은
대체합니다. 클라이언트는 기록을 쓰기 전에 그 키가 `evalDefHash`,
`fingerprints`, 추가 필드인 `artifactKinds`와 일치하는지 확인합니다. 새 키에서 kind 맵은
정확히 fingerprint 이름들을 포함합니다. 이를 바꾸거나 빼면
kind를 고려하는 키를 충족할 수 없습니다. 조회는 돌려주는 기록의 마지막 사용 시각을 갱신하고,
삽입은 기록 100,000개 또는 4 GiB를 넘으면 가장 오래 쓰지 않은 기록부터 내보냅니다.

**업그레이드.** 스키마 1 또는 2인 `review-store.sqlite`(artifactize 0.3 또는 0.4)는
서버가 열 때 그 자리에서 스키마 3으로 업그레이드됩니다. 토큰은 유지됩니다. 저장된
기록은 버려집니다. 0.4가 계산한 fingerprint와 Eval 정의 해시를 키로 했는데,
어떤 0.5 키도 이와 일치하지 않으므로 클라이언트는 한 번 다시 리뷰하고 새로
게시합니다. 0.5 서버는 0.4와 0.3 클라이언트(그 조회 키와
`PUT /v1/entries/{evalDefHash}/{fingerprint}` 경로)에 410과 업그레이드하라는 메시지로
답합니다. 0.3의 `staleKey` 별칭은 사라졌습니다. 서버와 클라이언트는
함께 업그레이드하세요.

0.9는 재사용 키 v2를 쓰므로, 스크립트 fingerprint
결과와 Human 승인을 포함해 0.9 이전의 어떤 기록도 재사용하지 않습니다. 기존 저장소 기록은 새 키를 충족할 수 없으며,
클라이언트는 한 번 리뷰하고 새 기록을 게시합니다. 이는 로컬 상태
업그레이드와 별개입니다. 스키마 1–5는 교체가 필요하고, 기존 스키마 6~8은
Run, 캐시 기록, Human 승인을 보존한 채 자동으로 스키마 9로 업그레이드됩니다.

## 원격 리뷰 저장소 클라이언트 {#remote-review-store-client}

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/ [--share full]
artifactize remote status [--json]
artifactize remote push [--dry-run] [--json]
artifactize remote logout
```

클라이언트는 `.artf` 선언이 아니라 상태 디렉터리와 환경 변수로만 설정되므로,
복제한 저장소가 토큰을 다른 곳으로 돌릴 수 없습니다. `remote
login URL`은 stdin에서 토큰 한 줄을 읽습니다(argv에서는 결코 읽지 않음). 터미널
입력을 가리려고 시도하며, Windows에서 그것이 실패하면 토큰이 보일 것이라고 경고하고
계속 읽습니다. 터미널이 아닌 stdin은 터미널 에코를 바꾸지 않고 읽습니다.
`GET /v1/whoami`로 토큰을 확인한 뒤 `$STATE/auth/remote-token.json`(0700 디렉터리,
링크가 하나뿐인 0600 파일, 심볼릭 링크를 따라가지 않음)에 저장하고
`$STATE/remote.json`에 `{"url":"https://reviews.example/","share":"summary"}`를 씁니다.
URL은 시스템 신뢰 저장소와 함께 HTTPS를 써야 하며, 일반 `http://`는
`localhost`, `127.0.0.0/8`, `[::1]`에서만 받아들입니다. 자격 증명, 쿼리, 프래그먼트는
거부됩니다. 저장된 토큰은 발급된 출처에 묶입니다.

`ARTIFACTIZE_REMOTE`(URL 또는 `off`), `ARTIFACTIZE_REMOTE_TOKEN`(CI용),
`ARTIFACTIZE_REMOTE_SHARE`(`summary` 또는 `full`)는 `remote.json`과
저장된 토큰을 덮어씁니다. `remote status`는 URL, 공유 수준, 토큰 출처
(`env`, `file`, `none`), 도달 가능 여부, 주체, 범위를 출력하며,
저장소가 토큰을 받아들이면 0으로 종료합니다. 요청은 연결 2초, 전체 5초 시간 제한을 쓰며 결코
리디렉션을 따라가지 않습니다. `remote logout`은 저장된 토큰과 `remote.json`을 삭제하며,
서버는 `server token revoke` 전까지 그 토큰을 유효하게 유지합니다. 토큰은 결코 출력되거나
기록되지 않습니다.

저장소가 설정되어 있으면 `verify`는 저장소를 통해 읽고 씁니다. Run이 무엇이든
점유하기 전에, 이미 로컬 기록이 있는 키를 포함해 모든 키를 한 번씩
최대 1000개 키 묶음의 `POST /v1/lookup`으로 조회합니다. 키가 1000개를 넘으면
요청이 여러 번 필요합니다. 그리고 마지막으로 완료된 기록을 재사용합니다.
키의 가장 최근 로컬 기록이나 저장소의 가장 최근 기록입니다. 로컬 최신 기록보다 나중에 완료된
(또는 로컬 기록이 아예 없는) 저장소 기록은 `origin`을 가진, 자체적으로 완결된
`remote-<executionId>` 실행으로 로컬 이력에 추가되고 로컬 기록처럼
재사용되므로, `run show`, `cache`, GC, monitor, Run
요약이 이를 재사용으로 셉니다. 아직 결과가 없는 키는 로컬 점유 직전과,
artifactize가 소유한 작업자가 Human 대기를 조정하는 동안 최대 초당 한 번
다시 조회합니다. Human을 기다리는 키에 대한 원격 결과는
기다리는 요청들을 확정하며, 한 번도 리뷰되지 않은 그 대기 실행은
ERROR(`SUPERSEDED`)가 됩니다. 텍스트 출력은 출처를 알려 줍니다.

```text
  app/check [run-Hq2b9X-1]: GREEN (reused from remote: alice@laptop, run-x05qFq)
  brand/signoff [run-Hq2b9X-2]: GREEN (reused from remote: Human sign-off by alice, published by alice-signoff, run-Ksl1Qr)
```

만든 사람(`user@host`)과 Human 리뷰어는 게시하는 머신이
기록한 값이고, 게시자는 서버가 인증한 토큰 이름입니다. Agent 결과의
`producer`에는 만든 머신에 있는 [저장된 대화](../agent.md#saved-conversations)에 대한 참조인
`session`도 들어 있습니다. 대화 자체는 결코 게시되지 않습니다. 이전 클라이언트는 이 필드를 무시하고,
서버는 기록을 그대로 저장하므로 프로토콜과 기록 `schema`(2)는
바뀌지 않습니다. 재사용된 원격 결과의 요청에는 그 참조가 나오며, 그에 대한 `session show`는
대화가 있는 머신과 상태를 알려 줍니다. 로컬 확정이
재사용 키가 있는 GREEN/RED를 기록하면(fingerprint 재확인 뒤), verify는
데이터베이스 트랜잭션 밖에서 요약 기록을, 공유 수준이 `full`이면 전체 기록을 보냅니다.
`--speculate` Run은 Human 리뷰가 아직 대기 중이더라도 스케줄링이
끝날 때까지 이 자동 게시를 미룹니다. 그 뒤 막힌 작업을 재사용에서 거둔 다음,
살아남은 결과만 게시합니다. 기록에는 키, Eval 정의 해시, 키가 포함하는 각 Artifact의
fingerprint, 그 `artifactKinds` 맵(`folder`/`file`/`graph`), 실행
`options`(백엔드, 모델,
reasoning, 제한, 프로필 변형), Agent 결과의 `executionPaths` 고정 값이 들어 있습니다. `request submit`은 Human 승인을 게시합니다.
`read`가 없는 토큰은 아무것도 조회하지 않고, `publish`가 없는 토큰은 아무것도 게시하지 않으므로,
읽기 전용 CI 토큰은 재사용만 합니다. Human 승인에는 `human`도 필요하며, 그 밖의
토큰으로는 경고와 함께 로컬에만 남습니다. 재사용 키가 없는 eval은 아무것도 게시하지 않습니다.
`--force`는 결코 저장소에서 읽지 않지만, 강제된 결과는 새 기록이며
다른 기록처럼 게시되므로, (더 새 기록이 없다면) 저장소의 최신 기록도 됩니다.
`status`는 미러링 없이 읽기 전용 조회로 같은 비교를 하므로,
그 `reuse` 예측에는 더 새 원격 결과가 포함됩니다.
`status --force`는 원격 호출을 하지 않습니다.

`remote push`는 키마다 이 머신이 만든 가장 최근 GREEN/RED 기록을 보냅니다.
저장소에 닿을 수 없던 동안, `remote login` 전에,
또는 `ARTIFACTIZE_REMOTE=off`로 만든 결과입니다. 미러는 결코 보내지 않습니다. `read`가 있는 토큰은 먼저
각 키에 대한 저장소의 최신 기록을 조회하며, 이미 최신인 기록은
다시 보내지 않습니다. 저장소는 각 실행을 한 번만 보관하므로, 이미 가진 기록에는
`created: false`로 답합니다. 둘 다 `existing`으로 세며, 두 번 푸시해도 문제없습니다.
`human` 범위가 없는 Human 승인과 크기 제한을 넘는 기록은
stderr에 이유와 함께 `skipped`가 됩니다. `--dry-run`은 아무것도 보내지 않고 무엇이
푸시될지 보고합니다. 출력은 `Pushed N, already in the store M, skipped K.`이거나, JSON
`{"dryRun":false,"pushed":N,"existing":M,"skipped":K}`입니다. `verify`와 달리 `push`는
원격 실패가 하나라도 있으면 실패하며, `publish` 범위가 필요합니다.

연결 오류, 요청 시간 초과, 5xx 응답은 실패해도 진행을 막지 않습니다. 경고를 한 번 한 뒤
원격 없이 계속하며, 각 키의 로컬 최신 기록을 재사용하고
없는 것은 로컬에서 리뷰합니다. 401/403, TLS 실패, 그 밖의 오류 응답, 잘못된
설정, 그리고 본문을 읽거나 디코딩할 수 없는 성공 응답(본문
시간 초과 포함)은 오류로 처리되어 진행을 멈춥니다. 401/403에는 저장소가 토큰을 거부했다는
artifactize 자신의 메시지가 붙고, 다른 4xx 응답에는 저장소가 보낸 경우 저장소 자신의 오류 메시지가
붙습니다. 검증은 Run 접수를 영속적으로 기록한 뒤 원격 증거를 조회합니다.
조회에 실패하면 Run이 없는 것이 아니라 ERROR Run이 남습니다. 제출만 하는
`verify`는 접수되면 0으로 종료하고, `verify --wait`는 그 ERROR를 보고 2로 종료합니다.
잘못된 설정은 여전히 접수 전에 제출을 거부할 수 있습니다. `status` 같은 동기
명령은 오류로 처리된 조회를 바로 종료 코드 2로 보고합니다.
검증을 통과하지 못한 기록은 경고와 함께 무시되고, 그 eval은
로컬에서 리뷰됩니다.
