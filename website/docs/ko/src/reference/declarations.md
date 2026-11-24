# 선언(.artf) 레퍼런스 {#declarations-artf-reference}

`.artifactize/`가 들어 있는 폴더를 하나의 프로젝트로 봅니다. `.artifactize/<name>.artf` 파일들이 어떻게 Artifact Graph를 이루는지는 [Artifact와 Artifact Graph](../concepts/artifacts.md)를 보세요.

선언 키는 `snake_case`(`timeout_ms`, `pass_schema`, `input_schema`, `execution_paths`)를 씁니다. JSON Schema 키워드(`minLength`, `additionalProperties`)와 JSON으로 직렬화된 선언 필드(`passSchema`, `timeoutMs`, `agentTools`)는 `camelCase`를 씁니다. 그 외 일부 CLI JSON 출력은 `signed_in`이나 `display_name`처럼 명령별 필드 이름을 씁니다.

## 최상위 Artifact 필드 {#declaration-validation}

```toml
# .artifactize/app.artf
path = "crates/app/"
select = ["src", "Cargo.toml"]           # 선택: 폴더 대상을 aspect로 지정
parts = ["app-ui", "app-db"]             # 선택: 명시적 하위 부분 구성
basis = false                            # 선택: eval이 없을 때만 true 가능
tags = ["type:code", "tier:core"]        # 선택: 화면 표시용 메타데이터
fingerprint = { files = ["src"], ignore = ["*.snap"] }  # 선택: 기본값은 artifactsum

[annotations.style]
at = [120, 40]
parts = { app-ui = [40, 80] }

[annotations.comment]
text = "Canvas note shown in the desktop app."
```

| 필드 / 식별자 | 규칙 |
|---|---|
| 식별자 | Artifact 이름은 `.artf` 파일 이름(확장자 제외)으로 정해지며 `.artifactize/` 전체에서 대소문자를 무시하고 비교했을 때 겹칠 수 없습니다(`con`, `nul` 같은 Windows 장치 이름은 피하세요). eval ID는 해당 Artifact 안에서, 도구·세션·대체 실행 프로필 ID는 각 선언 테이블 안에서 겹치지 않아야 합니다. |
| `path` | **필수.** 프로젝트 루트 기준 상대 경로나 Live URI를 지정합니다. 로컬 경로는 `/`를 쓰며, 폴더는 `"dir/"`나 `"./"`처럼 끝에 `/`를 붙여야 합니다. 일반 파일 하나를 지정할 때는 `"file.ext"`처럼 적습니다. Live 대상은 URI로 적습니다(예: `"https://example.com"`). HTTP(S)만 허용하는 내장 `open` 도구와 달리 Live 대상에는 다른 URI 스킴도 쓸 수 있습니다. |
| `type` | [Artifact 유형](../guides/viewer-plugins.md)을 지정하는 선택 필드입니다. 내장 유형이나 설치한 유형을 지정합니다. 생략하면 Custom Artifact가 됩니다. |
| `select` | 폴더 `path` 안에서 고른 상대 경로 배열로 **Aspect**를 정의합니다. 파일이나 Live Artifact에는 쓸 수 없습니다. |
| `parts` | 상위 Artifact를 구성하는 하위 Artifact의 이름을 배열로 적습니다. 이 Artifact의 이름을 `parts`에 넣을 수 없으며, `parts`만 따라 원래 Artifact로 돌아오는 순환 관계도 만들 수 없습니다. |
| `basis` | 불리언 값입니다. `true`로 설정하면 이 Artifact 자체는 리뷰하지 않으며, `evals`가 있으면 이 값을 쓸 수 없습니다. |
| `tags` | 비어 있지 않은 표시용 문자열 배열(`status`와 `config graph`에 표시). |
| `fingerprint` | 선택:<br>• **폴더**: 생략하거나 `{}`이면 기본 `artifactsum`, `{ files? = [...], ignore? = [...] }`(`files` 기본값은 `["."]`, `ignore` 기본값은 `[]`), `{ script = { command = "...", args = [...], files? = [...], timeout_ms? = N } }`, 또는 `false`.<br>• **Aspect**: 생략하면 `select`에 대한 기본 `artifactsum`, `{ files = [...], ignore? = [...] }`(`files` 필수이며 `select` 범위 안; Aspect에서 `fingerprint = {}`는 거부됨), `{ script = ... }`, 또는 `false`.<br>• **파일**: 생략하거나 `{ files = ["<대상파일이름>"] }`(`ignore` 불가), `{ script = ... }`, 또는 `false`.<br>• **Live**: 생략(스냅샷 없음), `{ script = ... }`, 또는 `false`.<br>자세한 규칙은 [fingerprint와 재사용](../concepts/reuse.md#fingerprints-artifactsum-script-false)을 참고하세요. |
| `views.agent_tools` | `<tool_id>`별 `{ builtin = "...", args? = [...], description? = "..." }`(`declarations`, `graph`, `help`, `section`에는 `args`가 필수이고 고정 대상 `read`/`list`에는 선택; [내장 도구](../guides/tools.md#builtin-tools) 참고) 또는 `{ description, protocol = "json"\|"plain", command, args, input_schema?, execution_paths?, timeout_ms?, session? }` 매핑. |
| `views.agent_sessions` | `<session_id>`별 `{ start = { command, args, execution_paths? }, stop = { command, args, execution_paths? } }` 매핑(`start`와 `stop`에는 `timeout_ms`를 쓰지 않으며, 연결된 개별 도구에서 `timeout_ms`를 선언합니다). |
| `views.human_tools` | `<tool_id>`별 `{ builtin = "...", args = [...], description?, kind? }` 또는 `{ description, kind = "launch"\|"output", command, args, timeout_ms? }` 매핑(`timeout_ms`는 `output` 실행 시간을 제한하며 `launch`에는 적용되지 않음). 뷰어를 제공하는 유형에서는 `{ kind = "viewer", description = "..." }`로 데스크톱 앱에서 Artifact Viewer를 엽니다. CLI에서는 뷰어를 열지 않습니다. |
| `annotations` | 화면에 표시할 메타데이터(`style.at`, `style.parts`, `comment.text`)를 지정합니다. `artifactsum`이 `.artf` 내용을 정규화할 때와 eval 정의 해시를 계산할 때 제외됩니다. |

## eval 필드 (`[evals.<id>]`) {#eval-fields-evalsid}

각 `[evals.<id>]` 테이블은 `<artifact>/<id>` eval을 정의합니다.

| 필드 | 타입 및 규칙 |
|---|---|
| `title` | **필수.** 통과 조건을 요약하는 문자열. |
| `profile` | **필수.** 평가자 프로필 테이블(`kind = "runtime"`, `"agent"`, `"human"` 중 하나) 또는 Agent [프로필 별칭](#project-settings)(`!name`이나 `!name(field = value, ...)`):<br>• `runtime`: `{ kind = "runtime", command = "...", args = [...], timeout_ms? = N }`<br>• `agent`: `{ kind = "agent", backend = "...", model = "...", reasoning? = "...", timeout_ms? = N, max_tokens? = N, max_tool_calls? = N }`<br>• `human`: `{ kind = "human" }` |
| `profile_variants` | 같은 `kind`의 대체 실행 프로필 매핑(`verify --profile NAME` 또는 `status --profile NAME`으로 선택). `agent` eval에서는 인라인 테이블 대신 [프로필 별칭](#project-settings)도 쓸 수 있습니다. 직접 선택했거나 재귀 옵션으로 포함한 eval 중 실행 가능한 eval마다 대체 실행 프로필을 정의해야 합니다. Runtime 대체 실행 프로필의 `args`에 적은 참조 때문에 새로 포함된 선행 eval은 기본 프로필로 실행합니다. |
| `mutex` | `runtime` 또는 `agent` eval에 지정하는 계층형 잠금 태그 배열(`"use-computer"` 또는 `"use-computer:display"` 같은 하위 경로). 태그가 같거나 상하위 관계에 있으면(`use-computer`는 모든 하위 태그와 충돌) 한 eval이 끝날 때까지 다른 eval은 대기합니다. |
| `payload.instruction` | **필수.** 평가 기준 문자열. `payload.instruction`이나 Runtime `args`에서 다른 Artifact를 `{name}`으로 참조하면 그 Artifact가 해당 eval의 입력이 됩니다. 해당 eval의 평가 대상 자체를 참조할 때는 의존성을 추가하지 않습니다. |
| `red_at` | `agent` eval에만 지정하는 선택적 우선순위 기준(`"P0"`, `"P1"`, `"P2"`, `"P3"` 중 하나이며 `P0`이 가장 높음). `red_at` 이상의 지적 사항이 있으면 `RED`로 판정하고, 생략하면 지적 사항이 하나만 있어도 `RED`로 판정합니다. 기준보다 낮은 지적 사항은 `GREEN` 판정에서도 결과에 남습니다. |
| `pass_schema`, `fail_schema` | `human` eval의 결과 필드를 검증할 JSON Schema로, 필요할 때만 지정합니다(`agent` eval에서는 쓸 수 없습니다). `runtime` 선언에도 적을 수 있으며, 종료 코드 판정은 바꾸지 않고 eval 정의 해시에만 반영됩니다. |

## 프로젝트 설정 (`.artifactize/settings.toml`) {#project-settings}

선택 파일인 `.artifactize/settings.toml`에 Agent 평가자 프로필 별칭을 한 번만 정의해 두면 여러 `.artf` 파일에 같은 프로필을 반복해서 적지 않아도 됩니다.

```toml
# .artifactize/settings.toml
[profiles.sol]
kind = "agent"
backend = "codex"
model = "gpt-6.2-sol"
reasoning = "high"
timeout_ms = 1800000
```

```toml
# .artifactize/app.artf
[evals.review]
title = "The implementation satisfies the spec"
profile = !sol
red_at = "P1"
payload.instruction = "Review {app} against {spec}."

[evals.review.profile_variants]
fast = !sol(reasoning = "low", timeout_ms = 600000)
```

- **별칭 정의**: `[profiles.<name>]`에는 `kind = "agent"`인 Agent 프로필만 적을 수 있습니다. 사용할 수 있는 필드는 인라인 Agent 프로필과 같습니다(`backend`, `model`, `reasoning`, `timeout_ms`, `max_tool_calls`, `max_tokens`).
- **별칭 사용과 필드 재정의**: eval의 `profile`이나 `profile_variants`에 `!name`을 적으면 별칭을 그대로 쓰고, `!name(field = value, ...)`처럼 괄호 안에 필드를 적으면 해당 Agent 필드만 바꿔서 적용합니다(`kind`는 바꿀 수 없습니다). 별칭 표기는 `profile`과 `profile_variants`에서만 쓸 수 있습니다.
- **재사용과 fingerprint**: 프로필 별칭은 실행 옵션이므로 `.artifactize/settings.toml`을 바꿔도 eval 정의 해시, 재사용 키, 그래프 fingerprint는 달라지지 않습니다. `artifactsum`으로 `.artf` 파일을 해시할 때는 다른 선언 필드와 마찬가지로 별칭의 재정의 항목도 정규화합니다.

## 결과 스키마 (`pass_schema`와 `fail_schema`) {#owner-result-schemas}

Human 결과에는 `"verdict": "GREEN" | "RED"`와 함께 사용자가 선언한 결과 필드가 저장됩니다.

- **최상위 객체 규칙**: 최상위 `type = "object"`와 `additionalProperties = false`는 생략해도 자동으로 적용됩니다. 허용할 최상위 결과 필드는 모두 `properties`에 명시해야 합니다. `properties`에 없는 최상위 필드는 `$ref`나 `patternProperties`를 써도 허용할 수 없습니다. `allOf`/`anyOf`/`oneOf`/`not` 같은 스키마 조합 키워드는 최상위와 Human 개별 속성 모두에서 쓸 수 없습니다. 실행 기록에 쓰는 이름(`verdict`, `reference`, `reusedFrom`, `executionProvenance`, `attemptId`, `provider`, `model`, `stdout`, `stderr`, `durationMs`, `exitCode`)은 결과 필드에 사용할 수 없습니다. 특정 판정의 스키마를 생략하면 그 판정의 결과 필드에는 빈 객체(`{}`)만 허용합니다.
- **Human 양식 형태** (`#human-result-schemas`): 사람은 UI 양식에 결과를 입력하므로 `human` eval의 `pass_schema`와 `fail_schema`에는 화면 양식에서 지원하는 아래 필드 형태만 쓰세요.
  - `{ type = "string", minLength?, maxLength? }` (텍스트 입력 칸)
  - `{ type = "integer" | "number", minimum?, maximum?, exclusiveMinimum?, exclusiveMaximum?, multipleOf? }` (숫자 입력 칸)
  - `{ type = "boolean" }` (참/거짓 선택)
  - `{ enum = ["a", "b"] }` (문자열 선택지)
  - `{ const = ... }` (자동으로 채워지는 상수)
  - `{ type = "array", items = { type = "string" } }` (텍스트 입력 칸 목록)
  - `{ type = "array", items = { type = "object", properties = { <위의 단일 필드들> } } }` (반복 입력 그룹)

## 선언 파일 이전과 Artifact 이름 변경 {#upgrading-an-in-tree-project}

- **`artifactize config migrate [--repo PATH]`**: 기존 디렉터리 곳곳에 있던 `index.artf`, 개별 `.artf`, 루트 `.artf` 선언 파일을 `.artifactize/<name>.artf` 형식으로 옮기면서 `path`와 `parts`를 명시합니다. 이때 `agent` eval에 남아 있던 기존 `pass_schema`와 `fail_schema` 선언은 제거하고 Human 스키마는 그대로 유지합니다. `.artfignore`에 따라 변환에서 제외한 하위 프로젝트를 알리고, 새 형식으로 옮긴 선언의 원본 파일과 `.artfignore`를 삭제합니다. 이미 이전한 프로젝트에서도 리뷰에 필요한 도구가 빠진 로컬 Artifact에 `type = "files"`를 추가할 수 있습니다. 기존 사용자 지정 도구가 플러그인 도구와 충돌하면 필요한 도구를 직접 추가하세요.
- **`artifactize config rename OLD NEW`**: 프로젝트 유효성을 확인한 뒤 `.artifactize/**/<OLD>.artf` 파일 이름을 `<NEW>.artf`로 바꾸고 `parts`, `{OLD}` 참조, `[annotations.style]`에 저장한 하위 Artifact의 위치를 함께 갱신합니다.

## 운영체제별 규칙 {#operating-systems}

논리 경로의 구분자로 `/`를 쓰면 Linux, macOS, Windows, WSL 2에서 같은 선언을 그대로 쓸 수 있습니다.

- **프로그램 탐색**: `python3`, `cargo`처럼 경로 구분자가 없는 실행 파일 이름은 `PATH`에서 찾습니다. Windows에서는 `PATHEXT`도 확인하므로 `npx`를 지정하면 `npx.cmd`도 찾습니다. `/`가 들어 있는 상대 실행 경로(`./tools/check`)는 대상 종류에 따라 탐색 기준이 다릅니다. 폴더와 Aspect에서는 허용된 범위 안에서 찾고, 파일 Artifact에서는 파일이 들어 있는 폴더 아래에서, Live 대상에서는 프로젝트 루트 아래에서 찾습니다. 모든 대상에서 `..`와 심볼릭 링크는 허용하지 않습니다.
- **경로 대소문자 구분**: 모든 OS에서 논리 경로의 대소문자를 구분하며, 대소문자를 구분하지 않는 macOS·Windows 볼륨에서도 실제 디스크에 저장된 파일 이름의 대소문자가 정확히 일치하는지 확인합니다.
- **파일 및 브라우저 열기**: `artifactize login codex`를 실행하면 로그인을 마칠 수 있도록 운영체제의 기본 브라우저가 열립니다.
