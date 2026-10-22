# 선언(.artf) {#declarations-artf}

리뷰하려는 폴더 안에 `index.artf`를, 파일이라면 그 옆에 `NAME.artf`를 쓰고 확인하세요.

```sh
artifactize config check
```

선언이 유효하면 `Folder configuration is valid.`를 출력하고, 오류가 있으면 오류를 출력한 뒤 종료 코드 2로 끝납니다. 선언 내용에
오류가 있으면 파일 이름과 줄·열 번호가 나오고, 대상 파일이 없는 것처럼 디스크에 문제가 있으면 그 경로만 나옵니다. 이 페이지에는 선언, 폴더와 파일 Artifact, fingerprint, 참조, 명령 실행 방식의
정확한 규칙이 있습니다. 개념 소개는 [Artifact와 eval](artifact.md)에 있습니다.

## 없어진 의존성 선언 옮기기 {#migrating-removed-dependency-declarations}

`kind = "dependency"`와 `depends_on`은 없어졌습니다. 의존성 eval을 지우고, 필요한 Artifact를
그 결과를 쓰는 eval의 `payload.instruction`(또는 runtime `args`)에 `{name}` 참조로 넣으세요.
예를 들어 `spec`과 `assets`를 나열하는 `ready` eval을 따로 만들지 말고, 실제 리뷰 지시문을
`"Review {app} against {spec} and {assets}."`처럼 쓰세요. 그 입력을 쓰는 eval은 입력에 필요한 eval이 끝난 뒤 실행되고, 관련 없는 형제 eval은 그대로 실행됩니다.

자식 폴더는 부모의 모든 eval이 참조하는 것으로 칩니다. 그 밖의 Artifact의 리뷰가 먼저
끝나야 한다면, eval의 지시문이나 argv에서 그 Artifact를 직접 참조하세요.

예전 프로필을 쓰면 설정을 해석하는 단계에서
`unknown variant `dependency`, expected one of `agent`, `human`, `runtime`` 오류가 납니다.
지원하는 프로필에 `depends_on` 필드가 있으면 알 수 없는 필드 오류입니다. 이는 리뷰 판정이
아니라 설정 오류이므로, verify하기 전에 선언을 옮기세요.

## 선언 검사 {#declaration-validation}

`index.artf`는 그 파일이 있는 폴더를 Artifact로 선언합니다. `<filename>.artf`는 이름이 확장자까지
정확히 `<filename>`인 옆 파일을 선언합니다. 둘 다 표준 TOML을 쓰며 `name`이 필요합니다.
저장소 루트의 이름 없는 `.artf`는 소스 파일 없이 [그래프 Artifact](#graph-artifact)를
선언합니다.

선언을 찾을 때는 저장소 루트부터 내려가며, `.git`, `node_modules`, 그리고 루트에 둘 수 있는
`.artfignore`(gitignore 문법)에 적은 하위 디렉터리와 사이드카 경로는 건너뜁니다. 방문하는
디렉터리마다 `index.artf`는 그 파일 이름만 가리키는 ignore 패턴이 있어도 읽습니다. 하위
프로젝트의 폴더 선언을 빼려면 그 폴더 자체를 탐색에서 빼세요. ignore된 사이드카는 해석하지도
대상을 확인하지도 않으며, 다른 선언이 포함하지 않는 한 그 대상은 일반 파일로 남습니다. 별도
프로젝트, 픽스처, 예제가 든 폴더를 적어 두세요.

```gitignore
crates/app/tests/
examples/
```

이름, eval ID, 도구·세션 이름, 프로필 변형 이름은 글자나 숫자로 시작하고, 글자, 숫자, 밑줄,
하이픈으로 이뤄진 64자 이하여야 합니다(`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`). Artifact 이름은
전체에서 하나뿐이어야 합니다. eval은 배열이 아니라 ID를 키로 쓰는 테이블이며, `app`의
`[evals.check]`는 `app/check`가 됩니다. eval에 `id` 필드를 직접 쓰면 거부합니다. 선언 안의
eval은 TOML에 적힌 위치가 아니라 ID 순서로 정렬합니다.

| 필드 | 규칙 |
|---|---|
| `name` | 필수. Artifact 이름 |
| `tags` | 선택. 비어 있지 않고 ASCII 제어 문자가 없는 고유한 문자열 최대 64개의 목록 |
| `basis` | 선택. 불리언. `true`면 eval 없는 입력으로 선언하며, 그 Artifact의 리뷰 의무는 없애지만 의존성의 의무는 없애지 않음 |
| `fingerprint` | 생략하면 artifactsum. `false`면 재사용 안 함. 그 밖에는 artifactsum이나 스크립트 테이블 |
| `views.agent_tools`, `views.human_tools` | 선택. 선언한 도구의 맵 |
| `views.agent_sessions.<name>.start`, `.stop` | 도구 세션 명령. `command`와 `args`는 필수, `execution_paths`는 선택. 다른 필드는 없음 |
| `views.agent_tools.<name>.session` | 선택. 같은 Artifact에 선언한 도구 세션 이름. 명령 Agent 도구에만 씀 |
| `evals.<id>.title` | 필수. 비어 있지 않은 제목 |
| `evals.<id>.gate` | 선택. `"blocking"`이면 `--defer-human`에서도 Human 승인을 엄격하게 유지. 생략하면 요청 모드를 따름. 다른 값은 없음 |
| `evals.<id>.mutex` | 선택. 고유한 계층형 mutex 태그 최대 64개의 목록. runtime과 Agent eval에만 씀. 기본값 없음 |
| `evals.<id>.profile` | 필수. `runtime`, `agent`, `human` 프로필 |
| `evals.<id>.payload.instruction` | runtime, Agent, Human eval에 필수. 비어 있지 않은 지시문 |
| `evals.<id>.pass_schema`, `fail_schema` | 선택. 실행 가능한 eval의 소유자 필드 JSON Schema. Human eval은 [양식으로 입력할 수 있는 형태](#human-result-schemas)만 받음 |
| `evals.<id>.profile_variants` | 기본 종류를 유지하는, 이름 붙은 완전한 프로필 최대 64개 |

[도구 세션](tools/agent.md#tool-sessions)은 리뷰 하나 동안 테스트 하네스를 계속 실행해 둡니다.
시작, 종료, 정리는 artifactize가 맡으므로 수명이나 시점 옵션을 선언할 일은 없습니다.

선언 키는 모두 snake_case입니다: `timeout_ms`, `max_tool_calls`, `max_tokens`,
`input_schema`, `execution_paths`, `pass_schema`, `fail_schema`, `profile_variants`,
`gate`, `mutex`. 그 밖의 키와 `timeout_ms` 대신 쓴 `timeoutMs` 같은 camelCase 표기는 거부합니다.
`additionalProperties`, `minLength`, `maxItems` 같은 JSON Schema 키워드는 표준 이름을 그대로
씁니다. 소유자가 정한 payload 필드는 정한 이름을 그대로 씁니다. 이 필드는 eval과 함께 남고 재사용 키를
바꾸지만, Agent는 `payload.instruction`만 보므로 리뷰어에게 필요한 내용은 모두 거기에 쓰세요. `--json`, 도구 프로토콜, 저장된
상태는 `timeoutMs`, `passSchema`, `agentTools`, `blockedBy`처럼 기존 camelCase 필드를 계속
쓰며, 이는 선언 문법이 아닙니다.

인라인 테이블, 점으로 이은 키, 여러 줄 문자열은 표준 TOML입니다.

```toml
name = "app"
tags = ["type:code"]

[evals.review]
title = "The implementation matches the spec"
profile = { kind = "agent", backend = "codex", model = "YOUR_EXACT_MODEL_ID", reasoning = "high", timeout_ms = 240000 }

[evals.review.payload]
instruction = """
Read {spec}, then review {app}.
Return GREEN when the implementation matches the spec.
"""

[evals.review.pass_schema]
type = "object"
required = ["summary"]
additionalProperties = false
properties.summary = { type = "string", minLength = 1 }
```

TOML에는 null 값이 없습니다. 선택 필드는 생략하고 `null`을 쓰지 마세요. 스키마에
`type = "null"`은 쓸 수 있지만, null `const` 같은 null 값 자체는 쓸 수 없습니다. 따옴표 없는
TOML 날짜·시각과 유한하지 않은 숫자(`inf`, `nan`)는 JSON으로 나타낼 수 없으므로 payload와
스키마를 포함해 어디서든 거부합니다. 날짜를 문자열로 쓰려면 따옴표로 감싸세요. 선언 오류는 TOML 해석 오류든 값의 타입, 필드, 참조 오류든 오류가 난 `.artf` 파일과 줄, 열을 알려 줍니다. 값이 잘못됐으면 `evals.check.profile.args`처럼 그 키도 알려 줍니다.

`config check`는 fingerprint 스크립트를 실행하거나 파일을 해시하지 않고 검사합니다. 선언을
다시 쓴 뒤에 쓰세요. 예전 선언 파일과 ignore 파일은 읽지 않습니다. ignore되지 않은 예전 선언
파일이 있으면 선언 찾기가 실패합니다. 예전 선언 파일 이름을 가진 디렉터리는 디렉터리 ignore
패턴이 빼지 않는 한 평소처럼 탐색합니다. `.artfignore`로 예전 파일 하나만 ignore하거나 그
폴더 전체를 건너뛸 수 있습니다. 저장소 루트에 예전 ignore 파일이 있으면 항상 실패합니다. 진단
메시지는 다음과 같습니다.

```text
artifactize.json is no longer read; declare this Artifact in index.artf (TOML).
.artifactizeignore was renamed to .artfignore.
```

변환기는 없습니다. 에이전트에게 선언을 다시 쓰게 하고 새 `.artf` 파일을 커밋하세요. family는
없어졌으니, 반복되는 선언은 프로젝트의 생성기로 만들고 그 `.artf` 출력을 커밋하세요.
`staleKey`, `stale`, `critics`, `envRequirements`, `resultCheck`,
`reviewPolicy.maxConcurrentExecutors`, 도구 메타데이터 `observation` 같은 예전 필드는
거부하며, 새 필드의 별칭이 아닙니다. artifactsum 형식의 예전 `dependencies` 옵션도
거부합니다.

## 소유자 결과 스키마 {#owner-result-schemas}

Agent와 Human 결과에는 `verdict`와 소유자가 정한 필드가 들어 있습니다. `pass_schema`(GREEN)와
`fail_schema`(RED)에는 소유자 필드만 선언하세요. `verdict`는 artifactize가 넣습니다. 예를 들어
위의 `summary` 스키마는 Agent에게서 `{"verdict":"GREEN","summary":"All requirements are
covered."}`를 받습니다. Human은 GREEN을 따로 고르고, 양식 필드로
`{"summary":"All requirements are covered."}`만 입력합니다.

이 스키마는 JSON Schema입니다. 결과에 어떤 필드가 있을 수 있고 어떤 값을 받는지 정하는
규칙입니다. 결과 객체 전체의 스키마인 **결과 envelope**은 일반 JSON Schema보다 허용하는 것이
적으며, 참조(다른 스키마를 가리키는 `$ref`)와 조합(여러 스키마를 합치는 `allOf`, `anyOf`,
`oneOf`, `not`)에도 제한이 있습니다.

- `type`이 있으면 `"object"`여야 합니다. `additionalProperties`가 있으면 `false`여야
  합니다. 생략해도 artifactize가 최종 스키마에 객체 타입과 추가 속성 금지 조건을 적용합니다.
- 허용하는 최상위 소유자 필드는 모두 `properties`에 직접 적어야 합니다. `patternProperties`나
  `$ref`로 선언하지 않은 최상위 필드를 허용할 수 없습니다.
- 최상위의 `allOf`, `anyOf`, `oneOf`, `not`은 거부합니다. 조합은 소유자 필드 안에서 하세요.
  중첩 객체, 배열, 로컬 참조는 스키마 검사기가 지원합니다.
- 다음 예약된 이름은 선언하거나 필수로 정할 수 없습니다: `verdict`, `reference`,
  `reusedFrom`, `executionProvenance`, `attemptId`, `provider`, `model`, `stdout`,
  `stderr`, `durationMs`, `exitCode`. 감사·실행 데이터는 소유자 필드 밖에 있습니다.
- 한 판정의 스키마를 생략하면 그 판정에서는 **소유자 필드를 하나도 허용하지 않습니다**.
  Agent는 `{"verdict":"GREEN"}`이나 `{"verdict":"RED"}`만 돌려주고, Human 양식 필드는
  `{}`입니다. 빈 객체 스키마도 똑같이 닫힌 형식입니다.

### Human 결과 스키마 {#human-result-schemas}

Human 결과는 사람이 양식으로 입력하므로, Human eval의 `pass_schema`와 `fail_schema`에는 양식으로
입력할 수 있는 형태만 쓸 수 있습니다. 그 밖의 형태는 `.artf`를 읽을 때 선언 오류가 되며, 키
경로와 대신 쓸 수 있는 형태를 알려 줍니다.

| 소유자 필드 스키마 | 리뷰에서 보이는 모습 |
|---|---|
| `{ type = "string" }`. `minLength`, `maxLength`를 붙일 수 있음 | 텍스트 상자 |
| `{ type = "integer" }`나 `"number"`. `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`를 붙일 수 있음 | 숫자 상자 |
| `{ type = "boolean" }` | true / false(선택 항목이면 – 도) |
| `{ enum = ["a", "b"] }`. 문자열만, `type = "string"`은 써도 되고 안 써도 됨 | 선택지 |
| `{ const = … }` | 표시 없음: 값이 채워짐 |
| `{ type = "array", items = { type = "string" } }`. 항목에 `minLength`, `maxLength`를 붙일 수 있음 | 텍스트 상자 목록 |
| `{ type = "array", items = { type = "object", properties = … } }`. 속성은 위의 단일 값 형태만 | 그 필드들의 반복 그룹 |

배열에는 `minItems`와 `maxItems`를 쓸 수 있습니다. 결과 객체와 각 그룹의 객체에는
`type = "object"`, `properties`, `required`, `additionalProperties`를 씁니다. `required`에
적은 이름은 모두 선언한 속성이어야 합니다. 어느 스키마에나 `title`, `description`,
`$comment`를 붙일 수 있으며, 속성의 `description`은 도움말로 나옵니다. 필수 필드에는 `*`가
붙습니다. 비어 있는 선택 필드에는 *Optional*이 표시되고 결과에서 빠지며, 빈 목록 항목과 아무것도
입력하지 않은 그룹도 빠집니다. 데스크톱 앱에서는 `+ Add`와 ✕ 버튼으로 항목과 그룹을 추가하고
지우며, 터미널 UI에서는 [Review 양식](../tui/index.md)에서 설명하는 키를 씁니다. 패턴, 형식,
중첩 객체, 그룹 안의 목록, `$ref`, `anyOf` 같은 조합은 사람이 입력할 수 없으므로 거부합니다.
Agent 스키마는 검사기 기능을 모두 씁니다.

스키마는 8 MiB까지입니다. Agent의 최종 응답 원문은 1 MiB까지이고, 정규화한 결과는 UTF-16 코드
단위 256000개까지입니다. 최종 응답이 잘못되면 정해진 범위에서 한 번 수정을 요청할 뿐, 알림 없이
필드를 지우지는 않습니다. [Human 제출 인터페이스](../cli/index.md#command-reference)는 따로 입력 크기를
제한하며, 검사 오류를 알릴 뿐 요청을 확정하지 않습니다.

## 그래프 Artifact {#graph-artifact}

저장소 루트에 이름 없는 `.artf`를 두면 프로젝트 전체의 Artifact Graph를 선언합니다.
`index.artf`는 여전히 루트 폴더를, `<filename>.artf`는 여전히 옆 파일을 선언합니다. 다른
디렉터리에 이름 없는 `.artf`가 있으면 선언 오류입니다. 루트 `.artf`는 `index.artf`처럼
`.artfignore` 패턴이 그 파일 이름을 가리켜도 읽습니다. 그래프 Artifact는 하나만 둘 수
있습니다.

```toml
# .artf at the repository root
name = "project-graph"

[views.agent_tools.structure]
builtin = "graph"
args = []

[views.human_tools.viewer]
builtin = "graph"
kind = "launch"
args = []

[evals.review]
title = "The Artifact boundaries and evaluation criteria fit the project"
profile = { kind = "human" }
payload = { instruction = "Use the graph viewer to review the current structure." }
```

이름 있는 일반 Artifact이므로 runtime, Agent, Human eval과 도구를 무엇이든 선언할 수 있고,
`verify`, `status`, CI, `--reuse-only`에서 이름으로 고를 수 있습니다. 기본 eval이 없고, 자식 Artifact를 갖지 않으며,
다른 Artifact와의 관계도 자동으로 생기지 않습니다. 지시문이나 argv에서 직접 참조하는 것은 평소처럼 동작합니다.
명령은 저장소 루트에서 실행되지만, 그래프를 선언해도 그 폴더가 그래프 Artifact에 속하지는 않습니다. 범위는
`kind = "graph"`를 쓰고 선언 경로는 `.artf`입니다.

`fingerprint`는 `false`나 빈 테이블이라도 선언하지 마세요. fingerprint는 항상 현재 추출한
그래프입니다. 그 밖에는 일반 Artifact 규칙을 따릅니다. `basis = true`로 선언한 Artifact에는 eval을 둘 수 없고 그 Artifact의 리뷰도 필요 없지만, 의존하는 Artifact에 필요한 리뷰까지 면제되지는 않습니다.
`basis = true`가 없고 eval도 없는 그래프는 eval 없는 다른 Artifact처럼 UNREVIEWED로 남습니다.
소스 파일을 만들지 않으며, 소스 파일이 없어도 됩니다. 일반 Artifact의 순환도 허용하며, 그래프
Artifact를 선언한 내용도 그래프 fingerprint에 들어갑니다.

[그래프 fingerprint](fingerprint.md#graph-fingerprint)는 소스 파일 내용이나 eval 실행 옵션이
아니라 구조에 관한 정의가 바뀔 때 바뀝니다. 차이 비교, 이력, 자동 리뷰 정책은 없습니다. 결과는
현재 상태만 기준으로 리뷰하거나 재사용합니다.

## 파일 Artifact {#file-artifacts}

`hero.png.artf`는 같은 폴더의 일반 파일 `hero.png`를 선언합니다. `name`은 필수이며, 파일이 들어 있는 폴더의 Artifact 이름과 별개로 정합니다. 없는 대상, 디렉터리, 심볼릭 링크, 특수 파일은 대상이 될 수
없습니다. 범위가 정해진 도구 경로로 나타낼 수 없으므로 이름에 `:`가 든 대상 파일은 선언을 찾을
때 거부합니다. `.artf` 선언 자체도 일반 파일이어야 합니다. 심볼릭 링크, 특수 파일,
`bundle.artf` 같은 디렉터리는 선언 찾기에서 실패합니다. `index.artf`는 항상 폴더를 선언하므로
이름이 `index`인 파일에는 사이드카를 둘 수 없습니다. `.artf` 파일은 선언이며 대상이 되지
않습니다.

파일 Artifact는 들어 있는 폴더 Artifact와 자식 관계나 의존 관계를 맺지 않습니다. 하위 폴더에 `index.artf`로 선언한 Artifact는 그대로 가장 가까운 상위 폴더 Artifact의 자식이 됩니다. 폴더의
artifactsum은 옆에 있는 파일 Artifact의 대상을 포함하고, `*.artf` 선언은 모두 뺍니다.

들어 있는 폴더가 runtime 명령, 도구, fingerprint 스크립트의 작업 디렉터리이므로, 상대 경로
실행 파일로 옆 파일을 쓸 수 있습니다. 그렇다고 다른 옆 파일에 접근할 수 있는 것은 아닙니다.
내장 도구와 범위가 정해진 argv 참조는 대상 파일과 참조한 Artifact만 봅니다. `{name}`은 폴더가
아니라 대상 파일 경로가 되며, 파일 참조에는 `/path`를 붙일 수 없습니다. `list .`는 파일
Artifact의 가상 루트를 가리키며 대상 파일 이름을 보여 줍니다. 그 루트 안의 경로는 들어 있는
폴더를 기준으로 합니다. 기본 artifactsum은 대상 파일만 포함합니다. 작업 공간에 파일
Artifact만 있어도 되며, 폴더 선언이 없어도 됩니다.

파일을 해시하려면 `fingerprint`를 생략하세요. artifactsum의
[파일 Artifact 형식](fingerprint.md#artifactsum)은 대상만 적으며, 스크립트나
`fingerprint = false`도 쓸 수 있습니다.

JSON 명령 도구의 `context.artifactPath`는 파일을 가리키고, 프로세스의 작업 디렉터리는 그
파일이 들어 있는 폴더입니다. 범위 항목에는 `kind = "file"`, `"folder"`, `"graph"`가
있습니다. `execution_paths`에는 어떤 버전의 도구가 실행됐는지 해시로 기록할 파일을 프로젝트 기준 경로로 적으며, 이 필드는 세 종류 모두에서 똑같이 동작합니다.

대상은 리뷰하는 동안 내내 심볼릭 링크가 아닌 일반 파일이어야 하며, `fingerprint = false`여도
마찬가지입니다.

- eval이 시작되기 전에 없거나 잘못됨: eval은 ERROR/PREPARATION_FAILED입니다.
- Runtime이나 Agent 리뷰가 끝날 때 없거나 잘못됨: 판정 대신 ERROR/INPUT_CHANGED입니다.
- Human 리뷰 중에 없거나 잘못됨: 도구와 제출이 실패하고 요청은 계속 남아 있으니, 파일을 되돌려
  놓고 다시 시도하세요.
- 내용이 바뀜: 파일 내용의 변경은 fingerprint로 확인하므로, `fingerprint = false`면 내용이 바뀌어도 감지하지 못합니다.
  그 밖에는 Runtime이나 Agent 리뷰, 또는 승인이 ERROR/INPUT_CHANGED로 끝납니다.
- 없거나 잘못된 대상에 대한 도구 호출은 도구를 실행하지 않고 오류를 돌려줍니다.

파일을 되돌려 놓은 뒤 다시 verify하세요.

없어진 예전 정책 필드와 대신 쓸 것은
[업그레이드 안내](../getting-started/install.md#upgrading-from-011)를 보세요.

## eval mutex 태그 {#eval-mutex-tags}

runtime이나 Agent eval은 `gate`와 `profile` 옆에 `mutex`를 선언할 수 있습니다.

```toml
[evals.suite-passes]
title = "The end-to-end suite passes"
mutex = ["use-computer:display", "use-computer:callback"]
profile = { kind = "runtime", command = "pnpm", args = ["e2e"], timeout_ms = 1800000 }
payload.instruction = "Run the end-to-end suite."
```

태그는 `use-computer`이거나 `use-computer:display`처럼 콜론으로 이은 하위 태그입니다. 하위
태그의 각 부분은 `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`에 맞아야 합니다. 다른 최상위 이름, 빈 부분,
잘못된 부분, 중복 태그, Human eval에 선언한 태그는 오류입니다. `mutex`를 생략하거나
`mutex = []`로 선언하면 아무 제약도 없습니다.

두 eval의 태그 중 하나라도 같거나 한쪽이 다른 쪽의 상위 태그면 두 eval은 충돌합니다.
`use-computer`는 모든 하위 태그와 충돌하고, `use-computer:db`는 `use-computer:db:users`와
충돌하지만 `use-computer:browser`나 `use-computer:database`와는 충돌하지 않습니다. 같은 화면을
조작하는 두 eval처럼 함께 실행하면 안 되는 eval에는 같은 태그나 상위·하위 관계의 태그를 주세요.
다른 eval이 충돌하는 태그를 쓰고 있으면, 실행할 eval은 태그를 쓰고 있는 eval의 이름과 함께 `QUEUED`로
표시되고(아래 참고), 태그가 풀리면 시작합니다. 태그는 우선순위나 순서를 정하지 않으며, 서로의 태그를 기다리다
교착 상태에 빠지는 일은 없습니다.

태그는 한 Run 안에서만이 아니라 같은 상태를 쓰는 모든 Run, 워크트리, `verify`에 적용됩니다.
`--state-dir`이 다르면 태그도 따로입니다. 대기 중인 요청은 `QUEUED`로 남고, `blockedReason`에
태그를 잡고 있는 eval과 Run이 나옵니다. 대기하는 동안에는 실행 예산을 쓰지 않고, 백엔드 자리나
`--jobs` 자리도 차지하지 않으며, `timeout_ms`는 eval이 시작할 때부터 셉니다. 태그는 eval이 끝나거나 취소되면
풀리고, 프로세스가 죽거나 Run이 끝날 때도 풀립니다.

태그는 Artifact가 아니라 그 eval에서 실행하는 명령이나 도구를 나타냅니다. 테스트 코드를 읽기만 하는
Agent는 다른 eval이 그 테스트를 실행한다고 기다릴 필요가 없습니다. 암묵적인 태그는 없습니다.
mutex 태그는 평가 기준이 아니라 실행 옵션이므로, 바꿔도 fingerprint나 재사용 키가 바뀌지 않고,
충돌하는 eval이 실행 중일 때도 캐시 결과는 재사용할 수 있습니다.

## Human 게이트 정책 {#human-gate-policy}

`gate = "blocking"`은 프로필이나 전역 설정이 아니라 eval에 다는 선택 필드입니다.
`verify --defer-human`에서도 Human eval의 게이트를 대기 상태로 유지합니다. 생략하면 기본 Run에서는 승인 게이트를 엄격하게 적용하고, 요청에서 승인 미루기를 지정한 경우에만 승인을 미룰 수 있습니다. runtime이나 Agent
eval에서는 아무 효과도 없습니다. 미룰 수 있는 것은 Human 승인뿐입니다. 게이트 정책은
스케줄링만 바꾸며, 판단 정의나 재사용 키는 바꾸지 않습니다.

```toml
[evals.signoff]
title = "The delivery is approved"
gate = "blocking"
profile = { kind = "human" }
payload.instruction = "Approve the delivery before dependent work proceeds."
```

[verify 모드](../cli/index.md#verify-modes)와 [미룬 승인](human.md#deferred-sign-offs)을
보세요.

## eval 의존성 {#eval-dependencies}

Artifact 참조는 `payload.instruction`과 runtime `profile.args`에 `{input}`이나
`{input}/path`로 씁니다. 의무, 자식 Artifact, 순환, 스케줄링, Human 선행 조건은
[eval 의존성](artifact.md#eval-dependencies)에 있습니다.

## Artifactsum {#artifactsum}

`fingerprint`를 생략하면 artifactsum을 씁니다. 폴더라면 `fingerprint = {}`도 같습니다.
`fingerprint = false`는 재사용을 끄고, `fingerprint = true`는 거부합니다. 무엇을 포함하는지,
`files`, `ignore`, 파일 Artifact 형식은 [Fingerprint와 재사용](fingerprint.md#artifactsum)에
있습니다. 태그는 해시하지 않습니다.

탐색은 범위가 정해진 경로 규칙을 따릅니다. 심볼릭 링크와 특수 파일은 ignore하지 않으면
실패합니다. 적용되는 `.gitignore`는 해시 대상에서 빠지더라도 규칙을 적용하려고 모두 읽으므로,
심볼릭 링크가 아닌 1 MiB 이하의 일반 파일이어야 합니다. 소유 범위 밖으로는 따라가지
않으며, 항목 10,000개나 1 GiB를 넘으면 오류와 함께 탐색을 멈춥니다. 해시는 준비 단계와 리뷰가
끝날 때의 재확인에서 계산합니다. 그래서 리뷰 중에 Python의 `__pycache__`처럼 ignore된 경로에
파일이 생겨도 `INPUT_CHANGED`가 일어나지 않습니다. 결과마다 포함한 파일 목록을 저장하므로,
`status`가 그 뒤로 바뀐 파일(`path`, `+path`, `-path`)과 바뀐 의존성을 알려 줄 수 있습니다.
아주 큰 폴더는 무언가 바뀌었다는 것만 보여 줄 수도 있습니다.

## Fingerprint 스크립트 {#fingerprint-scripts}

eval을 실행하기 전에 `verify`는 선택한 eval에 필요한 의존성 범위의 fingerprint를 계산하며,
eval을 선택하지 않은 의존성도 포함합니다. 동시에 몇 개를 계산할지는
[`--fingerprint-jobs`](../cli/index.md#runtime-cli)로 정합니다. 실패하면 이름 순서로 처음 실패한
Artifact를 알리고 나머지는 멈춥니다. 각 스크립트에는 별도의 출력 폴더를 줍니다.

스크립트 출력은 그대로 Artifact의 fingerprint가 되며, 그 Artifact의 모든 eval에 같은 값이
쓰이고 요청마다 저장됩니다. 이 값이 기준, 의존성과 어떻게 합쳐지는지는
[재사용 키](fingerprint.md#the-reuse-key)에 있습니다. artifactsum이 실패해도 똑같이 준비를
멈춥니다.

명령은 소유자의 작업 디렉터리에서 stdin으로 JSON `{"version":1,"artifactId":"example"}`을
받아 실행됩니다. 경로 없는 명령은 PATH에서 찾고, 절대 경로 실행 파일은 그대로 실행합니다.
`./fingerprint.sh`처럼 `/`가 든 상대 경로 실행 파일은 폴더 Artifact의 허용된 범위 안에서
찾습니다. 파일 Artifact라면 들어 있는 폴더 아래에서 찾으며 상관없는 옆 파일을 가리킬 수도
있습니다. 어느 쪽이든 상위 경로로 올라가기와 심볼릭 링크는 거부합니다. 인자는 범위가 정해진
Artifact 참조만 빼고 적은 그대로 전달합니다. `{name}`은 전역 Artifact 이름입니다. 참조한
Artifact는 자식까지 포함해 스크립트의 범위에 들어오지만 그래프 관계가 되지는 않습니다.
`config check`는 모르는 참조와 형식이 잘못된 참조를 거부하며, 존재 여부와 심볼릭 링크는 명령을
준비할 때 확인합니다. 인터프리터별 플래그, 래퍼, 진입 파일 규칙은 없습니다.

`fingerprint.script.files`는 소유자 기준의 고유한 파일·디렉터리 경로를 64개까지 받으며, 각
경로는 UTF-16 코드 단위 1024개까지입니다. 호출할 때마다 심볼릭 링크를 거치지 않고 존재해야
하며, 내용은 해시하지 않습니다. 실행 환경, 취소 처리, 원본 출력의 크기 제한은 Runtime 명령과 같으며, 기본 시간 제한은 30,000ms입니다(`timeout_ms`는 1–2,147,483,647). 별도의 출력·임시
디렉터리는 실행이 끝나면 지웁니다. stdout은 `[A-Za-z0-9._:-]`로 이뤄진 1–128자의 ASCII여야
하며, 그 뒤에 LF 하나(Windows에서는 CRLF 하나)만 올 수 있습니다. 그 밖의 공백은 허용하지 않고
앞뒤 공백을 지워 주지도 않습니다. 종료 코드가 0이 아니거나, 출력 형식이 잘못됐거나, 시간이
초과됐거나, 취소됐거나, 파일이 없거나, 정리에 실패하면 동작 오류로 준비를 멈추며, eval도, 캐시 없는
리뷰도 시작하지 않습니다. stderr는 fingerprint 진단으로 전달하지 않습니다.

runtime이나 Agent 리뷰가 끝나면 GREEN이나 RED 판정을 확정하기 전에 포함한 fingerprint를 다시
계산합니다. 값이 바뀌었으면 판정 없이 ERROR/INPUT_CHANGED를 기록하고, 재확인에 실패하면
ERROR를 기록합니다. force를 줘도 준비와 재확인은 건너뛰지 않습니다. `fingerprint = false`인
Artifact는 준비할 fingerprint가 없으며, 키 없는 eval은 fingerprint 값 재확인을 건너뜁니다. 파일
대상 확인은 키 없는 eval을 포함해 준비, eval 시작과 끝, 도구 호출 때 항상 합니다. 필요한 다른
Artifact의 fingerprint는 그래도 준비할 수 있습니다. 작업 공간을 지켜보지는 않습니다.
artifactsum은 준비와 재확인 때만 해시합니다.

## 명령 안의 Artifact 참조 {#artifact-references-in-commands}

runtime 인자, fingerprint 스크립트, 도구 인자는 `{name}`, `{name}/path`,
`--flag={name}/path`로 다른 Artifact를 가리킵니다. `{owner}/child/path`는 폴더 Artifact에서
시작하고, 파일 참조는 파일 자체이므로 경로를 붙이지 않습니다. 참조는 명령이 실행될 때 절대
경로로 바뀝니다. 지시문과 runtime 인자에서 참조하면 eval이 그 Artifact에 의존하게 되고,
fingerprint 스크립트에서 참조하면 스크립트가 그 Artifact를 읽을 수 있게 될 뿐입니다. 도구
인자에는 그 eval이 이미 참조한 Artifact만 쓸 수 있습니다. 나머지는 적은 그대로 전달합니다.
이스케이프한 중괄호, 두 겹 중괄호, `${variables}`, JSON, 짝이 맞지 않는 중괄호는 쓴 대로
남으며, artifactize는 문자열로 명령줄을 만들지 않습니다. eval은 그 대상과 참조한 Artifact를
자식까지 읽을 수 있습니다. 참조한 Artifact의 eval 지시문은 리뷰에 더해지지 않습니다. 프로젝트
전체의 기준은 [`graph`](tools/builtin.md#current-project-graph) 도구로 볼 수 있습니다.

참조의 경로는 절대 경로일 수 없고, `..`로 올라가거나, 빈 부분, 백슬래시, 콜론, 제어 문자를
담거나, 4096자를 넘을 수 없습니다. 직접 선언하는 경로(`fingerprint.files`,
`fingerprint.script.files`, `execution_paths`)는 1024자까지입니다. `config check`는 eval마다
선언한 프로필의 이름과 문법을 확인합니다. `profile_variants` 항목에만 나오는 참조는 그 변형을
고를 때 확인하므로, 변형을 쓰기 전에 `status SELECTOR --profile NAME`을 실행하세요. 참조한 파일이
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
`TMPDIR`/`TMP`/`TEMP`를 받습니다. 이 폴더들은 나만 읽을 수 있고, 리뷰하는 프로젝트 안에 두지
않습니다.

명령은 작성자의 환경에서 실행되므로, 다른 터미널 명령과 마찬가지로 실행 환경에 따라 결과가 달라질 수 있습니다.
환경은 [재사용 키](fingerprint.md#the-reuse-key)에 들어가지 않으므로 도구를 업그레이드해도 캐시
결과가 무효가 되지 않습니다. 도구 버전이 바뀔 때 fingerprint도 바뀌어야 한다면, fingerprint 계산에 도구 버전을 넣으세요.

runtime의 제한 시간은 기본 30,000ms이며, 1~2,147,483,647ms로 설정할 수 있습니다. 결과에는 stdout과
stderr의 앞부분 128 KiB씩, 잘렸는지 표시, 종료 상태, 걸린 시간이 남습니다. 텍스트는 UTF-8로
읽고(잘못된 바이트는 대체), 터미널 색상 코드와 탭, LF, CR을 뺀 ASCII 제어 문자를 지웁니다(DEL은
남깁니다). 전체 스트림은 Run 출력에 남으며, `request logs`와 MCP `request_logs`로 로그의 끝부분, 지정한 범위,
검색 조건에 맞는 줄을 읽을 수 있습니다. 캡처를 지우면 저장된 앞부분 128 KiB만 남습니다. `artifactize prune`은
Run의 `output` 폴더를 지우지만 이 스트림 캡처는 남깁니다. 캡처가 없거나 일부만 남았거나 이전 버전·원격 실행의 기록만 있으면 그 상태를 알려 줍니다.

명령이 정상으로 끝나든, 취소되거나 시간이 초과되든 그 명령이 시작한 프로그램도 함께 멈추므로,
백그라운드에서 시작한 것이 명령보다 오래 남지 않습니다. 계속 실행되어야 하는 보조 프로그램에는
Agent [도구 세션](tools/agent.md#tool-sessions)이나 Human [launch 도구](tools/human.md#running)를
쓰세요. 명령은 신뢰하는 로컬
코드이며 **샌드박스가 아닙니다**. 내 계정이 할 수 있는 일은 무엇이든 할 수 있고, 리뷰하는
입력을 바꾸면 안 되며, 부모 프로세스와 일부러 분리해 따로 실행되는 프로그램은 계속 실행될 수 있습니다.
