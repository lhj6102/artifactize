# Human 리뷰 {#human-reviews}

Human eval은 사람에게 GREEN이나 RED 승인을 요청합니다. 리뷰어는 대기 중인 요청을 claim하고,
선언한 Human 도구를 실행하고, `artifactize request` 명령이나 `artifactize review` 터미널 UI로
판정을 제출합니다.

Human eval은 같은 Artifact의 Human이 아닌 다른 eval이 모두 GREEN이고 의존성 게이트도 충족됐을
때만 준비됩니다. 사람은 기계 검사 eval을 이미 통과한 것만 승인합니다. 그 기계 검사 eval 중
하나라도 RED나 ERROR면 Human eval은 BLOCKED가 되고 사유에 그 eval이 나옵니다. 같은 Artifact의
다른 Human eval은 이 조건에 들어가지 않습니다. [Human 승인](artifact.md#human-sign-offs)을
보세요.

검사가 준비되면 artifactize는 사람의 결정을 기다리는 작업인 **WAITING_HUMAN 요청**을
저장합니다. 사람을 기다리는 동안에는 Run의 `--jobs` 실행 자리도, `--max-executions` 예산도
쓰지 않으며, 그 예산이 0이어도 마찬가지입니다. 기본 모드에서 Run은 Human 시간 제한 없이
기다리며, 승인이 제출되면 같은 Run 안에서 그에 의존하는 검사가 이어서 진행됩니다. 터미널을
지켜보거나 닫는다고 결정이 내려지지는 않습니다. 제출, 지켜보기, 직접 취소는
[Run 수명 주기](run.md#submit-observe-and-cancel)에서 설명합니다.

**리뷰어 claim**은 리뷰하는 사람에게 요청을 맡겨 둡니다. 이와 별개로, 다른 체크아웃에서
온 것이라도 다른 Run이 같은 입력과 프로필로 같은 승인을 필요로 하면 다시 요청하지 않고 대기 중인
리뷰에 합류합니다. 이렇게 합류한 요청을 **팔로워**라고 하며, 팔로워에서 한 Human 작업은 원래
요청과 그 저장소에서 처리됩니다. `--force`는 합류하지 않고 새 승인을 요청하지만, 이때도 사람이 먼저
claim해야 동작할 수 있습니다.

승인은 [리뷰한 입력](fingerprint.md)이 그대로면 기본으로 재사용합니다. 나중의 `verify`는 제출된
결과를 재사용하고 그 결과를 쓰는 eval을 리뷰할 수 있습니다. 대상이나 포함한 의존성에
`fingerprint = false`가 있으면 승인은 현재 Run만 확정하고, 다음 Run은 새 Human 결정을
요청합니다. 현재 검증은 있는 증거를 쓰지만, 나중에 제출해도 지난 Run에 기록된 판정을 다시 쓰지는
않습니다.

아래 명령 표기에서 대괄호는 생략할 수 있는 부분이며, 대괄호는 입력하지 않습니다. 대문자로 쓴
부분은 바꿔 넣을 값입니다. `request list`가 돌려준 실제 `REQUEST_ID`와 `NAME` 자리의 리뷰어
이름을 쓰세요.

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

이 명령들의 옵션, 출력, 종료 코드는 모두 [Human 요청](../cli/index.md#human-requests)에
있습니다.

[Run 종료 코드](../cli/index.md#command-reference)로 지켜보기 중단, 직접 취소, 빠진 리뷰 의무를
구분할 수 있습니다. `verify --reuse-only human`은 Human 요청을 기록하거나 기다리지 않습니다. CI에
맞게 Human eval은 승인을 재사용하거나 [실행되지 않습니다](../cli/index.md#reuse-only).

`request list`와 `request show`의 필드, Run 요약은 [레퍼런스](tools/human.md#in-a-review)에
있습니다.

## 미룬 승인 {#deferred-sign-offs}

사람이 실행 하나하나를 이끌지 않고 최종 상태를 리뷰하는 흐름이라면 `verify --defer-human`을
고르세요. 머신 설정이 아니라 요청마다 정하며, 기본으로 꺼져 있습니다. `--speculate`로 기계 검사
작업을 따로 빠르게 할 수 있으며, 승인 미루기와 함께 써도 미룰 수 있는 승인은 마지막에
요청합니다.

리뷰어는 미룬 승인도 DEFERRED 표시가 붙은 일반 승인과 같은 방식으로 처리합니다. `review`, monitor, 데스크톱,
요청 목록에 WAITING_HUMAN 요청과 함께 나오고, claim, 도구, 해제, 제출도 똑같이 Human 권한을
확인합니다. 막힌 승인은 사람에게 보여 주지 않습니다. Run이 어떻게 끝나는지(SIGNOFFS_PENDING,
종료 코드 5)와 다음 verify가 결과를 어떻게 재사용하는지는
[추측 실행과 미룬 승인](run.md#speculation-and-deferred-sign-offs)에 있습니다. 이 재사용에는
fingerprint가 필요하므로 `fingerprint = false`면 재사용하지 않습니다.

같은 작업 트리에서 같은 eval에 더 새로운 요청이 오면, 재사용 키가 다른 이전 DEFERRED 승인은
끝나고 Review에서 사라집니다. 이렇게 끝나는 규칙, 합류, Run이 끝난 뒤의 승인 제출은
[Run 규칙](run.md#speculation-and-deferred-sign-offs)을 보세요.

반려되면 하위 작업이 크게 헛수고가 되는 승인이라면, 엄격한 대기 게이트를 직접 지정하세요.

```toml
[evals.release]
title = "The release is approved"
gate = "blocking"
profile = { kind = "human" }
payload.instruction = "Review the release before dependent work proceeds."
```

`gate`가 없으면 Human eval은 요청의 기본 모드를 따릅니다. CI는
`verify --all --wait --reuse-only agent,human`으로 엄격하게 유지됩니다. 승인 미루기도 추측
실행도 재사용할 수 있는 실제 Human GREEN 증거를 대신하지 않습니다.

## Human 도구 {#human-tools}

`views.human_tools`는 미리 정해 둔 도구를 안전한 이름으로 담는 별도의 맵입니다. 리뷰어가
실행하며, 자유 인자는 받지 않습니다. [내장 도구](tools/builtin.md)는 Linux, macOS, Windows에서
똑같이 동작합니다. `open`은 파일, 폴더, `http(s)` URL을 데스크톱의 기본 애플리케이션으로 열고,
`graph`는 현재 프로젝트 그래프 뷰어를 실행합니다. `read`, `list`, `section`, `help`는 터미널에
텍스트를 출력합니다.

```toml
[views.human_tools]
open = { description = "Open the review notes of {artifactName}.", builtin = "open", args = ["{artifactPath}/notes.md"] }
notes = { description = "Print the review notes of {artifactName}.", builtin = "read", args = ["{artifactPath}/notes.md"] }
```

내장 도구는 `builtin`과 `args`, 선택 항목인 `description`을 받습니다. 종류는 도구에 따라
정해집니다. `open`과 `graph`는 실행하는 종류이고, 나머지는 출력을 보여 주는 종류입니다.

명령 도구는 고른 프로그램을 실행합니다. `description`, `kind = "launch"` 또는
`kind = "output"`, `command`, `args`가 정확히 필요하고, 선택 항목으로 `timeout_ms`
(1–2147483647)를 받습니다. `input_schema`, 자유 호출 인자, `protocol`, `execution_paths`,
`metadata`, `script` 래퍼는 받지 않습니다. 설명은 `{artifactName}`을 포함해 Agent 설명 규칙을
따릅니다.

```toml
[views.human_tools]
edit = { description = "Open {artifactName} in VS Code.", kind = "launch", command = "code", args = ["{artifactPath}"] }
```

도구가 출력하는 내용은 리뷰가 열려 있는 동안 리뷰어의 [**Tool log**](tools/human.md#tool-log)에
실시간으로 나오며, 리뷰어가 결과에 복사해 넣을 수 있습니다.

자리 표시자, 실행 파일 찾기, `launch`와 `output` 종류는 [레퍼런스](tools/human.md#human-tools)에
있습니다.

## 결과 양식 {#result-forms}

리뷰어는 eval의 `pass_schema`(GREEN)나 `fail_schema`(RED)로 만든 양식에 결과를 입력합니다.
텍스트, 숫자, true/false, 선택지, 텍스트 목록, 필드를 반복하는 그룹이 있으며, 상수는 미리
채워집니다. Human eval의 스키마에는 이런 형태만 쓸 수 있습니다.
[선언 레퍼런스](declarations.md#human-result-schemas)에 그 목록과 각각이 어떻게 보이는지
있습니다.

대화형 리뷰는 [TUI](../tui/index.md)나 [데스크톱](../desktop/index.md)에서 하세요.
