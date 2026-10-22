# 평가 Run {#evaluation-run}

`verify`는 선택한 eval을 리뷰하고 Run을 기록합니다. `status`는 eval을 실행하지 않고,
다음 `verify`가 무엇을 실행하고, 재사용하고, 기다리고, 막을지 예측합니다.
`status`는 선언된 fingerprint 스크립트를 실행할 수 있습니다.

## Eval 실행 관문 {#eval-execution-gates}

`verify`는 각 eval을 그 eval의 [실행 관문](artifact.md#eval-dependencies)에 따라 스케줄하며,
[Human 승인의 기계 선행 조건](artifact.md#human-sign-offs)도 포함합니다.
`status`는 eval을 실행하지 않고 그 관문을 보여 줍니다.

## Verify와 Run {#verify-and-runs}

`verify`는 정확히 하나의 [선택자](../cli/index.md#command-reference)를 받습니다. Artifact,
정규화된 eval(`green/check`), 또는 `--all`입니다. Artifact 선택자는 그 조각의 검사를 실행하라는 요청이고, 개별 eval 선택자는
그 검사 하나만 실행하라는 요청입니다. 그래도 Run이 통과하려면 선택한 검사가 직접 또는 간접적으로 의존하는 모든 Artifact의 유효한 결과가 필요합니다. 이렇게 연결된 묶음이
**의존성 범위(dependency closure)** 이고, 그 범위에 필요한 결과가 **최종 의무**입니다.
이번 Run에서 실행하라고 요청하지 않은 것이라도 마찬가지입니다.

`--recursive`는 선택한 Artifact의 다른 검사를 포함해 그 묶음의 모든 검사도 설정 순서대로
선택합니다. 서로 순환으로 의존하는 조각은 **순환 상대(cycle peer)** 이며,
함께 포함됩니다. `--all`은 이미 모든 검사와, 검사가 없는 조각의 의무까지 선택합니다.
파일 Artifact 선택자는 자기 eval만 선택하며, 주변 폴더의 eval은 결코 선택하지 않습니다.

예를 들어 `code/tests`가 `plan`에 기대고, `plan`에는 사람의
`plan/signoff`가 필요하다고 합시다. `verify --eval code/tests`는 테스트를 선택하지만,
여전히 유효한 그 승인이 없으면 GREEN으로 끝날 수 없습니다. `verify --eval code/tests --recursive`는
`code/style`, `plan/signoff` 같은 그 묶음의 다른 검사도 선택하므로, 그 검사들이
실행되거나 결과를 재사용할 수 있습니다. 검사를 더 선택한다고 승인 자체가 생기지는 않습니다.


READY인 eval은 모두 준비되는 즉시 시작하며, **기본 동시 실행 상한은 없습니다**.
동시 실행이 문제를 일으킬 때만 `--jobs N`(최소 1)을 쓰세요. 스케줄링은
막힐 일이 없는 작업과 남은 의존성 경로가 가장 긴 작업을 우선하며, 우선순위가 같으면 선택 순서와 설정 순서를 따릅니다. 새 READY 작업은 묶음 전체가 끝난 뒤가 아니라 결과가 하나 나올 때마다 시작합니다.
fingerprint 점유를 기다리는 작업은 명시적인 jobs 상한에 포함되지만, 캐시 적중은
포함되지 않습니다. Agent 리뷰에는 [백엔드의 슬롯](others/state.md#backend-capacity)도 필요합니다.
QUEUED는 실행 허가 대기, 백엔드 슬롯 대기, 합류한 실행 대기, 또는
명시적인 Run jobs 상한을 뜻할 수 있습니다. 백엔드 슬롯 대기는 그 jobs 상한을 쓰지 않습니다.

### 제출, 지켜보기, 취소 {#submit-observe-and-cancel}

Artifactize는 요청한 쪽과 상관없이 Run을 관리합니다. 그냥 `verify`는 제출하고,
Run ID(`--json`이면 `{"run","state"}`)를 출력한 뒤 승인이 아니라 **접수**되면
0으로 종료합니다. `verify --wait`는 ID를 stderr에 알리고, 확정될 때까지 지켜본 뒤
저장된 결과를 보여 줍니다. 지켜보던 명령을 중단하거나 터미널을 닫아도 지켜보기만 멈추고, Run은 계속 실행됩니다. 취소를 명시적으로 요청하는 것은 `run cancel RUN_ID`뿐입니다.
이 명령은 새 실행 허가와 소유한 활성 작업을 멈추고, 정리를 기다린 뒤, 이미 확정된 증거는 지우지 않고
ERROR/CANCELLED를 기록합니다. 이 Run이 다른 Run에서 합류한 실행은 죽이지 않습니다.
미룬 승인을 포함해 아직 기다리던 승인도 ERROR로 끝나며, 다른 Run이 그 승인에
합류한 요청도 마찬가지입니다. 다음 `verify`가 다시 요청합니다. 이는 Run이 실행 중일 때만 적용됩니다.
이미 SIGNOFFS_PENDING으로 끝난 Run은 완료된 것이므로, 취소해도 아무것도 바뀌지 않고 미룬 승인 요청은 계속 대기 상태로 남습니다. 비정상 종료된 작업자는 ERROR/OWNER_DIED로 정리되며,
중단된 작업은 자동으로 다시 실행되지 않습니다.

접수할 때 선택된 범위에 대해 해석된 정의를 저장합니다. 실행 전에
Artifactize는 그 정의를 두 번 확인합니다. fingerprint를 준비하기 전에 한 번,
그리고 fingerprint와 원격 증거를 준비한 뒤 스케줄링 경계에서 다시 한 번입니다.
접수 이후 관련 선언이 바뀌었다면, 다른 기준이나 프로필로 몰래 실행하지 않고
Run은 **ERROR/INPUT_CHANGED**로 끝납니다. 선언 편집을 마친 뒤
새 Run을 제출하세요. 접수된 Run의 대상이 바뀌지는 않습니다.

엄격 모드에서 Run은 다른 리뷰처럼 WAITING_HUMAN 요청을 기다립니다.
제출하면 같은 Run 안에서 의존하는 eval의 막힘이 풀립니다. Human 대기 마감 시간이나
`verify --timeout-ms`는 없으며, 모든 eval kind가 같은 확정 규칙을 공유합니다. 아래의 선택
모드는 실행 순서를 바꾸거나 승인을 미룰 뿐, Human 권한을 바꾸지는 않습니다.

지켜보기는 Run의 결과로 종료하며, 그 코드는 [CLI 레퍼런스](../cli/index.md#command-reference)에
나와 있습니다.

최종 검증에는 필요한 범위 전체에 걸쳐 실제 GREEN 증거나 명시적인 `basis = true`가
필요합니다. basis가 그 의존성까지 면제하지는 않습니다. RED이면 하위 작업을 막고, 증거가 없거나 운영 오류로 얻지 못했으면 하위 작업을 대기 상태로 둡니다. 선택된 GREEN 리뷰에 충족되지 않은
의무가 남으면 INCOMPLETE가 되며, 그 의무는 텍스트와 JSON에 나열됩니다.
제거된 예전 관문 우회 설정은
[업그레이드 레퍼런스](../getting-started/install.md#removed-gate-bypass-settings)에서 다룹니다. `--reuse-only KINDS`
(예: CI의 `agent,human`)는 그 kind에 대해서는 결과를 재사용하기만 합니다. 재사용할 수 있는
증거가 없으면 실행하지 않고 INCOMPLETE로 남습니다. [CLI](../cli/index.md#reuse-only)를 보세요.

[`--request-key`](../cli/index.md#submission-and-watching)는 호출자의 시도에 이름을 붙입니다.
같은 키를 반복하면 같은 Run이 돌아오므로, 잃어버린 답을 다시 받을 때는 같은 키를 쓰고
수정한 뒤에는 새 키를 쓰세요. eval 결과 재사용은 이 키와 무관합니다.

### 추측 실행과 미룬 승인 {#speculation-and-deferred-sign-offs}

두 가지 [요청별 verify 모드](../cli/index.md#verify-modes)인
`--speculate`와 `--defer-human`은 기본으로 꺼져 있으며 Run의 `modes`에 저장됩니다.
같은 요청 키는 원래의 선택을 유지합니다.

추측 실행은 요청된 runtime, Agent, Human eval을 해결되지 않은
관문보다 먼저 `--jobs`, 백엔드 용량, `--max-executions` 한도 안에서 시작합니다. Human eval은
그 Artifact의 Human이 아닌 다른 eval이 모두 GREEN이 되기 전에는 결코 요청되지 않습니다. 준비된 작업이
우선하며, 추측 작업은 남는 용량을 쓰고 가장 먼저 자리를 내줍니다. 작업을 다시 배정할 때도
실행기 시작 예산을 1회 씁니다. 상위의 RED나 ERROR는
그 때문에 무효가 되는 작업, 즉 자기
Artifact와 순환 밖에서 그것에 의존하는 eval만 즉시 취소하고, 재사용할 수 있는 취소 결과 없이 BLOCKED로 표시합니다.
GREEN이면 같은 파일에 대해 이미 만든 결과를 유지합니다. 결과는 스케줄링이 끝난 뒤에만,
그리고 끝까지 유효한 결과만 [팀 리뷰 저장소](team/reference.md)와 공유됩니다.

미루기는 대기 중인 Human 관문을 지나 실행하게 해 줍니다. 미룰 수 있는 승인의
요청은 가능한 한 늦게, 그 Artifact의 다른 기계 eval이
GREEN이 되고 어떤 관문도 막지 않을 때 발행됩니다. 요청은 DEFERRED로, 의존하는 결과는 PROVISIONAL로 표시됩니다.
어느 상태도 실제 승인이 아닙니다. `gate = "blocking"`은 엄격한 Human
관문을 유지합니다. 다른 의무가 모두 통과하면 Run은 SIGNOFFS_PENDING(종료 코드 5)으로 끝나며,
이는 **승인만 남았다**는 뜻입니다. Run은 그 제출을 기다리지 않습니다.
이후의 제출은 과거 Run의 판정이 아니라 요청을 갱신합니다. 새 verify는
바뀌지 않은 잠정 기계 결과를 재사용합니다. GREEN 승인이면 SATISFIED가 될 수 있고,
RED 승인이면 의존하는 eval이 막힙니다. `--reuse-only agent,human`은 대기 중인
승인을 결코 GREEN으로 취급하지 않으므로 CI에 계속 적합합니다.

더 새 Run이 같은 워크트리에서 같은 eval에 대해 Human 요청을 발행하면,
재사용 키가 다른 이전 DEFERRED 승인은 그에 합류한 요청을 포함해 ERROR/SUPERSEDED로 끝납니다.
맡은 승인도 만료됩니다. 만료된 승인은
Review에서 사라지고 다시 맡을 수 없습니다. 같은 키라면 여전히 기존 리뷰에 합류합니다.
다른 워크트리의 원래 승인과 아직 끝나지 않은 WAITING_HUMAN 원본은
그대로 둡니다. 만료되기 전까지는 Run이 끝난 뒤에도 미룬 승인을 제출할 수 있습니다.
제출할 때 fingerprint를 다시 확인하며 파일이 바뀌었으면 거부합니다.

`status`는 현재 미룬 대기와 잠정 결과에 라벨을 붙이지만, 실행
예측은 엄격하게 유지합니다. 새 기본 verify는 여전히 Human 증거를 기다립니다.
Run/결과를 읽는 쪽은 SIGNOFFS_PENDING을 종료 상태이면서 충족되지 않은 상태로 보여 줍니다.

텍스트 출력은 요청마다 한 줄을 출력하며, 다른
요청의 실행에서 가져온 결과(fingerprint 적중 또는 합류한 실행)에는 그 출처 Run을 표시합니다
([원격 리뷰 저장소](team/reference.md#remote-review-store-client)에서 온 결과는
만든 사람이나 Human 리뷰어도 나옵니다).

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed`는 ERROR 결과와 기록한 Human
요청을 포함해 Run의 실행을 세고, `reused`는 캐시, 합류, 원격 결과를 셉니다. 다른 프로필이 만든 재사용 결과
(다른 모델, 제한, 그 밖의 설정. 모두 재사용 키에 들어가지 않음)는 그 줄에 `profile NAME`을 덧붙이며,
그러면 요약은 `; N produced by another profile`로 끝납니다(JSON에서는 `summary.reused.otherProfile`).
설정이 동일한 변형에서 온 결과에는 표시가 붙지 않으며, 저장된
`options.variant`가 그 변형을 가리킵니다.
`Usage`(사용량이 보고되었을 때만 출력)는 이번 Run에서 쓴 카운터와
재사용된 실행들의 원래 카운터를 합한 `saved`를 구분합니다. JSON
verify와 `run show`는 같은 숫자를 `summary.executed`, `summary.reused`,
Run 수준의 `usage: {spent, saved}`로 담습니다. `summary.usage`는 `usage.spent`와 같게 유지됩니다.

JSON에서 각 요청의 `source`는 그 결과가 어디서 왔는지 말해 줍니다. 요청이
스스로 실행했으면(또는 아직 결과가 없으면) null이고, 그렇지 않으면 캐시, 합류, 원격 재사용에서
결과를 만든 실행의 요청을 가리킵니다.

```json
"source": {"runId": "run-Ksl1Qr", "requestId": "run-Ksl1Qr-3", "kind": "cache"}
```

`kind`는 재사용 키의 완료된 기록이면 `cache`, 요청이 기다린
그 키로 진행 중인 실행이면(다른 `verify`에서, 또는 같은 Run의
형제 eval) `joined`, [원격 리뷰 저장소](team/reference.md#remote-review-store-client)의 기록이면 `remote`입니다.
`run show`, `request show`, `request list --json`도
`source`를 담습니다.

`verify --wait --json`과 `run show RUN_ID`는 저장된 Run 전체를 출력합니다. `run show`는 결코
선언을 탐색하거나 코드를 실행하지 않습니다. 각 Run은 첫 요청과 함께 그 선택, 정책, 프로필 옵션,
의존성 범위의 해석된 정의를 저장하므로, 현재 선언에 결코
의존하지 않습니다.

부분 Run은 실행 선택 밖의 상위 eval처럼, 요청을 만들지 않은 eval에 대해서도
저장된 증거를 쓸 수 있습니다. Run은
그 판정을 정규화된 eval ID에서 저장된 상태로 가는 맵인 `evidence`에 기록합니다.

```json
{"evidence": {"code-style/approved": "GREEN"}}
```

이는 최신 결과가 아니라 이 Run이 쓴 증거입니다. 그 JSON 형태는
[CLI 레퍼런스](../cli/index.md#saved-run-evidence-and-terminal-views)에 있습니다. 저장된 정의,
`evidence`, 요청 상태 덕분에 monitor는 현재 선언 없이 관문을 판단할 수 있습니다.

`run list [--repo-only | --all]`은 저장된 Run을 최신순으로 텍스트 표로 나열하거나,
`--json`이면 JSON 배열로 나열합니다. 기본값은 정규화된 `--repo` 경로입니다(생략하면
현재 디렉터리). `--repo-only`는 그 기본값을 명시하고,
`--all`은 공유 상태의 모든 저장소를 포함합니다. `--limit N`(기본 50)과
`--offset N`(기본 0)으로 걸러진 결과를 페이지 단위로 넘깁니다. 각 행에는 Run
ID, 저장소, 생성/완료 시각, 상태, 판정/상태별 요청 수가 들어 있습니다(없는 상태는 요청 0개).
읽을 수 없는 요청은 세지 않으며,
`unreadable`이 손상된 기록과 그 이유를 알려 주고 올바른 증거는 계속
쓸 수 있습니다. [상태 문제 해결](others/state.md#unreadable-records)을 보세요. 읽기는 소유자 코드를 실행하거나,
없는 상태를 만들거나, 저장소가 여전히 존재할 것을 요구하지 않습니다.
별도의 `history` 명령은 없습니다.

리뷰 정책, `--force`, `--max-executions`, 선택 파일, `--profile`
변형은 [레퍼런스](../cli/index.md#runtime-cli)에 있으며,
[명령 표](../cli/index.md#command-reference)와
[상태 구조](others/state.md#state)도 함께 보세요.

## Status와 정적 그래프 {#status-and-static-graph}

```sh
artifactize --repo PROJECT status
artifactize --repo PROJECT status ARTIFACT --recursive --json
artifactize --repo PROJECT config graph ARTIFACT --json
artifactize --repo PROJECT config check --json
```

`status`는 `verify`와 같은 배타적 선택자를 받으며 기본값은
`--all`이고, 같은 `--profile`, `--recursive`, `--force` 정책
옵션을 받습니다. 현재 Artifact/eval 상태, 충족되지 않은 최종 의무,
그리고 `execute` / `reuse` / `wait` / `blocked` 동작을 보고합니다. 필요한
범위의 모든 eval이 표시되며, `selected`와 `included`가 명시적인
선택과 포함된 실행(재귀적 의존성)을 구분합니다.
JSON에서는 각 eval의 [재사용 키](fingerprint.md#the-reuse-key)가 무엇으로 이루어졌는지도 보여 줍니다. `evalDefHash`,
대상의 `fingerprint`, `fingerprints`(eval이 의존하는 각 Artifact와 그
현재 fingerprint, 없으면 null), 그리고 조합된 `key`(fingerprint가
꺼져 있으면 null)이며, `verify`가 Run의 키로 쓸 값과 같습니다.
동작 개수는 포함된 eval만 셉니다. 종료 코드 0은 현재
검증이 충족되었다는 뜻이고, 1은 의무가 남았다는 뜻이며, 잘못된 입력이나 상태 오류는
2로 종료합니다.

status는 선택된 필요 범위에서 실행형 eval에 필요한 fingerprint를 verify와 같은
격리, 검증, `--fingerprint-jobs` 한도로 준비합니다.
fingerprint 실패는 2로 종료하며, 저장된 예전 fingerprint로 대체하지 않습니다.
도구나 eval 명령을 실행하거나, Run을 만들거나, 작업을 예약하거나, 없는 데이터베이스를 만들거나,
캐시 접근 시각을 갱신하는 일은 결코 없습니다. fingerprint 스크립트는
상태 디렉터리 아래의 일회용 출력을 쓰며, 이 상태 디렉터리는 데이터베이스가 없어도 만들어질 수 있습니다. `config graph`와
`config check`는 완전히 정적이며 소유자 코드를 결코 실행하지 않습니다.

eval의 현재 재사용 키에 대한 완료된 기록이 있으면 PASS 또는 RED와
`reuse` 동작이 나옵니다. 그 이유에 결과를 재사용한다고 나오며, 요청한 프로필과 다른 프로필이 만든 결과라면 그 프로필도 함께 나옵니다. verify는 관문이 해결되기 전에 캐시된 결과를 붙이므로,
의존성이 대기 중이거나 RED여도 동작은 `reuse`로 남습니다. 그러면
상태는 관문(WAIT_DEPENDENCY 또는 BLOCKED)을 보여 주고 이유가 그 관문을 가리킵니다.
캐시된 결과가 없는 eval은 관문이 GREEN이 되어야만 실행됩니다. RED 의존성 뒤에서는 `blocked`로,
의존성이 캐시된 결과로 GREEN이면 `execute`로 보입니다. 의존성의 결과가 직접 또는 더
상위에서, verify가 실행한 뒤에야(또는 진행 중인 실행이 끝난 뒤에야) 생긴다면 `wait`로 보입니다.
이것이 예측의 한계입니다. status는 `wait` eval이
실행될지 말할 수 없습니다. 텍스트는 예를 들어 `Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`으로 끝나므로, 병합된
체크아웃에서 status를 실행하면 verify가 그곳에서 무엇을 다시 리뷰할지 실행하지 않고도 알 수 있습니다. force는 여전히
선택된 실행형 eval에만 적용됩니다. Human 실행 동작은 대기 중인
요청을 기록합니다. 같은 키의 활성 Human 실행이 있으면 원래 verify가 종료한 뒤에도
WAITING_HUMAN과 `wait` 동작이 나타납니다. 저장된 시도는
이 정규화된 저장소에 대해서만 읽습니다. 각 eval의 선택적인 `last: {runId, verdict, fingerprint?}`는 과거 기록이지
현재 증거가 아닙니다. 전체 출처는 `run show RUN_ID`를 쓰세요. 캐시되지 않은 GREEN/RED는
자기 Run만 충족하므로, 이후 status는 재사용이 아니라 STALE이 됩니다(ENG-24).
basis만 있는 범위는 충족될 수 있습니다. 충족되지 않은 의존성이 있는 basis는 INCOMPLETE입니다.

재사용 키가 있는 eval에 현재 캐시된 결과가 없으면 `status`가 그 이유를 설명합니다.
현재 키를 같은 eval과 eval 정의 해시에 대한(이 상태의 어느 저장소에서든)
가장 새 캐시된 GREEN/RED 기록과 비교해,
`changes: {sinceRunId, files?, dependencies?, summary}`를 보고합니다. 텍스트에서는 `Fingerprint changed since Run RUN_ID: ...` 줄로 나오며, 예를 들어 `changed: +docs/new.md, -old.md, src/a.py; dependency core changed`입니다. `files`는
대상의 파일을 `path`(바뀜), `+path`(추가됨), `-path`(제거됨)로
나열하고, `dependencies`는 fingerprint가 바뀐 자식과 이름으로 가리킨 Artifact를
`name`, `+name`, `-name`으로 나열합니다. 스크립트
fingerprint나 파일 맵이 빠진 매니페스트는 대상에 대해 `fingerprint changed`나
`inputs changed`만 보고할 수 있습니다. 그 eval 정의가 한 번도 캐시된 적이 없거나
강제된 eval이면 설명이 나오지 않습니다. 키가 없는, 강제하지 않은 READY Runtime/Agent eval은
실행 이유에서 `fingerprint = false`를 선언한 대상이나 의존성을 알려 줍니다.
READY Human eval은 키가 없어도 대신 "Record a request awaiting
a Human claim and submission"이라고 말하며, 막히거나 기다리는 eval은
대신 그 관문이나 대기 상태를 설명합니다.

기다리는 eval에서 `blockedBy`는
충족되지 않은 정규화된 eval ID를 가리키고, BLOCKED eval에서는 그 이유와 마찬가지로
막힘의 근원인 RED eval을 가리킵니다. Detail은 이 관문을 입력 Artifact별로 묶어
`Waits for` 아래에 보여 주며, `w`는 대기 중인 eval을 보여 줍니다.

나중의 Human 제출이 기다리던 Run을 확정할 수 있습니다. `run show`는 저장된
검증과 요청을 유지합니다.
monitor는 저장된 정의와 증거에, 이후의 Human 제출을 포함한 지금 시점의 요청
상태를 더해 트리를 만듭니다. 머리말은
**Run 종료 시점의 SATISFIED / NOT SATISFIED**를 유지하며, 흐린 `*`는 파일이 바뀐 것이 아니라
그 종료 이후 파생 상태가 바뀐 행을 표시합니다. [Monitor](../tui/index.md#monitor)를 보세요.

`config graph [ARTIFACT]`는 기본으로 프로젝트 전체를 보여 주거나, 순환 상대를 포함해
선택된 필요 범위를 보여 줍니다. `--impact`는 하위 소비자와 그 필요
입력을 포함합니다. 이는 정확한 재사용 키 예측이 아니라 보수적인 구조적 도달 범위입니다.
MCP `config_graph`는 같은 정의를 Artifact 또는 eval 하나나 여러 개의 범위로
보여 줍니다. 실제 실행/재사용 결정에는 `status`를 쓰세요. 텍스트는 태그와 파일 kind 표시가 있는 Artifact, eval, 구성 요소,
입력→소비자 관계를 나열합니다. 전체 JSON에는 해석된 정적 Artifact/eval
정의(프로필, payload, 스키마, 도구 뷰), 태그,
자식/지시문/argv 관계 메타데이터, 순환 표시, 그리고
의존성이 먼저 오는 순서의 강한 연결 요소(SCC), 즉 서로 의존하는
Artifact 묶음이 들어 있습니다. Artifact 항목에는 `kind`(`file`, `folder`, `graph`)와
워크스페이스 기준 상대 대상 `path`가 붙습니다. status도 파일 kind/경로를 보여 주며, monitor는
트리 행에 `[file]`을 쓰고 대상 경로는 Detail에 둡니다.
구성 요소 ID는 전체 그래프를 기준으로 하므로 선택된 투영에서는
연속적이지 않을 수 있습니다. `config check`는 정적 유효성
확인과 JSON Artifact/eval 개수를 유지합니다. 출력 수준은 텍스트 또는 전체 JSON 하나뿐이며,
`plan`, `--compact`, `--full`은 없습니다. 예전 최상위 `graph`
명령은 도움말에서 숨겨져 있으며, `graph moved to "artifactize config graph"`를 출력하고 2로 종료합니다.

### 현재 그래프 리뷰 도구 {#current-graph-review-tools}

`config graph`는 여전히 설정 검사 도구입니다. 리뷰에는
어떤 Artifact에든 [내장 `graph` 도구](tools/builtin.md#current-project-graph)를 선언하세요.
Agent는 추출된 현재 그래프를 트리로 보고, Human은
저장된 Run 없이도 브랜치의 [Current](../desktop/index.md#current)에서 데스크톱 앱을 엽니다.
선택적인 루트 [`.artf` graph Artifact](declarations.md#graph-artifact)는
일반 eval을 가지며, 다른 Artifact처럼 Run과 status에 나타납니다.
다른 것은 fingerprint 알고리즘뿐이며, 그래프 전용 검증
흐름이나 루트 폴더 Artifact와의 자동 관계는 없습니다.
