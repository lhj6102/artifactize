# 도구 {#tools}

도구를 쓰면 리뷰어가 직접 빌드 작업에 매달리지 않고도 Artifact를 살펴볼 수 있습니다.
**Agent 도구**는 모델이 호출할 수 있는 읽기 전용 작업입니다. **Human
도구**는 사람이 리뷰를 맡은 뒤 실행하는 미리 정의된 명령입니다. 어느 쪽도
eval의 기준을 대신하거나 판정을 지어내지 않습니다.

## 내장 도구 {#built-in-tools}

래퍼를 직접 만들기 전에 [내장 도구](builtin.md)를 쓰세요. 파일 읽기, 폴더 나열,
텍스트 검색, 이미지나 Markdown 섹션 보기, 프로그램 도움말 확인,
현재 Artifact Graph 보기를 할 수 있습니다. Human 도구는 파일이나 URL을 기본
애플리케이션으로 열 수도 있습니다. 파일을 읽고, 폴더를 나열하고, 텍스트를 검색하고,
이미지나 섹션을 보여 주는 Agent 내장 도구는 머신 전체가 아니라 eval의 범위에 들어간 Artifact만 봅니다.
`graph`는 eval 범위와 상관없이 프로젝트의 현재 선언을 모두 읽습니다.
`help`는 일반적인 실행 파일 검색으로 찾은 실제 프로그램을 실행하며,
선언된 명령 도구도 신뢰한다고 보고 실행하는 프로그램입니다. 어느 쪽도 **OS
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

재사용할 도구는 구현 파일이 있는 폴더에 Artifact 선언을 추가하세요.
예를 들어 `tools/checker/index.artf`는 Python 프로그램 `inspect.py`와 그 옆의 `tests/` 폴더가 들어 있는
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

도구를 쓸 eval의 기준에서 그 도구 Artifact를 참조하세요.

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

Agent는 `inspect_checker`를, 사람은 `source_checker`를 쓸 수 있습니다. 두 eval 모두
참조된 도구 Artifact가 eval 범위에 들어가므로, 그 fingerprint가 재사용 여부에
영향을 줍니다. 필요하면 도구 자체도 테스트하거나 리뷰하세요. 끝나지 않은 작업을 우회하려고
도구를 `basis`로 선언하지 마세요. `execution_paths`로 실행된 바이너리의 빌드 정보를 기록할 수 있지만, 이 기록은 재사용 키에 들어가지 않습니다.

`json` 명령은 stdin에서 `context`와 `args`가 든 JSON 요청 하나를 읽고,
`{"content":[{"type":"text","text":"The required headings are present."}]}` 같은
JSON 콘텐츠 객체 하나를 출력합니다.
`plain` 명령은 선언된 스키마 속성을 별도의 argv
항목으로 치환하고, 크기가 제한된 stdout을 텍스트로 돌려줍니다. Human 명령은 자유 인자를 받지 않으므로,
출력을 수집하는 `output` 방식이나 애플리케이션을 실행하는 `launch` 방식 중 하나로 선언하세요. 전체 프로토콜, 범위, 제한은
[Agent 도구](agent.md)와 [Human 도구](human.md)를 보세요.

## 게임이나 앱 하네스를 계속 실행해 두기 {#keep-a-game-or-app-harness-running}

**하네스**는 리뷰하는 동안 애플리케이션을 실행하거나 조종하는 보조 프로그램입니다.
도구 세션을 쓰면 Agent 리뷰에서 도구를 여러 번 호출하는 동안 하네스 하나를 계속 실행해 둘 수 있습니다.
예를 들어 게임 도구 Artifact는 보이는 창 없이 게임을 **헤드리스**로 실행하고,
읽기 전용 보드 확인 작업을 제공할 수 있습니다.

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

start 명령은 평소와 같은 JSON 객체를 읽고, 공유 상태를 전용
`ARTIFACTIZE_SESSION_DIR`에 쓰고, 준비 완료 줄 하나를 출력한 뒤 계속 실행됩니다. 세션에 묶인
도구 호출과 stop 명령은 같은 디렉터리를 쓰고, 세션은 리뷰마다 따로 만들어집니다.
Agent는 인스턴스를 시작하거나, 멈추거나, 이름을 정하지 않습니다. 오류와 취소를 포함해
모든 세션을 멈추고 정리하는 일은 artifactize가 맡습니다. 리뷰 결과를 재사용할 때는 하네스를
시작하지 않습니다. 세션은 [저장된 Agent 대화](../agent.md#saved-conversations)가 아닙니다. 하네스가
구현해야 할 규약은 [도구 세션](agent.md#tool-sessions)에서 확인하세요.

## 리뷰하기 전에 선언 확인하기 {#check-a-declaration-before-reviewing}

`artifactize tools check`는 프로젝트 명령을 실행하지 않고 사용 가능 여부와 선언을
확인합니다. 특정 도구를 시험하려면 명시적인
[실행 형식](../../cli/index.md#tool-diagnostics)을 쓰세요. 이 형식은 Run, 리뷰, 캐시
항목을 만들지 않습니다. 세션에 묶인 Agent 도구는 그 진단 호출에만 쓰는 하네스를 새로 실행합니다.
