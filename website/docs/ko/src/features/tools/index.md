# 도구 {#tools}

도구를 쓰면 리뷰를 빌드 세션으로 바꾸지 않고도 리뷰어가 Artifact를 살펴볼 수 있습니다.
**Agent 도구**는 모델이 호출할 수 있는 읽기 전용 작업입니다. **Human
도구**는 사람이 리뷰를 맡은 뒤 실행하는 미리 정의된 명령입니다. 어느 쪽도
eval의 기준을 대신하거나 판정을 지어내지 않습니다.

## 내장 도구 {#built-in-tools}

래퍼를 직접 만들기 전에 [내장 도구](builtin.md)를 쓰세요. 파일 읽기, 폴더 나열,
텍스트 검색, 이미지나 Markdown 섹션 보기, 프로그램 도움말 확인,
현재 Artifact 그래프 보기를 할 수 있습니다. Human 도구는 파일이나 URL을 평소의
애플리케이션으로 열 수도 있습니다. 파일을 읽고, 폴더를 나열하고, 텍스트를 검색하고,
이미지나 섹션을 보여 주는 Agent 내장 도구는 머신 전체가 아니라 받아들여진 Artifact 범위만 봅니다.
`graph`는 eval이 어떤 Artifact를 받아들이든 프로젝트의 현재 선언을 모두 읽습니다.
`help`는 평범한 찾기로 발견한 실제 프로그램을 실행하며,
선언된 명령 도구도 신뢰하는 프로그램입니다. 어느 쪽도 **OS
샌드박스가 아닙니다**.

```toml
[views.agent_tools]
read = { builtin = "read" }
list = { builtin = "list" }

[views.human_tools]
notes = { builtin = "read", args = ["{artifactPath}/notes.md"] }
```

eval은 실제로 참조하는 Artifact의 도구를 봅니다. 도구 이름은
`<operation>_<artifactName>`이 되므로, `spec`에 선언한 `read`는 `read_spec`입니다.

## 직접 도구 Artifact 만들기 {#build-your-own-tool-artifact}

재사용할 도구는 그 구현 옆에 두고 Artifact 선언을 붙이세요.
예를 들어 `tools/checker/index.artf`는 Python 프로그램 `inspect.py`와 그 옆의 `tests/` 폴더를 가진
`checker` Artifact를 선언할 수 있습니다.

```toml
name = "checker"

[evals.tests]
title = "The inspection tool handles the requirements and JSON protocol"
profile = { kind = "runtime", command = "python3", args = ["-m", "unittest", "discover", "-s", "tests"] }
payload.instruction = "Run the protocol and requirements tests of {checker}."

[views.agent_tools.inspect]
description = "Inspect the specification for {artifactName}."
protocol = "json"
command = "python3"
args = ["inspect.py", "{spec}/requirements.json"]
input_schema = { type = "object", properties = { section = { type = "string" } }, required = ["section"], additionalProperties = false }

[views.human_tools]
source = { builtin = "read", args = ["{artifactPath}/inspect.py"] }
```

eval의 기준에서 도구 Artifact를 참조해 소비자에 붙이세요.

```toml
name = "app"

[evals.review]
title = "The app satisfies the specification"
profile = { kind = "agent", backend = "openai", model = "YOUR_EXACT_MODEL_ID" }
payload.instruction = "Review {app} against {spec}; use inspect_checker from {checker} to inspect the requirements."

[evals.approved]
title = "The implementation is approved"
profile = { kind = "human" }
payload.instruction = "Review {app} and the implementation of {checker}; run its source tool before deciding."
```

Agent는 `inspect_checker`를, 사람은 `source_checker`를 받습니다. 두 eval 모두
참조된 도구 Artifact를 받아들이므로, 그 fingerprint가 재사용에
관여합니다. 필요하면 도구에도 실제 테스트나 리뷰를 주세요. 끝나지 않은 작업을 우회하려고
도구를 `basis`로 선언하지 마세요. `execution_paths`는 어떤 빌드된
바이너리가 실행되었는지 기록할 수 있지만, 그 출처 고정 경로 자체가 재사용을 무효로 만들지는 않습니다.

`json` 명령은 stdin에서 `context`와 `args`가 든 JSON 요청 하나를 읽고,
`{"content":[{"type":"text","text":"The required headings are present."}]}` 같은
JSON 콘텐츠 객체 하나를 출력합니다.
`plain` 명령은 선언된 스키마 속성을 별도의 argv
항목으로 치환하고, 크기가 제한된 stdout을 텍스트로 돌려줍니다. Human 명령은 자유 인자를 받지 않으므로,
캡처한 `output`이나 애플리케이션 `launch` 중 하나로 선언하세요. 전체 프로토콜, 범위, 제한은
[Agent 도구](agent.md)와 [Human 도구](human.md)를 보세요.

## 게임이나 앱 하네스를 계속 실행해 두기 {#keep-a-game-or-app-harness-running}

**하네스**는 리뷰하는 동안 애플리케이션을 실행하거나 조종하는 보조 프로그램입니다.
도구 세션은 Agent 리뷰의 여러 호출에 걸쳐 하네스 하나를 살려 둡니다.
예를 들어 게임 도구 Artifact는 보이는 창 없이 게임을 **헤드리스**로 실행하고,
읽기 전용 보드 확인 작업을 노출할 수 있습니다.

```toml
[views.agent_sessions.game]
start = { command = "game-harness", args = ["start"] }
stop = { command = "game-harness", args = ["stop"] }

[views.agent_tools.board]
session = "game"
description = "Read the current game board."
protocol = "json"
command = "game-harness"
args = ["board"]
```

start는 평소의 JSON 봉투를 읽고, 공유 상태를 전용
`ARTIFACTIZE_SESSION_DIR`에 쓰고, 준비 완료 줄 하나를 출력한 뒤 계속 실행됩니다. 세션에 묶인
호출과 stop은 같은 디렉터리를 받으며, 리뷰마다 별도의 세션을 받습니다.
에이전트는 인스턴스를 시작하거나, 멈추거나, 이름 붙이지 않습니다. 준비,
중지, 프로세스 정리는 오류와 취소를 포함해 Artifactize가 맡습니다. 재사용된 리뷰는
아무것도 시작하지 않습니다. 세션은 [저장된 Agent 대화](../agent.md#saved-conversations)가 아닙니다.
수명 주기와 복구 계약은 [도구 세션](agent.md#tool-sessions)을 보세요.

## 리뷰하기 전에 선언 확인하기 {#check-a-declaration-before-reviewing}

`artifactize tools check`는 프로젝트 명령을 실행하지 않고 사용 가능 여부와 선언을
확인합니다. 특정 도구를 시험하려면 명시적인
[실행 형식](agent.md#tool-diagnostics)을 쓰세요. 이 형식은 Run, 리뷰, 캐시
항목을 만들지 않습니다. 세션에 묶인 Agent 도구는 그 진단 호출을 위해 새로 소유한 하네스를 받습니다.
