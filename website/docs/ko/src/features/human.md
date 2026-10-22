# Human 리뷰 {#human-reviews}

Human eval은 사람에게 GREEN 또는 RED 승인을 요청합니다. 리뷰어는
대기 중인 요청을 맡고, 선언한 Human 도구를 실행한 뒤 판정을 제출합니다.
`artifactize request` 명령으로 하거나 `artifactize review` 터미널 UI에서 할 수 있습니다.

Human eval은 같은 Artifact의 Human이 아닌 다른 eval이 모두
GREEN이고 의존성 관문이 충족되었을 때만 준비됩니다. 사람은
기계 eval이 이미 통과한 것만 승인합니다. 그 기계 eval 중 하나가 RED나 ERROR이면
Human eval은 BLOCKED가 되고 그 이유에 해당 eval의 이름이 나옵니다. 같은 Artifact의 다른 Human eval은
이 조건에 들어가지 않습니다. [Human 승인](artifact.md#human-sign-offs)을 보세요.

검사가 준비되면 artifactize는 **WAITING_HUMAN 요청**을 저장합니다.
사람의 결정이 필요한 작업입니다. 사람의 결정이 나기 전까지는 Run의
`--jobs` 실행 슬롯도, `--max-executions` 예산도 쓰지 않으며, 예산이
0이어도 마찬가지입니다. 일반 모드에서 Run은 Human 시간 제한 없이 기다리고, 승인이 제출되면
의존하는 검사가 같은 Run 안에서 이어집니다. 터미널을 지켜보거나 닫는다고
결정이 내려지지는 않습니다. 제출, 지켜보기, 명시적 취소는
[Run 수명 주기](run.md#submit-observe-and-cancel)에서 설명합니다.

**리뷰어 맡기(claim)** 는 요청을 리뷰하는 사람을 위해 그 요청을 예약합니다. 이는
같은 리뷰를 두 Run이 동시에 시작하지 못하게 막는 artifactize의 자동 **재사용 키 잠금**과
다릅니다. 다른 체크아웃에서 시작한 Run이라도 다른 Run은 기존 대기 리뷰에 합류할 수 있으며,
그 요청을 **팔로워(follower)** 라고 부릅니다. 팔로워에 대한 Human 동작은 원래 요청과
저장소로 갑니다. `--force`는 그 자동 재사용 키 잠금을 우회할 뿐, 사람이 동작하기 전에
요청을 맡아야 한다는 조건까지 우회하지는 않습니다.

승인은 [리뷰된 입력](fingerprint.md)이 여전히 일치하면 기본적으로 재사용됩니다.
이후 `verify`는 제출된 결과를 재사용하고 그것을 쓰는 eval을 리뷰할 수 있습니다.
대상이나 포함된 의존성에 `fingerprint = false`가 있으면 승인은
현재 Run만 확정하며, 이후 Run은 새로운 Human 결정을 요청합니다. 현재
검증은 쓸 수 있는 증거를 쓰지만, 나중의 제출이 과거 Run에 기록된 판정을 다시 쓰는 일은 결코 없습니다.

아래 명령 표기에서 대괄호는 선택 부분을 뜻하니 대괄호는 입력하지 마세요.
대문자 자리 표시자는 바꿔 넣을 값입니다. `request list`가 돌려준 실제
`REQUEST_ID`를 쓰고, `NAME`에는 리뷰어 이름을 쓰세요.

```sh
artifactize verify --all --wait
# In another terminal, using the same state directory:
artifactize request list [--run RUN_ID] [--json]
artifactize request show REQUEST_ID
artifactize request claim REQUEST_ID [--reviewer NAME]
artifactize request unclaim REQUEST_ID [--reviewer NAME]   # release without a verdict
artifactize request tool REQUEST_ID inspect_child [--reviewer NAME]
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true}'
# Alternatively: --fields-file /path/to/fields.json
# Or claim, run tools and submit in a terminal UI (see Review):
artifactize review [REQUEST_ID]
```

claim, unclaim, tool, submit은 리뷰어 기본값으로 `$USER`를 씁니다. `--reviewer NAME`으로
각 동작에 같은 명시적 리뷰어를 고를 수 있습니다. 리뷰어 이름은 인증된 계정이 아니라 로컬
협업용 잠금입니다. 요청을 맡은 사람만 도구를 실행하고,
제출하거나 맡기를 풀 수 있습니다. `request claim`은 맡은 내용을, `request unclaim`은
풀린 내용을 출력합니다. 다른 사람이 맡은 요청이나 더 이상 기다리지 않는 요청을 풀려고 하면
2로 종료합니다. 도구 이름은 `<operation>_<artifactId>`이며 자유 인자를 받지 않습니다.
텍스트 출력은 캡처한 텍스트나 실행 알림을 출력하고, `--json`은 도구
결과를 출력합니다. 도구가 실패하면 2로 종료하며 판정을 지어내지 않습니다.

제출에는 `--verdict GREEN|RED`가 필요합니다. `--fields`와 `--fields-file`은
함께 쓸 수 없는, 소유자 필드의 JSON 객체(기본값 `{}`)이며, 판정을 감싼 객체가 아닙니다.
파일은 일반 파일이어야 하고, 필드와 전체 결과는
256000바이트로 제한됩니다. 잘못된 JSON, 스키마, 리뷰어 이름, 판정은 2로 종료하고
요청을 고칠 수 있는 상태로 둡니다. 제출에 성공하면 RED라도 0으로 종료하며,
별도의 검증기를 시작하지 않습니다.

[Run 종료 코드](../cli/index.md#command-reference)는 지켜보기 중단,
명시적 취소, 빠진 리뷰 의무를 구분합니다.
`verify --reuse-only human`은
Human 요청을 기록하거나 기다리지 않습니다. Human eval은 승인을 재사용하거나,
CI가 원하는 대로 [실행되지 않습니다](../cli/index.md#reuse-only).

라이브러리 API,
`request list`와 `request show` 필드, Run 요약은 [레퍼런스](tools/human.md#human-reviews)에 있습니다.

## 미룬 승인 {#deferred-sign-offs}

사람이 매번 실행을 이끄는 대신 최종 상태를 리뷰하는 작업 흐름이라면
`verify --defer-human`을 고르세요. 이는 머신 설정이 아니라 요청별 선택이며,
기본으로 꺼져 있습니다. `--speculate`는 이와 별개로 기계 작업의 속도를 높일 수 있으며,
미루기와 함께 써도 미룰 수 있는 승인은 여전히 마지막에 요청합니다.

리뷰어에게 미룬 승인은 DEFERRED 표시가 붙은 일반 승인입니다.
`review`, monitor, 데스크톱, 요청 목록에서
WAITING_HUMAN 요청과 함께 나열되며, 맡기, 도구, 놓기, 제출은 같은 방식으로
Human 권한을 확인합니다. 막힌 승인은 결코 사람에게 제시되지 않습니다. Run이 어떻게 끝나는지
(SIGNOFFS_PENDING, 종료 코드 5)와 다음 verify가 결과를 어떻게 재사용하는지는
[추측 실행과 미룬 승인](run.md#speculation-and-deferred-sign-offs)에 있습니다.
그 재사용에는 fingerprint가 필요하므로 `fingerprint = false`면 재사용되지 않습니다.

같은 워크트리에서 같은 eval에 대한 더 새 요청이 오면, 재사용 키가 다른 이전
DEFERRED 승인은 만료되어 Review에서 사라집니다. 만료, 합류,
Run이 끝난 뒤의 승인 제출은 [Run 규칙](run.md#speculation-and-deferred-sign-offs)을 보세요.

거부되면 상당한 하위 작업이 헛수고가 되는 승인이라면,
엄격한 대기 관문을 명시적으로 유지하세요.

```toml
[evals.release]
title = "The release is approved"
gate = "blocking"
profile = { kind = "human" }
payload.instruction = "Review the release before dependent work proceeds."
```

`gate`가 없으면 Human eval은 요청의 기본 모드를 따릅니다. CI는
`verify --all --wait --reuse-only agent,human`으로 엄격하게 유지됩니다. 미루기도
추측 실행도 실제로 재사용할 수 있는 Human GREEN 증거를 대신하지 않습니다.

## Human 도구 {#human-tools}

`views.human_tools`는 미리 정의된 도구의 별도 안전한 이름 맵입니다. 리뷰어가
도구를 실행하며, 도구는 자유 인자를 받지 않습니다. [내장 도구](tools/builtin.md)는
Linux, macOS, Windows에서 똑같이 동작합니다. `open`은 파일, 폴더, 또는 `http(s)`
URL을 데스크톱의 기본 애플리케이션으로 열고, `graph`는 현재 프로젝트
그래프 뷰어를 실행합니다. `read`, `list`, `section`, `help`는 터미널에 텍스트를 출력합니다.

```toml
[views.human_tools]
open = { description = "Open the review notes of {artifactName}.", builtin = "open", args = ["{artifactPath}/notes.md"] }
notes = { description = "Print the review notes of {artifactName}.", builtin = "read", args = ["{artifactPath}/notes.md"] }
```

내장 도구는 `builtin`과 `args`, 그리고 선택적인 `description`을 받습니다. 그 kind는
도구에 따라 정해집니다. `open`과 `graph`는 실행(launch)하고, 나머지는 출력을 보여 줍니다.

명령 도구는 직접 고른 프로그램을 실행합니다. 정확히 `description`,
`kind = "launch"` 또는 `kind = "output"`, `command`, `args`가 필요하며, 선택적으로
`timeout_ms`(1–2147483647)를 받습니다. `input_schema`, 자유 호출 인자, `protocol`,
`execution_paths`, `metadata`, `script` 래퍼는 받지 않습니다. 설명은
`{artifactName}`을 포함해 Agent 설명 규칙을 따릅니다.

```toml
[views.human_tools]
edit = { description = "Open {artifactName} in VS Code.", kind = "launch", command = "code", args = ["{artifactPath}"] }
```

도구가 출력한 내용은 리뷰가 열려 있는 동안 리뷰어의 [**Tool log**](tools/human.md#tool-log)에
실시간으로 나타나며, 리뷰어는 이를 결과에 복사할 수 있습니다.

자리 표시자, 실행 파일 찾기, `launch`와 `output` kind는
[레퍼런스](tools/human.md#human-tools)에 있습니다.

## 결과 양식 {#result-forms}

리뷰어는 eval의 `pass_schema`(GREEN) 또는 `fail_schema`(RED)로 만든 양식에
결과를 입력합니다. 텍스트, 숫자, true/false, 선택지, 텍스트 목록,
필드의 반복 그룹이 있습니다. 상수는 채워져 있습니다. Human eval의 스키마는
이 형태만 쓸 수 있습니다. [선언 레퍼런스](declarations.md#human-result-schemas)에
그 형태와 각각이 보이는 방식이 정리되어 있습니다.

대화형 리뷰에는 [TUI](../tui/index.md)나 [데스크톱](../desktop/index.md)을 쓰세요.
