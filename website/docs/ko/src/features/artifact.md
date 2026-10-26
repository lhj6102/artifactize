# Artifact {#artifact}

리뷰할 대상이라면 무엇이든 Artifact로 선언할 수 있습니다. 코드, 문서, 디자인, 이미지 모두
Artifact입니다. 폴더에 `index.artf`를 추가하면(파일이라면 옆에 `notes.md.artf`를 두면)
Artifact를 선언할 수 있습니다.

```toml
name = "docs"

[evals.links]
title = "Links resolve"
profile = { kind = "runtime", command = "python3", args = ["check_links.py"] }
payload.instruction = "Check that every link in the docs resolves."
```

그다음 `artifactize config check`를 실행하면 `Folder configuration is valid.`가 나오거나,
오류가 나옵니다. 선언 내용이 잘못되었으면 파일 이름과 줄·열 번호가 나오고, 대상 파일이 없는 등의 문제라면 해당 경로가 나옵니다. artifactize는 이 선언들을 읽어 의존성 그래프 하나로
만듭니다.

## Artifact Graph {#artifact-graph}

**Artifact Graph**는 입력과 그 입력을 쓰는 eval을 연결합니다. eval에 `{spec}` 같은 참조를
쓰면 그 eval이 `spec`을 읽을 수 있습니다. `spec`이 바뀌면 `spec`을 참조한 eval만 다시 리뷰하고,
나머지 결과는 그대로 재사용합니다. 자식 폴더로 입력을 정리할 수 있지만, 같은 저장소에 있다는
이유만으로 모든 Artifact가 모든 eval의 입력이 되지는 않습니다.

각 eval은 참조한 입력을 판정하는 eval의 결과가 나올 때까지 실행되지 않습니다. [eval 의존성](#eval-dependencies)을 보세요.

Artifact Graph 자체도 리뷰할 수 있습니다. 저장소 루트의 이름 없는 `.artf`가 그래프 Artifact를
선언합니다. 그 fingerprint는 소스 내용이나 실행 설정이 아니라, 추출한 기준과 이름이 붙은
관계를 포함합니다. 프로젝트의 리뷰 구조를 사람이 리뷰할 때 쓰세요. 문법은
[선언](declarations.md#graph-artifact)에 있고, [graph 도구](tools/builtin.md#current-project-graph)는
Agent에게는 현재 프로젝트 구조를 텍스트로 보여 주고, 사람에게는 데스크톱 그래프 뷰어를 엽니다.

## 폴더 설정 {#folder-configuration}

`index.artf`가 있는 폴더는 Artifact입니다. `name`은 필수이고, `combat::Code`처럼 앞에
space를 붙여 다른 Artifact와 묶을 수 있습니다([이름](declarations.md) 참고). `basis`, `views`,
`fingerprint`, `tags`는 선택입니다. 예전 선언은
[예전 정책 업그레이드 안내](../getting-started/update.md#upgrading-from-011)를 보세요.

eval은 로컬 ID를 키로 쓰는 테이블입니다. `usage` Artifact의 `[evals.heading]`은 eval ID
`usage/heading`이 됩니다. `id` 필드는 선언하지 마세요. eval마다 `title`과, `kind`가 `runtime`,
`agent`, `human` 중 하나인 `profile`이 있습니다. 모든 eval에는 `payload.instruction`이 필요하며,
`pass_schema`, `fail_schema`, `profile_variants`를 둘 수 있습니다.

Runtime eval은 프로그램의 종료 코드로 통과 여부를 판정하며, 0이면 GREEN입니다. Agent eval과 Human eval은 소유자 필드를 포함한 GREEN 또는 RED 판정을 돌려줍니다.

다음은 [runtime-relations 예제](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations)의
`guide` Artifact이며, 그 README에서 자식, `{artifact}` 참조, basis Artifact를 설명합니다.

```toml
name = "guide"

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{glossary}/terms.txt", "{guide}/intro/page.md", "{usage}/page.md"], timeout_ms = 10000 }
payload.instruction = "Check that every bold term in {intro} and {usage} is defined in {glossary}."
```

선언 키는 snake_case입니다. `minLength`, `additionalProperties` 같은 JSON Schema 키워드는 표준
이름을 그대로 쓰고, 소유자가 정한 payload 필드는 정한 이름을 그대로 씁니다. 다만 `--json` 출력과
저장된 상태에는 지금도 `passSchema`, `timeoutMs`, `agentTools` 같은 필드가 쓰입니다. 정확한 문법은
[선언(.artf)](declarations.md)을 보세요.

## 파일 Artifact {#file-artifacts}

일반 파일 `hero.png` 옆에 사이드카 `hero.png.artf`를 두면 그 파일을 Artifact로 선언합니다.
폴더 Artifact와 같은 스키마를 쓰며, 프로젝트 전체에서 겹치지 않는 `name`을 지정해야 합니다.

```toml
# hero.png.artf
name = "hero-art"
tags = ["type:image", "scope:player"]

[evals.approved]
title = "The hero sprite matches the style guide"
profile = { kind = "human" }
payload.instruction = "Check {hero-art} against {style-guide}."
```

파일 Artifact는 기본으로 대상 파일 하나만 해시합니다. 작업 디렉터리는 그 파일이 들어 있는
폴더이지만, 내장 도구로는 대상 파일과 허용 범위에 포함된 파일에만 접근할 수 있습니다. 사이드카를
추가해도 그 파일 Artifact가 상위 폴더 Artifact의 자식이 되지는 않으며, 상위 폴더의 해시에는 대상
파일이 계속 들어갑니다. 반면 `index.artf`로 선언한 폴더는 그 위에서 가장 가까운 Artifact 폴더의 자식입니다.
이름 짓기, fingerprint 형식, 가상 경로, 잘못된 대상과 재확인 규칙은
[파일 Artifact 레퍼런스](declarations.md#file-artifacts)에 있습니다.

## eval 의존성 {#eval-dependencies}

의존성은 Artifact의 모든 eval에 일괄 적용되지 않고, 해당 입력을 쓰는 eval에만 적용됩니다. `payload.instruction`이나
Runtime `args` 인자에 있는 `{name}` 참조가 입력 Artifact를 가리키며, 그 eval은 입력을 판단하는
eval에 의존합니다. 한 Artifact가 다른 Artifact를 쓴다면, 그 부분을 검사하는 eval에서 그 Artifact를 `{name}`으로
참조하세요. 그 밖의 방식으로는 의존성이 추가되지 않습니다.

자식 Artifact와의 연결만은 eval에 적지 않아도 생깁니다. 폴더 Artifact에는 자식 폴더의 내용도 포함되므로,
부모 Artifact의 모든 eval은 자식 Artifact에 의존합니다. 정확히는 부모의 각 eval이 Artifact로 선언된 가장
가까운 하위 폴더의 eval에 의존합니다.

예를 들어 애플리케이션의 E2E 테스트를 실행하는 eval은 애플리케이션 Artifact에 선언합니다.

```toml
name = "app"

[evals.e2e]
title = "The application passes its end-to-end suite"
profile = { kind = "runtime", command = "node", args = ["{e2e-tests}/run.mjs", "{app}"] }
payload.instruction = "Run {e2e-tests} against {app}."
```

`e2e-tests` Artifact의 eval은 테스트 코드 자체를 평가합니다. 테스트 코드의 스타일과, 기능 문서를
얼마나 충실히 따르는지를 봅니다. 테스트 코드 자체를 리뷰할 때는 애플리케이션 테스트를 실행하지 않습니다. 테스트 코드와
상관없는 `app`의 스타일 eval은 지시문에서 테스트 코드를 참조하지 않는 한, 테스트 코드 리뷰가 끝나지 않아도
실행할 수 있습니다.

모든 eval은 입력의 판정과 상관없이 그 대상을 판단할 수 있습니다. 옵션 없는 `verify`는
효율을 위해 의존성을 먼저 실행합니다. 입력 리뷰가 끝나지 않았으면 그 입력을 쓰는 eval은 실행을 미루고, 입력 리뷰가 RED면 실행이 막힙니다. `--speculate`를 쓰면 입력 게이트가 확정되기 전에 작업을 시작할 수 있지만, 최종 통과에 필요한 요건은 그대로 모두 충족해야 합니다. 최종 결과가 GREEN이 되려면 상위 리뷰를
포함해 필요한 증거를 모두 확보해야 합니다.

게이트는 eval마다 적용됩니다. Artifact는 eval 중 하나라도 ERROR나 RED면 그 상태를, 그렇지 않고
하나라도 막혔으면 BLOCKED를 보여 줍니다. 게이트가 RED나 BLOCKED이면 이에 의존하는 eval도
차례로 차단되며, 자식 eval이 RED이면 부모의 모든 eval이 차단됩니다. eval이
없는 중간 Artifact는 실행 게이트가 되지 않습니다. 그런 Artifact를 참조해도 그 입력이나 자식이
실행을 막지는 않습니다. 다만 Run이 GREEN으로 끝나려면 그 입력과 자식에 필요한 리뷰도 모두 통과해야 하므로, 그 Artifact를
쓰는 eval이 실행되더라도 Run은 GREEN으로 끝나지 못할 수 있습니다.

한 Artifact의 eval이 다른 Artifact를 참조하고, 그 참조가 직접 또는 여러 단계를 거쳐 처음 Artifact로 돌아오면 이 Artifact들은 **순환**을 이룹니다. 부모를
참조하는 자식도 부모와 순환을 이룹니다. 순환 안의 eval은 서로의 게이트가 되지 않습니다. 예를 들어
`docs`의 eval이 `cli`를 참조하고 `cli`의 eval이 `docs`를 참조하면, `cli/tests`가 RED여도
`docs/matches-cli`는 막히지 않고 판정도 바뀌지 않습니다. 둘은 따로 실행되며, 순환 밖에서 이들을
쓰는 eval이 진행하려면 둘 다 통과해야 합니다.

순환에 속한 Artifact를 참조하는 eval도 순환 밖 입력의 eval 결과가 나올 때까지 실행을 미룹니다. `cli`의 eval이
`core`도 참조한다면, `core/tests`가 RED일 때 `cli/tests`가 막히고, `cli`를 참조하는
`docs/matches-cli`도 막힙니다. 이 Artifact들에 속한 eval이라도 순환에 속한 Artifact나 입력, 자식을 참조하지 않으면 이들의 결과가 나오기 전에도 실행할 수 있습니다. 다만 Run이 GREEN으로 끝나려면 순환에 속한 Artifact와 입력, 자식의 결과가 모두 필요합니다. 다른 곳에서 `docs`를
참조하는 eval은 `docs`와 `cli`의 모든 eval 결과가 나와야 실행할 수 있습니다. eval의 대상
Artifact도 마찬가지로 그 eval의 게이트가 되지 않으며, [Human 형제 선행 조건](#human-sign-offs)만
예외입니다. 어떤 eval을 대기 상태로 두고 어떤 추측 실행 작업을 취소할지도 이 게이트에 따라 정해집니다.

BLOCKED인 eval의 사유에는 `Dependency verdict RED: core/tests`처럼 실행을 막은 근본 원인인 RED eval만
표시하고, 단순히 실행 중인 eval은 표시하지 않습니다. 추측 실행하는 Run에서는 상위 eval의 ERROR도
그에 의존하는 작업을 막으며, 사유는 `Dependency ERROR: …`로 나옵니다. `blockedBy`에도 같은 eval이 나오고, 확정된 Run에는
그 eval이 모두 나옵니다.

### Human 승인 {#human-sign-offs}

Human eval은 의존성 게이트에 더해, 같은 Artifact의 Human이 아닌 다른 eval이 모두 GREEN일 때만
시작합니다. 사람은 자동 eval을 이미 통과한 것만 승인하며, 그전까지 Human eval은 대기 상태로 남습니다.
자동 eval의 판정이 GREEN이면, 그 eval이 의존하는 승인이 아직 남아 있어 PROVISIONAL로 보이더라도
통과한 것으로 칩니다. 그
자동 eval 중 하나라도 RED나 ERROR면 Human eval은 시작하지 않고 BLOCKED가 되며, 사유에
`Sibling machine eval RED: spec/review`처럼 그 eval이 나옵니다. 같은 Artifact의 다른 Human
eval에는 이 조건을 적용하지 않으므로, 두 승인 요청은 서로의 완료를 선행 조건으로 삼지 않습니다. 옵션 없는 `verify`와
`--speculate` 모두 규칙이 같습니다. 추측 실행하는 Run도 그 자동 eval이
GREEN이 되기 전에는 사람에게 요청하지 않습니다.

## fingerprint 필드 {#the-fingerprint-field}

`fingerprint`를 생략하면 내장 방식인 **artifactsum**으로 그 Artifact에 속한 파일을 해시합니다.
`fingerprint = false`는 재사용을 끄고, 스크립트로 다른 값을 정할 수도 있습니다. 형식, 포함하는
입력, GREEN/RED 결과 재사용은 [Fingerprint](fingerprint.md)에서 설명합니다.

## 태그와 검사 {#tags-and-validation}

`tags = ["type:code", "scope:player"]`는 화면에 보여 주기만 하는 메타데이터를 더합니다. 태그는
ASCII 제어 문자가 없고 비어 있지 않은 고유한 문자열이며 64개까지입니다. `status`,
`config graph`, `monitor`와 그 JSON의 Artifact 출력에 나옵니다. 태그를 바꿔도 fingerprint, eval 정의
해시, 재사용 키, 의존 관계, `verify`와 `status`로 선택하는 대상은 바뀌지 않습니다. 이런 명령에는 태그로
거르는 기능이 없고, 태그로 관계를 정하는 규칙도 없습니다. `artifactize review`에서 `tag:` 검색어를 쓸 때만 태그로 결과를 걸러 볼 수 있습니다.

프로젝트에 템플릿이 필요하면 `.artf` 파일을 직접 생성해서 커밋하세요. 선언 찾기와 검사 규칙은
[선언 검사](declarations.md#declaration-validation)에 있습니다.
