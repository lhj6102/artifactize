# Artifact {#artifact}

Artifact는 리뷰하는 대상이면 무엇이든 될 수 있습니다. 코드, 문서, 디자인, 이미지 모두
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
오류가 있으면 그 파일, 줄, 열이 나옵니다. artifactize는 이 선언들을 읽어 의존성 그래프 하나로
만듭니다.

## Artifact Graph {#artifact-graph}

**Artifact Graph**는 입력과 그 입력을 쓰는 eval을 연결합니다. `{spec}` 같은 참조는 그
Artifact를 eval의 입력으로 들이며, 그 fingerprint가 바뀌면 키에 그 Artifact를 포함한 리뷰만
무효가 됩니다. 자식 폴더로 입력을 정리할 수 있지만, 같은 저장소에 있다는 이유만으로 모든
Artifact가 모든 검사의 의존성이 되지는 않습니다.

실행 게이트는 eval마다 그 eval이 참조하는 Artifact에 따라 정해집니다. 자식 폴더는 부모 내용의 일부이므로 부모의 모든
eval이 참조하는 것으로 칩니다. [Eval 의존성](#eval-dependencies)을 보세요.

그래프 자체도 리뷰할 수 있습니다. 저장소 루트의 이름 없는 `.artf`가 그래프 Artifact를
선언합니다. 그 fingerprint는 소스 내용이나 실행 설정이 아니라, 추출한 기준과 이름이 붙은
관계를 포함합니다. 프로젝트의 리뷰 구조를 사람이 리뷰할 때 쓰세요. 문법은
[선언](declarations.md#graph-artifact)에 있고, [graph 도구](tools/builtin.md#current-project-graph)는
에이전트에게 현재 구조 전체를 보여 주거나 사람을 위해 데스크톱에서 엽니다.

## 폴더 설정 {#folder-configuration}

`index.artf`가 있는 폴더는 Artifact입니다. `name`은 필수이고, `basis`, `views`,
`fingerprint`, `tags`는 선택입니다. 예전 선언은
[예전 정책 업그레이드 안내](../getting-started/install.md#upgrading-from-011)를 보세요.

eval은 로컬 ID를 키로 쓰는 테이블입니다. `usage` Artifact의 `[evals.heading]`은 eval ID
`usage/heading`이 됩니다. `id` 필드는 선언하지 마세요. eval마다 `title`과, `kind`가 `runtime`,
`agent`, `human` 중 하나인 `profile`이 있습니다. 모든 eval에는 `payload.instruction`이 필요하며,
`pass_schema`, `fail_schema`, `profile_variants`를 둘 수 있습니다.

runtime eval은 종료 코드가 판정입니다(0이면 GREEN). Agent나 Human은 소유자 필드와 함께 GREEN이나
RED를 돌려줍니다. 각 eval의 의존성은 그 eval이 참조하는 Artifact와 대상 Artifact의 자식으로 정해집니다.

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
이름을 그대로 쓰고, 소유자가 정한 payload 필드는 정한 이름을 그대로 씁니다. `--json` 출력과
저장된 상태는 여전히 `passSchema`, `timeoutMs`, `agentTools` 같은 필드를 씁니다. 정확한 문법은
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
폴더이지만, 내장 도구는 상관없는 옆 파일이 아니라 그 파일과 허용된 범위만 봅니다. 사이드카를
둔다고 둘러싼 폴더의 자식이 되지는 않으며, 그 폴더는 여전히 대상 파일을 자기 파일의 일부로
해시합니다. 반면 `index.artf`로 선언한 폴더는 그 위에서 가장 가까운 Artifact 폴더의 자식입니다.
이름 짓기, fingerprint 형식, 가상 경로, 잘못된 대상과 재확인 규칙은
[파일 Artifact 레퍼런스](declarations.md#file-artifacts)에 있습니다.

## Eval 의존성 {#eval-dependencies}

의존성은 그 Artifact의 모든 Eval이 아니라 입력을 쓰는 Eval에 속합니다. `payload.instruction`이나
runtime `args` 인자에 있는 `{name}` 참조가 입력 Artifact를 가리키며, 그 Eval은 입력을 판단하는
Eval에 의존합니다. 한 Artifact가 다른 Artifact를 쓴다면, 그 쓰임을 확인하는 eval이 `{name}`으로
가리키면 됩니다. 그 밖에는 의존성을 더하는 것이 없습니다.

자식 Artifact와의 연결만은 eval에 적지 않아도 생깁니다. 폴더 Artifact의 파일에는 자식 폴더가 포함되므로,
자식은 부모의 모든 eval이 참조한 것과 똑같이 칩니다. 부모의 각 eval은 Artifact로 선언된 가장
가까운 하위 폴더의 eval에 의존합니다.

예를 들어 애플리케이션 E2E 실행은 애플리케이션에 속합니다.

```toml
name = "app"

[evals.e2e]
title = "The application passes its end-to-end suite"
profile = { kind = "runtime", command = "node", args = ["{e2e-tests}/run.mjs", "{app}"] }
payload.instruction = "Run {e2e-tests} against {app}."
```

`e2e-tests` Artifact는 테스트 코드 자체, 즉 그 스타일과 기능 문서에 얼마나 충실한지를
평가합니다. 자기 테스트 코드를 리뷰한다며 애플리케이션 테스트를 실행하지는 않습니다. 상관없는
`app` 스타일 Eval은 지시문에서 테스트 코드 리뷰를 참조하지 않았다면, 그 리뷰가 끝나지 않아도
실행할 수 있습니다.

모든 Eval은 입력의 판정과 상관없이 자기 대상을 판단할 수 있습니다. 옵션 없는 `verify`는
효율을 위해 의존성을 먼저 실행합니다. 입력 리뷰가 대기 중이면 그 입력을 쓰는 Eval이 늦어지고,
입력 리뷰가 RED면 막힙니다. `--speculate`는 입력 게이트가 정해지기 전에 작업을 시작하게 해 주지만,
빠진 요구 사항을 승인해 주지는 않습니다. 최종 GREEN에는 여전히 상위 리뷰를 포함해 필요한 증거가
모두 있어야 합니다.

게이트는 Eval마다 적용됩니다. Artifact는 Eval 중 하나라도 ERROR나 RED면 그 상태를, 그렇지 않고
하나라도 막혔으면 BLOCKED를 보여 줍니다. Eval의 실행 게이트는 참조한 Artifact의 Eval과, 자기
Artifact에서 가장 가까운 자식 Artifact의 Eval입니다. 게이트가 RED나 BLOCKED면 그 Eval도 막히므로,
막힘은 이 Eval 게이트를 따라 퍼집니다. 자식의 Eval이 RED면 부모의 모든 Eval이 막힙니다. Eval이
없는 중간 Artifact는 실행 게이트가 되지 않습니다. 그런 Artifact를 참조해도 그 입력이나 자식이
실행을 막지는 않습니다. 다만 그것들은 여전히 최종 승인 의무에 속하므로, 그 Artifact를 쓰는
Eval이 실행되더라도 Run은 GREEN으로 끝나지 못할 수 있습니다.

eval의 대상 Artifact와 의존성 순환으로 묶인 Artifact에는
[Human 형제 선행 조건](#human-sign-offs)을 빼고 이 게이트를 적용하지 않습니다. 순환은 Eval 참조와
자식을 따라갑니다. 부모를 직접 또는 여러 단계를 거쳐 참조하는 자식은 부모와 순환으로 묶입니다.
순환으로 묶인 대상을 참조하면 그 순환 바깥의 게이트를 이어받고, 다른 순환을 참조하면 그 순환의
모든 Eval을 게이트로 받습니다. 대기와 추측 실행 취소도 이 게이트로 정해집니다.

예를 들어 `docs`의 eval이 `cli`를 참조하고 `cli`의 eval이 `docs`를 참조하면 둘은 순환을
이룹니다. 이때 `cli/tests`가 RED여도 `docs/matches-cli`는 막히지 않고 판정도 바뀌지 않습니다. 둘은
따로 실행되며, 순환 밖에서 이들을 쓰는 eval이 진행하려면 둘 다 통과해야 합니다. `cli`의 eval이
`core`도 참조한다면, 순환은 바깥 입력을 함께 쓰므로 `core/tests`가 RED일 때 `cli/tests`와
`docs/matches-cli`가 모두 막힙니다. 다른 곳에서 `docs`를 참조하는 eval은 `docs`와 `cli`의 모든
eval을 기다립니다.

BLOCKED인 Eval의 사유에는 `Dependency verdict RED: core/tests`처럼 막힘의 뿌리가 된 RED Eval이
나오며, 아직 실행 중이었을 뿐인 Eval은 나오지 않습니다. 추측 실행하는 Run에서는 상위 ERROR도
막으며 `Dependency ERROR: …`로 나옵니다. `blockedBy`에도 같은 Eval이 나오고, 확정된 Run에는
그 Eval이 모두 나옵니다.

### Human 승인 {#human-sign-offs}

Human Eval은 의존성 게이트에 더해, 자기 Artifact의 Human이 아닌 다른 Eval이 모두 GREEN일 때만
시작합니다. 사람은 기계 검사 Eval을 이미 통과한 것만 승인합니다. 그전까지는 기다립니다. 그
기계 검사 Eval 중 하나라도 RED나 ERROR면 Human Eval은 시작하지 않고 BLOCKED가 되며, 사유에
`Sibling machine eval RED: spec/review`처럼 그 Eval이 나옵니다. 같은 Artifact의 다른 Human
Eval에는 이 조건을 적용하지 않으므로, 두 승인 요청이 서로의 완료를 기다리는 일은 없습니다. 옵션 없는 `verify`,
`--speculate`, `--defer-human` 모두 규칙이 같습니다. 추측 실행하는 Run도 그 기계 검사 Eval이
GREEN이 되기 전에는 사람에게 요청하지 않습니다.

## fingerprint 필드 {#the-fingerprint-field}

`fingerprint`를 생략하면 내장 방식인 **artifactsum**으로 그 Artifact에 속한 파일을 해시합니다.
`fingerprint = false`는 재사용을 끄고, 스크립트로 다른 값을 정할 수도 있습니다. 형식, 포함하는
입력, GREEN/RED 결과 재사용은 [Fingerprint](fingerprint.md)에서 설명합니다.

## 태그와 검사 {#tags-and-validation}

`tags = ["type:code", "scope:player"]`는 화면에 보여 주기만 하는 메타데이터를 더합니다. 태그는
ASCII 제어 문자가 없고 비어 있지 않은 고유한 문자열이며 64개까지입니다. `status`,
`config graph`, `monitor`와 그 JSON의 Artifact 출력에 나옵니다. 태그는 fingerprint, Eval 정의
해시, 재사용 키, 관계, `verify`와 `status`의 선택을 바꾸지 않습니다. 그런 명령에는 태그 필터가
없고, 관계 규칙도 없습니다. 태그로 걸러 보는 곳은 `artifactize review` 검색의 `tag:` 검색어뿐입니다.

템플릿이 필요한 프로젝트는 `.artf` 파일을 직접 생성해서 커밋하세요. 선언 찾기와 검사 규칙은
[선언 검사](declarations.md#declaration-validation)에 있습니다.
