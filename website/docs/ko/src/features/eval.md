# Eval {#eval}

**eval**은 Artifact를 검사하는 단위입니다. 바뀌지 않는 로컬 ID, 통과 조건을 나타내는 제목,
리뷰 주체를 정하는 프로필을 설정하세요. 예를 들어 `library` Artifact의
`[evals.tests]`는 전체 ID가 `library/tests`입니다.

세 종류의 eval은 실행부터 끝날 때까지의 처리 방식과 결과 재사용 규칙이 같습니다.

| 종류 | 리뷰어 | 판정 |
|---|---|---|
| [Agent](agent.md) | 미리 선언된 읽기 전용 도구로 리뷰하는, 선택한 백엔드의 지정 모델 | 스키마로 검사한 소유자 필드와 함께 GREEN 또는 RED |
| [Human](human.md) | 대기 중인 요청을 맡아 승인을 제출하는 사람 | 스키마로 검사한 소유자 필드와 함께 GREEN 또는 RED |
| [Runtime](runtime.md) | 선언한 프로그램과 인자 목록 | 종료 코드 0이면 GREEN, 그 밖이면 RED |

실행에 실패했거나 모델 결과가 잘못되면 RED가 아니라 **ERROR**입니다. RED는 eval의 실제 판정 결과
Artifact가 통과 조건을 충족하지 못했다는 뜻입니다. 어느 종류든
[입력과 기준](fingerprint.md#the-reuse-key)이 그대로인 동안은 재사용할 수 있습니다.

리뷰 기준은 `payload.instruction`에 쓰세요. Agent와 Human eval에서는 `pass_schema`와
`fail_schema`가 소유자의 결과 필드를 정하고, 판정은 artifactize가 덧붙입니다. 추가 필드를 허용하지 않는 결과 형식,
예약된 이름, 스키마를 생략했을 때 판정만 돌려주는 기본 응답은 [소유자 결과 스키마](declarations.md#owner-result-schemas)를
보세요. Runtime eval은 소유자 스키마 대신 명령의 종료 상태를 씁니다. 이름을 붙인
`profile_variants`를 두면 호출하는 쪽이 다른 실행 프로필을 고를 수 있습니다. Agent 변형은 리뷰를 실행하는 방식만 바꾸지만, 명령이나 인자가 다른 Runtime 변형은 별도의 재사용 키와 입력을 쓰는 다른 eval입니다. [선언](declarations.md)을 보세요.

의존성은 해당 입력을 쓰는 eval에만 적용됩니다. 입력 Artifact를 지시문이나 Runtime 인자에 `{name}`으로
적으세요. [eval 의존성](artifact.md#eval-dependencies)을 보세요.

실제 입력만 선언하세요. eval은 대상 Artifact, 지시문이나 Runtime 인자가 가리키는 Artifact, 그리고
그 자식 Artifact를 모두 볼 수 있습니다. 예를 들어 지시문이 `Check {app} against {spec}.`인
`app/review`는 `app`과 `spec`을 읽고 그 도구를 쓸 수 있습니다. `ui`가 `app`의 자식이라면
`app/ui`도 마찬가지입니다. 하지만 `spec`의 eval이 참조하는 다른 Artifact에는 접근할 수 없습니다. 도구 인자에 Artifact를 적는다고
입력이 늘어나지는 않습니다.

eval이 읽을 수 있는 범위는 재사용 여부를 판단하는 범위나, 실행 전에 결과가 나와야 하는
Artifact의 범위보다 넓을 수 있습니다. 재사용 여부는 [재사용 키](fingerprint.md#the-reuse-key)를, 실행
전에 결과가 나와야 하는 Artifact는 [eval 의존성](artifact.md#eval-dependencies)을 보세요.

`artifactize verify library/tests --wait`는 eval 하나로 [Run](run.md)을 시작합니다. 먼저
`artifactize status library/tests`로 무엇을 재사용할 수 있고 무엇을 리뷰해야 하는지 확인하세요.
status는 fingerprint를 계산하지만 eval을 실행하지는 않습니다.
