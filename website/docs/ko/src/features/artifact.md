# Artifact {#artifact}

Artifact는 리뷰하는 모든 것입니다. 코드, 문서, 디자인, 이미지가 모두 Artifact가 될 수 있습니다. 폴더나
파일 하나가 정적 TOML `.artf` 파일에 자기 eval을 선언합니다. artifactize는
이 선언들을 읽어 하나의 의존성 그래프로 만듭니다.

## Artifact 그래프 {#artifact-graph}

**Artifact 그래프**는 입력을 그 입력을 쓰는 eval에 연결합니다. `{spec}` 같은
참조를 쓰면 그 Artifact가 eval의 입력에 들어갑니다. 그 Artifact의 fingerprint가 바뀌면
그것을 키에 포함한 리뷰만 무효가 됩니다. 자식 폴더는 입력을 정리해 주지만,
같은 저장소에 있다는 것만으로 모든 Artifact가 모든 검사의 의존성이 되지는 않습니다.

실행 관문은 각 eval 자신의 참조에 속합니다. 자식 폴더는
부모 내용의 일부이므로 부모의 모든 eval이 참조하는 것으로 칩니다.
[eval 의존성](#eval-dependencies)을 보세요.

그래프 자체도 리뷰할 수 있습니다. 이름 없는 루트 `.artf`가 Graph
Artifact를 선언합니다. 그 fingerprint는 소스 바이트나 실행 조정값이 아니라 추출된 기준과 라벨이 붙은 관계를 포함합니다.
프로젝트의 리뷰 구조를 사람이 리뷰할 때 쓰세요. 문법은 [선언](declarations.md#graph-artifact)에 있습니다.
[graph 도구](tools/builtin.md#current-project-graph)는 현재 구조 전체를 에이전트에게 보여 주거나
사람을 위해 데스크톱에서 엽니다.

## 폴더 설정 {#folder-configuration}

`index.artf`가 있는 폴더는 Artifact입니다. `name`은 필수이고, `basis`,
`views`, `fingerprint`, `tags`는 선택입니다. 이전 선언은
[예전 정책 업그레이드 레퍼런스](../getting-started/install.md#removed-gate-bypass-settings)를 보세요.

eval은 로컬 ID를 키로 하는 테이블입니다. `usage` Artifact의 `[evals.heading]`은 정규화된 ID
`usage/heading`이 됩니다. `id` 필드는 선언하지 마세요. 각 eval에는
`title`과, `kind`가 `runtime`, `agent`,
`human` 중 하나인 `profile`이 있습니다. 모든 eval에는 `payload.instruction`이 필요하며,
`pass_schema`, `fail_schema`, `profile_variants`를 둘 수 있습니다.

Runtime eval은 종료 코드가 판정입니다(0이면 GREEN). Agent나 Human은
소유자 필드와 함께 GREEN 또는 RED를 돌려줍니다. 의존성은 각 eval 자신의 참조와
그 Artifact의 자식에서 나옵니다.

다음은 [runtime-relations 예제](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations)의 `guide` Artifact입니다. 이 예제의 README는 자식,
`{artifact}` 참조, basis Artifact를 설명합니다.

```toml
name = "guide"

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{glossary}/terms.txt", "{guide}/intro/page.md", "{usage}/page.md"], timeout_ms = 10000 }
payload.instruction = "Check that every bold term in {intro} and {usage} is defined in {glossary}."
```

선언 키는 snake_case입니다. `minLength`나
`additionalProperties` 같은 JSON Schema 키워드는 표준 이름을 그대로 쓰고, 소유자가 정의한 payload 필드는 정한
이름을 그대로 씁니다. `--json` 출력과 저장된 상태는 여전히 `passSchema`,
`timeoutMs`, `agentTools` 같은 필드를 씁니다. 정확한 문법은 [선언 (.artf)](declarations.md)을 보세요.

## 파일 Artifact {#file-artifacts}

일반 파일 `hero.png` 옆의 사이드카 `hero.png.artf`는 그 파일을
Artifact로 선언합니다. 같은 스키마를 쓰며, 전체에서 고유한 자기 `name`이 필요합니다.

```toml
# hero.png.artf
name = "hero-art"
tags = ["type:image", "scope:player"]

[evals.approved]
title = "The hero sprite matches the style guide"
profile = { kind = "human" }
payload.instruction = "Check {hero-art} against {style-guide}."
```

파일 Artifact는 기본적으로 자기 대상 하나만 해시합니다. 작업 디렉터리는
그 파일이 있는 폴더이지만, 내장 도구는 그 파일과 받아들여진 범위만 보며
관련 없는 형제 파일은 보지 않습니다. 사이드카가 그 파일을 주변 폴더의 자식으로 만들지는 않습니다.
그 폴더는 여전히 자기 파일의 일부로 대상을 해시합니다. 반면 `index.artf`로 선언한
폴더는 그 위에 있는 가장 가까운 Artifact 폴더의 자식입니다.
이름 규칙, fingerprint 형식, 가상 경로, 잘못된 대상과 재검증 규칙은
[파일 Artifact 레퍼런스](declarations.md#file-artifacts)를 보세요.

## Eval 의존성 {#eval-dependencies}

의존성은 Artifact의 모든 eval이 아니라 그 입력을 쓰는 eval에 속합니다.
`payload.instruction`이나 runtime `args` 피연산자 안의 `{name}` 참조가
입력 Artifact를 가리키며, 그 eval은 입력을 판정하는 eval들에 의존합니다.
어떤 Artifact가 다른 Artifact를 쓴다면, 그 사용을 검사하는 eval이 그것을 `{name}`으로 가리킵니다.
그 밖의 어떤 것도 의존성을 추가하지 않습니다.

자식은 어떤 eval도 적지 않는 유일한 연결입니다. 폴더 Artifact의 파일에는 자식
폴더가 포함되므로, 자식은 부모의 모든 eval이 그것을 참조한 것과 똑같이 취급됩니다.
부모의 각 eval은 표시된 가장 가까운 자식들의 eval에 의존합니다.

예를 들어 애플리케이션 E2E 실행은 애플리케이션에 속합니다.

```toml
name = "app"

[evals.e2e]
title = "The application passes its end-to-end suite"
profile = { kind = "runtime", command = "node", args = ["{e2e-tests}/run.mjs", "{app}"] }
payload.instruction = "Run {e2e-tests} against {app}."
```

`e2e-tests` Artifact는 테스트 코드 자체, 즉 그 스타일과
기능 문서에 대한 충실도를 평가합니다. 자기 테스트 코드에 대한 리뷰로 애플리케이션 테스트 모음을
실행하지는 않습니다. 독립적인 `app` 스타일 eval은 자기 지시문이 테스트 코드 리뷰를
참조하지 않는 한 그 리뷰를 기다리지 않습니다.

모든 eval은 입력의 판정과 상관없이 자기 대상을 독립적으로 판정할 수 있습니다.
그냥 `verify`는 효율을 위해 의존성을 먼저 실행하는 스케줄링을 고릅니다. 대기 중인 입력
리뷰는 그것을 쓰는 eval을 늦추고, RED인 입력 리뷰는 그 eval을 막습니다. `--speculate`를 쓰면
해결되지 않은 입력 관문보다 먼저 작업을 시작할 수 있지만, 빠진 요구 사항을 승인하지는 않습니다.
최종 GREEN에는 여전히 상위 리뷰를 포함해 필요한 모든 증거가 있어야 합니다.

막히는 것은 언제나 eval이지 Artifact가 아닙니다. eval의 실행 관문은
그 eval이 참조하는 Artifact들의 실제 eval과, 그 eval이 속한 Artifact의 표시된 가장 가까운 자식들의 실제 eval입니다. RED
또는 BLOCKED인 관문은 차례로 그 eval을 막으므로, 막힘은 이 eval 관문을 따라 전파됩니다.
자식의 RED eval은 부모의 모든 eval을 막습니다. eval이 없는 중간 Artifact는
실행 관문을 제공하지 않습니다. 그 Artifact를 참조해도 그 Artifact의 입력이나 자식에 실행이
묶이지 않습니다. 그것들은 여전히 최종 승인 의무에 속하므로,
그것을 쓰는 eval은 실행되더라도 Run은 GREEN으로 끝나지 못할 수 있습니다.

eval 자신의 Artifact와 의존성 순환에 함께 있는 eval들은
[Human 형제 선행 조건](#human-sign-offs)을 빼면 이 관문에서 제외됩니다. 순환은
eval 참조와 자식을 따라갑니다. 부모를 직접 또는 참조 사슬을 거쳐 참조하는 자식은
부모와 같은 순환에 있습니다. 같은 순환의 상대를 참조하면 그 순환의 외부 관문이 따라오고,
다른 순환을 참조하면 그 순환의 모든 eval이 따라옵니다.
이 관문들은 대기와 추측 실행의 취소도 결정합니다.

예를 들어 `docs`의 eval이 `cli`를 참조하고 `cli`의 eval이 `docs`를 참조하면
둘은 순환을 이룹니다. RED인 `cli/tests`는 `docs/matches-cli`를 막거나
그 판정을 대신하지 않습니다. 둘 다 독립적으로 실행되며, 순환 밖에서 이를 쓰는 eval이
진행하려면 둘 다 통과해야 합니다.

BLOCKED eval의 이유에는 막힘의 근원인 RED eval의 이름이 나옵니다. 예를 들어
`Dependency verdict RED: core/tests`처럼 나오며, 아직 실행 중이었을 뿐인 eval은 나오지 않습니다.
추측 실행 Run에서는 상위의 ERROR도 막힘을 일으키며 `Dependency ERROR: …`로
이름이 나옵니다. `blockedBy`는 같은 eval들을 나열하며, 끝난 Run에는
그 eval이 모두 나옵니다.

### Human 승인 {#human-sign-offs}

Human eval은 의존성 관문에 더해, 같은 Artifact의 Human이 아닌 다른 eval이 모두
GREEN일 때만 시작합니다. 사람은 기계 eval이 이미 통과한 것만 승인합니다.
그때까지 Human eval은 기다립니다. 그 기계 eval 중 하나가 RED나
ERROR이면 Human eval은 시작하지 않고 BLOCKED가 되며, 그 이유에 해당 eval의 이름이 나옵니다. 예를 들어
`Sibling machine eval RED: spec/review`처럼 나옵니다. 같은 Artifact의 다른 Human eval은
이 조건에 들어가지 않으므로, 두 승인이 서로를 기다리는 일은 없습니다.
이 규칙은 그냥 `verify`에서도, `--speculate`나 `--defer-human`을 써도 같습니다.
추측 실행 Run에서도 그 기계 eval이 GREEN이 되기 전에는 사람에게 묻지 않습니다.

## fingerprint 필드 {#the-fingerprint-field}

`fingerprint`를 생략하면 Artifact 자신의 파일에 대한 내장 해시인 **artifactsum**이
쓰입니다. `fingerprint = false`는 재사용을 끄며, 스크립트로 다른 값을 정의할 수도 있습니다.
[Fingerprint](fingerprint.md)에서 형식, 포함되는 입력, GREEN/RED
결과 재사용을 설명합니다.

## 태그와 검증 {#tags-and-validation}

`tags = ["type:code", "scope:player"]`는 표시용 메타데이터만 추가합니다. 태그는 ASCII 제어 문자가 없는,
비어 있지 않은 고유 문자열 64개까지입니다. `status`, `config graph`, `monitor`와 그 JSON
Artifact 출력에 태그가 나옵니다. 태그는 fingerprint, eval 정의 해시, 재사용
키, 관계, `verify`와 `status`의 선택을 전혀 바꾸지 않습니다. 이들에 대한 태그 필터도,
관계 규칙도 없습니다. 태그로 거르는 것은 `artifactize review` 검색의 `tag:` 검색어뿐입니다.

템플릿이 필요한 프로젝트는 자기 `.artf` 파일을 직접 생성해 커밋합니다.
탐색과 검증 규칙은 [선언 검증](declarations.md#declaration-validation)에 있습니다.
