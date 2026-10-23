# 캐시와 유지보수 {#cache-and-maintenance}

`artifactize prune --older-than 30d --dry-run`을 실행하면 끝난 지 30일이 지나 출력을 지울
Run 목록(`wouldRemove`)이 나옵니다. 확인했으면 `--dry-run` 없이 다시 실행해 지우세요.
이 페이지는 이 명령과 함께, 재사용할 결과를 살펴보고 지우는 명령과 이 머신의 설정을
점검하는 명령을 다룹니다. 이 명령들이 다루는 대상은 [상태, 캐시, 제한](../features/others/state.md)에서
설명합니다.

## 캐시 살펴보기 {#cache-inspection}

```sh
artifactize cache list [--history] [--json]
artifactize cache show KEY [--history]
artifactize cache rm KEY
```

이 명령들은 공유 상태나 `--state-dir PATH`를 쓰며 저장소가 없어도 됩니다. 재사용 키마다
완료된 GREEN/RED 기록을 모두 보관하고, `verify`는 그중 가장 최근 기록을 재사용합니다.
`list`는 키마다 가장 최근 기록을 보여 줍니다. 키, eval, 판정, 완료 시각, 만든 사람,
저장소(또는 그 기록을 가져온 원격 저장소), 그 키에 쌓인 기록 수, 크기, 마지막 사용 시각이
나옵니다. JSON에는 eval 정의 해시, 대상의 fingerprint, 프로필 변형도 들어 있습니다.
`--history`는 모든 기록을 최근 것부터 보여 줍니다.

`show`는 그 키의 가장 최근 기록을 JSON으로 출력합니다. 결과, 실제로 쓴 `profile`,
`options`(백엔드, 모델, reasoning, 제한, 변형), 키가 포함하는 각 Artifact의 `fingerprints`,
`artifactKinds`(포함한 이름마다 `folder`, `file`, `graph` 중 하나), 사용량, `producer`
(`user@host`와 artifactize 버전, Human 결과라면 `reviewer`까지)가 나옵니다.
[공유 원격 리뷰 저장소](review-store.md)에서 온 기록에는 `origin`(저장소, 게시자, 게시 시각)이
붙어 있습니다. 키가 없으면 `null`을 출력하고 4로 종료합니다. `--history`는 모든 기록을 배열로
출력합니다. 읽기만 하므로 아무것도 바뀌지 않습니다.

`rm`은 그 키의 기록이 더는 재사용되지 않게 하고 `{"removed":true}`(없었으면 false)를
출력합니다. 저장된 Run의 결과는 그대로 남습니다. 지금 리뷰 중인 키의 기록은 지울 수 없습니다. 키는
`cache list`에서, 또는 `verify --wait --json`과 `run show`에 나오는 요청의 `key`에서 얻을
수 있습니다. 키를 만드는 방법과 캐시가 적중했을 때 무엇을 돌려주는지는
[완료된 결과 재사용](../features/fingerprint.md#completed-result-reuse)에 있습니다.

## 로컬 진단과 유지보수 {#local-diagnostics-and-maintenance}

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models PROVIDER [--json]
artifactize login openai|anthropic|gemini|codex [--json]
artifactize logout openai|anthropic|gemini|codex [--json]
artifactize provider list [--json]
artifactize provider add NAME (--alias PROVIDER | --api API --url URL [--key]) [--json]
artifactize provider remove NAME [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune`은 현재 저장소만이 아니라 선택한 상태에 있는 모든 저장소의 Run을 다룹니다.
**GREEN, RED, ERROR, INCOMPLETE**로 끝났고 아직 실행 중인 작업이 없는 Run의 출력을
지웁니다. YELLOW Run은 승인이 모두 제출된 뒤에도 **항상 건너뛰며**, 끝나지 않은
Run도 건너뜁니다. `--older-than`에는 `s`, `m`, `h`, `d`, `w`를 붙인 정수로 기간을 지정합니다. Run이 끝난 시각을 기준으로 이 기간이 지났는지 확인합니다. 이 옵션이 없으면 대상이 되는 Run을 모두 정리합니다. `prune`은 `--older-than`과 상관없이
[Agent 세션 저장소](../features/others/state.md#agent-sessions)도 크기 한도까지 줄입니다. 텍스트
형식으로는 지운 폴더마다 `Removed PATH`(`--dry-run`이면 `Would remove PATH`), 건너뛴 Run마다
`Skipped Run ID`, 지운 세션마다 `Removed Agent session ID`(`--dry-run`이면 `Would remove Agent
session ID`)를 한 줄씩 출력하고, 마지막에 데이터베이스와 저장소 파일은 그대로 두었다고 알립니다.
`--json`이면 같은 내용을 `removed` 또는 `wouldRemove`, `skippedRuns`, `removedSessions` 또는
`wouldRemoveSessions`로 돌려줍니다. 미리
보기에서는 실행 중인 `session send`가 쓰는 세션을 확인할 수 없으므로, 실제 정리에서는 이보다
적게 지울 수 있습니다.

## Doctor, models, prune {#doctor-models-and-prune}

`doctor`는 provider를 호출하지 않고 Run, 판정, 캐시 항목, 로그인 정보도 만들지 않습니다.
상태 디렉터리를 알려 주고, 쓸 수 있는지와 작업 공간 밖에 있는지 확인합니다. 상태 데이터베이스의
스키마를 artifactize가 지원하는지 확인하고 [필요하면](../features/others/state.md#state) 업그레이드합니다.
지원하지 않는 스키마이면 심각한 오류로 처리하며, 현재 스키마는 `schema`에, 지원하는 스키마는 `supported`에 표시합니다.
`--repo`를 주면 `config check`의 검사도 함께 합니다. 그 밖의 검사는 다음과 같습니다.

- `limits`: [백엔드 수용량](../features/others/state.md#backend-capacity)입니다. `limits.json`이
  잘못되었으면 심각한 오류입니다.
- `sessions`: [Agent 세션 저장소](../features/others/state.md#agent-sessions)입니다. 세션 수와
  바이트 수, `maxBytes`, `targetBytes`, `enabled`, 저장 위치 `directory`, 상태의
  `stateId`가 나옵니다.
- `openai`, `anthropic`, `gemini`: API 키가 변수에 있는지, 저장되어 있는지, 없는지만 알려 주며, 키가 유효한지
  검사하거나 값을 출력하지는 않습니다. 사용하는 백엔드의 키만 있으면 되므로, 키가 없으면 오류가 아니라
  경고로 처리합니다. [테스트 엔드포인트](../features/agent.md#test-against-a-fake-provider)를
  쓰면 어떤 엔드포인트인지 `testEndpoint`로 알려 주는 경고가 나오고, 잘못된 엔드포인트는 심각한 오류입니다.
- `codex`: 로그인 정보를 갱신하거나 온라인에 접속하지 않고 읽습니다. 로그인하지 않았으면
  경고입니다. 로그인 정보가 저장되어 있으면 액세스 토큰이 만료됐어도 검사를 통과합니다. 만료된 토큰은 다음에 쓸 때 갱신합니다.
  `ARTIFACTIZE_CODEX_AUTH_FILE`의 토큰이 만료됐으면 경고입니다. `$STATE/auth`가
  [저장소 안에](../features/others/state.md#state) 있으면 거부 메시지를 `refused`에 담은
  경고가 나오고, 그곳에 로그인 정보가 저장되어 있으면 심각한 오류가 됩니다. 세부 정보에는 `source`
  (`stored`, `file`, `none`), `expiresAt`, `expired`, 테스트 엔드포인트가 나옵니다.
- `providers`: 이 머신에 등록한 [커스텀 provider](../features/agent.md#custom-providers)와 각각이
  연결되는 곳을 보여 줍니다. 키 값은 보여 주지 않습니다. `$STATE/auth`가 저장소 안에 있으면, 저장한 것이
  없는 동안에는 경고이고 저장한 것이 있으면 심각한 오류입니다.
- 원격 리뷰 저장소: 이것도 오프라인으로 확인하며 URL, 공유 수준, 토큰 출처를 보여 줍니다.
  토큰이 없으면 경고이고, 설정이 잘못됐거나(루프백이 아닌 호스트에 일반 HTTP를 쓰는 경우 포함)
  토큰 파일이 안전하지 않으면 심각한 오류입니다.
- `desktop`: 데스크톱 앱이 설치되어 있는지 앱을 실행하지 않고 확인합니다. `artifactize` 명령과
  같은 폴더, `PATH`, 이 시스템의 설치 프로그램이 앱을 두는 위치에서 찾고, 세부 정보의 `path`에 앱
  위치가 나옵니다. 앱이 없으면 [설치 페이지](https://artifactize.dev/#start) 링크와 함께 경고가
  나옵니다. 명령은 앱 없이도 동작합니다.
- `release`: artifactize의 새 릴리스가 나왔는지 확인합니다. `doctor`가 네트워크에 접속하는
  유일한 검사입니다([새 릴리스 알림](../getting-started/update.md#newer-releases)). 새 릴리스가
  있으면 경고를 표시하고, `latest`에는 최신 버전을, `update`에는 현재 artifactize의 설치 방식에
  맞는 업데이트 방법을 담습니다. 실행할 `command`, 내려받을 파일 `download`(`.deb`는 설치할 `command`
  포함), 설치 방법을 알 수 없으면 릴리스 `page`입니다. `channel`은 설치 방식을, `running`은 지금
  실행 중인 artifactize의 버전을 나타냅니다. 확인에 실패해도 경고입니다. Microsoft Store 설치, 개발 빌드,
  `ARTIFACTIZE_NO_UPDATE_CHECK`를 설정한 경우에는 확인하지 않습니다.

심각한 오류가 없으면 0, 있으면 1, 호출이 잘못되었으면 2로 종료합니다.

`models PROVIDER`에는 프로필의 `backend`에 쓸 수 있는 이름이면 무엇이든 줄 수 있습니다.
`models openai`, `models anthropic`, `models gemini`는 API 키로 쓸 수 있는 모델을, `models codex`는
로그인한 [Codex](../features/agent.md#codex) 계정의 모델을, 커스텀 provider는 그 별칭이나
엔드포인트가 제공하는 모델을 보여 줍니다. 여기서도 테스트 엔드포인트가 provider를 대신하며, 다른
계정이나 백엔드로 넘어가지 않습니다. 텍스트 출력은 탭으로 구분한 slug와 이름 쌍이고, JSON은
provider가 준 순서대로 `{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`
형태입니다.

## 키와 커스텀 provider {#keys-and-custom-providers}

`login openai|anthropic|gemini`는 stdin에서 API 키를 읽어 저장합니다. 키는 `remote login`이
[토큰](review-store.md#remote-review-store-client)을 읽을 때와 같이 한 줄을 처음 8192바이트까지만 읽고 앞뒤
공백을 지웁니다. 터미널에서는 입력이 화면에 나타나지 않으며, Windows에서 입력을 숨기는 데 실패하면 키가
화면에 보인다고 경고한 뒤 계속 읽습니다. 같은 이름으로 `logout`을 실행하면 저장한 키를 지웁니다. 키 변수가 설정되어 있으면
그 변수를 먼저 씁니다. `login codex`와 `logout codex`는 [Codex 로그인](../features/agent.md#codex)입니다.

`provider add NAME`은 [커스텀 provider](../features/agent.md#custom-providers)를 저장하며, 같은 이름이
있으면 바꿉니다. `--alias PROVIDER`는 기본 provider를 다른 이름으로 쓰고, 엔드포인트는
`--api openai-responses|openai-chat|anthropic|gemini`와 `--url`로 지정합니다. `--key`는 선택 사항이며,
주면 같은 방식으로 키를 stdin에서 읽습니다. `--key` 없이 저장한 엔드포인트에는 키가 없으며, 키가 있던
엔드포인트를 바꿀 때도 마찬가지입니다. `provider remove NAME`은 삭제합니다. `provider list`는 기본 provider마다 키의
`source`(`environment`, `saved`, `absent` 중 하나이며 변수를 먼저 봅니다), 변수를 쓰는 동안에도 키가 저장되어 있는지(`saved`), 커스텀
provider 전체를 출력하며, 키 값은 출력하지
않습니다.

```json
{"keys":[{"provider":"openai","variable":"OPENAI_API_KEY","source":"environment","saved":true}],
 "custom":{"local":{"kind":"endpoint","api":"openai-chat","url":"http://localhost:11434/v1","key":false},
           "work":{"kind":"alias","provider":"codex"}}}
```

`prune`은 `state/runs/<run-id>` 안의 출력 폴더만 지웁니다. Runtime의 `output`(이전
버전이 남긴 `tmp`, `home`, `cache`도 포함)과 남은 도구 출력이 대상입니다. Run 기록, 결과,
재사용 기록, 알 수 없는 파일은 남으므로 `run show`와 `request show`는 계속 동작합니다.
`--dry-run`일 때도 심볼릭 링크와 프로젝트처럼 보이는 폴더(`.git`이나 Artifact 선언이 있는
폴더)는 건드리지 않습니다. 다른 프로그램이 그 폴더를 바꾸는 동안에는 실행하지 마세요.
