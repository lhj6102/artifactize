# 선언 (.artf)

프로젝트는 `.artifactize/`가 있는 폴더입니다. 그 안의 일반 `*.artf` 파일 하나가 Artifact
하나를 정의합니다. 파일 이름이 곧 이름입니다. `.artifactize/engine/app.artf`의 이름은
`app`이며, 하위 폴더는 정의 파일을 정리하는 용도일 뿐 이름 공간이나 Artifact 간 관계를 만들지 않습니다.

```toml
# .artifactize/app.artf
path = "crates/app/"
parts = ["app-tests"]

[evals.tests]
title = "The application tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"] }
payload.instruction = "Test {app} with {app-tests}."
```

`artifactize config check`로 정의를 검사하세요. 정의가 잘못되면 파일, 줄, 열을 알려 주고
종료 코드 2로 끝납니다. 대상이 없거나 종류가 다른 경우는 정의 오류가 아닙니다. 검사는
안내와 함께 0으로 끝납니다. `status`와 `verify`는 그 대상을 다루는 eval에만 대상 경로를
표시하고 `ERROR/PREPARATION_FAILED`로 처리합니다. 관련 없는 eval의 실행 예측은 그대로
확인할 수 있으며, 검증할 때도 계속 실행할 수 있습니다.

## 선언 검사 {#declaration-validation}

`path`는 필수이며, 정의 파일이 아니라 프로젝트 루트를 기준으로 합니다. 대상이 아직 없어도
표기로 종류를 정합니다.

| PATH | 대상 |
|---|---|
| `"src/"` | 폴더. 끝의 `/`가 중요합니다. |
| `"./"` | 프로젝트 루트 |
| `"README.md"` | 일반 파일 하나 |
| `"src/"`와 `select = ["api", "model.rs"]` | 그 폴더 아래에서 고른 경로인 aspect |
| `"https://example.com"` | 외부의 실행 중인 상태. URI 스킴은 두 글자 이상입니다. |

로컬 경로는 `/`를 쓰며 절대 경로, `..`, 빈 경로 요소, 역슬래시를 쓸 수 없습니다.
여러 Artifact가 같은 경로나 겹치는 경로를 가리킬 수 있습니다. 명령과 상대 경로 선언의
기준은 대상 폴더입니다. 파일이면 그 파일이 있는 폴더, aspect이면 선택한 폴더,
Live Artifact이면 프로젝트 루트입니다. `execution_paths`는 프로젝트 기준입니다.

`name` 키는 쓸 수 없습니다. 파일 이름이 Artifact의 이름입니다. 이름은
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`에 맞아야 하며 모든 정의 하위 폴더를 통틀어
대소문자를 구분하지 않고 유일해야 합니다. `con`, `nul`, `com1` 같은 장치 이름은 거부합니다.
Eval, 도구, 세션, 프로필 변형의 ID도 같은 식별자 문법을 씁니다. `app.artf`의
`[evals.check]`는 `app/check`입니다. Eval에는 별도의 `id`를 쓰지 않습니다.
Eval 순서는 파일에 쓴 순서가 아니라 ID 순서입니다.

| 필드 | 규칙 |
|---|---|
| `path` | 위 표의 대상. 필수입니다. |
| `parts` | 부분으로 삼을 Artifact 이름 1~64개. 중복, 자기 참조, 구성 순환은 허용하지 않습니다. |
| `select` | 폴더 아래에서 고른 경로 1~64개. 이때 aspect가 됩니다. |
| `basis` | `true`이면 해당 Artifact에 대한 리뷰만 면제하며 eval은 선언할 수 없습니다. |
| `tags` | 중복 없는 문자열 최대 64개. 공백뿐인 값이나 제어 문자는 쓸 수 없습니다. |
| `fingerprint` | 로컬 대상의 기본값은 artifactsum. `false`는 재사용을 끄고, 테이블로 artifactsum이나 스크립트를 지정합니다. |
| `evals.<id>.title` | 필수 제목 |
| `evals.<id>.profile` | 필수 `runtime`, `agent`, `human` 프로필 |
| `evals.<id>.payload.instruction` | 필수 평가 지시문 |
| `evals.<id>.pass_schema`, `fail_schema` | 아래에서 설명하는 선택 결과 스키마 |
| `evals.<id>.profile_variants` | 같은 종류의 완전한 프로필 최대 64개 |
| `evals.<id>.mutex` | Runtime과 Agent eval의 선택 mutex 태그 |
| `views.agent_tools`, `views.human_tools` | Artifact가 제공하는 도구 |
| `views.agent_sessions` | [도구 세션](tools/agent.md#tool-sessions) |
| `annotations` | 구조가 아닌 배치와 메모. 아래에서 설명합니다. |

선언 키는 `timeout_ms`, `input_schema`, `execution_paths`, `pass_schema`처럼 snake_case를
씁니다. JSON Schema 키워드와 작성자가 정한 payload 필드 이름은 그대로 둡니다.
알 수 없는 선언 키는 오류지만 `annotations` 안은 검사하지 않습니다. TOML null 값은
지원하지 않습니다. Annotations 밖의 날짜는 따옴표로 감싸고 숫자는 유한한 값을 써서 JSON으로
나타낼 수 있어야 합니다. JSON 출력과 도구 프로토콜의 camelCase 이름은 선언 문법이 아닙니다.

<a id="migrating-removed-dependency-declarations"></a>

## 프로젝트 안에 흩어진 선언 옮기기 {#upgrading-an-in-tree-project}

`artifactize config migrate`를 실행하세요. 다른 프로젝트는 `--repo`로 고릅니다. 이 명령만
예전 `index.artf`, 파일 옆의 선언, 루트 `.artf`, `.artfignore`를 읽습니다. 일반 명령은
`.artifactize/`만 읽습니다. 그 밖의 파일은 정의가 아닌 일반 내용이며, `.artfignore`는 더는
탐색 범위를 정하지 않습니다.

폴더 구조만으로 정해지던 구성은 `parts`에 명시하고, `path`를 추가한 뒤 `name`을 삭제합니다.
주석과 나머지 선언은 보존한 뒤 예전 파일을 지웁니다. Artifactsum을 쓰는 루트 폴더 Artifact에는
`.artifactize/` ignore를 더합니다. 예전 루트 Artifact Graph는 `.artifactize/`를 대상으로 하는 일반
폴더 Artifact가 됩니다. 예전 ignore로 제외했던 별도 프로젝트는 그대로 두고 알려 줍니다.
각 프로젝트에서도 `--repo`로 선택해 옮기세요.

이미 정의가 있거나, 이름이 대소문자만 다르거나, 예약된 이름이 있거나, 예전 프로젝트나 변환
결과가 잘못되면 아무것도 쓰지 않습니다. 다만 예전 Artifact Graph나 중첩 프로젝트의 `.artf` 파일까지 내용에 포함하게 된 폴더
Artifact는 예외이며, 그 밖의 경우 로컬 fingerprint와 평가 기준은 유지됩니다. 출력된 변경을
확인한 뒤 검증하세요.

## 결과 필드 스키마 {#owner-result-schemas}

Agent와 Human 결과에는 `verdict`와, eval 작성자가 정한 추가 결과 필드가 들어 있습니다. `pass_schema`(GREEN)와
`fail_schema`(RED)에는 이 추가 결과 필드만 선언하세요. `verdict`는 artifactize가 넣습니다.
예를 들면 다음과 같습니다.

```toml
[evals.review.pass_schema]
type = "object"
properties.summary = { type = "string", minLength = 1 }
required = ["summary"]
additionalProperties = false
```

Agent는 `{"verdict":"GREEN","summary":"All requirements are covered."}`를 돌려줍니다.
Human은 GREEN을 따로 고르고, 양식 필드로 `{"summary":"All requirements are covered."}`만
입력합니다.

이 스키마는 JSON Schema입니다. 결과에 어떤 필드가 있을 수 있고 어떤 값을 허용하는지 정하는
규칙입니다. 결과 객체 전체에 적용되는 스키마(**결과 형식**)에는 일반 JSON Schema의 일부 기능만
쓸 수 있으며, 참조(다른 스키마를 가리키는 `$ref`)와 조합(여러 스키마를 합치는 `allOf`, `anyOf`,
`oneOf`, `not`)도 제한됩니다.

- `type`이 있으면 `"object"`여야 합니다. `additionalProperties`가 있으면 `false`여야
  합니다. 생략해도 artifactize가 최종 스키마에 객체 타입과 추가 속성 금지 조건을 적용합니다.
- 허용하는 최상위 추가 결과 필드는 모두 `properties`에 직접 적어야 합니다. `patternProperties`나
  `$ref`로 선언하지 않은 최상위 필드를 허용할 수 없습니다.
- 최상위의 `allOf`, `anyOf`, `oneOf`, `not`은 거부합니다. 조합은 추가 결과 필드 안에서 하세요.
  중첩 객체, 배열, 로컬 참조는 스키마 검사기가 지원합니다.
- 다음 이름은 예약되어 있으므로 필드로 선언하거나 필수 항목으로 지정할 수 없습니다: `verdict`, `reference`,
  `reusedFrom`, `executionProvenance`, `attemptId`, `provider`, `model`, `stdout`,
  `stderr`, `durationMs`, `exitCode`. 감사·실행 데이터는 추가 결과 필드 밖에 있습니다.
- 한 판정의 스키마를 생략하면 그 판정에서는 **추가 결과 필드를 하나도 허용하지 않습니다**.
  Agent는 `{"verdict":"GREEN"}`이나 `{"verdict":"RED"}`만 돌려주고, Human 양식 필드는
  `{}`입니다. 빈 객체 스키마도 마찬가지로 추가 결과 필드를 허용하지 않습니다.

### Human 결과 스키마 {#human-result-schemas}

Human 결과는 사람이 양식으로 입력하므로, Human eval의 `pass_schema`와 `fail_schema`에는 양식으로
입력할 수 있는 형태만 쓸 수 있습니다. 그 밖의 형태는 `.artf`를 읽을 때 선언 오류가 되며, 키
경로와 대신 쓸 수 있는 형태를 알려 줍니다.

| 추가 결과 필드 스키마 | 리뷰에서 보이는 모습 |
|---|---|
| `{ type = "string" }`. `minLength`, `maxLength`를 붙일 수 있습니다 | 텍스트 상자 |
| `{ type = "integer" }`나 `"number"`. `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`를 붙일 수 있습니다 | 숫자 상자 |
| `{ type = "boolean" }` | true / false(선택 항목이면 – 도) |
| `{ enum = ["a", "b"] }`. 문자열만 받으며, `type = "string"`은 써도 되고 안 써도 됩니다 | 선택지 |
| `{ const = … }` | 표시하지 않고 값을 자동으로 채웁니다 |
| `{ type = "array", items = { type = "string" } }`. 항목에 `minLength`, `maxLength`를 붙일 수 있습니다 | 텍스트 상자 목록 |
| `{ type = "array", items = { type = "object", properties = … } }`. 속성은 위의 단일 값 형태만 쓸 수 있습니다 | 그 필드들의 반복 그룹 |

배열에는 `minItems`와 `maxItems`를 쓸 수 있습니다. 결과 객체와 각 그룹의 객체에는
`type = "object"`, `properties`, `required`, `additionalProperties`를 씁니다. `required`에
적은 이름은 모두 선언한 속성이어야 합니다. 어느 스키마에나 `title`, `description`,
`$comment`를 붙일 수 있으며, 속성의 `description`은 도움말로 나옵니다. 필수 필드에는 `*`가
붙습니다. 비어 있는 선택 필드에는 *Optional*이 표시되며, 해당 필드는 결과에 포함되지 않습니다. 빈 목록 항목과 아무것도 입력하지 않은 그룹도 결과에 포함되지 않습니다. 데스크톱 앱에서는 `+ Add`와 ✕ 버튼으로 항목과 그룹을 추가하고
지우며, 터미널 UI에서는 [Review 양식](../tui/index.md)에서 설명하는 키를 씁니다. 패턴, 형식,
중첩 객체, 그룹 안의 목록, `$ref`, `anyOf` 같은 조합은 사람이 입력할 수 없으므로 거부합니다.
Agent 스키마에는 검사기가 지원하는 기능을 모두 쓸 수 있습니다.

스키마는 8 MiB까지입니다. Agent의 최종 응답은 다 받은 뒤에 검사하며, 1 MiB를 넘으면
잘못된 응답입니다. 그 응답에서 artifactize가 보관하는 결과는 256000자까지입니다. 이모지처럼 기본 다국어 평면 밖의 문자는 2자로 셉니다. 최종 응답이 잘못되면 한 번만 수정을 요청하며, 이때도 정해진 제한을 적용합니다. 알리지 않고 필드를 지우지는 않습니다. [Human 제출 인터페이스](../cli/index.md#command-reference)는 따로 입력 크기를
제한하며, 검사 오류를 알릴 뿐 요청을 확정하지 않습니다.

## 정의 리뷰하기 {#graph-artifact}

정의도 일반 파일입니다. 모두 리뷰하려면 `path = ".artifactize/"`, 한 묶음만 리뷰하려면
그 하위 폴더, 정의 하나만 리뷰하려면 그 파일의 전체 경로를 지정하세요. 특별한 종류나
fingerprint는 필요 없습니다. 정의 파일 자체를 대상으로 지정해도 fingerprint 계산이 재귀적으로 이어지지는
않습니다. `graph` 내장 도구는 어느 Artifact에서든 프로젝트 전체의 평가 기준과 관계를 보여 줍니다.

```toml
# .artifactize/graph.artf
path = ".artifactize/"

[views.agent_tools.structure]
builtin = "graph"
args = []

[evals.review]
title = "The definitions describe the intended project"
profile = { kind = "human" }
payload.instruction = "Review the definitions and their relationships."
```

Artifactsum이 읽는 모든 `.artf` 파일에는 artifactsum과 파일 내장 도구에서 기본 **artf 보기**를 적용합니다.
정의에 적힌 값은 fingerprint에 반영하지만 `annotations`, 서식, `#` 주석은 반영하지
않습니다. `pass_schema.properties.comment`처럼 사용자가 정한 스키마 안의 필드는
annotation이 아니라 정의 내용입니다. TOML이나 UTF-8이 올바르지 않은 파일은 바이트가
하나라도 바뀌면 반영합니다.

파일 내장 도구는 annotation, 서식, `#` 주석을 뺀 정의를 보여 줍니다. Runtime 명령과
fingerprint 스크립트는 실제 파일을 읽습니다. 이 동작에 별도 선언 옵션은 필요 없습니다.

정의를 리뷰하는 eval은 모델, 제한 시간, 도구, 기준이 바뀌면 다시 실행하지만 annotations나
서식 변경으로는 다시 실행하지 않습니다. 루트 폴더 Artifact도 `fingerprint.ignore`로
제외하거나 부분의 대상으로 삼지 않으면 `.artifactize/`를 포함합니다.

## 배치와 메모 {#positions-and-comments}

각 노드의 정의 파일에 배치와 일반 텍스트 메모를 둘 수 있습니다.

```toml
[annotations.style]
at = [120, 40]
parts = { app-tests = [40, 80] }

[annotations.comment]
text = "An accessibility review will go here next."
```

`at`는 최상위에서의 위치입니다. 전체가 가진 `parts` 위치는 그 전체의 상자 안에서 쓰며,
공유하는 부분은 전체마다 위치를 가질 수 있습니다. 메모는 노드 하나에 속하고 그 노드의
모든 사본에 나타나며 함께 움직입니다. 별도 배치 파일이나 캔버스에 떠 있는 메모는 없습니다.
배치와 메모는 검증이나 재사용을 바꾸지 않습니다.

읽을 때 모양이 잘못됐거나 아직 모르는 annotation 값은 무시합니다. 상자는 자동 배치하고 메모는
숨깁니다. 읽을 수 없는 섹션을 고치려는 작업은 파일, 줄, 열과 함께
거부합니다. 편집은 모르는 annotation 키와 손대지 않은 텍스트를 보존합니다. 더는 맞지 않는
위치는 그 style을 다음에 쓸 때 지우며, 빈 섹션은 없앱니다. 노드 배치, 메모 편집, 에이전트의
파일 수정과 충돌하는 경우는 [데스크톱](../desktop/index.md)을 보세요.

## 파일 Artifact {#file-artifacts}

끝에 `/`가 없는 PATH는 파일 하나를 가리킵니다. 명령은 그 파일이 있는 폴더에서 실행하며
`{name}`은 파일 자체입니다. `{name}/suffix`는 오류입니다. 기본 artifactsum은 그 파일만
다룹니다. `fingerprint.files`를 직접 쓰면 파일 이름 하나만 쓸 수 있고 `fingerprint.ignore`는
파일 Artifact에서 지원하지 않습니다.

심볼릭 링크, 디렉터리, 특수 파일은 일반 파일 대상이 아닙니다. 없거나 종류가 다른 대상은
`config check`에서는 안내, verify에서는 eval 준비 오류이며 정의 오류가 아닙니다.
Fingerprint를 껐거나 스크립트가 상수를 돌려주어도 Runtime·Agent eval은 시작과 끝에
대상을 다시 확인하며, 도구 호출 때도 확인합니다. 재사용 키가 없고 Live 입력도 없는 Human
제출에서는 대상의 존재 여부를 다시 확인하지 않습니다.

## Aspect {#aspects}

폴더 PATH에 `select`를 쓰면 aspect입니다. aspect에는 그 폴더에서 선택한 경로 중 부분 Artifact의 대상에
속하지 않는 파일이 포함됩니다. 기본 artifactsum은 선택한 경로를 다루며, `fingerprint.files`를
직접 쓰면 선택 안에 있어야 합니다. Aspect는 겹칠 수 있고 `.artf` 파일도 고를 수 있습니다.
명령은 선택한 폴더에서 실행하고 `{name}`은 그 폴더를 뜻합니다. 파일과 Live 대상에는 `select`를
쓸 수 없습니다.

구성은 `parts`만 정합니다. 대상 경로가 다른 대상 아래에 있거나 정의 파일이 하위 폴더에
있다는 이유로 관계가 생기지 않습니다. 폴더 부분은 eval이 이름으로 지정해야 전체의 도구로
읽을 수 있으며, 파일·aspect 부분은 전체를 통해서도 읽을 수 있습니다.
[Artifact와 eval](artifact.md#composition)에서 판정을 설명합니다.

## Live Artifact {#live-artifacts}

URI PATH는 외부 상태를 가리킵니다. Artifactize는 URI를 열지 않습니다. Runtime, 도구,
fingerprint 스크립트 인수의 `{name}`에 URI를 넣고 `{name}/x`에는 `/x`를 붙입니다.
명령은 작성자의 환경에서 프로젝트 루트를 작업 디렉터리로 삼아 실행합니다. Live Artifact에는 파일 내장 도구와
artifactsum fingerprint를 쓸 수 없습니다.

`fingerprint`를 생략하면 관측한 상태의 스냅샷이 없습니다. Agent·Human 리뷰를 재사용하려면
배포 커밋처럼 안정적인 스냅샷을 알려 주는 fingerprint 스크립트를 선언하세요. 스크립트 계약은
아래와 같습니다. Live Artifact를 다루는 Runtime eval은 스냅샷이 같아도 매번 실행합니다.
Agent·Human 리뷰는 스냅샷이 유지되는 동안 재사용합니다. 리뷰 도중 스냅샷이 바뀌면
`INPUT_CHANGED`가 됩니다.

Eval은 관측하며 배포하거나 고치지 않습니다. 정상 배포가 기능 브랜치 작업 때문에 실패하지
않도록 운영 상태는 작업 트리의 HEAD가 아니라 원격 릴리스나 `origin/main`과 비교하세요.

## eval mutex 태그 {#eval-mutex-tags}

Runtime이나 Agent eval은 `profile`과 함께 `mutex`도 선언할 수 있습니다.

```toml
[evals.suite-passes]
title = "The end-to-end suite passes"
mutex = ["use-computer:display", "use-computer:callback"]
profile = { kind = "runtime", command = "pnpm", args = ["e2e"], timeout_ms = 1800000 }
payload.instruction = "Run the end-to-end suite."
```

태그는 `use-computer`이거나 `use-computer:display`처럼 콜론으로 이은 하위 태그입니다. 하위
태그의 각 부분은 `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`에 맞아야 합니다. 다른 최상위 이름, 빈 부분,
잘못된 부분, 중복 태그, Human eval에 선언한 태그는 오류입니다. eval 하나에는 중복 없이
태그를 최대 64개 선언할 수 있으며, 그보다 많으면 선언 오류입니다. `mutex`를 생략하거나
`mutex = []`로 선언하면 아무 제약도 없습니다.

두 eval의 태그 중 하나라도 같거나 한쪽이 다른 쪽의 상위 태그면 두 eval은 충돌합니다.
`use-computer`는 모든 하위 태그와 충돌하고, `use-computer:db`는 `use-computer:db:users`와
충돌하지만 `use-computer:browser`나 `use-computer:database`와는 충돌하지 않습니다. 같은 화면을
조작하는 두 eval처럼 함께 실행하면 안 되는 eval에는 같은 태그나 상위·하위 관계의 태그를 주세요.
다른 eval이 충돌하는 태그를 쓰고 있으면, 실행할 eval은 태그를 쓰고 있는 eval의 이름과 함께 `QUEUED`로
표시되고(아래 참고), 태그가 풀리면 시작합니다. 태그로 우선순위나 실행 순서를 정할 수는 없으며, eval들이 서로 점유한 태그 때문에
교착 상태에 빠지는 일도 없습니다.

태그는 한 Run 안에서만이 아니라 같은 상태를 쓰는 모든 Run, 워크트리, `verify`에 적용됩니다.
`--state-dir`이 다르면 태그도 따로 적용됩니다. 대기 중인 요청은 `QUEUED`로 남고, `blockedReason`에
태그를 잡고 있는 eval과 Run이 나옵니다. 실행이 보류된 동안에는 실행 예산을 쓰지 않고, 백엔드 자리나
`--jobs` 자리도 차지하지 않으며, `timeout_ms`는 eval이 시작할 때부터 셉니다. 태그는 eval이 끝나거나 취소되면
풀리고, 프로세스가 죽거나 Run이 끝날 때도 풀립니다.

태그는 Artifact가 아니라 그 eval에서 실행하는 명령이나 도구를 나타냅니다. 다른 eval이 테스트를 실행하는 동안에도, 테스트 코드를 읽기만 하는 Agent eval은 실행을 미룰 필요가 없습니다. 암묵적인 태그는 없습니다.
mutex 태그는 평가 기준이 아니라 실행 옵션이므로, 바꿔도 fingerprint나 재사용 키가 바뀌지 않고,
충돌하는 eval이 실행 중일 때도 캐시 결과는 재사용할 수 있습니다.

## eval 의존성 {#eval-dependencies}

Artifact 참조는 `payload.instruction`과 Runtime `profile.args`에 `{input}`이나
`{input}/path`로 씁니다. 최종 검증 요건, 순환, 스케줄링, Human 선행 조건은
[eval 의존성](artifact.md#eval-dependencies)에, 부분은 [구성](artifact.md#composition)에 있습니다.

## Artifactsum {#artifactsum}

`fingerprint`를 생략하면 artifactsum을 씁니다. 폴더라면 `fingerprint = {}`도 같습니다.
`fingerprint = false`는 재사용을 끄고, `fingerprint = true`는 거부합니다. 무엇을 포함하는지,
`files`, `ignore`, 파일 Artifact 형식은 [Fingerprint와 재사용](fingerprint.md#artifactsum)에
있습니다. 태그는 해시하지 않습니다.

artifactsum은 범위가 정해진 경로 규칙에 따라 Artifact 아래의 파일을 읽습니다. 읽을 파일에 ignore되지 않은 심볼릭 링크나 특수 파일이 있으면 오류가 발생해 검증을 중단합니다. 적용되는 `.gitignore`는 해시 대상에서 빠지더라도 규칙을 적용하려고 모두 읽으므로,
심볼릭 링크가 아닌 1 MiB 이하의 일반 파일이어야 합니다. 해당 Artifact에 속하지 않은 파일은 해시하지
않습니다. 다만 무엇을 제외할지 알기 위해 저장소 루트까지 그 위 폴더들의 `.gitignore`도 읽습니다. 탐색 항목이 10,000개를 넘거나 전체 용량이 1 GiB를 초과하면 오류를 내고 중단합니다. 해시는 준비 단계와 리뷰가
끝날 때의 재확인에서 계산합니다. 그래서 리뷰 중에 Python의 `__pycache__`처럼 ignore된 경로에
파일이 생겨도 `INPUT_CHANGED`가 일어나지 않습니다. 결과마다 포함한 파일 목록을 저장하므로,
`status`가 그 뒤로 바뀐 파일(`path`, `+path`, `-path`)과 바뀐 의존성을 알려 줄 수 있습니다.
폴더가 아주 크면 `status`가 구체적인 변경 내역 대신 변경 여부만 알려 줄 수도 있습니다.

## Fingerprint 스크립트 {#fingerprint-scripts}

eval을 실행하기 전에 `verify`는 선택한 eval에 필요한 모든 Artifact의 fingerprint를 계산하며,
eval을 선택하지 않은 의존 Artifact도 포함합니다. 동시에 몇 개를 계산할지는
[`--fingerprint-jobs`](../cli/index.md#runtime-cli)로 정합니다. 실패하면 이름 순서로 처음 실패한
Artifact를 알리고 나머지는 멈춥니다. 각 스크립트에는 별도의 출력 폴더를 줍니다.

스크립트 출력은 그대로 Artifact의 fingerprint가 되며, 그 Artifact의 모든 eval에 같은 값이
쓰이고 요청마다 저장됩니다. 이 값을 평가 기준 및 의존성과 어떻게 조합하는지는
[재사용 키](fingerprint.md#the-reuse-key)에 있습니다. artifactsum이 실패해도 똑같이 준비를
멈춥니다.

명령은 그 Artifact의 작업 디렉터리에서 stdin으로 JSON `{"version":1,"artifactId":"example"}`을
받아 실행됩니다. 실행 파일은 [프로그램 찾기](others/platforms.md#program-lookup) 규칙을
따릅니다. 인자는 [Artifact 참조](#artifact-references-in-commands)만 빼고 적은 그대로
전달합니다. 스크립트는 참조한 Artifact에 접근할 수 있지만, Artifact Graph에 관계가 추가되지는
않습니다. `config check`는 모르는 참조와 형식이 잘못된 참조를 거부하며, 준비 단계에서 파일의
존재 여부와 심볼릭 링크를 확인합니다. 인터프리터별 플래그, 래퍼, 진입 파일 규칙은 없습니다.

`fingerprint.script.files`는 Artifact 기준의 고유한 파일·디렉터리 경로를 64개까지 받으며, 각
경로는 1024자까지입니다(이모지처럼 기본 다국어 평면 밖의 문자는 2자로 셉니다). 호출할 때마다 각 경로가 존재하고 심볼릭 링크를 거치지 않는지
확인하며, 내용은 해시하지 않습니다. 실행 환경, 취소 처리, 스트림마다 128 KiB인 원본 출력 크기 제한은 Runtime 명령과 같으며, 기본 시간 제한은 30,000ms입니다(`timeout_ms`는 1–2,147,483,647). 별도의 출력·임시
디렉터리는 실행이 끝나면 지웁니다. stdout은 `[A-Za-z0-9._:-]`로 이뤄진 1–128자의 ASCII여야
하며, 그 뒤에 LF 하나(Windows에서는 CRLF 하나)만 올 수 있습니다. 그 밖의 공백은 허용하지 않고
앞뒤 공백을 지워 주지도 않습니다. 다음 중 하나라도 해당하면 동작 오류로 처리하고 준비 단계를 중단합니다. 종료 코드가 0이 아닐 때, 출력 형식이 잘못됐을 때, stdout이나 stderr가 128 KiB를 넘을 때(올바른 fingerprint를 출력했더라도), 시간이 초과됐을 때, 취소됐을 때, 파일이 없을 때, 정리에 실패했을 때입니다. eval을 시작하지 않으며, 캐시 없이 리뷰하는 방식으로 전환하지도 않습니다. stderr는 fingerprint 진단으로 전달하지 않습니다.

Runtime이나 Agent 리뷰가 끝나면 GREEN이나 RED 판정을 확정하기 전에 포함한 fingerprint를 다시
계산합니다. 값이 바뀌었으면 판정 없이 ERROR/INPUT_CHANGED를 기록하고, 재확인에 실패하면
ERROR를 기록합니다. force를 줘도 준비와 재확인은 건너뛰지 않습니다. `fingerprint = false`인
Artifact는 준비할 fingerprint가 없습니다. 재사용 키가 없어도 Live 상태를 다루는 eval은
준비된 스냅샷을 모두 재확인합니다. 그 밖에 온전한 재사용 키가 없는 eval은 fingerprint 값
재확인을 건너뜁니다.
그래도 준비 단계, Runtime·Agent eval 실행 전후, 도구 호출 때 파일 Artifact의 대상 파일을
확인합니다. 재사용 키가 없고 Live 입력도 없는 Human 제출에서는 대상의 존재 여부를 다시
확인하지 않습니다. 필요한 다른
Artifact의 fingerprint는 그래도 준비할 수 있습니다. 작업 공간의 변경을 계속 감시하지는 않습니다.
artifactsum은 준비와 재확인 때만 해시합니다.

## 명령 안의 Artifact 참조 {#artifact-references-in-commands}

참조는 `{name}`, `{name}/path`, `--flag={name}/path` 형태입니다. eval의 **범위(scope)**는
읽을 수 있는 Artifact 집합입니다. 대상 Artifact와 지시문이나 Runtime 인자에서 이름을
참조한 Artifact가 포함됩니다.

| 문맥 | 규칙 | 예시 |
|---|---|---|
| 지시문 | eval이 이름을 참조한 Artifact에 의존하게 됩니다 | `Review {spec}.` |
| Runtime 인자 | 참조를 치환하고 eval 의존성을 추가합니다. 폴더 부분은 전체를 통하지 않고 직접 참조합니다 | `args = ["{spec}/api.md"]` |
| Fingerprint 스크립트 인자 | 스크립트가 그 Artifact를 읽을 수 있지만 eval 의존성은 추가하지 않습니다 | `args = ["{rules}/STYLE.md"]` |
| 도구와 도구 세션 인자 | 이미 eval 범위에 있는 Artifact만 가리킬 수 있으며 도구가 의존성을 추가하지는 않습니다 | 지시문 `Review {spec}.`, 도구 `args = ["{spec}"]` |
| 로컬 폴더나 aspect | 그 폴더에서 시작해 절대 경로가 됩니다. aspect는 선택한 범위를 벗어나지 않습니다 | `{code}/src/lib.rs` |
| 파일 | 파일 자체를 가리키며 `/path`를 붙이지 않습니다 | `{logo}` |
| Live | 파일 시스템에 접근하지 않고 URI를 그대로 전달하며 뒤의 경로도 그대로 붙입니다 | `{service}/health` |

Fingerprint 스크립트나 도구의 경로가 폴더 부분을 지나려면 그 부분도 범위에 있어야 합니다.
그렇지 않으면 준비나 호출이 실패합니다. aspect의 선택 범위는 폴더 부분 안까지 닿을 수
있습니다. 허용된 폴더의 경로로 파일 부분의 대상이나 aspect가 선택한 파일을 읽을 수 있지만,
허용되지 않은 폴더 부분은 읽을 수 없습니다. 부분 관계만으로 부분의 도구가 추가되지는
않습니다. 그 도구를 쓰려면 사용하는 eval의 지시문이나 Runtime 인자에서 부분을 참조하세요.

나머지 인자는 그대로 전달합니다. 이스케이프한 중괄호, 두 겹 중괄호, `${variables}`, JSON,
짝이 맞지 않는 중괄호는 치환하지 않으며, artifactize는 문자열로 셸 명령을 만들지 않습니다.
Artifact를 참조해도 그 eval의 지시문이 현재 리뷰의 기준에 추가되지는 않습니다. 프로젝트
전체의 기준은 [`graph`](tools/builtin.md#current-project-graph) 도구로 볼 수 있습니다.

참조의 경로는 절대 경로일 수 없고, `..`로 올라가거나, 빈 부분, 백슬래시, 콜론, 제어 문자를
담거나, 4096자를 넘을 수 없습니다. 직접 선언하는 경로(`fingerprint.files`,
`fingerprint.script.files`, `execution_paths`)는 1024자까지입니다. `config check`는 eval마다
선언한 프로필의 이름과 문법을 확인합니다. 위의 문법과 경로 규칙은 모든 `profile_variants` 항목에도
적용되므로, 어느 변형에든 `{app}/../outside`처럼 잘못된 참조가 있으면 `config check`가 실패합니다.
변형에만 나오는 참조가 실제 Artifact를 가리키는지는 그 변형을 고를 때 확인하므로, 변형을 쓰기 전에
`status SELECTOR --profile NAME`을 실행하세요. 참조한 파일이
있는지와 심볼릭 링크를 따라가지 않는지는 명령을 준비할 때 확인합니다.

## Runtime 실행 {#runtime-execution}

artifactize는 프로그램을 적은 인자 그대로, 셸 없이, Artifact 폴더(파일 Artifact면 들어 있는
폴더)에서 실행합니다.

명령은 작성자의 평소 터미널 환경에서 실행되므로, 프로젝트에 명령을 선언할 때도 터미널에 입력하듯 쓰면 됩니다. 실제 홈과 캐시 위치(`HOME`, `XDG_*`, Windows에서는 `USERPROFILE`, `APPDATA`,
`LOCALAPPDATA`와 시스템 변수)를 포함해 `artifactize` 프로세스의 환경을 모두 전달하므로, 버전
관리자로 설치한 `cargo`나 `pnpm`처럼 이 변수로 위치를 찾는 도구도 터미널에서처럼 동작합니다.
artifactize 전용 환경 변수는 전달하지 않습니다. 호출한 쪽의 `ARTIFACTIZE_*` 변수(`ARTIFACTIZE_REMOTE_TOKEN`
같은 자격 증명과 설정)는 모두 지웁니다. 명령은 `ARTIFACTIZE_WORKSPACE_DIR`,
`ARTIFACTIZE_OUTPUT_DIR`(출력용 별도 폴더로, Run이 끝나도 prune할 때까지 남음),
`ARTIFACTIZE_TMP_DIR`, 그리고 명령이 끝나면 지우는 별도 임시 폴더를 가리키는
`TMPDIR`/`TMP`/`TEMP`를 받습니다. 이 폴더는 현재 사용자만 읽을 수 있고, 리뷰하는 프로젝트 안에 두지
않습니다.

명령은 작성자의 환경에서 실행되므로, 다른 터미널 명령과 마찬가지로 실행 환경에 따라 결과가 달라질 수 있습니다.
환경은 [재사용 키](fingerprint.md#the-reuse-key)에 들어가지 않으므로 도구를 업그레이드해도 캐시
결과가 무효가 되지 않습니다. 도구 버전이 바뀔 때 fingerprint도 바뀌어야 한다면, fingerprint 계산에 도구 버전을 넣으세요.

Runtime의 제한 시간은 기본 30,000ms이며, 1~2,147,483,647ms로 설정할 수 있습니다. 결과에는 stdout과
stderr를 각각 앞에서부터 최대 128 KiB까지 저장하고, 출력이 잘렸는지 여부와 종료 상태, 실행 시간을 기록합니다. 텍스트는 UTF-8로
읽고(잘못된 바이트는 대체), 터미널 색상 코드와 탭, LF, CR을 뺀 ASCII 제어 문자를 지웁니다(DEL은
남깁니다). 전체 스트림은 Run 출력에 남으며, `request logs`와 MCP `request_logs`로 로그의 끝부분, 지정한 범위,
검색 조건에 맞는 줄을 읽을 수 있습니다. 캡처를 지우면 저장된 앞부분 128 KiB만 남습니다. `artifactize prune`은
Run의 `output` 폴더를 지우지만 이 스트림 캡처는 남깁니다. 캡처가 없거나 일부만 남았거나 이전 버전·원격 실행의 기록만 있으면 그 상태를 알려 줍니다.

명령이 정상으로 끝나든, 취소되거나 시간이 초과되든 그 명령이 시작한 프로그램도 함께 멈추므로,
백그라운드에서 시작한 프로그램이 명령보다 오래 남지 않습니다. 계속 실행되어야 하는 보조 프로그램에는
Agent [도구 세션](tools/agent.md#tool-sessions)이나 Human [launch 도구](tools/human.md#running)를
쓰세요. 명령은 사용자가 신뢰하는 로컬 코드로 보고
**샌드박스 없이** 실행합니다. 명령은 현재 사용자 계정의 권한으로 실행되므로, 그 계정에 허용된 작업은 모두 수행할 수 있습니다. 명령은 리뷰 대상인 입력을 바꾸면 안 됩니다. 부모 프로세스에서 일부러 분리해 실행한 프로그램은 명령이 끝난 뒤에도 계속 실행될 수 있습니다.
