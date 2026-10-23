# Human 리뷰 {#human-reviews}

Human eval은 사람에게 리뷰를 요청하고 GREEN이나 RED 판정을 받습니다. 리뷰어는 대기 중인 요청을 맡고,
선언한 Human 도구를 실행하고, `artifactize request` 명령이나 `artifactize review` 터미널 UI로
판정을 제출합니다.

Human eval은 같은 Artifact의 Human이 아닌 다른 eval이 모두 GREEN이고 의존성 게이트도 충족됐을
때만 준비됩니다. 사람은 자동 검사 eval을 이미 통과한 것만 승인합니다. 그 자동 검사 eval 중
하나라도 RED나 ERROR면 Human eval은 BLOCKED가 되고 사유에 그 eval이 나옵니다. 같은 Artifact의
다른 Human eval은 이 조건에 들어가지 않습니다. [Human 승인](artifact.md#human-sign-offs)을
보세요.

검사가 준비되면 artifactize는 사람의 결정이 필요한 작업을 **DEFERRED 요청**으로
저장합니다. 이 요청은 다른 작업을 막지 않습니다. 사람이 판정을 제출할 때까지는 Run의 `--jobs` 실행 자리도,
`--max-executions` 예산도 쓰지 않으며, 그 예산이 0이어도 마찬가지입니다. 그 승인에 의존하는 검사는 바로
실행하며, 그 검사의 GREEN 결과는 승인이 GREEN이 될 때까지 PROVISIONAL로 표시되고, 검사 자체의 RED나 ERROR는
그대로 표시됩니다. 다른 작업이 아직 실행 중일 수 있으니, 최종 상태를
리뷰하려면 그 작업이 끝날 때까지 기다리세요. 모든 승인은 `review`, monitor, 데스크톱, 요청 목록에 나오고, 담당
지정과 해제, 도구 실행, 판정 제출 때 Human 권한을 확인합니다. 막힌 승인은 사람에게 보여 주지 않습니다. 진행 상황을 지켜보거나 터미널을 닫아도 승인이나 거부가 제출되지는 않습니다. 제출, 지켜보기, 직접 취소는
[Run 수명 주기](run.md#submit-observe-and-cancel)에서 설명합니다.

리뷰어가 요청을 **맡으면**(`claim`) 그 리뷰어가 담당자로 지정됩니다. 이와 별개로, 다른 Run에서 같은 입력과
프로필에 대한 승인이 필요하면 다른 체크아웃에서 실행한 Run이라도 새로 요청하지 않고 대기 중인
리뷰에 합류합니다. 이렇게 합류한 요청을 **팔로워**라고 하며, 팔로워 요청에서 Human 작업을 해도 실제로는 원래 요청이 있는 저장소에서 원래 요청을 대상으로 처리합니다. `--force`는 합류하지 않고 새 승인을 요청하지만, 새 요청에서도 사람이 먼저
맡아야 도구를 실행하거나 판정을 제출할 수 있습니다.

승인은 [리뷰한 입력](fingerprint.md)이 그대로면 기본으로 재사용합니다. 이후 `verify`는 제출된
결과를 재사용하고 그 결과를 쓰는 eval을 리뷰할 수 있습니다. 대상이나 포함한 의존성에
`fingerprint = false`가 있으면 승인은 현재 Run에만 적용되므로, 다음 Run에서는 사람이
다시 판정해야 합니다. Run의 검증은 그 시점에 있는 결과로 정해지며, 나중에 승인을 제출해도 지난 Run에 기록된 판정은
바뀌지 않습니다.

아래 명령 표기에서 대괄호는 생략할 수 있는 부분이며, 대괄호는 입력하지 않습니다. 대문자로 쓴
부분은 바꿔 넣을 값입니다. `REQUEST_ID`에는 `request list`가 출력한 실제 요청 ID를 넣고, `NAME`에는
리뷰어 이름을 넣으세요.

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

[Run 종료 코드](../cli/index.md#command-reference)로 지켜보기 중단, 직접 취소, 충족되지 않은 최종 검증 요건을
구분할 수 있습니다. `verify --reuse-only human`은 Human 요청을 기록하지 않으며 결과가 나올 때까지 대기하지도 않습니다. CI에서는
Human eval이 기존 승인을 재사용하고, 재사용할 승인이 없으면 [실행되지 않습니다](../cli/index.md#reuse-only).

`request list`와 `request show`의 필드, Run 요약은 [레퍼런스](tools/human.md#in-a-review)에
있습니다.

## 승인과 Run {#sign-offs-and-the-run}

리뷰어가 승인 하나를 맡고 있는 동안, 그리고 남은
작업이 승인뿐이 된 뒤 유휴 유예 시간(따로 [바꾸지](others/state.md#sign-off-idle-grace) 않았다면 2분) 동안
Run은 종료되지 않고 **WAITING_HUMAN** 상태를 유지합니다. 그 시간 안에 승인을 맡아 제출하면 Run은 그 결과에 따라 GREEN이나
RED로 끝납니다. 아무도 맡지 않으면 Run은 **YELLOW**(종료 코드 5)로 끝나며, 승인만 남았다는 뜻입니다. 그 뒤에도
승인을 제출할 수 있고, 다음 verify가 그 결과를 재사용합니다. 자세한 내용은
[승인과 Run 종료](run.md#sign-offs-and-the-end-of-a-run)에 있습니다. 이 재사용에는 fingerprint가 필요하므로
`fingerprint = false`면 재사용하지 않습니다.

같은 워크트리에서 같은 eval에 더 새로운 요청이 오면, 재사용 키가 다른 이전 DEFERRED 승인은
끝나고 Review에서 사라집니다. 이전 승인이 종료되는 조건과 리뷰 합류 방식, Run이 끝난 뒤 승인을 제출하는 방법은 [Run 규칙](run.md#sign-offs-and-the-end-of-a-run)을 보세요.

CI 설정은 [대기 중인 승인을 병합 게이트에서 빼기](team/ci.md#keep-pending-sign-offs-out-of-the-merge-gate)를 보세요.

## Human 도구 {#human-tools}

리뷰어가 쓸 도구는 `views.human_tools` 아래에 도구마다 이름을 붙여 적으세요. 도구는
리뷰어가 실행하며, 실행할 때 임의의 인자를 덧붙일 수는 없습니다. [내장 도구](tools/builtin.md)는 Linux, macOS, Windows에서
똑같이 동작합니다. `open`은 파일, 폴더, `http(s)` URL을 데스크톱의 기본 애플리케이션으로 열고,
`graph`는 현재 프로젝트 그래프 뷰어를 실행합니다. `read`, `list`, `section`, `help`는 터미널에
텍스트를 출력합니다.

```toml
[views.human_tools]
open = { description = "Open the review notes of {artifactName}.", builtin = "open", args = ["{artifactPath}/notes.md"] }
notes = { description = "Print the review notes of {artifactName}.", builtin = "read", args = ["{artifactPath}/notes.md"] }
```

내장 도구는 `builtin`과 `args`, 선택 항목인 `description`을 받습니다. 종류는 도구에 따라
정해집니다. `open`과 `graph`는 프로그램 실행용이고, 나머지는 출력 표시용입니다.

명령 도구는 고른 프로그램을 실행합니다. `description`, `kind`, `command`, `args`는
필수 항목이며, `kind`는 `"launch"`나 `"output"`입니다. `timeout_ms`(1–2147483647)는
선택 항목입니다. `input_schema`, 호출할 때 임의로 넘기는 인자, `protocol`, `execution_paths`,
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
텍스트, 숫자, true/false, 선택지, 텍스트 목록, 반복해서 추가할 수 있는 필드 그룹이 있으며, 상수는 미리 채워집니다. Human eval의 스키마에는 이런 형태만 쓸 수 있습니다.
[선언 레퍼런스](declarations.md#human-result-schemas)에 지원하는 형태의 목록과 화면 표시 방식이
정리되어 있습니다.

대화형 리뷰는 [TUI](../tui/index.md)나 [데스크톱](../desktop/index.md)에서 하세요.
