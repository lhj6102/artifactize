# 상태, 캐시, 제한 {#state-cache-and-limits}

`artifactize doctor`를 실행하면 상태가 어디 있는지 볼 수 있습니다. 출력 첫 줄에 상태 디렉터리가 표시되고, 이어서 백엔드, 캐시, 저장된 대화를 점검한 결과가 나옵니다. 모두 준비됐으면 종료 코드 0으로 끝납니다. 이 페이지는 그 디렉터리, 재사용 캐시를 살펴보고 제한하는 방법, 머신 전체에서 Agent
백엔드 사용량을 제한하는 방법, 저장된 Agent 대화를 보관하고 크기를 제한하는 방식을 설명합니다.

## 상태 {#state}

상태 데이터베이스인 `state.sqlite` 하나에 이 머신의 모든 저장소에서 만든 Run이 저장됩니다.
위치는 `$ARTIFACTIZE_STATE_HOME`, 없으면 `$XDG_STATE_HOME/artifactize`, 없으면
(Windows에서는) `%LOCALAPPDATA%\artifactize`, 없으면 `$HOME/.local/state/artifactize`입니다.
`--state-dir PATH`를 주면 각 Run의 출력(`PATH/runs` 아래)을 포함해 상태 전체를 다른 폴더에
둡니다. 저장된 Run은 저장소를 지운 뒤에도 계속 읽을 수 있습니다.

상태는 리뷰하는 저장소나 다른 artifactize 작업 공간 안에 둘 수 없습니다. 상태 디렉터리나 그 위의 어느 폴더에든 `.artf` 선언, 예전 `artifactize.json`이나 `.artifactizeignore`가 있으면 명령은 아무것도 쓰지 않고 실패합니다. 프로젝트 폴더들을 담고만 있는 상위 폴더에는 둘 수 있습니다. 예를 들어 `/work/app/index.artf` 옆의 `/work/.state`는 괜찮습니다. 상위 폴더를 탐색할 수는 있지만 목록을 볼 수 없으면 artifactize가
그 안의 선언을 모두 확인할 수 없으므로, 상태는 프로젝트 바깥에 두세요.

**Microsoft Store 패키지**는 zip이나 Cargo로 설치한 버전과 별도로 Run, 로그인 정보, 캐시를 저장하며, 패키지를 제거하면 Windows가 이를 지웁니다. 그렇다고 격리된 환경은 아닙니다. eval이 실행하는
프로그램은 실제 AppData에 쓰고, Store 앱도 패키지 없이 설치한 쪽의 기존 파일을 읽을 수
있습니다. 두 버전을 함께 설치하더라도 데이터가 격리되는 것은 아닙니다.

상태에는 모든 Run과 그 요청, 판정, 각 eval이 어떻게 실행됐는지, 시작할 때의 정의가 남습니다.
`run list`, `run show`, `request list`, `request show`,
[`cache list`](../../cli/maintenance.md#cache-inspection)로 읽을 수 있습니다. Agent 리뷰의
진행 과정과 대화, 도구 호출 내역은 [저장된 세션](#agent-sessions)에서 확인할 수 있습니다.

**업그레이드.** 스키마 6, 7, 8, 9 형식으로 저장된 상태는 artifactize 명령으로 처음 열 때 현재 스키마 10으로
자동 업그레이드됩니다. Run, ID, 캐시 결과, Human 승인은 그대로 남고, SIGNOFFS_PENDING으로 끝난 Run은
YELLOW로 읽힙니다. 업그레이드에는 쓰기 권한이 필요하고 되돌릴 수 없습니다. 스키마 6~9만 지원하는 이전 바이너리로는 업그레이드된 상태를 열 수 없으므로, 이전 버전으로 돌아갈 수도 있다면 먼저 상태 디렉터리를 백업하세요. 스키마 1~5의 상태는
다음 메시지와 함께 거부됩니다(종료 코드 2).

```text
This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). Only schema 6 and later supported versions can be upgraded.
```

더 새로운 artifactize가 쓴 상태는 `Unsupported state schema version: N; a newer artifactize
wrote it.`로 거부됩니다. 열려면 artifactize를 업데이트하세요.

0.9로 업그레이드하면 모든 eval을 한 번 리뷰합니다.
[0.9로 업그레이드](../fingerprint.md#upgrading-to-09)를 보세요.

**토큰은 저장소 밖에 둡니다.** Codex 로그인 정보, 리뷰 저장소 토큰, 저장한 API 키와 커스텀
provider(`providers.json`)를 담는 `$STATE/auth`가
git 워크트리, artifactize 작업 공간, `--repo` 안에 있으면, gitignore된 폴더라도 거부합니다.
그러면 `login`, `provider add`, `remote login`은 아무것도 저장하기 전에 두 폴더 경로를 모두 알려
주며 실패합니다.

```text
Codex sign-in storage /work/project/.state/auth is inside the git work tree /work/project; artifactize keeps tokens outside repositories. Use a state directory outside it, or set ARTIFACTIZE_CODEX_AUTH_FILE.
```

저장소 밖의 상태 디렉터리를 쓰거나, 아무것도 저장하지 않는 `ARTIFACTIZE_CODEX_AUTH_FILE`과
`ARTIFACTIZE_REMOTE_TOKEN`을 쓰세요. `doctor`는 그런 상태에 아직 로그인 정보가 없으면
경고하고, 로그인 정보가 저장되면(예를 들어 기존 상태 디렉터리나 그 상위 디렉터리에서 `git init`을 한 경우) 심각한 오류로
알립니다.

**Run끼리 작업을 나눕니다.** 재사용 키가 있는 완료된 결과는 같은 상태의 모든 Run이
재사용합니다. 두 Run에서 같은 프로필로 같은 리뷰를 동시에 요청하면, 서로
다른 워크트리나 프로세스에서 실행했더라도 두 번째 Run은 다시 실행하지 않고 첫 번째 실행에 합류합니다. 합류한 요청은 원래 실행이
끝날 때까지 대기하며, 실행 시작 횟수에 포함되지 않고 백엔드 자리도 차지하지 않습니다. 합류한 요청도 같은 GREEN, RED, ERROR를
받으며, ERROR 결과는 이후 Run에서 재사용하지 않습니다. 첫 번째 Runtime이나 Agent 리뷰가
취소되거나 끝나지 못하면, 대기 중이던 Run이 직접 실행하고 나머지는 그 실행에 합류합니다. Human
승인은 다시 시작하지 않습니다. 승인을 요청한 Run이 취소되거나 그 프로세스가 죽으면(`OWNER_DIED`), 해당 승인 요청과 그 결과가 나올 때까지 대기 중이던 다른 요청이 모두 ERROR로 끝나며, `verify`를 새로 실행하면 승인을 다시 요청합니다. Run이 YELLOW로 끝나면 대기 중인 승인과 그 결과가 나올 때까지 대기 중인 요청은 그대로 남아 있어 나중에도 제출할 수 있습니다. 대기 중이던 Run을 취소해도 첫 번째
Run은 취소되지 않습니다. `--force`를 쓰면 항상 따로 실행합니다. 진행 중인 리뷰는 원격
저장소를 통해 공유하지 않으며, 이를 지원하는 설정도 없습니다.

## 읽을 수 없는 기록 {#unreadable-records}

다른 상태 데이터가 정상이더라도 저장된 기록 하나를 읽지 못할 수 있습니다. 그러면 artifactize는 임의로 판정을
정하지 않고 그 기록을 증거에서 뺍니다. 그 요청은 Run 요약에서 빠지고, Run 자체를 읽지 못하면
그 사실을 표시합니다. `status`와 `run show`는 그런 기록을 `unreadable` 아래에 `kind`, `id`,
`reason`과 함께 알리고, 증거를 읽는 명령은 stderr에 `Unreadable ...` 줄을 출력합니다.
`run list`는 그런 기록을 확인하지 않습니다. 개수가
모자란다고 그 리뷰가 통과했다는 뜻은 아닙니다. 진단할 수 있도록 상태를 그대로 두고, 새 증거가
필요하면 다시 verify하세요. `doctor`는 상태 데이터를 열 수 있는지와 해당 스키마를 지원하는지만 확인하며,
기록을 고치지는 않습니다.

## 캐시 제한 {#cache-limits}

`verify` Run이 끝나면 저장된 결과를 정리합니다. 아직 남아 있는 프로젝트 폴더(또는 워크트리)마다
eval별 가장 최근 결과는 그 폴더에서 다음에 `verify`를 실행할 때 재사용하므로 항상 남깁니다. 그래서 큰
프로젝트에서 결과가 많이 쌓여도 각 프로젝트의 eval별 최신 결과는 삭제되지 않습니다. 나머지 결과, 즉 같은 eval의 새 결과로
대체된 이전 결과와 이미 없어진 폴더의 결과는 **기록 10,000개**와 **1 GiB**를 넘지 않도록 가장 오랫동안
재사용하지 않은 기록부터 지우며, 승인 결과는 다른 결과를 모두 지운 뒤에야 지웁니다. 기록 하나는 최대
**16 MiB**입니다. 이보다 큰 결과도 그 결과를 만든 Run과 그 결과를 기다리던 Run에는 전달되지만, 나중에 재사용하도록 보관하지는 않습니다. 기록을 재사용한 경우만 사용으로 집계하며, 내용을
살펴보기만 한 경우에는 집계하지 않습니다. 실행 중인 리뷰나 다른 Run이 아직 필요로 하는 기록도 남깁니다.
정리에 실패하면 그 사실을 stderr에 출력하며, 판정은 절대 바뀌지 않습니다.

이 한도는 재사용에만 적용됩니다. 저장된 Run과 그 출력 파일은 남고, 시간이 지났다고 지워지는
것도 없습니다. 끝난 Run의 출력은 `artifactize prune`으로 지우세요.

## 백엔드 수용량 {#backend-capacity}

`verify`는 `--jobs N`을 주지 않으면 READY인 eval을 모두 한꺼번에 실행하며, `--jobs N`도 그
Run만 제한합니다. 한 머신에서 여러 Run(워크트리, CI 작업, 팀원의 실행)이 동시에 실행되면, 한 백엔드에 보내는
Agent 리뷰가 그 백엔드의 사용량 한도나 구독 요금제가 허용하는 수를 넘을 수 있습니다.
`$STATE/limits.json`은 그 상태를 쓰는 모든 `verify`를 통틀어 백엔드마다 동시에 진행하는 Agent
리뷰 수를 제한합니다.

```json
{"backends": {"codex": 4, "openai": 16}}
```

키는 프로필에 적는 백엔드 이름(`openai`, `anthropic`, `gemini`, `codex`, 또는
[커스텀 provider](../agent.md#custom-providers) 이름)이고 한도는 1–100000입니다. 이 파일에
항목이 없는 백엔드는 동시 실행 수를 제한하지 않으며, 파일 자체가 없으면 어떤 백엔드도 제한하지 않습니다. `verify`는 시작할 때 이 파일을 읽습니다. 파일은 artifactize가
지원하는 필드만 담은 64 KiB 이하의 일반 UTF-8 JSON 파일이어야 합니다. 그렇지 않으면 `verify`가 Run을
만들기 전에 멈추고(종료 코드 2), `doctor`는 심각한 오류로 알립니다.

리뷰는 실행되는 동안에만 그 백엔드의 동시 실행 수에 포함됩니다. 결과를
재사용하거나 합류한 요청은 여기에 포함되지 않습니다. 리뷰 프로세스가 죽으면 쓰던 자리가 비고
그 리뷰는 ERROR(`OWNER_DIED`)로 끝납니다. 자리가 모두 차 있으면 요청은 `Waiting for a free codex
slot: all 4 are in use on this machine (limits.json).` 같은 사유와 함께 QUEUED로
표시되고, 자리가 비면 Run의 [스케줄링 순서](../run.md#verify-and-runs)대로 대기 중인 요청을
실행합니다. 대기 중인 요청은 `--jobs`나 `--max-executions`에 포함되지 않으므로 Runtime eval과 Human 요청, 다른 백엔드의 Agent 리뷰는 계속 진행됩니다. [eval mutex 태그](../declarations.md#eval-mutex-tags)가 겹치는 eval은 백엔드 자리를 차지하지 않은 채 별도로 대기합니다. `run cancel RUN_ID`는 그 Run에서 대기 중이던
요청을 ERROR(`CANCELLED`)로 끝내고 그 자리를 비웁니다. `verify --wait`를 중단해도 지켜보기만 멈추며 Run은 계속 실행됩니다.

오류로 [백엔드 사용이 중단되면](../agent.md#stopping-a-backend), 그 Run에서 대기 중이던 요청은 해당
백엔드로 실행하지 않습니다.

## 승인 유휴 유예 시간 {#sign-off-idle-grace}

`limits.json`으로 아무도 맡지 않은 승인만 남은 Run이 YELLOW로 종료되기까지의 대기 시간도
설정합니다([승인과 Run 종료](../run.md#sign-offs-and-the-end-of-a-run)).

```json
{"signoffIdleGraceSeconds": 120}
```

값은 0–86400초이며, 필드나 파일이 없으면 120입니다. 0이면 그런 Run은 바로 끝납니다. 백엔드 한도와
마찬가지로 이 상태를 쓰는 모든 Run에 적용되고 Run이 시작할 때 읽으므로, 바꾼 값은 그 뒤에 시작한
Run에만 적용됩니다. `artifactize limits`(데스크톱 Settings에서도 가능)로 확인하거나 바꾸며, 파일의 다른
필드는 그대로 둡니다.

```sh
artifactize limits [--signoff-idle-grace SECONDS] [--state-dir PATH] [--json]
```

## Agent 세션 {#agent-sessions}

Agent 리뷰의 대화는 상태 디렉터리에 저장되며,
[`session show`와 `session send`](../agent.md#saved-conversations)로 읽거나 이어서 질문할 수 있습니다.
`limits.json`으로 대화 저장 여부와 저장 공간의 크기를 정합니다.

```json
{"agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368}}
```

| 필드 | 기본값 | 뜻 |
|---|---|---|
| `enabled` | `true` | `false`면 새 대화를 저장하지 않음. 이미 저장된 대화는 계속 읽을 수 있음 |
| `maxBytes` | 1 GiB | 이 크기를 넘으면 정리를 시작함. 1 이상 |
| `targetBytes` | `maxBytes`가 768 MiB보다 크면 768 MiB, 아니면 `maxBytes`를 4로 나눈 몫(소수점 이하 버림)에 3을 곱한 바이트 수 | 정리할 때 줄이는 목표 크기. `maxBytes`보다 작아야 함 |

정리는 `verify` Run이 eval을 마쳤을 때와 `prune`을 실행할 때 합니다. 대화가 `maxBytes`보다
많은 공간을 쓰면, 가장 오래된 세션부터 지워 `targetBytes` 이하로 줄입니다. 실행 중인 리뷰의
세션이나 `session send`가 쓰고 있는 세션은 지우지 않으므로, 다음 정리 때까지 목표 크기를 넘을
수 있습니다. `verify` 중에 정리가 실패하면 그 사실을 stderr에 출력하고 Run은 그대로 둡니다.
`agentSessions` 값이 잘못되면 `verify`와 `prune`이 실패하고(종료 코드 2) `doctor`가 이를
알립니다.

저장 위치는 `$STATE/agent-sessions/`이고 나만 읽을 수 있습니다. `session show --json`은 세션의
이벤트를 출력하며, 이벤트마다 `kind`가 있습니다.

| `kind` | 담는 내용 |
|---|---|
| `review` | 첫 줄. 세션, Run, 요청, eval ID, 대상, 만든 사람, 백엔드, 모델, reasoning, 매개변수, 예산, 제공한 도구 |
| `message` | 모델과 주고받은 메시지 하나와 그 텍스트, 도구 호출, 도구 결과. follow-up에서 사람이 쓴 말은 `question`에 있음 |
| `delivery` | 스트리밍되는 동안 보여 준 텍스트(`delta`, `complete`, `interrupted`). 대화나 사용량에는 들어가지 않음 |
| `attempt` | provider 시도 한 번. 사용량 수치와, 실패했으면 `error`와 `errorCode` |
| `end` | 리뷰의 `result`, 또는 `errorCode`와 `error` |
| `send` | follow-up 시작. 번호, 사람이 쓴 `text`, 제공한 도구 |
| `answer` | follow-up 답변 `text`, 또는 `errorCode`와 `error` |

판정, 결과, 사용량은 Run에 남습니다. 대화와 그 도구 호출은 세션에만 기록되며,
[`session show --summary`](../../cli/index.md#session-summary)가 이를 집계합니다.

### follow-up 재생 {#follow-up-replay}

`session send`로 같은 백엔드, 모델, reasoning 설정을 유지한 채 저장된 리뷰 대화에 이어서 질문할 수 있습니다. 현재 프로젝트의 도구를 다시 제공하며,
리뷰 뒤에 그 리뷰가 다룬 Artifact 중 하나가 바뀌었다면 머리말에 그 사실을 표시하고 모델에도 알립니다. 질문은 새 리뷰가 아니라
follow-up입니다. 모델은 일반 텍스트로 답하고, 도구를 쓸 수 있으며, 기록된 판정은 절대 바뀌지
않습니다. `session show`는 질문을 쓴 그대로 보여 줍니다.

질문, 답변, 도구 호출은 같은 대화에 이어서 쌓이며, 질문은 한 번에 하나씩 처리하고 질문마다
프로필 예산이 새로 적용됩니다. 아직 실행 중인 리뷰에는 질문할 수 없습니다.

## 로컬 진단과 유지보수 {#local-diagnostics-and-maintenance}

`cache list`, `cache show`, `cache rm`은 재사용할 결과를 살펴보고 지우며, `doctor`,
`models`, `prune`은 이 머신의 설정을 점검하고 끝난 Run을 정리합니다.
[캐시와 유지보수](../../cli/maintenance.md)를 보세요.
