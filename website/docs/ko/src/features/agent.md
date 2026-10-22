# Agent eval과 백엔드 {#agent-evals-and-backends}

Agent eval은 명시한 백엔드 하나를 통해 정확한 모델 하나에게, 선언한 읽기 전용 도구로
Artifact를 리뷰하고 엄격한 GREEN 또는 RED
판정을 돌려 달라고 요청합니다.

## Agent 리뷰 {#agent-reviews}

Agent eval은 명시한 모델과 백엔드 하나를 쓰며, 내장 카탈로그, 별칭,
자격 증명 검색, 대체 수단은 없습니다.

```toml
# Inside [evals.review]
profile = { kind = "agent", backend = "openai", model = "YOUR_EXACT_MODEL_ID", reasoning = "high", timeout_ms = 240000 }
```

`backend`는 `openai`, `anthropic`, `codex`를 받습니다. 앞의 둘은 각각
`OPENAI_API_KEY`나 `ANTHROPIC_API_KEY`만 쓰며 다른 자격 증명을 결코
찾지 않습니다. `codex`는 ChatGPT 로그인을 씁니다([Codex](#codex) 참조). `provider`와
`effort`는 설정 별칭이 아닙니다. 0.5.0에서 `chatgpt`(Sign in with ChatGPT)와
`claude`(Claude CLI) 백엔드가 제거되었습니다. 아직 이를 선언한 프로필이 있으면
`config check`가 대체할 것을 알려 줍니다.

```sh
artifactize models openai            # model ID and display name, in provider order
artifactize models anthropic --json
artifactize models codex
```

`reasoning`은 선택입니다. 지정하면 OpenAI와 Codex는 정확히
`reasoning.effort`(`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`)를 받습니다. Anthropic은
적응형 사고(adaptive thinking)와 정확히 `output_config.effort`(`low`, `medium`, `high`,
`max`)를 받습니다. 다른 값은 거부되며 다른 값으로 바꾸지 않습니다. 요청한 설정을 지원하지 않는
모델은 제공자에서 실패합니다. artifactize는 모델을 바꾸거나
effort를 낮추지 않습니다. 응답이 모델 ID를 보고하면 정확히 일치해야 합니다.

Agent eval은 runtime eval과 의존성 관문, 재사용 키 점유, 재사용, 최종
fingerprint 재확인을 공유합니다. 백엔드, 모델, reasoning, 제한은 실행
옵션이며 재사용 키에 들어가지 않습니다. 같은 eval, Artifact kind, fingerprint에 대해
다른 모델이나 프로필이 만든 결과도 재사용되며, 그 기록에 어느 쪽이 만들었는지 나옵니다.

평가자의 시스템 프롬프트에는 리뷰 규칙이 들어 있습니다. 사용자 메시지에는
eval 제목과 ID, 대상 Artifact, 지시문만 들어 있습니다. Artifact 참조는
Artifact ID와 쓸 수 있는 도구로 펼쳐지고, 파일 Artifact는 워크스페이스 기준 상대
대상 경로도 포함합니다. 그래서 지시문에 도구 이름을 반복해 쓸 필요가 없습니다.

Agent 리뷰는 스키마로 검증된 GREEN 또는 RED 판정을 돌려줍니다. 모델이
잘못된 결과를 돌려주면 artifactize가 한 번 수정을 요청하고, 그래도 잘못되면
리뷰는 ERROR가 됩니다. [Agent 응답](tools/agent-responses.md)에서
백엔드 형식, 소유자 필드 검증, 복구, 제한을 정합니다.

rig 어댑터, 재시도, 도구 결과, 사용량 카운터는
[Agent 백엔드](#agent-backends)에 있습니다.

## Agent 프로필 제한 {#agent-profile-limits}

Agent 프로필은 `max_tokens`와 `max_tool_calls`로 리뷰를 제한할 수 있습니다.

```toml
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", max_tokens = 100000, max_tool_calls = 100 }
```

각 선택 예산은 1부터 9007199254740991까지의 정수를 받습니다.
생략하면 그 예산은 무제한입니다. 이는 제공자 생성 매개변수가 아니라
클라이언트 쪽 리뷰 예산입니다. `max_tokens`는 성공한 각 응답이
보고한 `total_tokens`를, 보고된 캐시 읽기/쓰기를 포함해 모든
턴과 복구 턴에 걸쳐 더합니다. 합계가 보고되지 않으면 적용 시 0으로 셉니다. 확인은
응답 뒤에 이루어지며 합계가 제한을 넘을 때만 실패합니다. 그래서
제한을 넘어설 수 있으며, 생성이나 지출을 딱 끊는 상한이 아닙니다.

`max_tool_calls`는 성공한 도구뿐 아니라 알 수 없거나
잘못되었거나 실패한 호출을 포함해, 호출하기 전에 시도된 호출을 셉니다. 어느 예산이든 넘으면
`PROVIDER_BUDGET_EXCEEDED`로 이후 작업을 멈춥니다. 복구는 원래의
리뷰 마감 시간과 예산 안에서 이루어지며 새 허용량을 더하지 않습니다.

## Agent 도구 {#agent-tools}

리뷰어에게 필요한 읽기 전용 도구를 선언하세요. [도구](tools/index.md) 영역에서
내장 도구, 사용자 정의 명령, 호출 사이에 하네스를 계속 실행해 두는 세션을
설명합니다. 아래의 저장된 Agent 대화는 도구 세션과 별개입니다.

## Agent 백엔드 {#agent-backends}

위의 [Agent 리뷰](#agent-reviews)에서 Agent 프로필, 백엔드,
`reasoning`을 보여 줍니다. `models` 출력은
[로컬 진단과 유지 보수](others/state.md#doctor-models-and-prune)에 설명된 JSON 형식을 씁니다.

Agent 리뷰는 모델의 답을 만들어지는 대로 스트리밍합니다. 리뷰 마감 시간 하나
(기본 240초)가 모든 턴, 재시도, 도구를 포함합니다. 잘리거나 불완전한
응답은 JSON 판정이 들어 있더라도 ERROR입니다. Artifactize는 아래와 같이
재시도를 처리합니다. API 설정과 재생 동작은 [제공자 전송 레퍼런스](others/state.md#provider-transport)에서
다룹니다.

### 재시도와 오류 코드 {#retries-and-error-codes}

모든 요청은 대화 전체를 다시 보내므로(서버 쪽 상태 없음) 어느 턴이든
재시도할 수 있습니다. 시도가 출력을 만들거나 사용량을 보고하기 전에
속도 제한이나 일시적 오류로 실패하면, 그 턴은 모두 합쳐 최대 세 번까지 시도됩니다.
재시도는 제공자가 `retry-after-ms`나
`Retry-After`(초 또는 HTTP 날짜)로 요청한 만큼 기다리고, 그렇지 않으면 250 ms, 다음은 500 ms를 기다립니다.
제공자가 요청한 대기 시간이 리뷰 마감 시간에 닿거나 넘으면, 리뷰는
원래의 `RATE_LIMIT` 또는 `TRANSIENT` 분류로 바로 실패합니다. 출력이나 사용량이 나온 뒤의 실패는 결코 재시도하지 않으므로
같은 것에 두 번 비용을 내지 않습니다. 각 시도는 카운터, `error`,
`errorCode`와 함께 `usage`에 저장됩니다.

### 프롬프트 캐싱 {#prompt-caching}

각 Agent 리뷰는 무작위 세션 ID를 받으며, 이는 요청에 `sessionId`로 저장됩니다
(`verify --wait --json`, `run show`, `request show`). 리뷰의 모든 요청, 즉 턴,
재시도, 복구 턴은 `openai`와 `codex`에서 이를 `prompt_cache_key`로 보내고,
`codex`는 `session-id` 헤더로도 보냅니다. 그러면 제공자는 각 턴이 다시 보내는
대화 앞부분을 프롬프트 캐시에서 제공할 수 있습니다. 강제
재리뷰를 포함해 모든 리뷰는 새 세션을 시작합니다. `anthropic`은 앞부분 기준으로 캐시하며 세션을 보내지 않습니다.
`usage`의 각 시도에 있는 `cacheReadTokens` 카운터가 캐시된 입력을 보여 줍니다.
저장된 리뷰에 대한 [후속 질문](#saved-conversations)은 그 세션 ID와
대화 앞부분을 재사용합니다. 캐시 적중은 여전히 제공자에 달려 있습니다. 후속 질문은
일반 텍스트 답을 위해 구조화된 출력 형식을 빼므로, 형식에 따라 달라지는
Anthropic의 프롬프트 캐시는 무효가 됩니다.

실패한 Agent 리뷰는 다음 `errorCode` 값 중 하나를 기록합니다.

| `errorCode` | 원인 | 재시도 |
|---|---|---|
| `AUTHENTICATION` | 없거나 만료되었거나 거부된 자격 증명: HTTP 401/403, API 키 없음, Codex 로그인 없음, 만료된 인증 파일 | 아니요. 백엔드를 멈춤 |
| `QUOTA` | 결제, 크레딧, 요금제 사용량 한도(`insufficient_quota`, Codex 사용량 한도) | 아니요. 백엔드를 멈춤 |
| `RATE_LIMIT` | 할당량 원인이 없는 HTTP 429. 재시도 뒤이거나 `Retry-After`가 마감 시간에 닿은 경우 | 예 |
| `TRANSIENT` | 연결 실패, HTTP 5xx 또는 과부하, 끊긴 스트림. 요청된 재시도 지연이 지나치게 길어도 이 분류를 유지 | 예, 출력 전에 한해 |
| `TIMEOUT` | 리뷰의 `timeout_ms` 마감 시간 | 아니요 |
| `CANCELLED` | 명시적인 Run 취소(`run cancel` 또는 MCP `verify_cancel`) | 아니요 |
| `PROVIDER_BUDGET_EXCEEDED` | `max_tokens` 또는 `max_tool_calls` 초과 | 아니요 |
| `INVALID_RESULT` | 한 번의 형식 복구 뒤에도 올바른 판정이 없음 | 아니요 |
| `PROVIDER_ERROR` | 그 밖의 제공자 실패: 거부된 요청, 알 수 없거나 다른 모델, 불완전한 응답, 형식이 잘못된 도구 호출 | 아니요 |
| `AGENT_ERROR` | 잘못된 테스트 엔드포인트나 쓸 수 없는 도구 같은 그 밖의 모든 것 | 아니요 |
| `BACKEND_STOPPED` | 시작되지 않음: Run이 그 백엔드에서 리뷰를 더 받지 않게 됨 | — |

Ctrl-C는 `verify --wait` 지켜보기를 중단할 뿐, 그것이 지켜보는 Agent 리뷰는 중단하지 않습니다.
Run은 계속됩니다. 명시적인 취소 요청은 Run이 소유한 작업을 멈춥니다.

### 백엔드 멈추기 {#stopping-a-backend}

`AUTHENTICATION`이나 `QUOTA` 실패는 같은 백엔드의 모든 리뷰에서 반복될 것이므로,
Run은 그 백엔드에서 새 리뷰를 더 받지 않습니다. 그 Run에서 이후의 그
백엔드 리뷰는 모두 시작되지 않은 채 `BACKEND_STOPPED`인 ERROR가 되며, 그 오류에
실패 내용이 나옵니다. 그 백엔드의 빈 슬롯을 기다리던 리뷰
([백엔드 용량](others/state.md#backend-capacity))도 같은 방식으로
기다림을 멈추고, 멈춘 백엔드는 슬롯을 차지하지 않습니다. 이미 실행 중이던 리뷰는
끝까지 실행되고, 재사용할 수 있는 결과는 여전히 재사용되며, 다른 백엔드는 계속 진행합니다. Run은 멈춘 내용을
`stoppedBackends`에 나열하고(`verify --wait --json`과 `run show`에서), `verify`는 이를 출력합니다.

```text
Stopped backend openai after AUTHENTICATION in docs/review: 2 reviews not started.
```

다음 Run에서는 그 백엔드를 다시 쓸 수 있습니다.

eval 범위 안의 도구만 노출되며, 도구는 차례로 실행됩니다. 명령
도구는 작성자의 [환경](declarations.md#runtime-execution-library)에서 실행되며,
호출마다 전용 출력 디렉터리와 임시 디렉터리를 가집니다. 내장 도구는
artifactize 안에서 실행되며 호출별 디렉터리가 없습니다. `help`는 artifactize 자신이 받은
환경에서 프로그램을 실행합니다. 도구 결과는 구조화된 데이터를 평탄화하지 않고 텍스트, JSON, 검증된 base64 이미지 블록을
그대로 유지합니다. PNG/JPEG/WebP 결과는 두 제공자 전송 방식 모두에 전달되며, 이미지를 지원하지 않는 모델에
이미지를 요청하면 제공자 오류로 실패합니다. 제공자 호출 ID가 중복되면 안전하게
실패합니다. 요청 시도별 `usage`는 성공과 ERROR 모두에서 저장되며, 캐시 적중 때는
시도가 `reusedUsage`로 옮겨지고 지출로 세지 않습니다. 카운터는
추론한 합계나 비용이 아니라 제공자가 보고한 값입니다(0으로 보고된 경우 포함).
Anthropic의 `inputTokens`는 자체적인 캐시되지 않은 입력이며, 캐시 읽기/쓰기 카운터는
따로 있습니다. OpenAI 입력에는 이미 캐시 읽기가 포함됩니다. 모든 카운터를 더하지 마세요.
보고되지 않은 필드는 빠져 있습니다. 요청에는 판정, 결과, 사용량이 남습니다.
메시지, reasoning, 도구 호출과 그 인자와 결과는 리뷰의
[저장된 대화](#saved-conversations)에만 있으며,
[`session show --summary`](#session-summary)가 이를 셉니다.

제공자에 의지하기 전의 소유자 검증: 백엔드마다(OpenAI 키, Anthropic 키, Codex 로그인)
접근 가능한 정확한 모델 ID와 선언한 도구로 실제 리뷰를 하나씩 실행하세요.
이 실제 리뷰는 아직 소유자가 해야 할 일로 남아 있습니다. 자동화 테스트는 가짜 HTTP
전송이나 [가짜 제공자](#test-against-a-fake-provider)를 쓰며 실제
추론 요청을 하지 않습니다.

## 저장된 대화 {#saved-conversations}

artifactize는 각 Agent 리뷰의 대화를 이 머신에 보관하므로, 리뷰어가 무엇을 보고
무엇을 말했는지 읽을 수 있고, 채팅을 이어 가듯 같은 대화에서 후속 질문을 할 수도
있습니다. 기록된 판정은 결코 바뀌지 않습니다.

**저장되는 것.** 각 리뷰 세션은
`$STATE/agent-sessions/` 아래에 소유자 전용 파일 하나를 받습니다([구조](others/state.md#agent-sessions)).
그 파일에는 시스템 프롬프트와 지시문, 모든 턴의 메시지, 각 도구 호출의
인자와 전체 결과, 실패 여부, 복구 턴(복구를 요청한 프롬프트와
그것이 대체한 답), 각 제공자 시도와 그 토큰
카운터, 그리고 대화를 정확히 재생하는 데 필요한 암호화된 reasoning 항목(Responses의
`reasoning.encrypted_content`, Anthropic 사고 서명)이 들어 있습니다.
백엔드, 모델, reasoning, 제공자
매개변수, 예산, 도구 정의, [`sessionId`](#prompt-caching),
Run과 요청 ID도 기록합니다. `anthropic`을 포함해 모든 백엔드가 저장됩니다. 세션은
리뷰의 도구 호출에 대한 유일한 기록입니다. 요청에는 판정, 결과, 사용량이 남습니다.
저장 때문에 리뷰가 실패하는 일은 없습니다. 쓰기 오류는 stderr에 보고되고 리뷰는
저장 없이 계속됩니다.

**참조.** 대화가 저장된 요청은
`verify --wait --json`, `run show`, `request show`에 `session`을 가집니다.

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

`producer`는 리뷰를 실행한 `user@host`이고, `state`는 그 상태 데이터베이스의
고정 ID입니다. 결과를 재사용하는 요청에는 그 결과를 만든 리뷰가 나옵니다.
[팀 리뷰 저장소](team/index.md)는 각 Agent 결과와 함께 대화가 아니라 참조를
(`producer.session`으로) 전달하므로, 팀원의 판정을 보면
누구의 상태에 그 대화가 있는지 알 수 있습니다.

**읽기.** `REF`는 `ref`, 요청 ID(`run-FiVNCV-1`), 또는 `sessionId`입니다.

```sh
artifactize session show run-FiVNCV-1
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · codex gpt-6-luna (reasoning max)
Reference: alice@laptop/34bb3c3a-…/run-FiVNCV/run-FiVNCV-1/e83bf374-…
Request run-FiVNCV-1 (Run run-FiVNCV), eval notes/review: GREEN

── System
You are an artifactize evaluator. …

── User (turn 1)
Eval: Review notes (notes/review)
Target Artifact: notes
Instruction: Check Artifact notes (tools: read_notes). …

── Assistant (turn 1)
[reasoning] Reading the notes
[tool call read_notes] {"path":"notes.md"}

── User (turn 2)
[tool result read_notes]
{"artifactId":"notes","lines":[{"number":1,"text":"notes: R1 holds.\n"}], …}

── Assistant (turn 2)
{"result":{"verdict":"GREEN","verdictFields":{}}}

── Repair prompt (turn 3)
Your final response did not match the required schema: …

── Assistant (turn 3)
{"result":{"verdict":"GREEN","verdictFields":{"covered":["R1"]}}}

── Result: {"covered":["R1"],"verdict":"GREEN"}
```

텍스트 출력은 도구 결과를 2000자에서 자르고 reasoning은
요약만 보여 줍니다. `--json`은 저장된 이벤트를 그대로 출력하며, 이것이
대화를 재생할 수 있는 내보내기이기도 합니다.

### 세션 요약 {#session-summary}

`--summary`는 세션 파일만으로 대화에서 쓴 토큰과 한 일을 집계합니다.
턴(제공자 요청)과 턴별, 전체 토큰, 도구별 도구 호출 수와
실패한 수, 후속 질문, 그리고 백엔드, 모델, 시간 범위입니다.
실패한 호출은 오류를 돌려받았거나, 예산이나 마감 시간, 또는 리뷰 종료가
먼저 막아서 실행되지 않은 호출입니다.

```sh
artifactize session show run-FiVNCV-1 --summary
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · codex gpt-6-luna (reasoning max)
Reference: alice@laptop/34bb3c3a-…/run-FiVNCV/run-FiVNCV-1/e83bf374-…
Request run-FiVNCV-1 (Run run-FiVNCV), eval notes/review: GREEN
Time: 2026-10-06T09:14:02.118604115Z to 2026-10-06T09:16:41.502330019Z (159.4 s)
Turns: 5 · follow-ups: 1
Tokens: input 21410 · output 1318 · cache read 15872 · reasoning 704
  turn 1: input 3920 · output 212 · cache read 0 · reasoning 128
  turn 2: input 4310 · output 260 · cache read 3840 · reasoning 160
  turn 3: input 4402 · output 96 · cache read 4224 · reasoning 64
  follow-up 1, turn 1: input 4366 · output 410 · cache read 3840 · reasoning 224
  follow-up 1, turn 2: input 4412 · output 340 · cache read 3968 · reasoning 128
Tool calls: 3 (1 failed)
  read_notes: 2
  search_notes: 1 (1 failed)
```

여러 번 시도한 턴에는 `(2 attempts)`처럼 그 사실이 표시됩니다. 토큰 카운터는
제공자가 보고한 값을 그 턴의 시도에 걸쳐 더한 것이며, 어느
시도도 보고하지 않은 카운터는 빠집니다. `--json`은 같은 내용을 객체 하나로 출력합니다: `backend`,
`model`, `reasoning`, `startedAt`, `endedAt`, `durationMs`, `followUps`, `turns`(각각
후속 질문에 답한 턴이면 `followUp`, `turn`, `attempts`, `tokens`), `tokens`,
`toolCalls`(`{"read_notes": {"calls": 2, "failed": 0}, …}`), `sessionId`,
`requestId`, `reference`.

**후속 질문하기.**

```sh
artifactize session send run-FiVNCV-1 "Which line shows R1, and does it still hold?"
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · follow-up 1 · codex gpt-6-luna (reasoning max) · files changed since this review

notes.md line 1 says R1 holds, but it now reads "R1 fails", so the verdict would change.
```

후속 질문은 같은 모델과 현재
프로젝트 도구로 저장된 대화를 이어 갑니다. 답은 일반 텍스트이며, 기록된 판정,
결과, 재사용을 바꾸지 않습니다. 리뷰된 입력이 바뀌었으면 머리말이 경고합니다. 질문마다
새 프로필 예산을 받으며, 한 세션에 대한 질문은 차례로 실행됩니다. 캐싱, 저장되는 메시지,
호환성 세부 사항은 [후속 질문 재생 레퍼런스](others/state.md#follow-up-replay)를
보세요.

다른 머신이나 상태에 대한 참조는 그것이 있는 곳을 알려 주며 실패합니다. 예를 들어
`Session e83bf374-… lives in alice@laptop's state 34bb3c3a-…; this is bob@desk's state
5d0e…`처럼 나옵니다. 크기 제한 정리로 삭제된 세션은 `was removed by the
session GC`를, 저장된 적이 없는 세션은 `was not saved`를 보고합니다.

**보존.** 세션 저장소는
[`limits.json`](others/state.md#agent-sessions)(`agentSessions`)의 크기로 제한됩니다.
`maxBytes`(1 GiB)를 넘으면, 가장 오래전에 쓴 세션부터
`targetBytes`(768 MiB) 이하가 될 때까지 삭제합니다. 이 정리는 `verify` Run이 eval을 스케줄하고
확정한 뒤 한 번 실행되며(그 전에 실패한 Run은 건너뜀), `artifactize prune`에서도 실행됩니다.
아직 실행 중인 요청의 세션이나
`session send`가 잡고 있는 세션은 결코 건드리지 않습니다. `doctor`가 저장소 크기를 보여 줍니다. `"enabled": false`는
새 대화 저장을 멈춥니다.

**개인정보.** 대화에는 리뷰어가 도구로 읽은 모든 것이
파일 내용까지 포함해 들어 있고, 리뷰어가 쓴 모든 것도 들어 있습니다. 대화는 이 머신의
소유자 전용 파일에 남으며, 팀 리뷰 저장소를 포함해 어디에도 업로드되지 않습니다.
후속 질문도 각 리뷰 턴과 마찬가지로 그 리뷰에 쓴 백엔드로 대화를 보냅니다.
`"agentSessions": {"enabled": false}`로 저장을 끄고,
저장된 것을 지우려면 `$STATE/agent-sessions`를 삭제하세요.

## Codex {#codex}

`codex`는 ChatGPT 요금제의 Codex 모델(`gpt-6-luna`,
`gpt-6.1-sol` 등)로 Codex Responses 엔드포인트
(`https://chatgpt.com/backend-api/codex/responses`)를 통해 리뷰합니다. Pi의
`openai-codex` 제공자 프로토콜을 따릅니다.

```sh
artifactize login codex     # sign in to ChatGPT in the browser
artifactize models codex    # the account's Codex models
# Declare backend = "codex", model = "gpt-6-luna", reasoning = "max", then:
artifactize verify --all --wait
artifactize logout codex    # delete artifactize's tokens; their revocation is attempted
```

- **로그인.** `login codex`는 Codex CLI와 마찬가지로 PKCE를 쓰는 Codex OAuth 클라이언트를
  씁니다. 다른 프로세스가 콜백 포트 1455를 쓰고 있으면, 로그인은 콜백을 기다리는 대신
  전체 리디렉션 URL을 요청합니다. 로드에 실패한 뒤 주소 표시줄에서, 또는 페이지가 계속
  로딩 중이면 개발자 도구 → Network에서 `http://localhost:1455/auth/callback?code=`로 시작하는
  URL을 복사해 터미널이나 데스크톱 로그인에 붙여 넣으세요. 브라우저 흐름은
  [로그인 설정](../getting-started/install.md#backends)을 보세요.
  토큰과 ChatGPT 계정 ID는 `$STATE/auth/codex.json`에 저장됩니다
  (0700 디렉터리, 링크가 하나뿐인 0600 파일, 심볼릭 링크를 따라가지 않음, 원자적으로
  교체). `login`, `models`, `verify`에는 같은 `--state-dir`을 쓰세요.
  토큰은 결코 저장소 안에 저장되지 않습니다. `$STATE/auth`가 git
  워크트리, artifactize 워크스페이스, 또는 `--repo` 안에 있으면 `login codex`는 거부하고 그
  폴더를 알려 주며, `doctor`는 경고합니다
  ([상태](others/state.md#state)). 그 밖의 상태 디렉터리를 쓰거나
  `ARTIFACTIZE_CODEX_AUTH_FILE`을 쓰세요. artifactize는 액세스 토큰이 만료되기 5분 전에 잠금을 잡고
  갱신하므로, 동시에 실행되는 Run들도 한 번만 갱신합니다. 서버가 갱신
  토큰을 영구히 거부하면 토큰을 삭제하고, 리뷰는 다시 로그인하라고 알려 줍니다.
- **기존 Codex 로그인.** `ARTIFACTIZE_CODEX_AUTH_FILE`을 예를 들어
  `~/.codex/auth.json`으로 설정하면, artifactize는 자기 토큰 대신 매 턴마다 그 파일의 `tokens.access_token`(과
  `tokens.account_id`)을 읽습니다. 그 파일을 쓰거나,
  복사하거나, 갱신하지 않습니다. 토큰이 1분 이하로 남으면 리뷰는
  `codex login` 같은 방법으로 Codex에 다시 로그인하라는 메시지와 함께 실패합니다. `logout codex`는
  그 파일을 건드리지 않습니다.
- **요청.** 각 턴은 `store:false`,
  `include: ["reasoning.encrypted_content"]`, 리뷰 도구를 function 도구로,
  리뷰의 시스템 프롬프트를 `instructions`로 담은 Responses 요청 하나를 스트리밍합니다. `reasoning`을 설정하면
  `reasoning: {"effort": <reasoning>, "summary": "auto"}`를 보내며, `max`는 그대로 전달합니다.
  헤더에는 bearer 토큰, `ChatGPT-Account-Id`, `originator: artifactize`,
  `OpenAI-Beta: responses=experimental`, 리뷰의 `session-id`가 들어갑니다
  ([프롬프트 캐싱](#prompt-caching)). 재시도, 예산, 도구 결과, 사용량은
  `openai`와 같게 동작합니다.
- **오류.** 사용량 한도는 `QUOTA`입니다. 요금제와 초기화 시점을 알려 주며,
  그 Run에서 `codex`를 멈춥니다. 일반 HTTP 429는 재시도되는 `RATE_LIMIT`입니다. HTTP 401과
  403은 `AUTHENTICATION`이며 다시 로그인하는 방법을 알려 줍니다. 갱신하는 동안 토큰 엔드포인트에
  닿을 수 없거나 실패하면 `TRANSIENT`입니다.
- **재사용.** 다른 백엔드처럼 `codex`도 각 결과에 백엔드, 모델, reasoning,
  제한을 재사용 키 밖에 기록합니다. 같은 eval과 fingerprint에 대한 `codex` 결과와 `openai` 또는
  `anthropic` 결과는 서로 재사용할 수 있습니다.
- **모델.** `models codex`는 `GET <root>/models?client_version=0.160.0`을 요청합니다. 이는
  artifactize가 프로토콜을 따르는 Codex 릴리스가 받는 카탈로그입니다. Codex 모델 선택기처럼
  `visibility`가 `list`인 모델을 서버 순서대로 나열합니다.
  프로필에는 목록에 있든 없든 계정이 쓸 수 있는 어떤 모델이든 지정할 수 있습니다.

## 가짜 제공자로 테스트하기 {#test-against-a-fake-provider}

모델 없이 도구, 스키마, 결과 파싱을 테스트하려면 백엔드가
이 머신의 가짜 제공자를 가리키게 하세요. 변수는 제공자의 API 루트를 바꿉니다.

| 백엔드 | 변수 | 예 | 가짜 제공자가 응답할 것 |
|---|---|---|---|
| `openai` | `ARTIFACTIZE_OPENAI_BASE_URL` | `http://127.0.0.1:8080/v1` | `POST <root>/responses`, `GET <root>/models` |
| `anthropic` | `ARTIFACTIZE_ANTHROPIC_BASE_URL` | `http://127.0.0.1:8080` | `POST <root>/v1/messages`, `GET <root>/v1/models` |
| `codex` | `ARTIFACTIZE_CODEX_BASE_URL` | `http://127.0.0.1:8080/backend-api/codex` | `POST <root>/responses`, `GET <root>/models` |
| Codex 로그인 | `ARTIFACTIZE_CODEX_AUTH_URL` | `http://127.0.0.1:8080` | `POST <root>/oauth/token`과 `<root>/oauth/revoke`. `login codex`는 브라우저를 `<root>/oauth/authorize`로 보냄 |

- 루트는 `localhost`, `127.0.0.0/8`, `[::1]`의 `http://` 또는 `https://`여야 하며,
  자격 증명, 쿼리, 프래그먼트가 없어야 합니다. 다른 값이면 리뷰,
  `models`, `doctor`가 연결하기 전에 그 변수를 알려 주는 메시지와 함께 실패합니다.
  빈 값은 설정하지 않은 것으로 칩니다. `anthropic`에서는 끝의 `/v1`도 받아들입니다.
- API 키 변수는 여전히 필요하며 가짜 제공자에게 그대로 전달되므로,
  임의의 키를 설정하세요. `codex`에서는 `ARTIFACTIZE_CODEX_AUTH_FILE`이
  `{"tokens":{"access_token":"dummy","account_id":"test"}}` 같은 파일을 가리키게 하세요.
- 그 밖에는 아무것도 바뀌지 않습니다. 요청 본문과 스트리밍, 재시도, 예산,
  도구 호출, 복구, 판정 검증이 모두 같습니다.
- 결과는 다른 결과처럼 기록되고 재사용되므로, 버려도 되는 상태
  디렉터리(`--state-dir` 또는 `ARTIFACTIZE_STATE_HOME`)를 쓰세요. 테스트 엔드포인트가
  설정되어 있으면 `verify`는 리뷰 저장소 사용을 거부하므로 `ARTIFACTIZE_REMOTE=off`로 설정하세요.
  가짜 결과가 든 상태는 결코 `remote push`하지 마세요.
- 테스트 엔드포인트가 설정되어 있으면 `doctor`가 경고하고 `testEndpoint`로 보고합니다.

가짜 제공자는 요청된 모델에 대해 제공자의 전송 형식으로 답을 `text/event-stream`으로
스트리밍합니다. `openai`에서는 `response.completed` 이벤트 하나면 충분합니다.
이 가짜 제공자는 GREEN 스키마가 `{"verdict":"GREEN"}`을 받아들이는 리뷰를 통과시킵니다.
`pass_schema`가 다른 필드를 요구하면, 가짜 제공자의 `text` 응답에
그에 맞는 올바른 값을 넣으세요. 그렇지 않으면 검증이 복구를 일으키고, 그래도
잘못되면 `INVALID_RESULT`가 됩니다.

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

도구를 시험하려면 첫 요청에 `function_call` 출력 항목
(`call_id`, `name`, JSON으로 인코딩한 `arguments`)으로 답하세요. 그 `response.completed` 이벤트는
그 호출에 대한 `response.output_item.added`와 `response.output_item.done` 이벤트 뒤에
와야 합니다. 그러면 다음 요청의 `input`에 도구의 `function_call_output`이 들어 있습니다.
그 요청에는 판정 메시지로 답하세요.
