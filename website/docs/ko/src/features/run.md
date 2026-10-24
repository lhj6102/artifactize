# 평가 Run {#evaluation-run}

`verify`는 선택한 eval을 리뷰하고 Run을 기록합니다. `status`는 eval을 실행하지 않고, 다음
`verify`에서 각 eval을 실행할지, 기존 결과를 재사용할지, 대기시키거나 차단할지 예측합니다. `status`도 선언된
fingerprint 스크립트는 실행할 수 있습니다.

## eval 실행 게이트 {#eval-execution-gates}

`verify`는 eval마다 그 eval의 [실행 게이트](artifact.md#eval-dependencies)에 따라 실행 시점을
정하며, 여기에는 [Human 승인의 자동 검사 선행 조건](artifact.md#human-sign-offs)도 포함됩니다.
`status`는 eval을 실행하지 않고 이 게이트를 보여 줍니다.

## Verify와 Run {#verify-and-runs}

`verify`는 [선택자](../cli/index.md#command-reference)를 정확히 하나 받습니다. Artifact,
eval(`green/check`), `--all` 중 하나입니다. Artifact 선택자는 그 Artifact의 검사를 실행해
달라는 뜻입니다. 서로 고리처럼 의존하는 대상은 **순환으로 묶인 대상**입니다. 이들의 검사는 선택한
Artifact를 리뷰하므로, Artifact 선택자는 순환으로 묶인 대상의 검사도 함께 실행합니다. eval 선택자는 그
검사 하나만 실행해 달라는 뜻입니다. 그래도 Run이 GREEN으로 끝나려면 그 검사가 의존하는 모든 대상의 유효한 결과가 필요하며, 이를 확인하려고 의존 관계를 필요한 범위까지 따라갑니다. 이렇게 이어진 묶음이
**의존성 범위**이고, 여기서 반드시 확보해야 하는 결과가 **최종 검증 요건**입니다. 이번 Run에서 실행해 달라고 하지
않았더라도 마찬가지입니다.

`--recursive`는 선택한 Artifact의 다른 검사를 포함해 그 묶음의 모든 검사를 설정 순서대로
선택합니다. `--all`은
이미 모든 검사를 선택하며, 검사가 없는 대상도 최종 검증 요건에 포함합니다. 파일 Artifact 선택자는 그 파일의 eval만
선택하며, 그 파일이 들어 있는 폴더의 eval은 선택하지 않습니다.

예를 들어 `code/tests`가 `plan`에 의존하고, `plan`에는 사람의 승인 `plan/signoff`가 필요하다고 가정합니다. `verify --eval code/tests`는 테스트를 선택하지만, 그 승인이 아직 유효하지 않으면 GREEN으로
끝날 수 없습니다. `verify --eval code/tests --recursive`는 `code/style`과 `plan/signoff` 같은
묶음의 다른 검사도 선택해서, 실행하거나 결과를 재사용할 수 있게 합니다. 검사를 더 선택한다고
승인이 자동으로 생기지는 않습니다. `--recursive`가 없어도 `code/style`과 그 입력에 필요한
리뷰는 통과해야 하므로, 이전 Run이나 캐시에 유효한 결과가 있어야 Run이 GREEN으로 끝납니다.


READY인 eval은 준비되는 즉시 시작하며, **동시 실행 수에 기본 상한이 없습니다**. 머신이 감당하기
어려울 만큼 프로세스가 많아지는 등 한꺼번에 실행하는 것이 문제를 일으킬 때만 `--jobs N`(최소
1)을 쓰세요. `--jobs`나 백엔드 한도 때문에 일부만 실행할 수 있으면 나머지는 이유와 함께 QUEUED로
표시되고, 앞선 작업이 끝나는 대로 시작합니다. `--max-executions`는 다릅니다. Run에서 시작할 수 있는 리뷰의 총수를 제한합니다. 한도에 도달하면 남은 요청은 `BUDGET_EXHAUSTED`가 되고 Run은 INCOMPLETE로 끝납니다([자세히](../cli/index.md#runtime-cli)). 다른 Run의 리뷰에 합류한 Runtime이나 Agent 요청은 직접 지정한
`--jobs`에 포함되지만, 승인에 합류한 Human 요청과 캐시 적중은 포함되지 않습니다. Agent 리뷰는
[백엔드 자리](others/state.md#backend-capacity)도 필요합니다. QUEUED는 접수 대기 중이거나, 백엔드 자리가 나지 않았거나, 합류한 실행이 아직 끝나지 않았거나, 직접 정한 Run의 동시 실행 상한에 걸렸을 때 표시됩니다. 백엔드 자리가
빌 때까지 대기 중인 요청은 Run의 `--jobs` 동시 실행 수에 포함되지 않습니다.
대기 중인 작업은 eval을 선택하거나 선언한 순서가 아니라, Run 전체가 더 빨리 끝나도록 artifactize가
정한 순서대로 시작합니다. `--max-executions`가 어떤 리뷰까지 실행할지도 이 순서로 정합니다.

### 제출, 지켜보기, 취소 {#submit-observe-and-cancel}

Run은 요청한 쪽과 상관없이 artifactize가 관리합니다. 옵션 없는 `verify`는 Run을 제출하고 Run ID
(`--json`이면 `{"run","state"}`)를 출력한 뒤, 승인 여부와 관계없이 Run이 **접수**되면 0으로 종료합니다.
`verify --wait`는 stderr에 Run ID를 출력하고, 결과가 확정될 때까지 지켜본 다음 저장된 결과를
보여 줍니다. 터미널을 닫거나 지켜보는 명령을 중단해도 지켜보기만 멈출 뿐 Run은 계속 실행됩니다. Run을 취소하는
방법은 `run cancel RUN_ID`뿐입니다. 요청이 기록되면 바로 0으로 종료하고, 그 뒤 Run은 새 작업을
시작하지 않고, 실행 중인 작업을 멈추고, 이미 저장된 결과는 남긴 채 ERROR/CANCELLED로 끝납니다.
작업이 정말 멈췄는지 확인하려면 `run watch RUN_ID`나 `run show RUN_ID`로 이어서 보세요. 이 Run이 다른 Run의 실행에 합류했다면 그 원래 실행은 멈추지 않습니다. 아직 대기 중이던 승인도
ERROR로 끝나고, 다른 Run에서 그 승인에 합류한 요청도 마찬가지이며, 다음 `verify`가 다시 요청합니다.
이는 Run이 아직 끝나지 않았을 때(RUNNING 또는 WAITING_HUMAN)만 해당합니다. 이미 YELLOW로 끝난 Run은
취소해도 바뀌는 것이 없고, 그 승인은 여전히 제출할 수 있습니다. 프로세스가 죽은 작업은
ERROR/OWNER_DIED로 끝나고 자동으로 다시 실행되지 않으니 다시 verify하세요.

Run은 접수될 때의 선언을 그대로 씁니다. 준비하는 동안 선언을 바꾸면 Run은 다른 기준이나
프로필로 실행하지 않고 **ERROR/INPUT_CHANGED**로 끝납니다. eval의 실행 일정이 잡힌 뒤에는
접수된 대로 실행합니다. 선언을 다 고친 다음 새 Run을 제출하세요(이전 Run이 아직 실행 중이면
취소하세요). 접수된 Run의 대상이 바뀌지는 않습니다.

승인은 다른 eval을 막지 않습니다. Run은 [승인과 Run 종료](#sign-offs-and-the-end-of-a-run)에 설명한
조건을 충족하는 동안 종료되지 않습니다. Human 대기 제한 시간이나 `verify --timeout-ms`는 없으며, 모든 eval
종류가 같은 확정 규칙을 따릅니다. 선택 기능인 [추측 실행](#speculation)은 실행 순서를 바꿀 뿐, Human의
결정 권한을 바꾸지 않습니다.

Run이 끝나면 지켜보는 명령도 그 Run의 결과에 맞는 종료 코드로 끝납니다. 종료 코드는 [CLI 레퍼런스](../cli/index.md#command-reference)에
있습니다.

최종 검증에는 필요한 범위 전체에서 실제 GREEN 증거나 직접 선언한 `basis = true`가 필요합니다.
basis로 선언해도 그 Artifact가 의존하는 Artifact의 리뷰 요건은 면제되지 않습니다. RED이면 하위 작업이 막히고, 필요한 증거가 없거나 실행 오류가 나면 하위 작업이 대기합니다. 선택한 리뷰가 GREEN이어도 충족되지 않은 최종 검증 요건이 있으면 INCOMPLETE이며, 남은 요건을 텍스트와
JSON으로 보여 줍니다. 없어진 예전 게이트 우회 설정은
[업그레이드 안내](../getting-started/update.md#upgrading-from-011)에 있습니다.
`--reuse-only KINDS`(CI에서는 예를 들어 `agent,human`)는 그 종류의 결과를 재사용만 합니다.
재사용할 증거가 없으면 실행하지 않고 INCOMPLETE로 남깁니다. [CLI](../cli/index.md#reuse-only)를
보세요.

[`--request-key`](../cli/index.md#submission-and-watching)는 호출하는 쪽이 요청 시도 하나를
식별하려고 붙이는 키입니다. 같은 키로 다시 요청하면 같은 Run을 돌려주므로, 응답을 놓쳤을 때 다시 시도하려면 같은
키를 쓰고 무언가를 고친 뒤에는 새 키를 쓰세요. eval 결과 재사용은 이 키와 상관없습니다.

### 승인과 Run 종료 {#sign-offs-and-the-end-of-a-run}

승인 요청은 그 승인의 선행 조건, 즉 같은 Artifact의 다른 자동 검사 eval이 GREEN이 되고(다른 승인 때문에 PROVISIONAL로
보이는 GREEN도 포함합니다) 이를 막는 게이트가 없다는 조건을 충족하는 즉시 보냅니다. 승인은 다른 eval을 막지 않습니다. 그 승인에 의존하는
eval도 바로 실행하며, 그 eval의 GREEN 결과는 승인이 GREEN이 될 때까지 PROVISIONAL로 표시됩니다. 그 eval 자체의
RED나 ERROR는 그대로 표시됩니다. 승인을 요청할 때 다른
자동 검사 작업이 아직 실행 중일 수 있으니, 최종 상태를 리뷰하려면 그 작업이 끝날 때까지 기다리세요.
요청은 DEFERRED로 표시됩니다. DEFERRED도 PROVISIONAL도 승인이 아닙니다. 이는 해당 Run에서 실행하도록
선택한 승인에 해당합니다. 선택하지 않은 승인도 최종 검증 요건에 포함되므로, 그 승인에 의존하는 eval은
재사용할 승인 결과가 있어야 실행되고, 그런 결과가 없으면 Run은 INCOMPLETE로 끝납니다. `--recursive`나 `--all` 등으로 그 승인도
선택하세요.

첫 승인을 요청한 때부터 Run은 **WAITING_HUMAN**입니다. RUNNING과 마찬가지로 아직 끝나지 않은 상태입니다.
승인만 남으면 리뷰어가 그중 하나를 맡고 있는 동안에는 Run이 종료되지 않으며, 아무도 맡지 않은 채
**유휴 유예 시간**이 지나면 종료됩니다. 유휴 유예 시간은 따로
[바꾸지](others/state.md#sign-off-idle-grace) 않았다면 2분입니다. 유예 시간은 다른 작업이 모두 끝난
시점과 승인 요청을 마지막으로 제출하거나 담당을 해제한 시점 중 더 늦은 시점부터 계산합니다. 새로 승인을 맡으면 멈춥니다. 그래서 승인을
요청한 사람은 다른 결과를 모두 재사용한 경우에도 승인을 맡을 시간이 있고, 한 리뷰어가 제출한 뒤에도 다음
리뷰어가 승인 요청을 맡을 시간이 있습니다. 그다음 Run은 이렇게 끝납니다.

- **GREEN**: 승인을 포함해 모든 결과가 GREEN입니다.
- **RED**: 승인을 포함해 결과 하나라도 RED입니다.
- **YELLOW**(종료 코드 5): 다른 결과는 모두 GREEN이지만 제출되지 않은 승인이 있습니다.
  **승인만 남았다**는 뜻이며 승인된 것은 아닙니다.

`verify --wait`, `run show --wait`, [MCP](../agent-plugin/mcp.md)의 대기 도구는 Run이 끝날 때까지 대기하므로,
리뷰어가 승인 요청을 맡고 있거나 유예 시간이 남아 있으면 계속 대기합니다. YELLOW는 최종 상태입니다. 나중에 제출하면 요청이
바뀔 뿐, Run의 판정은 바뀌지 않습니다. Run의 최종 판정에 승인 결과를 반영하려면 유휴 유예 시간이 지나기 전에 승인 요청을 맡으세요. 새 verify에서는 자동 검사의 잠정 결과가 바뀌지 않았으면 이를 재사용합니다. 승인이 GREEN이면
SATISFIED가 될 수 있고, RED면 그에 의존하는 eval을 막습니다. `--reuse-only agent,human`은 승인을 요청하지
않고 대기 중인 승인을 GREEN으로 간주하지 않으므로 CI에 그대로 쓸 수 있습니다.

같은 워크트리에서 더 새로운 Run이 같은 eval에 Human 요청을 보내면, 재사용 키가 다른 이전
DEFERRED 승인은 거기에 합류한 요청까지 포함해 ERROR/SUPERSEDED로 끝납니다. 누군가 맡은 승인도
마찬가지입니다. 이런 승인은 Review에서 사라지고 다시 맡을 수 없습니다. 같은 키라면 기존
리뷰에 그대로 합류합니다. 다른 워크트리의 원래 승인은 건드리지 않습니다. 이렇게 끝나기 전까지는 승인을
그 Run이 끝난 뒤에도 제출할 수 있습니다. 제출할 때 fingerprint를 다시 확인하며, 바뀐 파일이 있으면 요청은
판정 없이 ERROR(`INPUT_CHANGED`)로 끝나니 새 승인을 받으려면 다시 verify하세요.

`status`는 지금 대기 중인 승인과 잠정 결과를 표시하고, 결과가 없는 승인에 의존하는 eval은 실행된다고
예측합니다. Run과 결과를 보여 주는 화면은 WAITING_HUMAN인 Run을 아직 끝나지 않은 상태로, YELLOW인 Run을
끝났지만 최종 검증 요건은 충족하지 못한 상태로 표시합니다.

### 추측 실행 {#speculation}

`--speculate`는 [요청별 verify 모드](../cli/index.md#verify-modes)이며, 기본으로 꺼져 있고 Run의 `modes`에
저장됩니다. 같은 요청 키로 다시 제출하면 처음 선택을 그대로 씁니다.

추측 실행을 사용하면 게이트 조건의 충족 여부가 정해지기 전에도 요청한 Runtime, Agent eval을 시작합니다. 이때도 `--jobs`, 백엔드 수용량, `--max-executions` 한도를 지킵니다. 승인은 추측 실행으로 요청하지 않습니다. 준비된 작업이 먼저 실행됩니다. 준비된 작업에 추측 실행 리뷰가 쓰고 있는 자리가 필요하면 그 리뷰를 중단했다가 나중에 다시 시작하며, 시작할 때마다 `--max-executions`로 정한 실행 횟수에 포함됩니다. 상위 eval이 RED나 ERROR가
되면 그 결과로 무효가 된 하위 작업만 바로 취소합니다. 취소 대상은 실패한 상위 eval에 의존하는 eval이며, 실패한 eval과 같은 Artifact에
속하거나 같은 순환에 드는 eval은 제외합니다. 취소한 eval은 재사용할 수 있는 결과를 남기지 않고 BLOCKED로 표시합니다. 상위 eval이 GREEN이면 같은 파일을 대상으로 이미 만든 결과를 그대로 씁니다. 결과는 스케줄링이 끝난 뒤에, 끝까지 유효한 것만
[팀 리뷰 저장소](../cli/review-store.md)에 공유합니다.

텍스트 형식에서는 요청별 결과를 한 줄씩 보여 줍니다. fingerprint가 일치해 재사용한 결과나 실행 중인 리뷰에 합류해 얻은 결과에는 결과를 만든 Run을 표시합니다. [원격 리뷰 저장소](../cli/review-store.md#remote-review-store-client)에서 가져온 결과에는 결과를 만든 사람이나 Human 리뷰어도 표시합니다.

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed`는 ERROR 결과와 이 Run이 기록한 Human 요청을 포함해 이 Run이 직접 실행한 횟수이고,
`reused`는 캐시, 합류, 원격 결과를 셉니다. 모델, 제한 같은 설정은 재사용 키에
들어가지 않으므로, 설정이 다른 프로필로 얻은 결과도 재사용될 수 있습니다. 그런 결과의 줄에는
`profile NAME`이 붙습니다. 요약 끝에는 `; N produced by another profile`(JSON에서는
`summary.reused.otherProfile`)이 붙습니다. 설정이
같은 변형의 결과에는 표시가 붙지 않으며, 저장된 `options.variant`에 변형 이름이 있습니다.
`Usage`(사용량이 보고됐을 때만 출력)는 이 Run의 실제 사용량과, 재사용한 실행들의 원래 사용량 합계인
`saved`를 나눠 보여 줍니다. `verify`의 JSON 출력과 `run show`에는 같은 수치가 `summary.executed`,
`summary.reused`, Run 단위의 `usage: {spent, saved}`로 들어 있고, `summary.usage`는
`usage.spent`와 같습니다.

JSON에서는 요청마다 `source`가 결과의 출처를 알려 줍니다. 그 요청에서 리뷰를 직접 실행했거나 아직 결과가
없으면 null이고, 캐시, 합류, 원격 재사용이면 결과를 만든 실행의 요청을 가리킵니다.

```json
"source": {"runId": "run-Ksl1Qr", "requestId": "run-Ksl1Qr-3", "kind": "cache"}
```

`kind`는 해당 재사용 키로 저장된 완료 기록을 썼으면 `cache`, 같은 키로 실행 중인 작업(다른
`verify`나 같은 Run의 형제 eval)이 끝날 때까지 대기한 뒤 그 결과를 썼으면 `joined`,
[원격 리뷰 저장소](../cli/review-store.md#remote-review-store-client)의 기록이면 `remote`입니다.
`run show`, `request show`, `request list --json`에도 `source`가 있습니다.

`verify --wait --json`과 `run show RUN_ID`는 저장된 Run 전체를 출력합니다. `run show`는 선언을
찾거나 코드를 실행하지 않습니다. Run이 시작될 때의 선택, 정책, 프로필 옵션, eval 정의를
보여 주므로, 나중에 선언을 고쳐도 보이는 내용은 바뀌지 않습니다.

일부 eval만 실행하는 Run에서는 선택 범위 밖의 상위 eval 등에 새 요청을 만들지 않고 저장된 증거를 쓸 수 있습니다. Run은 eval ID를 키로, 저장된 상태를 값으로 하는 `evidence` 맵에 그 판정을 기록합니다.

```json
{"evidence": {"code-style/approved": "GREEN"}}
```

이는 이 Run이 쓴 증거이지 가장 최근 결과가 아닙니다. JSON 형태는
[CLI 레퍼런스](../cli/index.md#saved-run-evidence-and-terminal-views)에 있습니다. 선언을 바꾼 뒤에도 Monitor에서 이전 Run을 열면 그때 저장한 정의와 실행 게이트를 기준으로 실행 내역을 보여 줍니다.

`run list`는 저장된 Run을 보여 줍니다. [Run 목록](../cli/index.md#run-list)을 보세요.

리뷰 정책, `--force`, `--max-executions`, 선택 파일, `--profile` 변형은
[레퍼런스](../cli/index.md#runtime-cli)에 있으며, [명령 표](../cli/index.md#command-reference)와
[상태 구성](others/state.md#state)도 함께 보세요.

## Status와 정적 그래프 {#status-and-static-graph}

```sh
artifactize --repo PROJECT status
artifactize --repo PROJECT status ARTIFACT --recursive --json
artifactize --repo PROJECT config graph ARTIFACT --json
artifactize --repo PROJECT config check --json
```

`status`는 `verify`와 같은 선택자 중 하나를 받으며 기본은 `--all`이고, 같은 `--profile`,
`--recursive`, `--force` 정책 옵션을 받습니다. 현재 Artifact와 eval의 [상태](../cli/index.md#status-states), 아직 충족하지 못한 최종
검증 요건, `execute` / `reuse` / `wait` / `blocked` 동작을 알려 줍니다. 필요한 범위의 eval은 모두
나오며, `selected`와 `included`로 직접 선택한 것과 실행에 포함된 것(재귀로 들어온 의존성)을
구분합니다. JSON에서는 eval마다 [재사용 키](fingerprint.md#the-reuse-key)를 이루는 값도
보여 줍니다. `evalDefHash`, 대상의 `fingerprint`, `fingerprints`(eval이 의존하는 각
Artifact와 현재 fingerprint, 없으면 null), 이를 합친 `key`(fingerprint를 끈 경우 null)이며,
`verify`가 Run의 키로 쓸 값과 같습니다. 동작 개수는 실행에 포함된 eval만 셉니다. 종료 코드
0은 현재 검증 요건을 모두 충족했다는 뜻이고, 1은 아직 충족하지 못한 요건이 남았다는 뜻이며, 입력이나 상태가 잘못되면 2입니다.

status도 verify와 같은 격리와 검사 규칙을 적용하며, `--fingerprint-jobs` 한도 안에서
선택한 범위의 실행 가능한 eval에 필요한 fingerprint를 계산합니다. fingerprint 계산에 실패하면 2로 종료하며, 예전에 저장한
fingerprint로 대신하지 않습니다. 도구나 eval 명령을 실행하거나 Run을 만들지 않고, 실행할 작업을 예약하지도 않습니다. 없는 데이터베이스를 새로 만들지 않으며, 로컬 캐시의 사용 시각도 바꾸지 않습니다. 다만
팀 리뷰 저장소에서 찾은 결과는 그 저장소에서 사용한 것으로 기록되므로 더 오래 보관됩니다. fingerprint
스크립트는 상태 디렉터리 아래의 버려도 되는 출력 폴더를 쓰며, 데이터베이스가 없어도 이 폴더는
생길 수 있습니다. `config graph`와 `config check`는 완전히 정적이며 프로젝트 코드를 실행하지
않습니다.

eval마다 나오는 동작은 `verify`에서 그 eval이 실행되는지, 결과를 재사용하는지, 대기하는지, 막히는지를 나타냅니다.

| 동작 | 언제 | 사유 예 |
|---|---|---|
| `reuse` | 현재 재사용 키의 GREEN이나 RED 결과가 있음. 의존성이 대기 중이거나 RED여도 마찬가지(이때 상태는 WAIT_DEPENDENCY나 BLOCKED) | `The current eval and fingerprints have a completed cached result.` |
| `execute` | 결과가 없고, 의존성은 지금 있는 결과로 GREEN | `The current eval and fingerprints have no completed cached result.` |
| `blocked` | 결과가 없고, 의존성이 RED | `Dependency verdict RED: core/tests` |
| `wait` | 결과가 없고, 의존성의 결과는 verify가 실행해야 생김 | `Waiting for current GREEN dependency evidence: core/tests` |
| `wait` | 의존성은 준비됐고 같은 리뷰가 이미 실행 중임. verify는 새 리뷰를 시작하지 않고 그 실행에 합류함 | `The current eval and fingerprints have a live execution.` |

status는 `wait`인 eval이 실행될지는 알려 주지 못합니다. 텍스트 출력은 예를 들어
`Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`으로 끝나므로, 병합한
체크아웃에서 status를 실행하면 verify를 돌리지 않고도 무엇을 다시 리뷰할지 알 수 있습니다.
`--force`도 verify와 마찬가지로 직접 선택한 실행 가능한 eval에만 적용됩니다. Human eval의 `execute` 동작은 대기 요청을
기록한다는 뜻입니다. 같은 키의 Human 실행이 진행 중이면 원래 verify가 종료한 뒤에도 DEFERRED와
`wait` 동작으로 보입니다. 저장된 실행 기록 중에서는 정규화한 현재 저장소 경로에 해당하는 기록만 읽습니다. eval마다 있을 수
있는 `last: {runId, verdict, fingerprint?}`는 지난 기록이지 현재 증거가 아닙니다. 누가 만든
결과인지 전부 보려면 `run show RUN_ID`를 쓰세요. 재사용 키가 없는 GREEN이나 RED는 그 결과를 만든 Run의
요건만 충족하므로, 이후 status에서는 재사용이 아니라 STALE로 나옵니다. basis Artifact만으로 이루어진 범위도 검증 요건을
충족할 수 있지만, basis Artifact가 의존하는 Artifact의 요건이 충족되지 않으면 INCOMPLETE입니다.

재사용 키가 있는 eval에 현재 캐시 결과가 없으면, `status`는 같은 기준으로 낸 마지막 GREEN이나
RED 결과 이후 무엇이 바뀌었는지 설명하고 `changes: {sinceRunId, files?, dependencies?,
summary}`를 알려 줍니다. 텍스트로는 `Fingerprint changed since Run RUN_ID: ...` 줄이며, 예를
들어 `changed: +docs/new.md, -old.md, src/a.py; dependency core changed`입니다. `files`는
대상 Artifact에 속한 파일의 변경을 `path`(바뀜), `+path`(추가), `-path`(삭제)로 보여 주고, `dependencies`는
fingerprint가 바뀐 자식과 이름으로 참조한 Artifact의 변경을 `name`, `+name`, `-name`으로 보여
줍니다. fingerprint를 스크립트로 계산했거나 파일 목록을 남기지 않은 이전 기록처럼 파일별 변경 내역을 알 수 없으면, 대상의 변경 내역은 `fingerprint changed`나 `inputs changed`로만 표시합니다. 그 eval 정의가 캐시된 적이 없거나
강제로 실행한 eval이면 설명이 나오지 않습니다. 강제로 실행하지 않은 READY Runtime/Agent eval에 키가 없으면,
실행 사유에 `fingerprint = false`를 선언한 대상이나 의존성이 나옵니다. READY인 Human eval은 키가
없어도 "Record a request awaiting a Human claim and submission"이라고 나옵니다. 막혔거나
대기 중인 eval에는 실행을 막는 게이트나 대기 사유가 표시됩니다.

대기 중인 eval의 `blockedBy`에는 결과가 아직 없는 eval의 ID가, BLOCKED인 eval의 `blockedBy`에는 사유와 마찬가지로 실행을 막은 RED eval의 ID가 나옵니다. Detail은 이 게이트를 입력 Artifact별로 `Waits for`
아래에 묶어 보여 주며, `w`를 누르면 대기 중인 eval이 나옵니다.

나중에 Human이 승인을 제출하면 대기 중이던 Run이 확정될 수 있습니다. `run show`는 저장된 검증과
요청을 그대로 보여 줍니다. monitor는 저장된 정의와 증거에 지금의 요청 상태(나중에 제출한 Human
승인 포함)를 더해 트리를 만듭니다. 맨 위 줄은 **Run이 끝났을 때의 SATISFIED / NOT
SATISFIED**를 유지하며, 흐린 `*`는 그 뒤로 상태가 바뀐 행을 표시합니다. 파일이 바뀌었다는 뜻은
아닙니다. [Monitor](../tui/index.md#monitor)를 보세요.

`config graph`는 정적 그래프를 출력합니다. [Config graph](../cli/index.md#config-graph)를 보세요.

### 현재 그래프 리뷰 도구 {#current-graph-review-tools}

`config graph`는 설정을 확인하는 도구입니다. 리뷰에 쓰려면 아무 Artifact에나
[내장 `graph` 도구](tools/builtin.md#current-project-graph)를 선언하세요. Agent는 추출한 현재
그래프를 트리로 보고, Human은 저장된 Run 없이도 데스크톱 앱에서 그 브랜치의
[Current](../desktop/index.md#current)를 엽니다. 루트에 선택적으로 두는
[`.artf` 그래프 Artifact](declarations.md#graph-artifact)는 일반 eval을 가지며, 다른 Artifact처럼
Run과 status에 나옵니다. 다른 점은 fingerprint 계산 방식뿐이며, 그래프 전용 검증 절차나 루트
폴더 Artifact와의 자동 관계는 없습니다.
