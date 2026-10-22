# Eval {#eval}

**eval**은 Artifact에 대한 검사 하나입니다. eval에는 바뀌지 않는 로컬 ID, 통과 기준을
나타내는 제목, 그리고 누가 또는 무엇이 리뷰하는지 정하는 프로필을 지정하세요. 예를 들어
`library` Artifact의 `[evals.tests]`는 전체 ID가 `library/tests`입니다.

실행되는 세 kind는 같은 수명 주기와 재사용 규칙을 공유합니다.

| Kind | 리뷰어 | 판정 |
|---|---|---|
| [Agent](agent.md) | 선택한 백엔드를 통한 정확한 모델. 선언한 읽기 전용 도구를 씀 | 스키마로 검사한 소유자 필드와 함께 GREEN 또는 RED |
| [Human](human.md) | 대기 중인 요청을 맡아 승인을 제출하는 사람 | 스키마로 검사한 소유자 필드와 함께 GREEN 또는 RED |
| [Runtime](runtime.md) | 선언한 프로그램과 인자 벡터 | 종료 코드 0이면 GREEN, 다른 종료 코드면 RED |

호출에 실패하거나 모델 결과가 잘못되면 RED가 아니라 **ERROR**입니다. RED는
검사가 실제로 Artifact를 판정해 거부했다는 뜻입니다. 모든 kind는
[입력과 기준](fingerprint.md#the-reuse-key)이 여전히 일치하는 동안 재사용될 수 있습니다.

리뷰 기준은 `payload.instruction`에 쓰세요. Agent와 Human eval에서
`pass_schema`와 `fail_schema`는 소유자의 결과 필드를 기술하고, artifactize가
판정을 덧붙입니다. 선언한 필드만 허용하는 결과 객체, 예약된 이름, 판정만 있는 기본값은
[소유자 결과 스키마](declarations.md#owner-result-schemas)를 보세요. Runtime
eval은 소유자 스키마 대신 명령의 종료 상태를 씁니다. 이름 붙은 `profile_variants`를 쓰면 호출자가 판단을 고치지 않고
다른 실행 프로필을 고를 수 있습니다. [선언](declarations.md)을 보세요.

의존성은 그것을 쓰는 eval에 속합니다. 지시문이나 runtime 인자에서
입력 Artifact를 `{name}`으로 가리키세요. [eval 의존성](artifact.md#eval-dependencies)을 보세요.

실제 입력만 선언하세요. eval의 관찰 범위에는 대상 Artifact, 지시문이나 Runtime 피연산자가 명시적으로 참조한 Artifact, 그리고 이들의 모든 자식 Artifact가 들어갑니다. 참조된
Artifact의 eval 지시문을 따라가 의존성을 더 늘리지는 않습니다. eval 범위에 들어간 모든 Artifact는
도구를 제공할 수 있습니다. 도구 피연산자 자체로는 새 입력이 추가되지 않습니다.

관찰할 수 있는 범위는 [재사용 키](fingerprint.md#the-reuse-key)보다 넓습니다.
재사용 키는 대상, 그 직접 자식, 이 eval의 명시적인
입력을 포함할 뿐, 재귀적으로 닿을 수 있는 모든 관찰 대상을 포함하지는 않습니다. 실행 관문은 이 eval의 참조와 대상의 자식을 따르며, 형제 eval의 참조는 따르지 않습니다.
자기 Artifact와, 그것과 순환을 이루는 Artifact는 결코 관문이 되지 않습니다. RED인 형제나 순환 상대가 이 eval을 막지는 않습니다. 다만 [Human eval](artifact.md#human-sign-offs)은 자기 Artifact의 기계 eval을 기다립니다. 선언은 [Artifact](artifact.md)를 보세요.

`artifactize verify library/tests --wait`는 eval 하나에 대한 [Run](run.md)을 시작합니다.
먼저 `artifactize status library/tests`로 무엇을 재사용할 수 있고 무엇을 리뷰해야 하는지 보세요.
status는 fingerprint를 계산하지만 eval을 실행하지는 않습니다.
