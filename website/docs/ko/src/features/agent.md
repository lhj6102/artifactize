# Agent eval과 백엔드 {#agent-evals-and-backends}

Agent eval은 명시한 백엔드 하나를 통해 정확히 지정한 모델 하나에 Artifact 리뷰를 맡깁니다.
모델은 선언한 읽기 전용 도구로 Artifact를 살펴보고, 엄격한 형식의 GREEN 또는 RED 판정을
돌려줍니다.

## Agent 리뷰 {#agent-reviews}

모델 목록이나 별칭을 따로 제공하지 않고, 자격 증명을 찾아다니거나 다른 모델로 대신하지도
않습니다.


```toml
# Inside [evals.review]
profile = { kind = "agent", backend = "openai", model = "YOUR_EXACT_MODEL_ID", reasoning = "high", timeout_ms = 240000 }
```

`backend`는 `openai`, `anthropic`, `codex` 중 하나입니다. 앞의 둘은 각각
`OPENAI_API_KEY`나 `ANTHROPIC_API_KEY`만 쓰고 다른 자격 증명은 찾지 않습니다. `codex`는
ChatGPT 로그인을 씁니다([Codex](#codex) 참고). `provider`와 `effort`는 설정에서 쓸 수 있는
별칭이 아닙니다. 0.5.0에서 `chatgpt`(Sign in with ChatGPT)와 `claude`(Claude CLI) 백엔드가
없어졌으며, 아직 이를 선언한 프로필이 있으면 `config check`가 대신 쓸 백엔드를 알려 줍니다.

```sh
artifactize models openai            # model ID and display name, in provider order
artifactize models anthropic --json
artifactize models codex
```

`reasoning`은 선택 항목입니다. OpenAI와 Codex는 `none`, `minimal`, `low`, `medium`, `high`,
`xhigh`, `max`를, Anthropic은 `low`, `medium`, `high`, `max`를 받습니다. 그 밖의 값은 선언
오류이며, 다른 값으로 바꿔서 보내지 않습니다. 모델이 요청한 설정을 지원하지 않으면 provider에서
실패하며, artifactize가 다른 모델로 바꾸거나 강도를 낮추지 않습니다. 응답에 모델 ID가 나오면
정확히 일치해야 합니다.

Agent eval도 runtime eval과 똑같이 입력을 기다리고 결과를 재사용하며, 리뷰가 끝날 때 입력을
다시 확인합니다. 백엔드, 모델, reasoning, 제한은 실행 옵션일 뿐 재사용 키에
들어가지 않습니다. 같은 eval, Artifact 종류, fingerprint에 대해 다른 모델이나 프로필이 만든
결과도 재사용하며, 그 기록에 어느 쪽이 만들었는지 나옵니다.

`payload.instruction`에는 무엇을 확인하고 무엇을 GREEN으로 볼지 쓰세요. 리뷰어는 eval의
제목과 ID, 대상, 지시문을 받고, artifactize가 일반적인 리뷰 규칙을 덧붙입니다. `{name}`
참조마다 리뷰어에게 그 Artifact의 이름과 읽을 때 쓸 도구(파일 Artifact라면 경로도)를 알려
주므로, 지시문에 도구 이름을 적을 필요는 없습니다.

Agent 리뷰는 스키마로 검사한 GREEN 또는 RED 판정을 돌려줍니다. 모델이 잘못된 결과를 돌려주면
artifactize가 한 번 고쳐 달라고 요청하고, 그래도 잘못되면 리뷰는 ERROR가 됩니다.
[Agent 응답](tools/agent-responses.md)에서 결과, 검사, 수정 요청 턴을 설명합니다.

재시도, 도구 결과, 사용량 수치는 [Agent 백엔드](#agent-backends)에 있습니다.

## Agent 프로필 제한 {#agent-profile-limits}

Agent 프로필은 `max_tokens`와 `max_tool_calls`로 리뷰의 한도를 정할 수 있습니다.

```toml
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", max_tokens = 100000, max_tool_calls = 100 }
```

모델 서비스는 읽고 쓰는 텍스트를 **토큰**이라는 작은 단어 조각 단위로 셉니다. 로그인이나 리뷰
저장소의 토큰과는 상관없습니다. 두 예산 모두 선택 항목이며 1부터 9007199254740991까지의 정수를
받습니다. 생략하면 그 예산은
무제한입니다. 이는 provider의 생성 매개변수가 아니라 클라이언트 쪽 리뷰 예산입니다.
`max_tokens`는 성공한 응답마다 보고된 `total_tokens`(보고된 캐시 읽기·쓰기 포함)를 모든 턴과
수정 요청 턴에 걸쳐 더합니다. 합계가 보고되지 않으면 0으로 셉니다. 확인은 응답을 받은 뒤에
하고 합계가 한도를 넘을 때만 실패하므로, 한도를 조금 넘을 수 있으며 생성량이나 비용의 엄격한
상한은 아닙니다.

`max_tool_calls`는 도구를 실행하기 전에 호출 시도를 세며, 성공한 호출뿐 아니라 모르는 도구,
잘못된 호출, 실패한 호출도 포함합니다. 둘 중 하나라도 넘으면 `PROVIDER_BUDGET_EXCEEDED`로
작업을 멈춥니다. 수정 요청도 원래 리뷰 마감 시간과 예산 안에서 하며, 예산을 새로 받지 않습니다.

## Agent 도구 {#agent-tools}

리뷰어에게 필요한 읽기 전용 도구를 선언하세요. 내장 도구, 사용자 정의 명령, 호출 사이에
테스트 하네스를 계속 실행해 두는 세션은 [도구](tools/index.md)에서 설명합니다. 아래의 저장된
Agent 대화는 도구 세션과 별개입니다.

## Agent 백엔드 {#agent-backends}

Agent 프로필, 백엔드, `reasoning`은 위의 [Agent 리뷰](#agent-reviews)에 있습니다. `models`
출력의 JSON 형태는 [로컬 진단과 유지보수](../cli/maintenance.md#doctor-models-and-prune)에
있습니다.

Agent 리뷰는 모델의 답변을 만들어지는 대로 스트리밍합니다. 리뷰 마감 시간 하나(기본 240초)가
모든 턴, 재시도, 도구에 적용됩니다. 잘렸거나 끝나지 않은 응답은 JSON 판정이 들어 있어도
ERROR입니다. 재시도는 아래 설명대로 artifactize가 처리합니다. API 설정과 재생 방식은
[provider 전송 레퍼런스](others/state.md#provider-transport)에 있습니다.

### 재시도와 오류 코드 {#retries-and-error-codes}

턴이 사용량 제한이나 일시적인 오류로 실패했고, 아직 아무것도 출력하지 않았고 토큰도 쓰지
않았다면 모두 세 번까지 시도합니다. 재시도 전에는 provider가 요청한 만큼 기다리고, 요청이
없으면 250ms, 그다음 500ms를 기다립니다. provider가 요청한 대기 시간이 리뷰 마감 시간에 닿거나
넘으면, 리뷰는 원래 분류인 `RATE_LIMIT`나 `TRANSIENT`로 바로 실패합니다. 출력이나 사용량이
생긴 뒤의 실패는 재시도하지 않으므로 같은 비용을 두 번 내지 않습니다. 시도마다 수치, `error`,
`errorCode`가 `usage`에 저장됩니다.

### 프롬프트 캐싱 {#prompt-caching}

provider는 매 턴 반복되는 대화 부분을 캐시하므로 리뷰의 뒤쪽 턴일수록 비용이 줄어듭니다. 얼마나
캐시되었는지는 `usage`에 있는 시도별 `cacheReadTokens` 수치로 확인하세요(`verify --wait
--json`, `run show`, `request show`, `session show --summary`). 강제한 리뷰를 포함해 리뷰는
매번 새로 시작합니다. [follow-up](#saved-conversations)은 그 리뷰의 대화를 이어 가므로 캐시를
쓸 수 있습니다. 캐시 적중 여부는 provider가 정합니다. `anthropic` follow-up은 답이 판정이 아닌
일반 텍스트라서 대개 캐시가 적중하지 않습니다.

실패한 Agent 리뷰는 다음 `errorCode` 중 하나를 기록합니다.

| `errorCode` | 원인 | 재시도 |
|---|---|---|
| `AUTHENTICATION` | 자격 증명이 없거나 만료됐거나 거부됨: HTTP 401/403, API 키 없음, Codex 로그인 없음, 만료된 인증 파일 | 안 함. 백엔드를 멈춤 |
| `QUOTA` | 결제, 크레딧, 요금제 사용량 한도(`insufficient_quota`, Codex 사용량 한도) | 안 함. 백엔드를 멈춤 |
| `RATE_LIMIT` | 할당량 문제가 아닌 HTTP 429. 재시도 뒤에도 실패했거나 `Retry-After`가 마감 시간에 닿음 | 함 |
| `TRANSIENT` | 연결 실패, HTTP 5xx나 과부하, 끊긴 스트림. 요청한 재시도 대기가 너무 길어도 이 분류를 유지 | 출력 전이면 함 |
| `TIMEOUT` | 리뷰의 `timeout_ms` 마감 시간 | 안 함 |
| `CANCELLED` | Run을 직접 취소함(`run cancel` 또는 MCP `verify_cancel`) | 안 함 |
| `PROVIDER_BUDGET_EXCEEDED` | `max_tokens`나 `max_tool_calls`를 넘음 | 안 함 |
| `INVALID_RESULT` | 형식 수정을 한 번 요청한 뒤에도 유효한 판정이 없음 | 안 함 |
| `PROVIDER_ERROR` | 그 밖의 provider 실패: 거부된 요청, 모르거나 다른 모델, 끝나지 않은 응답, 잘못된 도구 호출 | 안 함 |
| `AGENT_ERROR` | 그 밖의 모든 경우. 예를 들어 잘못된 테스트 엔드포인트나 쓸 수 없는 도구 | 안 함 |
| `BACKEND_STOPPED` | 시작하지 않음: Run이 그 백엔드에 리뷰를 더 받지 않음 | — |

Ctrl-C는 `verify --wait`의 지켜보기만 멈출 뿐, 지켜보던 Agent 리뷰는 멈추지 않으며 Run은
계속됩니다. 직접 취소를 요청해야 Run의 작업이 멈춥니다.

### 백엔드 멈추기 {#stopping-a-backend}

`AUTHENTICATION`이나 `QUOTA` 실패는 같은 백엔드의 모든 리뷰에서 똑같이 반복되므로, 그 Run에서는 그 백엔드로 새 리뷰를 보내지 않습니다. 그 Run에서 뒤이어 오는 같은 백엔드의 리뷰는 시작하지 않고
`BACKEND_STOPPED`인 ERROR가 되며, 오류 메시지에 원래 실패가 나옵니다. 그 백엔드의 빈 자리를
기다리던 리뷰([백엔드 수용량](others/state.md#backend-capacity))도 같은 방식으로 기다리기를
멈추며, 멈춘 백엔드는 자리를 차지하지 않습니다. 이미 실행 중이던 리뷰는 끝까지 실행되고,
재사용할 수 있는 결과는 여전히 재사용하며, 다른 백엔드는 그대로 진행합니다. Run은 멈춘 백엔드를
`stoppedBackends`(`verify --wait --json`과 `run show`)에 남기고, `verify`는 이렇게 출력합니다.

```text
Stopped backend openai after AUTHENTICATION in docs/review: 2 reviews not started.
```

다음 Run에서는 그 백엔드를 다시 쓸 수 있습니다.

eval의 범위에 있는 도구만 제공하며, 도구는 하나씩 차례로 실행됩니다. 각 도구가 무엇을 받고
돌려주는지는 [Agent 도구](tools/agent.md#agent-tools)에 있습니다. PNG/JPEG/WebP 결과는 모든 백엔드에
보내며, 이미지를 지원하지 않는 모델은 provider의 오류로 실패합니다. 요청의 시도별 `usage`는
성공했을 때와 ERROR일 때 모두 저장하며, 캐시가 적중하면 그 시도는 `reusedUsage`로 옮겨지고
쓴 것으로 세지 않습니다. 수치는 provider가 보고한 값(0으로 보고한 경우 포함)이며, 추정한 합계나
비용이 아닙니다. Anthropic의 `inputTokens`는 캐시하지 않은 입력만 센 값이고 캐시 읽기·쓰기
수치는 따로 있습니다. OpenAI의 입력에는 캐시 읽기가 이미 포함되어 있습니다. 모든 수치를 더하지
마세요. 보고되지 않은 필드는 빠집니다. 요청에는 판정, 결과, 사용량이 남고, 메시지, reasoning,
도구 호출과 그 인자, 결과는 리뷰의 [저장된 대화](#saved-conversations)에만 있습니다.
[`session show --summary`](../cli/index.md#session-summary)가 이를 집계합니다.

백엔드를 믿고 쓰기 전에, 계정에서 쓸 수 있는 정확한 모델 ID와 선언한 도구 하나로 작은 리뷰를
한 번 실행하고 `run show`로 결과를 확인하세요. 모델 없이 도구와 스키마를 시험하려면
[가짜 provider](#test-against-a-fake-provider)를 쓰세요.

## 저장된 대화 {#saved-conversations}

artifactize는 Agent 리뷰마다 대화를 이 머신에 남겨 둡니다. 그래서 채팅을 이어 가듯 리뷰어가
무엇을 보고 무엇을 말했는지 읽고, 같은 대화에 이어서 질문할 수 있습니다. 기록된 판정은 절대
바뀌지 않습니다.

**저장되는 내용.** 리뷰 세션마다 내 계정만 읽을 수 있는 파일 하나에 저장합니다
([구성](others/state.md#agent-sessions)). 지시문, 모든 메시지, 도구 호출과 그 결과, 수정 요청 턴,
토큰 수, 리뷰에 쓴 설정이 들어 있습니다. 리뷰의 도구 호출은 세션에만 기록되며, 요청에는 판정,
결과, 사용량만 남습니다. 저장에 실패해도 리뷰는 실패하지 않습니다. 쓰기 오류는 stderr에 알리고 리뷰는 저장
없이 계속됩니다.

**참조.** 대화가 저장된 요청은 `verify --wait --json`, `run show`, `request show`에
`session`을 담습니다.

```json
"session": {
  "producer": "alice@laptop",
  "state": "34bb3c3a-9480-4a54-9ab3-3bce54e043b8",
  "runId": "run-FiVNCV",
  "requestId": "run-FiVNCV-1",
  "sessionId": "e83bf374-5136-484c-8d2f-2ce38ee4ca57",
  "ref": "alice@laptop/34bb3c3a-9480-4a54-9ab3-3bce54e043b8/run-FiVNCV/run-FiVNCV-1/e83bf374-5136-484c-8d2f-2ce38ee4ca57"
}
```

`producer`는 리뷰를 실행한 `user@host`이고, `state`는 그 상태 데이터베이스의 고정 ID입니다.
결과를 재사용한 요청은 그 결과를 만든 리뷰를 가리킵니다. [팀 리뷰 저장소](team/index.md)는
Agent 결과마다 대화가 아니라 이 참조만(`producer.session`으로) 함께 보내므로, 팀원의 판정을 보면
그 대화가 누구의 상태에 있는지 알 수 있습니다.

저장된 대화는 `session show`로 읽고, `session show --summary`로 집계하고, `session send`로
이어서 질문합니다. [Agent 세션](../cli/index.md#agent-sessions)을 보세요.

## Codex {#codex}

`codex`는 ChatGPT 요금제의 Codex 모델(`gpt-6-luna`, `gpt-6.1-sol` 등)로 리뷰하며, Codex
Responses 엔드포인트(`https://chatgpt.com/backend-api/codex/responses`)를 씁니다.

```sh
artifactize login codex     # sign in to ChatGPT in the browser
artifactize models codex    # the account's Codex models
# Declare backend = "codex", model = "gpt-6-luna", reasoning = "max", then:
artifactize verify --all --wait
artifactize logout codex    # delete artifactize's tokens; their revocation is attempted
```

- **로그인.** `login codex`는 Codex CLI처럼 브라우저에서 ChatGPT 로그인 페이지를 열고,
  로그인하면 artifactize로 돌아옵니다. 브라우저 단계, 주소를 붙여 넣는 대체 방법, 5분 제한은
  [로그인 설정](../getting-started/install.md#backends)에 있습니다. 토큰과 ChatGPT 계정 ID는
  나만 읽을 수 있는 `$STATE/auth/codex.json`에 저장합니다. `login`, `models`, `verify`에는 같은
  `--state-dir`을 쓰세요. 토큰은 절대 저장소 안에 저장하지 않습니다. `$STATE/auth`가 git 작업
  트리, artifactize 작업 공간, `--repo` 안에 있으면 `login codex`는 그 폴더를 밝히며 거부하고
  `doctor`는 경고합니다([상태](others/state.md#state)). 저장소 바깥의 상태 디렉터리나
  `ARTIFACTIZE_CODEX_AUTH_FILE`을 쓰세요. artifactize는 액세스 토큰이 만료되기 5분 전에 모든
  Run을 위해 한 번 갱신합니다. 서버가 갱신 토큰을 영구히 거부하면 토큰을 지우고, 리뷰는 다시
  로그인하라고 알립니다.
- **이미 있는 Codex 로그인 쓰기.** `ARTIFACTIZE_CODEX_AUTH_FILE`을 예를 들어
  `~/.codex/auth.json`(1 MiB 이하의 일반 파일이어야 하며, 그렇지 않으면 거부되어 로그인 정보로
  쓰이지 않습니다)으로 설정하면, artifactize는 자기 토큰 대신 매 턴 그 파일의
  `tokens.access_token`(과 `tokens.account_id`)을 읽습니다. 그 파일을 쓰거나 복사하거나
  갱신하지는 않습니다. 토큰 만료까지 남은 시간이 1분 이하이면 리뷰는 `codex login` 등으로 Codex에 다시
  로그인하라는 메시지와 함께 실패합니다. `logout codex`는 그 파일을 건드리지 않습니다.
- **요청.** provider는 대화를 보관하지 않습니다. 선언한 `reasoning`은 `max`를 포함해 그대로
  보내며, reasoning 요약은 Monitor에 나옵니다. 재시도, 예산, 도구 결과,
  [프롬프트 캐싱](#prompt-caching), 사용량은 `openai`와 같습니다.
- **오류.** 사용량 한도는 `QUOTA`이며, 요금제와 초기화 시각을 알려 주고 그 Run에서 `codex`를
  멈춥니다. 단순한 HTTP 429는 `RATE_LIMIT`이며 재시도합니다. HTTP 401과 403은
  `AUTHENTICATION`이며 다시 로그인하는 방법을 알려 줍니다. 갱신하는 동안 토큰 엔드포인트에
  닿지 못하거나 엔드포인트가 실패하면 `TRANSIENT`입니다.

- **모델.** `models codex`는 Codex 모델 선택기가 내 계정에 보여 주는 모델을 서버가 준 순서대로
  나열합니다. 프로필에는 목록에 있든 없든 내 계정이 쓸 수 있는 모델이면 무엇이든 쓸 수 있습니다.

## 가짜 provider로 테스트하기 {#test-against-a-fake-provider}

모델 없이 도구, 스키마, 결과 해석을 시험하려면 백엔드가 이 머신의 가짜 provider를 바라보게
하세요. 아래 변수가 provider의 API 루트를 대신합니다.

| 백엔드 | 변수 | 예 | 가짜가 응답할 요청 |
|---|---|---|---|
| `openai` | `ARTIFACTIZE_OPENAI_BASE_URL` | `http://127.0.0.1:8080/v1` | `POST <root>/responses`, `GET <root>/models` |
| `anthropic` | `ARTIFACTIZE_ANTHROPIC_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1/messages`, `GET <root>/v1/models` |
| `codex` | `ARTIFACTIZE_CODEX_BASE_URL` | `http://127.0.0.1:8080/backend-api/codex` | `POST <root>/responses`, `GET <root>/models` |
| Codex 로그인 | `ARTIFACTIZE_CODEX_AUTH_URL` | `http://127.0.0.1:8080` | `POST <root>/oauth/token`과 `<root>/oauth/revoke`. `login codex`는 브라우저를 `<root>/oauth/authorize`로 보냄 |

- 루트는 `localhost`, `127.0.0.0/8`, `[::1]`의 `http://`나 `https://`여야 하며, 자격 증명,
  쿼리, 프래그먼트가 없어야 합니다. 그 밖의 값이면 연결하기 전에 리뷰, `models`, `doctor`가 그
  변수를 밝히며 실패합니다. 빈 값은 설정하지 않은 것으로 칩니다. `anthropic`은 끝에 `/v1`이
  붙어도 됩니다.
- API 키 변수는 여전히 필요하고 가짜에 그대로 전달되므로 아무 값이나 넣어 두세요. `codex`는
  `ARTIFACTIZE_CODEX_AUTH_FILE`을 `{"tokens":{"access_token":"dummy","account_id":"test"}}`
  같은 파일로 지정하세요.
- 그 밖에는 바뀌는 것이 없습니다. 요청 본문과 스트리밍, 재시도, 예산, 도구 호출, 수정 요청,
  판정 검사가 모두 같습니다.
- 결과는 다른 결과처럼 기록되고 재사용되므로 버려도 되는 상태 디렉터리(`--state-dir`나
  `ARTIFACTIZE_STATE_HOME`)를 쓰세요. 테스트 엔드포인트가 설정되어 있으면 `verify`가 리뷰
  저장소 사용을 거부하므로 `ARTIFACTIZE_REMOTE=off`로 설정하세요. 가짜 결과가 든 상태는 절대
  `remote push`하지 마세요.
- 테스트 엔드포인트가 설정되어 있는 동안 `doctor`는 경고하고 이를 `testEndpoint`로 알립니다.

가짜는 요청한 모델에 대해 provider의 전송 형식대로 `text/event-stream`으로 답을 스트리밍합니다.
`openai`라면 `response.completed` 이벤트 하나면 됩니다. 이 가짜는 GREEN 스키마가
`{"verdict":"GREEN"}`을 받아들이는 리뷰를 통과시킵니다. `pass_schema`에 다른 필드가 필요하면
가짜의 `text` 응답에 그 필드의 유효한 값을 넣으세요. 그러지 않으면 검사에서 수정 요청이
일어나고, 그래도 맞지 않으면 `INVALID_RESULT`가 됩니다.

```python
#!/usr/bin/env python3
"""A fake OpenAI Responses provider on loopback that returns a verdict-only GREEN response."""
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class Fake(BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        text = json.dumps({"verdict": "GREEN"})
        response = {
            "id": "resp_fake", "object": "response", "created_at": 0,
            "model": request["model"], "status": "completed",
            "output": [{"type": "message", "id": "msg_fake", "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": text, "annotations": []}]}],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
        }
        event = {"type": "response.completed", "sequence_number": 0, "response": response}
        body = f"event: response.completed\ndata: {json.dumps(event)}\n\n".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("127.0.0.1", 8080), Fake).serve_forever()
```

```sh
python3 fake_openai.py &
export ARTIFACTIZE_OPENAI_BASE_URL=http://127.0.0.1:8080/v1 OPENAI_API_KEY=dummy
ARTIFACTIZE_REMOTE=off artifactize --state-dir "$(mktemp -d)" verify --all --wait
```

명령 도구를 모델 없이 따로 시험하려면
[`tools check --execute`](../cli/index.md#tool-diagnostics)를 쓰세요. 입력과 출력은
[명령 도구 프로토콜](tools/agent.md)을 따릅니다.
