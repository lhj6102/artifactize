# 선언 (.artf) {#declarations-artf}

TOML 선언, 폴더와 파일 Artifact, fingerprint,
eval 의존성, 범위가 정해진 참조, runtime 실행 환경에 대한 정확한 규칙입니다.
개념 소개는 [Artifact와 eval](artifact.md)에 있습니다.

## 제거된 의존성 선언 옮기기 {#migrating-removed-dependency-declarations}

`kind = "dependency"`와 `depends_on`은 제거되었습니다. 의존성 eval을 지우고,
필요한 Artifact를 그것을 쓰는 eval의
`payload.instruction`(또는 runtime `args`)에 `{name}` 참조로 넣으세요. 예를 들어 `spec`과 `assets`를
나열하는 파생 `ready` eval 대신, 실제 리뷰 지시문을
`"Review {app} against {spec} and {assets}."`처럼 쓰세요. 그것을 쓰는 각 eval은 그
입력에 필요한 eval을 기다리고, 독립적인 형제 eval은 계속 실행됩니다.

자식 폴더는 부모의 모든 eval이 참조하는 것으로 칩니다. 어떤 eval 전에
다른 Artifact의 리뷰가 필요하다면 그 eval의 지시문이나 argv에서
그 Artifact를 명시적으로 참조하세요.

예전 프로필은 설정 파싱에서
`unknown variant `dependency`, expected one of `agent`, `human`, `runtime``로 실패합니다.
지원되는 프로필의 `depends_on` 필드는 알 수 없는 필드 오류입니다. 이는
리뷰 판정이 아니라 설정 오류이므로, 검증하기 전에 선언을 옮기세요.

## 선언 검증 {#declaration-validation}

`index.artf`는 자신이 들어 있는 폴더를 선언합니다. `<filename>.artf`는
확장자를 포함한 전체 이름이 `<filename>`인 옆 파일을 선언합니다.
둘 다 표준 TOML을 쓰며 `name`이 필요합니다. 저장소 루트의 이름 없는 `.artf`는
소스 파일 없이 [graph Artifact](#graph-artifact)를 선언합니다.

탐색은 저장소 루트에서 시작해 `.git`, `node_modules`, 그리고
선택 사항인 루트 `.artfignore`(gitignore 문법)에 나열된 하위 디렉터리와 사이드카 경로를
건너뜁니다. 방문한 모든 디렉터리에서 `index.artf`는 그 파일 이름만 맞추는 무시 패턴이
있어도 읽힙니다. 하위 프로젝트의 폴더
선언을 숨기려면 대신 그 상위 디렉터리를 탐색에서 제외하세요. 무시된
사이드카는 파싱하지 않고 대상도 확인하지 않습니다. 다른 선언이 포함하지 않는 한
그 대상은 일반 파일로 남습니다. 별도 프로젝트, 픽스처, 예제가 들어 있는 폴더를
나열하세요.

```gitignore
crates/app/tests/
examples/
```

이름, eval ID, 도구/세션 이름, 프로필 변형 이름은
`[A-Za-z0-9][A-Za-z0-9_-]{0,63}`에 맞아야 합니다. Artifact 이름은 전체에서 고유합니다. eval은
배열이 아니라 ID를 키로 하는 테이블입니다. `app`의 `[evals.check]`는 `app/check`가 됩니다.
명시적인 eval `id` 필드는 거부됩니다. 각 선언 안의 eval은 TOML 소스에서의 위치가 아니라
ID 순서로 정렬됩니다.

| 필드 | 규칙 |
|---|---|
| `name` | 필수 Artifact 이름 |
| `tags` | 선택. ASCII 제어 문자가 없는, 비어 있지 않은 고유 문자열 최대 64개의 목록 |
| `basis` | 선택 불리언. `true`는 eval 없는 입력을 선언하며, 자기 리뷰 의무는 면제하지만 의존성은 면제하지 않습니다 |
| `fingerprint` | 생략하면 artifactsum. `false`는 재사용을 끔. 그 밖에는 artifactsum 또는 script 테이블 |
| `views.agent_tools`, `views.human_tools` | 선언한 도구의 선택적 맵 |
| `views.agent_sessions.<name>.start`, `.stop` | 도구 세션 명령. `command`와 `args`는 필수, `execution_paths`는 선택. 다른 필드는 없음 |
| `views.agent_tools.<name>.session` | 같은 Artifact에 선언된 도구 세션의 선택적 이름. 명령 Agent 도구 전용 |
| `evals.<id>.title` | 필수, 비어 있지 않은 제목 |
| `evals.<id>.gate` | 선택 `"blocking"`. `--defer-human`에서도 Human 승인을 엄격하게 유지. 생략하면 요청 모드를 따름. 다른 값은 없음 |
| `evals.<id>.mutex` | 고유한 계층형 mutex 태그 최대 64개의 선택적 목록. runtime과 Agent eval 전용. 기본값 없음 |
| `evals.<id>.profile` | 필수 `runtime`, `agent` 또는 `human` 프로필 |
| `evals.<id>.payload.instruction` | runtime, Agent, Human eval에 필수인 비어 있지 않은 지시문 |
| `evals.<id>.pass_schema`, `fail_schema` | 실행형 eval의 선택적 소유자 필드 JSON Schema. Human eval은 [양식 형태](#human-result-schemas)만 받음 |
| `evals.<id>.profile_variants` | 기본 kind를 유지하는, 이름 붙은 완전한 프로필 최대 64개 |

[도구 세션](tools/agent.md#tool-sessions)은 리뷰 하나가 진행되는 동안 하네스를 계속 실행해 둡니다.
시작, 중지, 정리, 비정상 종료 복구는 Artifactize가 맡으며, 선언에는
수명 주기나 정리 시점 옵션이 없습니다. 링크한 레퍼런스에 전체 계약이 있습니다.

모든 선언 키는 snake_case입니다: `timeout_ms`, `max_tool_calls`, `max_tokens`,
`input_schema`, `execution_paths`, `pass_schema`, `fail_schema`,
`profile_variants`, `gate`, `mutex`.
이런 타입이 정해진 경계에서 알 수 없는 필드와 camelCase 표기는 거부됩니다.
`additionalProperties`, `minLength`, `maxItems` 같은 JSON Schema 키워드는
표준 이름을 그대로 씁니다. 소유자가 정의한 payload 필드는 정한 이름을 그대로 씁니다.
`--json`, 도구 프로토콜, 저장된 상태는 `timeoutMs`, `passSchema`, `agentTools`, `blockedBy` 같은
기존 camelCase 필드를 그대로 씁니다. 이것은
선언 문법이 아닙니다.

인라인 테이블, 점으로 구분한 키, 여러 줄 문자열은 표준 TOML입니다.

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
`type = "null"`이라고 쓸 수는 있지만, null `const` 같은 리터럴 null 값은 쓸 수 없습니다.
따옴표 없는 TOML 날짜/시각과 유한하지 않은 숫자(`inf`, `nan`)는 JSON에 대응하는 값이 없으므로
payload와 스키마를 포함해 어디서든 거부됩니다.
날짜를 문자열로 쓰려면 따옴표로 감싸세요. 선언 오류는 내용을 파싱하거나 검증할 때
`.artf` 파일, 줄, 열을 알려 줍니다. 문법, 역직렬화, 의미 검증,
Artifact 간 참조 오류가 모두 해당합니다. 필드 검증 오류와 지원되지 않는 값 오류는
`evals.check.profile.args` 같은 타입이 정해진 키 경로도 알려 줍니다.

`config check`는 fingerprint 스크립트를 실행하거나 파일을 해시하지 않고 검증합니다.
선언을 다시 작성한 뒤 쓰세요. 예전 선언 파일과 무시 파일은
읽지 않습니다. 무시되지 않은 예전 선언 파일이 있으면 탐색이 실패합니다. 예전
선언 파일 이름을 가진 디렉터리는 디렉터리 무시 패턴이 제외하지 않는 한
보통대로 탐색합니다. `.artfignore`로 예전 파일을 하나씩 무시하거나 그 폴더 전체를
건너뛸 수 있습니다. 저장소 루트의 예전 무시 파일은 항상 실패합니다. 진단 메시지는
다음과 같습니다.

```text
artifactize.json is no longer read; declare this Artifact in index.artf (TOML).
.artifactizeignore was renamed to .artfignore.
```

변환기는 없습니다. 에이전트에게 선언을 다시 작성하게 하고 새
`.artf` 파일을 커밋하세요. Family는 제거되었습니다. 반복되는 선언은 직접 만든
프로젝트 생성기로 생성하고 그 `.artf` 출력을 커밋하세요. `staleKey`, `stale`, `critics`, `envRequirements`, `resultCheck`,
`reviewPolicy.maxConcurrentExecutors`, 도구 메타데이터 `observation` 같은 예전 필드는 거부되며,
새 필드의 별칭이 아닙니다. artifactsum 형식의 예전 `dependencies`
옵션도 거부됩니다.

## 소유자 결과 스키마 {#owner-result-schemas}

Agent와 Human 결과에는 `verdict`와 소유자 필드가 들어 있습니다. `pass_schema`(GREEN)와
`fail_schema`(RED)에는 그 소유자 필드만 선언하세요. `verdict`는 artifactize가
채웁니다. 예를 들어 위의 `summary` 스키마는 Agent가 보낸
`{"verdict":"GREEN","summary":"All requirements are covered."}`를 받아들입니다.
Human은 양식 필드로 `{"summary":"All requirements are covered."}`만 입력하고,
GREEN은 따로 선택합니다.

결과를 담는 최상위 JSON 객체에는 일부러 일반 JSON Schema보다 엄격한 제약을 둡니다.

- `type`이 있다면 `"object"`여야 합니다. `additionalProperties`가 있다면
  `false`여야 합니다. 생략해도 artifactize는 최종 결과 객체에서 객체 타입과 닫힌 속성을
  모두 강제합니다.
- 허용되는 모든 최상위 소유자 필드는 `properties`에 명시적으로 있어야 합니다.
  `patternProperties`나 `$ref`로 선언되지 않은 최상위 필드를 허용할 수 없습니다.
- 최상위 `allOf`, `anyOf`, `oneOf`, `not`은 거부됩니다. 대신 소유자 필드 안에서
  스키마를 조합하세요. 중첩 객체, 배열, 로컬 참조는
  스키마 검증기가 지원합니다.
- 예약된 이름은 선언하거나 필수로 지정할 수 없습니다: `verdict`, `reference`,
  `reusedFrom`, `executionProvenance`, `attemptId`, `provider`, `model`, `stdout`,
  `stderr`, `durationMs`, `exitCode`. 감사 데이터와 실행 데이터는 소유자
  필드 밖에 있습니다.
- 한 판정의 스키마를 생략하면 그 판정에는 **소유자 필드가 하나도 허용되지 않습니다**.
  Agent는 `{"verdict":"GREEN"}` 또는 `{"verdict":"RED"}`만 돌려주고,
  Human 양식 필드는 `{}`입니다. 빈 객체 스키마를 써도 마찬가지로 선언하지 않은 최상위 필드는 허용되지 않습니다.

### Human 결과 스키마 {#human-result-schemas}

사람은 Human 결과를 양식에 입력하므로, Human eval의 `pass_schema`와
`fail_schema`는 양식으로 입력할 수 있는 형태만 쓸 수 있습니다. 그 밖의 것은 `.artf`를 불러올 때
선언 오류가 되며, 오류는 키 경로와 대신 허용되는 것을 알려 줍니다.

| 소유자 필드 스키마 | 리뷰에서 보이는 방식 |
|---|---|
| `{ type = "string" }`, 선택적으로 `minLength`, `maxLength` | 텍스트 상자 |
| `{ type = "integer" }` 또는 `"number"`, 선택적으로 `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf` | 숫자 상자 |
| `{ type = "boolean" }` | true / false (선택 필드면 – 도) |
| `{ enum = ["a", "b"] }`, 문자열만, `type = "string"`은 선택 | 선택지 |
| `{ const = … }` | 표시 없음: 값이 채워짐 |
| `{ type = "array", items = { type = "string" } }`, 항목에 선택적으로 `minLength`, `maxLength` | 텍스트 상자 목록 |
| `{ type = "array", items = { type = "object", properties = … } }`이고 속성이 위의 스칼라인 경우 | 그 필드들의 반복 그룹 |

배열은 `minItems`와 `maxItems`를 받습니다. 결과 객체와 각 그룹의 객체는
`type = "object"`, `properties`, `required`, `additionalProperties`를 받으며, `required`의
모든 이름은 선언된 속성이어야 합니다. 어떤 스키마든 `title`,
`description`, `$comment`를 추가할 수 있고, 속성의 `description`은 힌트로 표시됩니다.
필수 필드에는 `*` 표시가 붙습니다. 비어 있는 선택 필드에는 *Optional*이 표시되고
결과에서 빠지며, 빈 목록 항목과 아무것도 입력하지 않은 그룹도 마찬가지입니다.
데스크톱 앱은 `+ Add`와 ✕ 버튼으로 항목과 그룹을 추가하고 제거하며,
터미널 UI는 [Review 양식](../tui/index.md)에 설명된 대로 키를 씁니다.
패턴, 포맷, 중첩 객체, 그룹 안의 목록, `$ref`, `anyOf` 같은 조합자는
거부됩니다. 사람이 입력할 수 없기 때문입니다. Agent 스키마는 전체 검증기를
그대로 씁니다.

스키마는 8 MiB로 제한됩니다. Agent의 원본 최종 응답은 1 MiB로 제한되고,
정규화된 결과는 UTF-16 코드 단위 256000개로 제한됩니다. 잘못된 최종
응답에는 필드를 몰래 지우는 대신 횟수가 제한된 복구 시도가 한 번 주어집니다.
[Human 제출 인터페이스](../cli/index.md#command-reference)는 따로
입력 크기를 제한하며, 요청을 확정하지 않고 검증 오류를 보고합니다.

## Graph Artifact {#graph-artifact}

저장소 루트의 선택적인 이름 없는 `.artf`는 프로젝트 전체의
Artifact 그래프를 선언합니다. 그래도 `index.artf`는 루트 폴더를 선언하고, `<filename>.artf`는
옆 파일을 선언합니다. 방문한 다른 디렉터리의 이름 없는 `.artf`는
선언 오류입니다. 루트 `.artf`는 `index.artf`처럼, `.artfignore` 패턴이 그 파일 이름과 맞아도
읽힙니다. graph Artifact는 하나만 있을 수 있습니다.

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

graph Artifact는 이름이 있는 일반 Artifact입니다. 어떤 runtime, Agent, Human
eval과 도구든 선언할 수 있고, `verify`, `status`, CI, `--reuse-only`에서 이름으로 선택할 수 있습니다.
기본 eval도, 자식 소유도, 다른 Artifact와의 관계도 없습니다. 명시적인
지시문/argv 참조는 보통대로 동작합니다. 명령은
저장소 루트에서 실행되지만, 그래프를 선언했다고 그래프가 그 폴더의 소유자가 되지는 않습니다.
그 범위는 `kind = "graph"`를 쓰고 선언 경로는 `.artf`입니다.

`false`나 빈 테이블이라도 `fingerprint`를 선언하지 마세요. 그 fingerprint는
언제나 현재 추출된 그래프입니다. 그 밖에는 일반 Artifact 규칙이 적용됩니다.
`basis = true`는 자기 리뷰 의무가 없음을 명시적으로 선언하며 eval을 가질 수 없고,
충족되지 않은 의존성을 충족시키지도 않습니다. `basis = true`가 없는 eval 없는 그래프는
다른 eval 없는 Artifact와 마찬가지로 UNREVIEWED로 남습니다. 소스 파일은
만들어지지도 필요하지도 않습니다. 일반 Artifact 사이의 순환은 허용됩니다. 추출은 평면적이며
graph Artifact의 선언도 재귀적인 fingerprint 계산 없이 포함합니다.

[그래프 fingerprint](fingerprint.md#graph-fingerprint)는
소스 파일 바이트나 평가 실행 옵션이 아니라 구조 정의에 따라
바뀝니다. diff, 이력, 자동 리뷰 정책은 없습니다. 결과는
현재 상태에 대해서만 리뷰되거나 재사용됩니다.

## 파일 Artifact {#file-artifacts}

`hero.png.artf`는 같은 폴더의 일반 파일 `hero.png`를 선언합니다. 그
`name`은 필수이며 주변 폴더 Artifact와 무관합니다. 존재하지 않는
대상, 디렉터리, 심볼릭 링크, 특수 파일은 올바른 대상이 아닙니다.
`:`가 들어간 대상 파일 이름은 범위가 정해진 도구
경로로 나타낼 수 없으므로 탐색 단계에서 거부됩니다.
`.artf` 선언 자체도 일반 파일이어야 합니다. 심볼릭 링크, 특수 파일,
`bundle.artf` 같은 디렉터리는 탐색에 실패합니다. `index.artf`는 언제나 폴더를 선언하므로 `index`라는 파일에는 사이드카를 둘 수 없습니다.
`.artf` 파일은 선언이며 결코 대상이 되지 않습니다.

파일 Artifact는 자신이 들어 있는 폴더 Artifact와 자식 관계나 의존성 관계를
만들지 않습니다. 하위 폴더의 `index.artf` 선언에는 여전히 가장 가까운 폴더가 소유한다는 규칙이
적용됩니다. 폴더의 artifactsum에는 옆에 있는 파일
Artifact 대상이 포함되고 모든 `*.artf` 선언은 제외됩니다.

들어 있는 폴더가 runtime 명령, 도구,
fingerprint 스크립트의 작업 디렉터리이므로, 상대 경로 실행 파일은 형제 파일이어도 됩니다. 그렇다고
다른 형제 파일에 접근할 수 있는 것은 아닙니다. 내장 도구와 범위가 정해진 argv 참조는
대상 파일과 참조된 Artifact만 봅니다. `{name}`은 폴더가 아니라 대상 파일 경로로 해석되며,
파일 참조에는 `/path` 접미사를 붙일 수 없습니다. `list .`은
가상 파일 Artifact 루트를 가리키고 대상 파일 이름을 나열합니다. 그 루트 안의 경로는
들어 있는 폴더 기준 상대 경로로 남습니다. 기본 artifactsum은
대상 파일만 포함합니다. 워크스페이스에는 파일 Artifact만 있어도 되며, 폴더
선언이 필요 없습니다.

파일을 해시하려면 `fingerprint`를 생략하세요. 명시적인 artifactsum 형식은
`hero.png.artf`의 경우 `fingerprint = { files = ["hero.png"] }`여야 합니다. 대상
파일 이름만 허용됩니다. `fingerprint = {}`와 `files = ["."]`는 거부되며,
빈 목록이라도 `fingerprint.ignore`도 거부됩니다. script fingerprint와
`fingerprint = false`는 그대로 쓸 수 있습니다.

JSON 명령 도구의 `context.artifactPath`는 파일을 가리키고, 프로세스 cwd는
그 파일이 들어 있는 폴더입니다. 범위 항목에는 `kind = "file"`, `"folder"`, `"graph"`가 붙습니다.
`execution_paths`는 세 kind 모두에서 워크스페이스 기준 출처 고정 경로로 남습니다.

대상은 탐색 시점뿐 아니라 계속 심볼릭 링크를 거치지 않는 기존 일반 파일로
남아 있어야 합니다. fingerprint 준비/재확인 때, 재사용 전에,
eval 시작/종료 때, 그리고 모든 Agent/Human 도구 호출 때 검증되며
`fingerprint = false`에서도 마찬가지입니다. eval을 시작하기 전에 파일이 잘못되면
ERROR/PREPARATION_FAILED가 됩니다. 완료된 Runtime/Agent 결과를 확정하다가
대상이 잘못된 상태가 된 것을 발견하면, 의미상 판정 대신 ERROR/INPUT_CHANGED를 기록합니다.
Human 동작은 먼저 기록된 범위와 선언을 다시 불러옵니다.
파일 대상이 없거나 잘못되어 탐색이나 재연결에 실패하면
그 동작은 대기 중인 요청을 확정하지 않고 실패합니다. Human 재연결에 성공한 뒤
fingerprint 값이 바뀌었다면, 키가 있는 요청은 여전히
ERROR/INPUT_CHANGED로 확정됩니다. 준비 실패는 재사용이나 실행 전에 중단되며,
도구 호출은 도구를 실행하지 않고 오류를 돌려줍니다.

제거된 예전 정책 필드와 그 대체 방법은
[업그레이드 레퍼런스](../getting-started/install.md#removed-gate-bypass-settings)를 보세요.

## Eval mutex 태그 {#eval-mutex-tags}

runtime 또는 Agent eval은 `gate`, `profile` 옆에 `mutex`를 선언할 수 있습니다.

```toml
[evals.suite-passes]
title = "The end-to-end suite passes"
mutex = ["use-computer:display", "use-computer:callback"]
profile = { kind = "runtime", command = "pnpm", args = ["e2e"], timeout_ms = 1800000 }
payload.instruction = "Run the end-to-end suite."
```

태그는 `use-computer`이거나 `use-computer:display`처럼 콜론으로 구분한 하위 태그입니다.
하위 태그의 각 부분은 `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`에 맞아야 합니다.
다른 루트, 빈 부분, 잘못된 부분, 중복 태그, Human eval에 대한 선언은 오류입니다.
`mutex`를 생략하면(또는 `mutex = []`로 선언하면) 아무 제약도 추가되지 않습니다.

두 eval은 태그 중 하나라도 같거나 한쪽이 다른 쪽의 상위 태그이면 충돌합니다.
`use-computer`는 모든 하위 태그와 충돌합니다.
`use-computer:db`는 `use-computer:db:users`와 충돌하지만
`use-computer:browser`나 `use-computer:database`와는 충돌하지 않습니다. eval의 태그는 모두
한꺼번에 획득하므로 일부만 획득해서 교착 상태가 생기는 일은 없습니다. 충돌하는
eval이 실행 중이지 않을 때까지 기다립니다. 이 규칙은 우선순위, 순서, 스케줄링 최적화를 추가하지 않습니다.

잡은 태그는 머신의 공유 상태 저장소에 실행 소유권과 함께 원자적으로 기록됩니다.
그래서 한 Run 안뿐 아니라, 그 저장소를 쓰는 여러 Run, 워크트리, verify 프로세스에 걸쳐
적용됩니다. 기다리는 요청은 `QUEUED`로 남고, 그
`blockedReason`이 태그를 잡고 있는 eval과 Run을 알려 줍니다. 기다리는 동안 실행
예산, 백엔드 슬롯, 작업 슬롯은 쓰지 않습니다. `timeout_ms`는 기다리는 동안이 아니라 eval이 실제로
시작할 때부터 셉니다. 태그는 완료나 취소 때 놓이며, 종료된 프로세스 소유자나
끝난 Run은 태그를 계속 잡고 있을 수 없습니다. 따로 명시적으로 고른
상태 디렉터리는 공유 잠금 이름공간이 아니라 서로 격리된 저장소입니다.

태그는 eval의 명령이나 도구를 나타내며, 결코 그 Artifact를 나타내지 않습니다. 테스트 코드를
읽기만 하는 Agent는 다른 eval이 그 테스트를 실행한다고 기다릴 필요가 없습니다. 암묵적인
태그는 없습니다. mutex 태그는 평가 기준이 아니라 실행 옵션입니다.
바꿔도 fingerprint나 재사용 키가 바뀌지 않으며, 충돌하는 eval이 실행 중이어도
캐시된 결과는 재사용될 수 있습니다. Human 요청을 맡으려 할 때 다른 eval이
mutex 태그를 쓰고 있으면 맡기를 미루는 동작은 아직 구현되어 있지 않습니다.

## Human 관문 정책 {#human-gate-policy}

`gate = "blocking"`은 프로필이나 전역 설정이 아니라 eval의 선택 필드입니다.
`verify --defer-human`에서도 Human eval의 대기 중인 관문을 유지합니다. 생략하면
기본 Run에서는 평소의 엄격한 동작을 유지하고, 요청이 선택했을 때만 미루기를
허용합니다. runtime이나 Agent eval에서는 추가
효과가 없습니다. 미룰 수 있는 것은 Human 승인뿐입니다. 관문 정책은
판단의 정의나 재사용 키가 아니라 스케줄링을 바꿉니다.

```toml
[evals.signoff]
title = "The delivery is approved"
gate = "blocking"
profile = { kind = "human" }
payload.instruction = "Approve the delivery before dependent work proceeds."
```

[verify 모드](../cli/index.md#verify-modes)와
[미룬 승인](human.md#deferred-sign-offs)을 보세요.

## Eval 의존성 {#eval-dependencies}

Artifact 참조는 `payload.instruction`과 runtime `profile.args`에
`{input}` 또는 `{input}/path`로 쓰세요. 그 의무, 자식
Artifact, 순환, 스케줄링, Human 선행 조건은
[eval 의존성](artifact.md#eval-dependencies)을 보세요.

## Artifactsum {#artifactsum}

[Fingerprint와 재사용](fingerprint.md#artifactsum)에서
`files`와 `ignore`를 설명합니다. `fingerprint`를 생략하면 artifactsum을 씁니다.
폴더에서는 `fingerprint = {}`도 같습니다. 파일 Artifact의 명시적인 artifactsum
형식에는 대상 파일 이름을 나열해야 합니다. `fingerprint = false`는 재사용을 끄고,
`fingerprint = true`는 거부됩니다.

값은 `artifactsum:` 뒤에, 각 입력 파일의 소유자 기준 상대
경로와 그 바이트의 SHA-256을 경로 순서대로 이어 계산한 SHA-256을 붙인 것입니다. Artifact의
파일만 포함합니다. 의존성의 변경은
[재사용 키](fingerprint.md#the-reuse-key)에 들어 있는 의존성의 fingerprint를 통해 eval에 전해집니다. 폴더 탐색은
자식 Artifact 폴더와 모든 `*.artf` 파일을 제외합니다. `fingerprint.files`에
`.artf` 파일을 명시적으로 나열하면 그 파일이 없더라도 `config check`가 실패합니다.
탐색은 디렉터리를 포함해 일반 파일이 아닌 `.artf` 항목도 거부합니다.
`bundle.artf`라는 디렉터리는 해시할 수 있는 일반 입력이 아닙니다. 파일 Artifact는
기본적으로 대상만 해시합니다. 태그는 해시되지 않으며 eval 정의 해시나
재사용 키를 바꾸지 않습니다.

탐색은 범위가 정해진 경로 규칙을 따릅니다. 무시 대상이 아닌 심볼릭 링크나
특수 파일을 만나면 안전을 위해 오류로 중단합니다. 적용되는 모든 `.gitignore`는 그 자신이
내용으로서 무시되더라도 규칙 메타데이터로 읽히므로, 심볼릭 링크가 아닌 일반 파일이어야 하고
1 MiB 이하여야 합니다. 소유자 밖으로 따라 나가는 일은 없으며, 탐색은 항목 10,000개나 1 GiB를
넘으면 오류로 멈춥니다. 해시는 준비 때와 리뷰 종료 시 재확인 때
계산합니다. 그래서 Python의 `__pycache__`처럼 리뷰가 무시된 경로에 쓰는 파일은
결코 `INPUT_CHANGED`를 일으키지 않습니다. Artifactsum은 실행과 함께 매니페스트를 기록합니다.
파일별 다이제스트(16자리 16진수)와 입력 다이제스트입니다. 64 KiB를 넘을
파일 맵은 매니페스트에서 빠지지만 여전히
fingerprint에는 포함됩니다. `status`는 이 매니페스트를 현재 매니페스트와 비교하고, 키에
기록된 `fingerprints`를 현재 의존성 fingerprint와 비교합니다.

## Fingerprint 스크립트 {#fingerprint-scripts}

eval을 실행하기 전에 `verify`는 선택된 eval에 필요한 의존성 범위 전체에서 fingerprint를 계산합니다.
eval이 선택되지 않은 의존성도 포함됩니다.
fingerprint는 한 번에 최대 `--fingerprint-jobs N`개까지
계산하며(기본값: 프로세스가 쓸 수 있는 CPU 수), `status`에서도
`verify`와 같습니다. Run은 그 한도를 준비 단계와
Runtime/Agent 리뷰 종료 시 재확인이 나눠 씁니다. **Human 도구 동작과 제출은
별개의 작업입니다**. 각각 Run의 `--fingerprint-jobs` 설정과 무관하게
CPU 수만큼의 fingerprint 병렬성을 따로 가집니다.
값은 완료 순서가 아니라 Artifact를 키로 저장됩니다. 실패하면 이름 순서로 처음
실패한 Artifact를 보고하고 그 뒤의 fingerprint를 취소합니다. 스크립트는
각각 별도의 전용 출력 디렉터리에서 실행됩니다. `--fingerprint-jobs 1`은 그 Run의
준비와 Runtime/Agent 재확인을 직렬화하지만, 모든 Human 확인이나 다른 Run까지 직렬화하지는 않습니다.

Artifact의 모든 실행형 eval은 같은 리터럴 값을 받으며, 그 값은
요청과 Run의 Artifact 검증에 저장됩니다. artifactize는 스크립트 출력에 저장소,
eval, 프로필, 도구 뷰, 의존성, 내용에 따른 솔트를 추가하지 않습니다. eval의
[재사용 키](fingerprint.md#the-reuse-key)는 이 값을
Artifact 이름과 kind(`folder`/`file`/`graph`), 의존성 fingerprint, eval
정의 해시와 결합합니다. 물리적 경로는 재사용 키에 절대 들어가지 않습니다. Artifactsum
실패도 같은 방식으로 준비를 중단합니다.

명령은 소유자의 작업 디렉터리에서 stdin으로 JSON
`{"version":1,"artifactId":"example"}`을 받아 실행됩니다. 경로 없는 명령은 PATH로 찾고,
절대 경로 실행 파일은 적힌 그대로 실행합니다. `/`가 들어간 상대 경로 실행 파일(예:
`./fingerprint.sh`)은 폴더 Artifact의 eval 범위 안에서 찾습니다. 파일
Artifact에서는 상대 경로 실행 파일을 대신 들어 있는 폴더 아래에서 찾으며,
관련 없는 형제 파일을 가리켜도 됩니다. 두 경우 모두 경로 탈출과 심볼릭 링크는 거부됩니다.
인자는 명시적인 범위 지정 Artifact 참조를 빼고는 리터럴로 남습니다. `{name}`은
전역 Artifact 이름입니다. 참조된 각 Artifact는 자식 범위 전체와 함께
그래프 관계가 되지 않고 스크립트의 범위에 들어갑니다.
`config check`는 알 수 없거나 형식이 잘못된 참조를 거부하며, 존재 여부와 심볼릭 링크는
명령을 준비할 때 확인합니다. 인터프리터별
플래그, 래퍼, 진입 파일 규칙은 없습니다.

`fingerprint.script.files`는 각각 UTF-16 코드 단위 1024개 이하인, 소유자 기준 상대
리터럴 파일/디렉터리 경로를 최대 64개까지 고유하게 받습니다. 이 경로는 호출할 때마다 심볼릭 링크를 거치지 않고 존재해야 하며,
그 내용은 결코 해시되지 않습니다. 명령은 runtime 환경, 취소,
크기가 제한된 원본 출력, 기본 30,000 ms 시간 제한을 공유합니다(`timeout_ms`는
1–2,147,483,647을 받음). 전용 출력 디렉터리와 임시 디렉터리는
호출마다 끝나면 제거됩니다. stdout은 정확히
`[A-Za-z0-9._:-]`의 ASCII 문자 1–128개여야 하며, 뒤에 LF 하나(Windows에서는 CRLF 하나)가 올 수 있습니다. 다른
공백은 허용되지 않으며 값을 다듬지도 않습니다. 0이 아닌 종료, 잘못된 출력,
시간 초과, 취소, 없는 파일, 정리 실패는 어떤 eval도 시작하지 않고, 캐시되지 않은 리뷰로 대체하지도 않은 채
운영 오류로 준비를 중단합니다.
stderr는 fingerprint 진단으로 전달되지 않습니다.

runtime이나 Agent 리뷰가 끝나면 GREEN이나 RED를 확정하기 전에 포함된 fingerprint를
다시 계산합니다. 값이 바뀌었으면 의미상 결과 없이 ERROR/INPUT_CHANGED를 기록하고,
재확인에 실패하면 ERROR를 기록합니다. 강제 실행(force)도 준비나 재확인을
건너뛰지 않습니다. `fingerprint = false`인 Artifact에는 준비할 fingerprint가 없으며,
키가 없는 eval은 fingerprint 값 재확인을 건너뜁니다. 파일 대상 검증은
키가 없는 eval을 포함해 준비, eval 시작/종료, 도구 호출 때 여전히 필수입니다.
필요한 다른 Artifact의 fingerprint는 여전히 준비될 수 있습니다. 워크스페이스 감시는 없습니다. artifactsum은
준비 때와 재확인 때만 해시합니다.

## 범위가 정해진 입력 라이브러리 {#scoped-input-library}

`config::read_workspace_config`(`config check`도 사용)는 가장 가까운
자식 소유를 해석하고, 명시적인 참조를 검증하며, 그래프 스케줄링을 위해 타입이 정해진
입력→소비자 `relations`를 돌려줍니다. 지시문 참조는
전역 Artifact 이름으로 해석됩니다. 백슬래시로 이스케이프한 중괄호, 두 번 쓴
중괄호, `${variables}`, 중첩/JSON 그룹, 짝이 맞지 않는 중괄호는 리터럴로 남습니다.
payload는 결코 바뀌지 않으며 참조가 파일 내용으로 펼쳐지지도 않습니다.

`scope::eval_scope`는 대상과 명시적인 참조, 그리고 그 자식
범위 전체를 eval 범위에 넣지만, 참조된 Artifact의 eval 지시문은 넣지 않습니다.
`Scope::resolve_path`는 논리적 자식 경로를 따라 정규 Artifact
ID를 찾고, `Scope::resolve_input`은 추가로 심볼릭 링크를 거치지 않는 기존 파일/디렉터리를
요구합니다. 논리적 경로는 절대 경로, 경로 탈출, 빈
구성 요소, 백슬래시, 콜론, ASCII 제어 문자, UTF-16 코드 단위 4096개를 넘는 길이를 거부합니다.
선언된 경로(`fingerprint.files`, `fingerprint.script.files`, `execution_paths`)는
더 짧아 각각 최대 1024 단위입니다.

runtime 명령을 준비하기 전에 eval 범위로 `scope::resolve_argv`를 호출하세요.
명시적인 `{name}`, `{name}/path`, `--flag={name}/path` 피연산자는
범위 안의 절대 입력 경로로 해석되며, 지시문 참조가 없어도 의존성을 추가합니다.
폴더 소유자에서 시작하는 논리적 경로에는 `{owner}/child/path`를 쓰세요.
파일 참조는 그 파일 자체로 해석됩니다. 다른 인자(이스케이프된
참조 포함)는 리터럴로 남으며 명령에 값을 끼워 넣는 일은 결코 없습니다. `config check`는
이름과 문법을 검증하지만 runtime 피연산자를 열거나 프로그램을 실행하지는 않습니다.
입력 존재 여부와 심볼릭 링크 확인은 인자를 준비할 때 이루어집니다. 그래프
범위 계산, runtime CLI 실행, Agent 도구 레지스트리가 이 해석기를 씁니다.

## Runtime 실행 라이브러리 {#runtime-execution-library}

`runtime::Command::prepare(program, args, workspace, run_dir, timeout_ms)`는
`runtime::execute`를 위한 호출 하나를 준비합니다. 인자는 리터럴이며 실행기는 결코
셸을 추가하지 않습니다. 기본 cwd는 정규화된 워크스페이스이고, 범위가 정해진 호출자는
`command.cwd`를 Artifact의 작업 디렉터리(파일 Artifact라면 들어 있는 폴더)로
설정할 수 있습니다. `process::run`은 완전한 명시적 환경을 가진 해석된 명령을 위한
하위 수준 API입니다. `verify`는 소유자 작업 디렉터리를 cwd로 씁니다.

명령은 작성자의 터미널 환경에서 실행되므로, 프로젝트는 명령을
터미널에 입력하듯 선언합니다. `artifactize` 프로세스의 환경 전체가 전달되며,
실제 홈과 캐시 위치(`HOME`, `XDG_*`, 그리고 Windows에서는
`USERPROFILE`, `APPDATA`, `LOCALAPPDATA`와 시스템 변수)도 포함됩니다. 그래서 버전 관리자 뒤의
`cargo`나 `pnpm`처럼 이 위치로 찾아가는 툴체인이 터미널에서처럼
동작합니다. artifactize 자체의 변수는 빠집니다. 호출자의 모든 `ARTIFACTIZE_*` 변수
(`ARTIFACTIZE_REMOTE_TOKEN` 같은 자격 증명과 설정)는
제거되고, 명령은 artifactize가 설정해 준 변수만 받습니다. 준비된
각 runtime 명령은 호출자의 외부 `run_dir` 아래에 소유자만 쓸 수 있는 새 디렉터리를 받고,
그 안에 소유자 전용 `output`과 `tmp` 하위 디렉터리가 생깁니다(Unix에서는 0700, Windows에서는 보호된
소유자 전용 DACL). 자식 프로세스는
`ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, `ARTIFACTIZE_TMP_DIR`, 그리고
전용 임시 디렉터리를 가리키는 `TMPDIR`/`TMP`/`TEMP`를 받으며, 이 임시 디렉터리는
명령이 끝나면 제거됩니다. 기존 출력 상위 경로는 만들기 전에 정규화됩니다.
정규화된 워크스페이스 안의 출력은 심볼릭 링크를 거친 경우를 포함해 거부됩니다.
실행 디렉터리는 호출자가 관리합니다. 준비된 출력은 실행 후에도 남아
실행 기록과 명시적인 정리에 쓰입니다. 준비에 실패하면 새로 소유한
호출 디렉터리만 제거하고, 호출자의 루트는 제거하지 않습니다.

명령이 작성자의 환경을 보기 때문에, 결과는 다른
터미널 명령처럼 그 환경에 의존하며, 환경은
[재사용 키](fingerprint.md#the-reuse-key)에 포함되지 않습니다. 툴체인을 업그레이드해도
캐시된 결과는 무효가 되지 않습니다. 도구 버전에 따라 바뀌어야 하는 fingerprint는
그 버전을 직접 읽어야 합니다.

runtime 마감 시간의 기본값은 30,000 ms이며 1부터 2,147,483,647 ms까지 받습니다.
제한 시간은 자식 프로세스를 등록하기 시작할 때부터 재며, 등록이 끝나고 실제로 실행을 시작해도 처음부터 다시 재지 않습니다. stdout과
stderr의 **저장된 감사 기록**은 각각 처음 128 KiB를 보존합니다. Runtime eval은 또한
그 캡처가 정리되거나 제거될 때까지 끝부분을 포함한 전체 원본 스트림을 전용 Run 출력에 보존합니다.
`request logs`/MCP `request_logs`는 크기가 제한된
끝부분/범위/검색 발췌를 읽습니다. 캡처가 사라지면 크기가 제한된 저장 감사 기록만
남을 수 있으며, 페이지를 넘겨도 사라진 끝부분은 복구할 수 없습니다. 스킬이 말하는 "정리 전까지"는
캡처가 남아 있는 동안만 로그를 볼 수 있다는 뜻이며, 모든 정리 명령이 모든 캡처를 지운다는 뜻은 아닙니다.
현재 `artifactize prune`은 인식된 스크래치 하위 디렉터리를 제거하지만,
원본 스트림 파일과 완료 표시는 호출 루트에 남겨 둡니다.
원본 캡처 보존 정책은 제공하지 않습니다. 현재 명령의 이 범위를
Run 출력 자체를 정리하거나 삭제하는 것과 구분하세요.
없는 증거, 일부만 있는 증거, 오래되었거나 원격에 있는 감사 기록뿐인 증거는 명시적으로 보고됩니다. Runtime
결과는 UTF-8을 손실 허용 방식으로 디코딩하고, ANSI CSI 시퀀스와 탭,
LF, CR을 뺀 C0 제어 문자를 제거하며(DEL은 보존), 잘림 표시와 실제 종료
상태/소요 시간을 보존합니다. 하위 수준 프로세스 결과는 바이트를 그대로 둡니다.

취소와 시간 초과는 소유한 자식 프로세스 트리를 멈춥니다. Unix에서 artifactize는
프로세스 그룹에 SIGTERM을 보내고, 최대 1초 뒤 SIGKILL을 보냅니다. Windows에서는
콘솔 그룹에 Ctrl-Break를 시도한 다음, 유예 시간이 지나면 소유한 Job
Object를 종료합니다. 리더 프로세스가 정상적으로 종료해도 그룹이나
Job을 정리합니다. 파이프가 열려 있으면 캡처는 정리 후 최대 1초까지만 기다립니다.
호출자를 버리면 감독자가 취소되며, 감독자는 그래도 정리를 마치고 리더 프로세스를
회수합니다. 설정된 프로그램은 신뢰하는 로컬 코드이며 **OS 샌드박스가 아닙니다**.
일반적인 OS 접근을 쓸 수 있고, 리뷰된 입력을 바꾸지 않아야 하며, 소유한 프로세스 그룹이나 Job을
일부러 떠나는 자손 프로세스는 정리를 피할 수 있습니다. 이
방식은 프로세스를 격리하지 않으며, 악의적인 일시적 쓰기를 모두 잡아낸다고 보장하지도 않습니다.
