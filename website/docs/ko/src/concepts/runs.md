# Run과 실행 흐름 {#runs-and-workflows}

`artifactize verify`는 선택한 검사들을 스케줄링하고 결과를 기록하는 **Run**을 시작합니다. `artifactize status`는 eval을 실행하지 않고 다음 `verify`에서 어떤 항목을 실행(`execute`), 재사용(`reuse`), 대기(`wait`), 차단(`blocked`)할지 미리 보여 줍니다. 선택하지 않은 필수 요건에는 `not included`를 표시합니다. `status`는 현재 선택 범위를 기준으로 범위 밖의 eval에 대해서도 예상 동작을 보여 줍니다. 상위 Human 승인을 선택했는지와 그 승인을 보류할 수 있는지도 예상 동작에 반영됩니다. 실제 실행 일정과 집계에는 선택한(`included`) eval만 포함합니다. 선택 범위를 넓혔을 때 계획이 어떻게 바뀌는지 보려면 실행하려는 선택자로 `status`를 다시 실행하세요.

## 검증 대상 선택하기 {#verify-and-runs}

`verify`에는 선택자(selector) 하나를 지정합니다.

```sh
artifactize verify --all --wait                  # 프로젝트의 모든 Artifact와 eval
artifactize verify app --wait                    # app, 하위 parts, 순환 참조 동료(cycle peers)
artifactize verify app/tests --wait              # 단일 eval (--eval app/tests와 동일)
artifactize verify --artifacts app,docs --wait   # 여러 Artifact (또는 --evals CSV)
artifactize verify app/tests --recursive --wait  # 의존성 클로저의 선행 eval까지 함께 실행
```

- **실행 게이트와 Run 통과 요건(Final obligations)의 차이**:
  - **실행 게이트(Execution gates)**는 선택한 eval이 언제 시작될 수 있는지 결정합니다([eval 단위 게이트](artifacts.md#eval-dependencies) 참고).
  - **Run 통과 요건(Final obligations)**: 이 요건을 모두 충족해야 Run이 통과합니다. 단일 eval(`verify app/tests`)만 선택했더라도 Run이 최종 `GREEN`이 되려면 `app`의 나머지 eval, 하위 `parts`, 그리고 직접 또는 간접으로 의존하는 모든 상위 Artifact(Human 승인 포함)에 유효한 `GREEN` 근거가 있어야 합니다. eval과 `parts`가 없고 `basis = true`도 아닌 Artifact를 선택했을 때도 미검증 요건으로 남습니다. 선택하지 않은 요건은 로컬 캐시나 원격 저장소에 남은 결과(Run 기록의 `evidence`)로 충족 여부를 확인하며, Run 결과 우선순위와 종료 코드는 [승인과 Run의 종료](#sign-offs-and-the-end-of-a-run)를 참고하세요. 선택한 eval과 여기에 직간접으로 연결된 모든 선행 eval을 함께 실행하려면 `--recursive`나 `--all`을 사용하세요.
- **동시 실행과 대기(`QUEUED`)**: 준비된(`READY`) 모든 eval은 게이트가 열리는 즉시 시작됩니다. 기본 설정에서는 Run 하나에서 동시에 실행할 수 있는 eval 수에 제한이 없습니다. 같은 [`mutex` 태그](../reference/declarations.md#eval-fields-evalsid)를 쓰는 다른 eval이 실행 중이면 해당 eval을 `QUEUED` 상태로 대기열에 넣습니다. 같은 상태 디렉터리를 공유하는 Run들에서 [`limits.json`](../reference/cli-and-mcp.md#state-cache-and-limits)의 백엔드별 동시 실행 작업 수 한도에 도달했거나 사용자가 `--jobs N` 제한을 지정했을 때도 `QUEUED` 상태로 대기합니다. `--max-executions N`은 새로 시작하는 `runtime` 및 `agent` 실행 횟수를 제한합니다. 실행 횟수에는 캐시 재사용, 실행 중인 작업의 결과 공유, Human 승인 요청을 포함하지 않습니다. 한도 때문에 준비된 실행을 시작하지 못하면 해당 요청은 `BUDGET_EXHAUSTED`가 되고 Run은 종료 코드 `4`인 `INCOMPLETE`로 끝납니다.

## Run 시작, 진행 상황 확인, 취소 {#submit-observe-and-cancel}

Artifactize는 Run을 시작한 터미널이나 MCP 세션과 독립적으로 백그라운드에서 Run을 관리합니다.

- 옵션 없는 `artifactize verify SELECTOR`는 Run을 접수한 즉시 `RUN_ID`를 출력하고 종료 코드 `0`으로 끝납니다.
- `artifactize verify SELECTOR --wait`는 Run이 끝날 때까지 기다린 뒤 Run 결과 코드로 끝납니다. `artifactize run show RUN_ID --wait [--timeout-ms MS]`도 아직 끝나지 않은 Run의 진행 상황을 지켜보며(기본 대기 제한 시간 10분), 제한 시간이 먼저 지나면 Run은 그대로 둔 채 종료 코드 `3`으로 끝납니다.
- `artifactize verify SELECTOR --watch`(또는 `artifactize run watch RUN_ID`)는 진행 이벤트(`update`, `eval_result`, `outcome`)를 이벤트마다 한 줄씩 JSON으로 출력합니다.
- `--wait`나 `--watch` 실행 중에 `Ctrl+C`를 누르면 진행 상황을 표시하던 명령만 종료 코드 `130`으로 끝납니다. **Run 자체는 취소되지 않고 계속 실행됩니다.**
- 아직 끝나지 않은 Run(`RUNNING` 또는 `WAITING_HUMAN`)을 멈추려면 `artifactize run cancel RUN_ID`(또는 MCP `verify_cancel`)를 실행하세요. 취소 요청이 기록되면 즉시 종료 코드 `0`으로 끝납니다. 이후 Run은 새 작업 시작을 멈추고 직접 시작한 실행 중 작업을 종료하며, 이미 저장된 결과는 유지한 채 `ERROR/CANCELLED`로 끝납니다. 다른 Run이 시작한 작업의 결과를 기다리던 경우에는 대기만 끝나고 원래 작업은 계속 실행됩니다. 완전히 멈췄는지 확인하려면 `run watch RUN_ID`나 `run show RUN_ID`를 이어서 실행하세요.
- `--comment "로그인 양식 수정"`으로 `run list`, 데스크톱 앱, TUI에 표시할 메모를 남기거나, `--request-key KEY`로 재시도 시 같은 Run을 돌려받게 할 수 있습니다.

## 선행 조건이 충족되기 전에 미리 실행하기 (`--speculate`) {#speculation}

기본 모드에서는 상위 의존성의 실행 조건이 충족(`GREEN`, `PROVISIONAL`, 또는 선택된 `DEFERRED` 승인)된 뒤에만 하위 eval을 시작합니다. `--speculate`를 넘기면 상위 게이트가 확정되기 전이라도 `--jobs`, `--max-executions`, 백엔드 슬롯, `mutex` 한도 안에서 요청한 `runtime` 및 `agent` eval을 먼저 시작합니다. Human 승인은 미리 요청하지 않습니다.

- 게이트가 충족된 일반 작업이 우선권을 가집니다. 일반 작업에 `--jobs`나 백엔드 슬롯이 필요하면 미리 실행 중이던 리뷰를 중단하고 대기열에 다시 넣습니다(시작할 때마다 `--max-executions` 횟수 차감).
- 상위 의존성이 나중에 `GREEN`으로 통과하면 미리 실행해 둔 결과를 그대로 인정해 전체 시간을 단축합니다.
- 하위 eval의 대상 Artifact나 순환 참조 밖의 상위 의존성이 `RED` 또는 `ERROR`가 되면, 그 입력에 의존하는 하위 eval 중 미리 실행 중인 작업을 즉시 취소하고 이미 완료된 실행 결과도 무효화해 재사용 기록에서 제외합니다. 이어서 영향받는 하위 요청(Human 승인 포함)을 `BLOCKED`로 표시합니다.
- 반복 수정([Focus then sweep](../agent-plugin/plugin.md#focus-then-sweep-iterate-on-red-evals)) 중에 `verify --eval ID --speculate --wait`를 쓰면 상위 입력이 아직 재검증되지 않은 상태에서도 수정 중인 eval만 바로 실행해 볼 수 있습니다.

## 승인과 Run의 종료 {#sign-offs-and-the-end-of-a-run}

Human eval이 시작되려면 `{name}`으로 참조한 상위 의존성 게이트를 모두 통과해야 합니다. 또한 **같은 Artifact**의 모든 자동(`runtime`, `agent`) eval이 `GREEN`(`PROVISIONAL` 포함)이어야 합니다. 같은 Artifact의 자동 eval이 `RED`나 `ERROR`이면 Human eval은 `BLOCKED`가 됩니다.

조건이 충족되면 선택한 Human 승인은 다른 eval을 막지 않는 **`DEFERRED` 요청**으로 기록됩니다. 아직 끝나지 않은 Run은 **`WAITING_HUMAN`** 상태가 되고, 그 승인을 참조하는 하위 eval들은 나머지 선행 조건을 충족하고 실행 한도에 여유가 있으면 시작합니다. 이때 통과한(`GREEN`) 하위 eval 결과는 승인이 제출될 때까지 **`PROVISIONAL`**로 표시됩니다(`RED`나 `ERROR`는 그대로 표시). 같은 로컬 상태에서 `--force` 없이 실행된 다른 Run에 같은 재사용 키의 승인이 필요하면 새 승인 요청을 만들지 않고 기존 요청의 결과를 공유합니다.

모든 자동 검사가 끝나면 제출되지 않은 Human 승인만 남습니다. 이때 리뷰어가 요청을 맡고 있으면 Run을 종료하지 않습니다. 맡은 사람이 없을 때는 기본 2분 동안 기다리며, 이 유예 시간은 `limits.json`의 `signoffIdleGraceSeconds`나 `artifactize limits --signoff-idle-grace SECONDS`로 바꿀 수 있습니다. 담당자 없이 유예 시간이 지나면 Run을 종료합니다. 제출하지 않은 승인만 남았다면 **`YELLOW`**로 끝나고, 필수 검사에서 실패했거나 Run 통과에 필요한 결과가 없다면 `RED`·`ERROR`·`INCOMPLETE`로 끝납니다.

| Run 결과 | 종료 코드 | 의미 |
|---|---|---|
| `GREEN` | `0` | Human 승인을 포함한 모든 필수 eval이 `GREEN`으로 통과했습니다. |
| `RED` | `1` | eval이나 그 밖의 필수 요건 중 하나라도 결과가 `RED`입니다(취소, `ERROR`, 또는 예산/`--reuse-only`로 인한 `INCOMPLETE`가 없을 때). |
| `ERROR` | `2` | 명시적인 Run 취소(`ERROR/CANCELLED`, 최우선 적용)나 Run 준비 단계 오류(fingerprint 스크립트 실패, 준비 중 선언 변경 `INPUT_CHANGED` 등)가 일어났거나, 취소되지 않은 Run에서 eval 실행 오류가 발생하면 `ERROR`가 됩니다. 다만 취소되지 않은 Run에서 실행 횟수 한도나 `--reuse-only`로 판정이 `INCOMPLETE`라면 그쪽이 개별 eval 오류보다 우선합니다. |
| `INCOMPLETE` | `4` | 취소되지 않은 Run에서 `--max-executions` 한도에 걸려 실행을 시작하지 못했거나(`BUDGET_EXHAUSTED`) [`--reuse-only`에서 재사용할 캐시 결과가 없으면](../guides/team-and-ci.md#ci) `INCOMPLETE`로 끝납니다. 이 두 경우는 `ERROR`나 `RED`보다 먼저 적용됩니다. 그 외에는 `ERROR`나 `RED`가 없더라도 충족하지 못한 요건(유효한 근거가 없는 미선택 요건, 또는 eval·`parts`·`basis = true`가 없는 선택된 Artifact)이 남으면 `INCOMPLETE`가 됩니다. |
| `YELLOW` | `5` | 모든 자동 검사는 통과(`GREEN` 또는 `PROVISIONAL`)했고 **제출되지 않은 Human 승인만 남아 있습니다**(최종 승인 아님). |

Run이 `YELLOW`로 끝난 뒤에도 `artifactize review`와 데스크톱 앱에서 대기 중인 승인 요청을 처리할 수 있습니다. 사람이 `GREEN`을 제출하면 해당 요청과 기존 승인 요청에 연결된 다른 요청이 완료되며, [재사용 키](reuse.md#the-reuse-key)가 유지되는 동안 다음 `artifactize verify`가 그 승인을 재사용합니다(`fingerprint = false`이면 다음 Run에서 새 승인을 요청). 같은 워크트리에서 새 Run이 다른 재사용 키로 같은 Human eval을 요청하면 이전 대기 요청은 `ERROR/SUPERSEDED`로 종료됩니다.
