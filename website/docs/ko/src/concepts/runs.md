# Run과 실행 흐름 {#runs-and-workflows}

`artifactize verify`는 선택한 eval을 스케줄링하고 결과를 기록하는 **Run**을 시작합니다. `artifactize status`는 eval을 실행하지 않고 다음 `verify`의 예상 동작을 미리 보여 줍니다. 각 eval을 새로 실행할지(`execute`), 이전 결과를 재사용할지(`reuse`), 선행 조건이 충족될 때까지 대기할지(`wait`), 실행이 막힐지(`blocked`) 구분해 표시합니다. 선택 범위에 없는 필수 항목에는 `not included`를 표시합니다. `status`는 현재 선택 범위를 기준으로 범위 밖의 eval에 대해서도 예상 동작을 보여 줍니다. 예상 동작에는 선행 Human 승인 요청이 검증 범위에 포함됐는지, 그 요청을 보류할 수 있는지도 반영합니다. 선택한(`included`) eval만 실행 계획과 집계에 넣습니다. 검증 범위를 넓힌 실행 계획을 미리 보려면 `verify`에서 쓸 선택자를 그대로 `status`에 넘기세요.

## 검증 대상 선택하기 {#verify-and-runs}

`verify`에는 선택자(selector) 하나를 지정합니다.

```sh
artifactize verify --all --wait                  # 프로젝트의 모든 Artifact와 eval
artifactize verify app --wait                    # app, 하위 parts, 순환 참조 동료(cycle peers)
artifactize verify app/tests --wait              # 단일 eval (--eval app/tests와 동일)
artifactize verify --artifacts app,docs --wait   # 여러 Artifact (또는 --evals CSV)
artifactize verify app/tests --recursive --wait  # 의존성 클로저의 선행 eval까지 함께 실행
```

- **eval 실행 조건과 Run 통과 요건(Final obligations)의 차이**:
  - **eval 실행 조건**은 선택한 eval이 언제 시작될 수 있는지 결정합니다([eval 실행 조건](artifacts.md#eval-dependencies) 참고).
  - **Run 통과 요건(Final obligations)**: 이 요건을 모두 충족해야 Run이 통과합니다. 단일 eval(`verify app/tests`)만 선택해도 Run 전체의 통과 요건은 그대로 적용됩니다. Run이 최종 `GREEN`이 되려면 `app`의 나머지 eval과 하위 `parts`에 유효한 `GREEN` 결과가 있어야 합니다. `app`이 직접 또는 간접으로 의존하는 선행 Artifact(Human 승인 포함)에도 같은 요건을 적용합니다. eval도 `parts`도 없고 `basis = true`도 아닌 Artifact를 선택하면, 해당 Artifact는 검증하지 못한 항목으로 남습니다. 선택 범위 밖의 요건을 충족했는지는 로컬 캐시나 팀 리뷰 저장소에 남은 결과(Run 기록의 `evidence`)로 확인합니다. Run 결과 우선순위와 종료 코드는 [승인과 Run의 종료](#sign-offs-and-the-end-of-a-run)를 참고하세요. 선택한 eval과 여기에 직간접으로 연결된 모든 선행 eval을 함께 실행하려면 `--recursive`나 `--all`을 사용하세요.
- **동시 실행과 대기(`QUEUED`)**: 준비된(`READY`) eval은 실행 조건을 충족하면 바로 시작합니다. 기본 설정에서는 Run 하나에서 동시에 실행할 수 있는 eval 수에 제한이 없습니다. 같은 [`mutex` 태그](../reference/declarations.md#eval-fields-evalsid)를 쓰는 다른 eval이 실행 중이면 해당 eval을 `QUEUED` 상태로 대기열에 넣습니다. [`limits.json`](../reference/cli-and-mcp.md#state-cache-and-limits)에서 백엔드별 동시 실행 한도를 정할 수 있으며, 같은 상태 디렉터리를 쓰는 모든 Run에 이 한도를 적용합니다. `--jobs N`으로 한 Run에서 동시에 실행할 eval 수를 제한할 수도 있습니다. 동시 실행 한도 중 하나라도 차면 eval은 `QUEUED` 상태로 대기합니다. `--max-executions N`은 새로 시작하는 `runtime` 및 `agent` 실행 횟수를 제한합니다. 실행 횟수에는 캐시 재사용, 실행 중인 작업의 결과 공유, Human 승인 요청을 포함하지 않습니다. 실행할 준비가 된 eval이 횟수 한도에 걸리면 해당 요청은 `BUDGET_EXHAUSTED`가 되고 Run은 종료 코드 `4`인 `INCOMPLETE`로 끝납니다.

## Run 시작, 진행 상황 확인, 취소 {#submit-observe-and-cancel}

Artifactize는 Run을 시작한 터미널이나 MCP 세션과 독립적으로 백그라운드에서 Run을 관리합니다.

- 옵션 없는 `artifactize verify SELECTOR`는 Run을 접수한 즉시 `RUN_ID`를 출력하고 종료 코드 `0`으로 끝납니다.
- `artifactize verify SELECTOR --wait`는 Run이 끝날 때까지 대기한 뒤 Run 결과에 해당하는 종료 코드를 반환합니다. `artifactize run show RUN_ID --wait [--timeout-ms MS]`도 Run이 끝날 때까지 대기합니다(기본 대기 제한 시간 10분). Run이 끝나기 전에 제한 시간이 지나면 Run은 계속 실행되고 CLI 명령만 종료 코드 `3`으로 끝납니다.
- `artifactize verify SELECTOR --watch`(또는 `artifactize run watch RUN_ID`)는 진행 이벤트(`update`, `eval_result`, `outcome`)를 이벤트마다 한 줄씩 JSON으로 출력합니다.
- `--wait`나 `--watch` 실행 중에 `Ctrl+C`를 누르면 진행 상황을 표시하던 명령만 종료 코드 `130`으로 끝납니다. **Run 자체는 취소되지 않고 계속 실행됩니다.**
- 아직 끝나지 않은 Run(`RUNNING` 또는 `WAITING_HUMAN`)을 멈추려면 `artifactize run cancel RUN_ID`(또는 MCP `verify_cancel`)를 실행하세요. 취소 요청이 기록되면 즉시 종료 코드 `0`으로 끝납니다. Run을 취소하면 새 작업은 시작하지 않습니다. 해당 Run에서 시작한 작업 중 아직 끝나지 않은 것만 중단합니다. 이미 저장한 결과는 그대로 남으며 Run은 `ERROR/CANCELLED`로 끝납니다. 다른 Run이 시작한 작업의 결과를 기다리던 경우에는 대기만 끝나고 원래 작업은 계속 실행됩니다. 완전히 멈췄는지 확인하려면 `run watch RUN_ID`나 `run show RUN_ID`를 이어서 실행하세요.
- `--comment "로그인 양식 수정"`을 지정하면 `run list`와 데스크톱 앱에 표시할 메모를 남깁니다. `--request-key KEY`를 지정하면 다시 요청해도 기존 Run이 반환됩니다.

## 선행 조건이 충족되기 전에 미리 실행하기 (`--speculate`) {#speculation}

기본 모드에서는 선행 eval의 결과가 `GREEN` 또는 `PROVISIONAL`이거나, 검증 범위에 포함된 선행 승인 요청이 `DEFERRED` 상태일 때만 하위 eval을 시작합니다. `--speculate`를 쓰면 선행 조건을 확인하기 전에 eval을 먼저 실행할 수 있습니다. 이 옵션은 요청한 `runtime`과 `agent` eval에 적용됩니다. 먼저 실행하는 eval도 `--jobs`, `--max-executions`, 백엔드별 동시 실행 한도, `mutex` 한도를 따릅니다. Human 승인은 미리 요청하지 않습니다.

- 실행 조건을 충족한 일반 작업부터 처리합니다. `--jobs`나 백엔드별 동시 실행 한도에 걸려 일반 작업을 시작할 수 없으면, 미리 시작한 리뷰를 중단하고 대기열에 다시 넣습니다(시작할 때마다 `--max-executions` 횟수 차감).
- 선행 eval이 나중에 `GREEN`으로 끝나면 미리 실행한 결과를 그대로 써서 전체 검증 시간을 줄입니다.
- 하위 eval의 대상 Artifact나 순환 참조 밖의 선행 항목에서 `RED`나 `ERROR`가 나올 수 있습니다. 이때 그 입력에 의존하는 하위 eval 가운데 미리 실행 중인 작업은 즉시 취소하고, 이미 완료된 결과는 무효화해 재사용 기록에서 뺍니다. 이어서 영향받는 하위 요청(Human 승인 포함)을 `BLOCKED`로 표시합니다.
- 반복 수정([Focus then sweep](../agent-plugin/plugin.md#focus-then-sweep-iterate-on-red-evals)) 중에는 `verify --eval ID --speculate --wait`를 쓸 수 있습니다. 입력 Artifact를 아직 다시 검증하지 않았어도 수정 중인 eval만 바로 실행해 볼 수 있습니다.

## 승인과 Run의 종료 {#sign-offs-and-the-end-of-a-run}

Human eval은 `{name}`으로 참조한 입력의 선행 조건을 모두 충족한 뒤에 시작합니다. 또한 **같은 Artifact**의 모든 자동(`runtime`, `agent`) eval이 `GREEN`(`PROVISIONAL` 포함)이어야 합니다. 같은 Artifact의 자동 eval이 `RED`나 `ERROR`이면 Human eval은 `BLOCKED`가 됩니다.

조건이 충족되면 선택한 Human eval의 승인 요청을 **`DEFERRED` 요청**으로 기록하며, 이 요청이 다른 eval의 실행을 막지는 않습니다. 아직 끝나지 않은 Run은 **`WAITING_HUMAN`** 상태로 바뀝니다. 해당 승인에 의존하는 하위 eval은 나머지 선행 조건을 충족하고 실행 한도에 여유가 있으면 시작합니다. 이때 통과한(`GREEN`) 하위 eval 결과는 리뷰어가 승인 요청에 대한 판정을 제출할 때까지 **`PROVISIONAL`**로 표시됩니다(`RED`나 `ERROR`는 그대로 표시). 같은 상태 디렉터리에서 `--force` 없이 다른 Run을 시작할 수 있습니다. 이때 재사용 키가 같은 승인 요청이 이미 있으면 새 요청을 만들지 않고 기존 요청의 결과를 공유합니다.

모든 자동 검사가 끝나면 리뷰어가 아직 판정을 내리지 않은 Human 승인 요청만 남습니다. 이때 리뷰어가 요청을 맡고 있으면 Run을 종료하지 않습니다. 맡은 사람이 없을 때는 기본 2분 동안 기다리며, 이 유예 시간은 `limits.json`의 `signoffIdleGraceSeconds`나 `artifactize limits --signoff-idle-grace SECONDS`로 바꿀 수 있습니다. 유예 시간 안에 요청을 맡은 사람이 없으면 Run을 종료합니다. 판정이 나오지 않은 승인 요청만 남았다면 **`YELLOW`**로 끝납니다. 필수 검사에서 실패했거나 Run 통과에 필요한 결과가 없다면 `RED`·`ERROR`·`INCOMPLETE`로 끝납니다.

| Run 결과 | 종료 코드 | 의미 |
|---|---|---|
| `GREEN` | `0` | Human 승인을 포함한 모든 필수 eval이 `GREEN`으로 통과했습니다. |
| `RED` | `1` | eval이나 그 밖의 필수 항목에서 하나라도 `RED`가 나오면 Run은 `RED`로 끝납니다. 다만 취소, `ERROR`, 실행 횟수 한도나 `--reuse-only`로 인한 `INCOMPLETE`가 있으면 그 결과가 우선합니다. |
| `ERROR` | `2` | Run을 직접 취소하면 `ERROR/CANCELLED`로 끝나며, 이 결과가 최우선으로 적용됩니다. 준비 단계에서 오류(fingerprint 스크립트 실패, 준비 중 선언 변경 `INPUT_CHANGED` 등)가 나도 `ERROR`가 됩니다. 취소되지 않은 Run에서 eval 실행 오류가 발생했을 때도 `ERROR`로 끝납니다. 다만 실행 횟수 한도나 `--reuse-only` 때문에 `INCOMPLETE`가 적용되는 경우에는 그쪽이 개별 eval 오류보다 우선합니다. |
| `INCOMPLETE` | `4` | 취소되지 않은 Run에 적용합니다. `--max-executions` 한도에 걸려 실행을 시작하지 못했거나(`BUDGET_EXHAUSTED`), [`--reuse-only`에서 재사용할 캐시 결과가 없으면](../guides/team-and-ci.md#ci) `INCOMPLETE`로 끝납니다. 이 두 경우는 `ERROR`나 `RED`보다 먼저 적용됩니다. 그 외에는 `ERROR`나 `RED`가 없어도 충족하지 못한 요건이 남으면 `INCOMPLETE`가 됩니다. 선택하지 않은 필수 항목에 유효한 판정 근거가 없을 때가 이에 해당합니다. 선택한 Artifact에 eval과 `parts`가 없고 `basis = true`도 설정하지 않았을 때도 `INCOMPLETE`로 끝납니다. |
| `YELLOW` | `5` | 모든 자동 검사는 통과(`GREEN` 또는 `PROVISIONAL`)했고 **아직 판정이 나오지 않은 Human 승인 요청만 남아 있습니다**(최종 승인 아님). |

Run이 `YELLOW`로 끝난 뒤에도 `artifactize review`와 데스크톱 앱에서 대기 중인 승인 요청을 처리할 수 있습니다. 리뷰어가 `GREEN` 판정을 제출하면 해당 승인 요청과 여기에 연결된 다른 요청이 모두 완료됩니다. [재사용 키](reuse.md#the-reuse-key)가 바뀌지 않는 동안에는 다음 `artifactize verify`에서 이 승인을 재사용합니다. 다만 `fingerprint = false`로 설정했다면 다음 Run에서 새 승인을 요청합니다. 같은 워크트리에서 새 Run이 다른 재사용 키로 같은 Human eval을 요청하면 이전 대기 요청은 `ERROR/SUPERSEDED`로 종료됩니다.
