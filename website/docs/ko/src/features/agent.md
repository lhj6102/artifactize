# Agent eval과 백엔드 {#agent-evals-and-backends}

Agent eval은 명시한 백엔드 하나를 통해 정확히 지정한 모델 하나에 Artifact 리뷰를 맡깁니다.
모델은 선언한 읽기 전용 도구로 Artifact를 살펴보고, 엄격한 형식의 GREEN 또는 RED 판정을
돌려줍니다.

## Agent 리뷰 {#agent-reviews}

artifactize는 모델 목록을 따로 제공하지 않으며, 여러 위치에서 자격 증명을 찾거나 지정한 모델을 다른 모델로
대체하지도 않습니다.


```toml
# Inside [evals.review]
profile = { kind = "agent", backend = "openai", model = "YOUR_EXACT_MODEL_ID", reasoning = "high", timeout_ms = 240000 }
```

`backend`에는 기본 provider(`openai`, `anthropic`, `gemini`, `codex`)나, eval을 실행하는
머신에 등록한 [커스텀 provider](#custom-providers)의 이름을 씁니다. `openai`, `anthropic`,
`gemini`는 `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`에 든 키를 쓰고, 없으면
`artifactize login openai|anthropic|gemini`나 데스크톱 앱의 Settings에서 저장한 키를 씁니다.
그 밖의 자격 증명은 찾지 않습니다. 키는 공백 없는 출력 가능한 ASCII 문자 1–4096개이며, 앞뒤 공백은
지웁니다. 변수가 설정되어 있으면 그 키를 씁니다. 값이 잘못되었으면 저장한 키로 넘어가지 않고 오류가
납니다. 유효한 유니코드가 아닌 값은 설정되지 않은 것으로 보아 저장한 키를 씁니다. `codex`는 ChatGPT 로그인을 씁니다([Codex](#codex) 참고).
`provider`와 `effort`는 설정에서 쓸 수 있는 별칭이 아닙니다. 0.5.0에서 `chatgpt`(Sign in with ChatGPT)와 `claude`(Claude CLI) 백엔드가
없어졌으며, 아직 이를 선언한 프로필이 있으면 `config check`가 대신 쓸 백엔드를 알려 줍니다.

```sh
artifactize models openai            # model ID and display name, in provider order
artifactize models anthropic --json
artifactize models gemini
artifactize models codex
```

`reasoning`은 선택 항목입니다. OpenAI와 Codex는 `none`, `minimal`, `low`, `medium`, `high`,
`xhigh`, `max`를, Anthropic은 `low`, `medium`, `high`, `max`를, Gemini는 `minimal`, `low`,
`medium`, `high`를 받습니다. 그 밖의 값은 선언 오류이며, 다른 값으로 바꿔서 보내지 않습니다.
커스텀 provider는 그 API가 받는 값을 따르며, 리뷰를 시작할 때 확인합니다. 요청한 설정을 모델이 지원하지 않으면 provider에서 오류가 납니다. artifactize가 다른 모델로 바꾸거나 강도를 낮추지는 않습니다. 응답에 모델 ID가 나오면
정확히 일치해야 합니다.

Agent eval도 Runtime eval과 똑같이 입력의 검사 결과가 나올 때까지 대기하고 결과를 재사용하며, 리뷰가 끝날 때 입력을
다시 확인합니다. 백엔드, 모델, reasoning, 제한은 실행 옵션일 뿐 재사용 키에
들어가지 않습니다. eval, Artifact 종류, fingerprint가 같으면 모델이나 프로필이 달라도 결과를 재사용하며, 결과를 만들 때 쓴 모델과 프로필은 기록에 표시됩니다.

`payload.instruction`에는 무엇을 확인하고 무엇을 GREEN으로 볼지 쓰세요. 리뷰어는 eval의
제목과 ID, 대상, 지시문을 받고, artifactize가 일반적인 리뷰 규칙을 덧붙입니다. 리뷰어는 리뷰만
합니다. 구현이나 수정은 하지 않고, 선언된 Artifact 도구만 실행합니다. 테스트를 실행하거나 테스트
대상 코드를 불러오지 않고, 테스트 코드를 읽어 평가 기준에서 요구하는 사항을 충분히 검증하는지 판단합니다. 테스트 모음이
통과하는지 확인하려면 그것을 실행하는 [Runtime eval](runtime.md)을 선언하세요. `{name}`
참조는 리뷰어에게 이름만으로 전달됩니다. artifactize가 지시문 뒤에 대상과 참조된 Artifact를
하나씩 한 번만, 역할(`target` 또는 `input`)과 읽을 때 쓸 도구 이름과 함께 나열하므로, 지시문에
도구 이름을 적을 필요는 없습니다.

Agent 리뷰는 스키마로 검사한 GREEN 또는 RED 판정을 돌려줍니다. 모델이 잘못된 결과를 돌려주면
artifactize가 한 번 고쳐 달라고 요청하고, 그래도 잘못되면 리뷰는 ERROR가 됩니다.
[Agent 응답](tools/agent-responses.md)에서 결과, 검사, 수정 요청 턴을 설명합니다.

재시도, 도구 결과, 사용량 수치는 [Agent 백엔드](#agent-backends)에 있습니다.

## Agent 프로필 제한 {#agent-profile-limits}

Agent 프로필에서 `max_tokens`와 `max_tool_calls`로 리뷰의 한도를 정할 수 있습니다.

```toml
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", max_tokens = 100000, max_tool_calls = 100 }
```

모델 서비스는 읽고 쓰는 텍스트를 **토큰**이라는 작은 단어 조각 단위로 셉니다. 로그인이나 리뷰
저장소의 토큰과는 상관없습니다. 두 예산 모두 선택 항목이며 1부터 9007199254740991까지의 정수를
받습니다. 생략하면 그 예산은
무제한입니다. 이는 provider의 생성 매개변수가 아니라 클라이언트 쪽 리뷰 예산입니다.
`max_tokens`로 제한하는 사용량은 모든 턴과 수정 요청 턴에서 성공한 응답에 보고된 `total_tokens`를 합산한 값입니다(보고된 캐시 읽기·쓰기 포함). 합계가 보고되지 않으면 0으로 셉니다. 사용량은 응답을 받은 뒤에
확인하고 합계가 한도를 넘을 때만 실패로 처리하므로, 실제 사용량은 한도를 넘을 수 있으며 이 설정이 생성량이나 비용의 엄격한
상한이 되지는 않습니다.

`max_tool_calls`는 도구를 실행하기 전에 호출 시도를 세며, 성공한 호출뿐 아니라 모르는 도구,
잘못된 호출, 실패한 호출도 포함합니다. 둘 중 하나라도 넘으면 `PROVIDER_BUDGET_EXCEEDED`로
작업을 멈춥니다. 수정 요청을 할 때도 원래 리뷰의 제한 시간과 예산을 적용하며, 예산을 추가하지 않습니다.

## Agent 도구 {#agent-tools}

리뷰어에게 필요한 읽기 전용 도구를 선언하세요. 내장 도구, 사용자 정의 명령, 호출 사이에
테스트 하네스를 계속 실행해 두는 세션은 [도구](tools/index.md)에서 설명합니다. 아래의 저장된
Agent 대화는 도구 세션과 별개입니다.

## Agent 백엔드 {#agent-backends}

Agent 프로필, 백엔드, `reasoning`은 위의 [Agent 리뷰](#agent-reviews)에 있습니다. `models`
출력의 JSON 형태는 [로컬 진단과 유지보수](../cli/maintenance.md#doctor-models-and-prune)에
있습니다.

Agent 리뷰에서는 모델이 답변을 생성하는 동안 그 내용을 스트리밍으로 받습니다. 리뷰 전체의 제한 시간은
기본 240초이며, 모든 턴, 재시도, 도구 실행에 걸린 시간을 합쳐 셉니다. 잘렸거나 끝나지 않은 응답은 JSON 판정이 들어 있어도
ERROR입니다. 답변이 긴 공백으로만 이어지면 몇 초 안에 `DEGENERATE_OUTPUT`과 함께 ERROR로
끝납니다. `verify`를 다시 실행하고, 계속 그러면 다른 모델이나 `reasoning`을 써 보세요.
Anthropic 응답은 리뷰의 `max_tokens`와 별개로 턴마다 출력 토큰 16384개로도
제한됩니다. 재시도는 아래 설명대로 artifactize가 처리합니다.

### 재시도와 오류 코드 {#retries-and-error-codes}

출력이나 토큰 사용이 발생하기 전에 사용량 제한이나 일시적인 오류로 턴이 실패하면,
provider가 요청한 시간이 지난 뒤 자동으로 몇 차례 재시도합니다. 출력이나 사용량이
생긴 뒤의 실패는 재시도하지 않으므로 같은 비용을 두 번 내지 않습니다. 재시도로도 해결되지 않거나
provider가 요구한 대기 시간이 리뷰의 남은 시간보다 길면 리뷰는 `RATE_LIMIT` 또는 `TRANSIENT`와 함께
ERROR로 끝납니다. 잠시 뒤 `verify`를 다시 실행하세요. 완료된 리뷰의 결과는 재사용되므로 실패한 리뷰만 다시 실행됩니다. 시도마다 수치, `error`, `errorCode`가 `usage`에 저장됩니다.

### 프롬프트 캐싱 {#prompt-caching}

provider는 매 턴 반복되는 대화 부분을 캐시하므로, 리뷰 후반에는 턴당 비용이 줄어듭니다. 얼마나
캐시되었는지는 `usage`에 있는 시도별 `cacheReadTokens` 수치로 확인하세요(`verify --wait
--json`, `run show`, `request show`, `session show --summary`). 강제로 실행한 리뷰도 포함해 리뷰는 매번 새로 시작합니다. [follow-up](#saved-conversations)은 그 리뷰의 대화를 이어 가므로 캐시를
쓸 수 있습니다. 캐시 적중 여부는 provider가 정합니다. `anthropic` follow-up은 답이 판정이 아닌
일반 텍스트라서 대개 캐시가 적중하지 않습니다.

실패한 Agent 리뷰는 다음 `errorCode` 중 하나를 기록합니다.

| `errorCode` | 원인 | 재시도 |
|---|---|---|
| `AUTHENTICATION` | 자격 증명이 없거나 만료됐거나 거부됨: HTTP 401/403, API 키 없음, Codex 로그인 없음, 만료된 인증 파일 | 안 함. 백엔드를 멈춤 |
| `QUOTA` | 결제, 크레딧, 요금제 사용량 한도(`insufficient_quota`, Codex 사용량 한도) | 안 함. 백엔드를 멈춤 |
| `RATE_LIMIT` | 할당량 문제가 아닌 HTTP 429. 재시도 뒤에도 실패했거나, `Retry-After`에 따른 대기 시간이 리뷰의 남은 시간 이상임 | 함 |
| `TRANSIENT` | 연결 실패, HTTP 5xx나 과부하, 끊긴 스트림. 요청한 재시도 대기가 너무 길어도 이 분류를 유지 | 출력 전이면 함 |
| `TIMEOUT` | 리뷰의 `timeout_ms` 제한 시간 | 안 함 |
| `CANCELLED` | Run을 직접 취소함(`run cancel` 또는 MCP `verify_cancel`) | 안 함 |
| `PROVIDER_BUDGET_EXCEEDED` | `max_tokens`나 `max_tool_calls`를 넘음 | 안 함 |
| `INVALID_RESULT` | 형식 수정을 한 번 요청한 뒤에도 유효한 판정이 없음 | 안 함 |
| `DEGENERATE_OUTPUT` | 답변이 긴 공백으로만 이어져 리뷰가 응답을 멈춤 | 안 함 |
| `PROVIDER_ERROR` | 그 밖의 provider 실패: 거부된 요청, 알 수 없거나 요청과 다른 모델, 끝나지 않은 응답, 잘못된 도구 호출 | 안 함 |
| `AGENT_ERROR` | 그 밖의 모든 경우. 예를 들어 잘못된 테스트 엔드포인트나 쓸 수 없는 도구 | 안 함 |
| `BACKEND_STOPPED` | 시작하지 않음: 그 Run에서 그 백엔드로 새 리뷰를 보내지 않음 | — |

provider의 오류 메시지에 artifactize가 보낸 자격 증명의 일부가 들어 있으면, 그 메시지는 보여
주거나 저장하는 모든 곳에서 `[withheld: it contained part of the credential sent]`로 바뀝니다. 소유자 필드와
명령 출력은 이렇게 가리지 않습니다.

Ctrl-C로 `verify --wait`를 중단해도 진행 상황 표시만 멈추며, Agent 리뷰와 Run은
계속 실행됩니다. 직접 취소를 요청해야 Run의 작업이 멈춥니다.

### 백엔드 멈추기 {#stopping-a-backend}

`AUTHENTICATION`이나 `QUOTA` 실패는 같은 백엔드의 모든 리뷰에서 똑같이 반복되므로, 그 Run에서는 그 백엔드로 새 리뷰를 보내지 않습니다. 이후 그 Run에서 같은 백엔드로 실행하려던 리뷰는 시작하지 않고
`BACKEND_STOPPED`인 ERROR가 되며, 오류 메시지에 원래 실패가 나옵니다. 그 백엔드의 자리가 날 때까지
대기하던 리뷰([백엔드 수용량](others/state.md#backend-capacity))도 같은 방식으로 대기를
멈추며, 멈춘 백엔드의 요청은 자리를 차지하지 않습니다. 이미 실행 중이던 리뷰는 끝까지 실행되고,
재사용할 수 있는 결과는 계속 재사용하며, 다른 백엔드는 그대로 진행합니다. Run은 멈춘 백엔드를
`stoppedBackends`(`verify --wait --json`과 `run show`)에 남기고, `verify`는 이렇게 출력합니다.

```text
Stopped backend openai after AUTHENTICATION in docs/review: 2 reviews not started.
```

다음 Run에서는 그 백엔드를 다시 쓸 수 있습니다.

eval의 범위에 있는 도구만 제공하며, 도구는 하나씩 차례로 실행됩니다. 각 도구가 무엇을 받고
돌려주는지는 [Agent 도구](tools/agent.md#agent-tools)에 있습니다. PNG/JPEG/WebP 결과는 모든 백엔드에
보내며, 이미지를 지원하지 않는 모델은 provider의 오류로 실패합니다. 요청의 시도별 `usage`는 성공했을 때와 ERROR일 때 모두 저장합니다. 캐시가 적중하면 해당 시도의 사용량을 `reusedUsage`에 기록하고, 이번 실행에서 발생한 사용량에는 포함하지 않습니다. 수치는 provider가 보고한 값(0으로 보고한 경우 포함)이며, 추정한 합계나
비용이 아닙니다. Anthropic의 `inputTokens`는 캐시하지 않은 입력만 센 값이고 캐시 읽기·쓰기
수치는 따로 있습니다. OpenAI의 입력에는 캐시 읽기가 이미 포함되어 있습니다. 모든 수치를 더하지
마세요. 보고되지 않은 필드는 빠집니다. 요청에는 판정, 결과, 사용량이 남고, 메시지, reasoning,
도구 호출과 그 인자, 결과는 리뷰의 [저장된 대화](#saved-conversations)에만 있습니다.
[`session show --summary`](../cli/index.md#session-summary)가 이를 집계합니다.

백엔드를 본격적으로 쓰기 전에, 계정에서 쓸 수 있는 정확한 모델 ID와 선언한 도구 하나로 간단한 시험용 리뷰를
한 번 실행하고 `run show`로 결과를 확인하세요. 모델 없이 도구와 스키마를 시험하려면
[가짜 provider](#test-against-a-fake-provider)를 쓰세요.

## 저장된 대화 {#saved-conversations}

artifactize는 Agent 리뷰마다 대화를 이 머신에 남겨 둡니다. 그래서 저장된 대화에서 리뷰어가
무엇을 보고 어떤 답변을 했는지 확인하고, 채팅하듯 이어서 질문할 수 있습니다. 기록된 판정은 절대
바뀌지 않습니다.

**저장되는 내용.** 리뷰 세션은 내 계정만 읽을 수 있는 곳에 저장합니다
([저장소](others/state.md#agent-sessions)). 지시문, 모든 메시지, 도구 호출과 그 결과, 수정 요청 턴,
토큰 수, 리뷰에 쓴 설정이 들어 있습니다. 저장에 실패해도 리뷰는 실패하지 않습니다. 쓰기 오류는 stderr에 출력하고 리뷰는 저장
없이 계속됩니다.

**참조.** 대화가 저장된 요청을 `verify --wait --json`, `run show`, `request show`로 확인하면 `session` 필드가 나옵니다.

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
그 대화가 어느 팀원의 상태 디렉터리에 저장되어 있는지 알 수 있습니다.

저장된 대화는 `session show`로 읽고, `session show --summary`로 집계하고, `session send`로
이어서 질문합니다. [Agent 세션](../cli/index.md#agent-sessions)을 보세요.

## Codex {#codex}

`codex`는 내 ChatGPT 요금제의 Codex 모델(`gpt-6-luna`, `gpt-6.1-sol` 등)로 리뷰합니다.

```sh
artifactize login codex     # sign in to ChatGPT in the browser
artifactize models codex    # the account's Codex models
# Declare backend = "codex", model = "gpt-6-luna", reasoning = "max", then:
artifactize verify --all --wait
artifactize logout codex    # delete artifactize's tokens; their revocation is attempted
```

- **로그인.** `login codex`는 Codex CLI처럼 브라우저에서 ChatGPT 로그인 페이지를 열고,
  로그인하면 artifactize로 돌아옵니다. 브라우저 단계, 주소를 붙여 넣는 대체 방법, 5분 제한은
  [로그인 설정](../getting-started/backends.md)에 있습니다. 토큰과 ChatGPT 계정 ID는
  나만 읽을 수 있는 `$STATE/auth/codex.json`에 저장합니다. `login`, `models`, `verify`에는 같은
  `--state-dir`을 쓰세요. 토큰은 절대 저장소 안에 저장하지 않습니다. `$STATE/auth`가 git 워크트리, artifactize 작업 공간, `--repo` 안에 있으면 `login codex`는 그 폴더 경로를 알려 주며 로그인을 거부하고
  `doctor`는 경고합니다([상태](others/state.md#state)). 저장소 바깥의 상태 디렉터리나
  `ARTIFACTIZE_CODEX_AUTH_FILE`을 쓰세요. artifactize는 액세스 토큰이 만료되기 전에
  갱신합니다. 서버가 갱신 토큰을 더 이상 허용하지 않으면 artifactize는 저장된 토큰을 지우고, 리뷰는 다시
  로그인하라는 오류로 끝납니다.
- **이미 있는 Codex 로그인 쓰기.** 예를 들어 `ARTIFACTIZE_CODEX_AUTH_FILE`을 `~/.codex/auth.json`으로 설정하면, artifactize에 저장된 토큰 대신 매 턴 그 파일의 `tokens.access_token`(과 `tokens.account_id`)을 읽습니다. 지정한 파일은 1 MiB 이하의 일반 파일이어야 하며, 그렇지 않으면 거부되어 로그인 정보로 쓰이지 않습니다. 그 파일을 쓰거나 복사하거나
  갱신하지는 않습니다. 토큰 만료까지 남은 시간이 1분 이하이면 리뷰는 `codex login` 등으로 Codex에 다시
  로그인하라는 메시지와 함께 실패합니다. `logout codex`는 그 파일을 건드리지 않습니다.
- **요청.** provider는 대화를 보관하지 않습니다. 선언한 `reasoning`은 `max`를 포함해 그대로
  보내며, reasoning 요약은 Monitor에 나옵니다. 재시도, 예산, 도구 결과,
  [프롬프트 캐싱](#prompt-caching), 사용량은 `openai`와 같습니다.
- **오류.** 사용량 한도는 `QUOTA`이며, 요금제와 초기화 시각을 알려 주고 그 Run에서 `codex`를
  멈춥니다. 단순한 HTTP 429는 `RATE_LIMIT`이며 재시도합니다. HTTP 401과 403은
  `AUTHENTICATION`이며 다시 로그인하는 방법을 알려 줍니다. 토큰을 갱신할 때 토큰 엔드포인트에
  연결하지 못하거나 오류 응답을 받으면 `TRANSIENT`로 분류합니다.

- **모델.** `models codex`는 지금 계정으로 Codex 모델 선택기에서 고를 수 있는 모델을 서버가 보내 준
  순서대로 나열합니다. 프로필에는 목록에 있든 없든 내 계정이 쓸 수 있는 모델이면 무엇이든 쓸 수 있습니다.

## 커스텀 provider {#custom-providers}

프로젝트에는 provider 이름만 적습니다. 그 이름이 어디로 연결되는지는 eval을 실행하는 머신마다
따로 설정합니다. 커스텀 provider는 Agent 프로필의 `backend`에 쓸 수 있는 이름으로, 데스크톱 앱의
**Settings → Providers**에서 **+ Custom provider**를 누르거나 `artifactize provider`로 추가합니다.
필요한 만큼 추가할 수 있으며, 종류는 둘 중 하나입니다.

- **별칭(alias)**: 기본 provider를 다른 이름으로 쓰며, 그 provider의 로그인이나 키를 그대로 씁니다.

  ```sh
  artifactize provider add work --alias codex
  ```

- **엔드포인트**: API, 루트 URL, 키(선택)를 지정합니다. API는 `openai-responses`, `openai-chat`(호환
  서버 대부분이 지원하는 Chat Completions), `anthropic`, `gemini` 중 하나입니다. `--key`를 주면 키를
  명령줄이 아닌 stdin에서 읽습니다.

  ```sh
  artifactize provider add local --api openai-chat --url http://localhost:11434/v1
  printf '%s\n' "$ROUTER_KEY" | artifactize provider add router --api openai-chat --url https://openrouter.ai/api/v1 --key
  ```

URL은 요청을 보낼 API 루트로, 기본 provider의 루트(`https://api.openai.com/v1`,
`https://api.anthropic.com`, `https://generativelanguage.googleapis.com`)와 같은 형태입니다. `http://`나
`https://`로 시작하고 호스트가 있어야 하며, 자격 증명(`user:pass@`), 쿼리(`?…`), 프래그먼트(`#…`)가 든 URL은
거부합니다. 키는 따로 입력하기 때문입니다. 끝의 `/`는 지웁니다. 이미 있는
이름으로 `provider add`를 실행하면 그 provider를 바꾸고, `provider remove NAME`은 삭제합니다.
`provider list`는 커스텀 provider 전체와 기본 provider의 키를 어디서 가져오는지 보여 주며, 키 값은
보여 주지 않습니다. 기본 provider의 이름은 커스텀 provider 이름으로 쓸 수 없습니다.

```toml
profile = { kind = "agent", backend = "local", model = "YOUR_EXACT_MODEL_ID" }
```

이 머신에 등록되지 않은 provider를 지정한 리뷰는 `AUTHENTICATION` ERROR로 끝나며,
오류 메시지에 그 provider 이름이 표시됩니다. 키와 커스텀 provider는 본인만 읽을 수 있는 `$STATE/auth/providers.json`에
저장되고, 저장소 안에는 저장되지 않습니다. provider도 모델, reasoning과 마찬가지로 실행 옵션이라 결과마다
기록되지만 재사용 키에는 들어가지 않습니다.

## 가짜 provider로 테스트하기 {#test-against-a-fake-provider}

모델 없이 도구, 스키마, 결과 해석을 시험하려면 백엔드를 이 머신의 가짜 provider에
연결하세요. 아래 변수로 provider의 API 루트를 바꿀 수 있습니다.

| 백엔드 | 변수 | 예 | 가짜 provider가 응답할 요청 |
|---|---|---|---|
| `openai` | `ARTIFACTIZE_OPENAI_BASE_URL` | `http://127.0.0.1:8080/v1` | `POST <root>/responses`, `GET <root>/models` |
| `anthropic` | `ARTIFACTIZE_ANTHROPIC_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1/messages`, `GET <root>/v1/models` |
| `gemini` | `ARTIFACTIZE_GEMINI_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1beta/models/<model>:streamGenerateContent`, `GET <root>/v1beta/models` |
| `codex` | `ARTIFACTIZE_CODEX_BASE_URL` | `http://127.0.0.1:8080/backend-api/codex` | `POST <root>/responses`, `GET <root>/models` |
| Codex 로그인 | `ARTIFACTIZE_CODEX_AUTH_URL` | `http://127.0.0.1:8080` | `POST <root>/oauth/token`과 `<root>/oauth/revoke`. `login codex`는 브라우저를 `<root>/oauth/authorize`로 보냄 |

- 루트는 `localhost`, `127.0.0.0/8`, `[::1]`의 `http://`나 `https://`여야 하며, 자격 증명,
  쿼리, 프래그먼트가 없어야 합니다. 그 밖의 값이면 연결하기 전에 리뷰, `models`, `doctor`가 그
  변수 이름을 알려 주며 실패합니다. 빈 값은 설정하지 않은 것으로 간주합니다. `anthropic`은 끝에 `/v1`이
  붙어도 됩니다.
- 이때도 키(변수나 저장한 키)가 필요하고 가짜 provider에 그대로 전달되므로 아무 값이나 넣어 두세요.
  커스텀 엔드포인트에는 변수가 필요 없으며, URL을 가짜 provider 주소로 지정하면 됩니다. `codex`는
  `ARTIFACTIZE_CODEX_AUTH_FILE`을 `{"tokens":{"access_token":"dummy","account_id":"test"}}`
  같은 파일로 지정하세요.
- 그 밖에는 바뀌는 것이 없습니다. 요청 본문과 스트리밍, 재시도, 예산, 도구 호출, 수정 요청,
  판정 검사가 모두 같습니다.
- 결과는 다른 결과처럼 기록되고 재사용되므로 버려도 되는 상태 디렉터리(`--state-dir`나
  `ARTIFACTIZE_STATE_HOME`)를 쓰세요. 테스트 엔드포인트가 설정되어 있으면 `verify`가 리뷰
  저장소 사용을 거부하므로 `ARTIFACTIZE_REMOTE=off`로 설정하세요. 가짜 결과가 든 상태는 절대
  `remote push`하지 마세요.
- 테스트 엔드포인트가 설정되어 있는 동안 `doctor`는 경고하고 이를 `testEndpoint`로 알립니다.

가짜 provider는 요청에 지정된 모델의 응답을 해당 provider의 전송 형식에 맞춰 `text/event-stream`으로 스트리밍합니다.
`openai`라면 `response.completed` 이벤트 하나면 됩니다. GREEN 스키마가 `{"verdict":"GREEN"}`만으로 충족되는 리뷰는 이 가짜 provider로 통과시킬 수 있습니다. `pass_schema`에 다른 필드가 필요하면
가짜 provider의 `text` 응답에 그 필드의 유효한 값을 넣으세요. 그러지 않으면 응답이 스키마에 맞지
않아 artifactize가 한 번 수정을 요청하고, 수정한 응답도 맞지 않으면 `INVALID_RESULT`가 됩니다.

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
