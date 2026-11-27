# 선언(.artf) 레퍼런스 {#declarations-artf-reference}

`.artifactize/`가 들어 있는 폴더를 하나의 프로젝트로 봅니다. `.artifactize/` 아래 어디에 있든 `.artf` 파일은 모두 `.artf` 언어로 쓴 코드이고, 이 파일들이 모여 하나의 프로그램이 됩니다. 한 파일에서 선언한 것은 다른 파일 어디서나 쓸 수 있습니다. 그 옆의 `.artfprofiles` 파일에는 Agent eval을 어떻게 실행할지 적습니다([Agent profile](#project-settings)). 이렇게 선언한 Artifact들이 어떻게 프로젝트 그래프를 이루는지는 [Artifact와 Artifact Graph](../concepts/artifacts.md)를 보세요.

이 언어는 작고 선언적입니다. 읽는다고 해서 무언가 실행되지는 않습니다. Artifact와 그 eval, toolset을 선언할 뿐이고, 반복되는 모양은 `artf!` **매크로**로 한 번만 씁니다. `artifactize config check`가 이 코드를 검사합니다. 오류에는 파일, 줄, 열이 나오고, 매크로가 만든 코드라면 그 매크로를 부른 곳도 함께 나옵니다.

```rust
// .artifactize/profiles.artfprofiles
profile reviewer : agent = { backend = "codex", model = "gpt-6.2-sol", reasoning = "high" }
```

```rust
// .artifactize/engine.artf
// 워크스페이스의 Rust 크레이트: 폴더, 코드 리뷰, 테스트.
artf! rust-crate(package, dir, extra_evals = {}) : files = {
  path = "crates/@{param dir}/"

  eval follows-style : reviewer = {
    title = "The crate follows the code checklist"
    red_at = "P1"
    instruction = """
      Read @{artf rules-code-style} and review @{artf self} against every item. Each item the
      code does not meet is an issue.
    """
  }

  // lockfile과 함께 크레이트의 테스트를 실행합니다.
  eval tests : runtime(command = "cargo", args = ["test", "--locked", "-p", "@{param package}"], timeout_ms = 1800000) = {
    title = "The crate's tests pass"
    deps = [cargo-lock]
  }

  ...extra_evals
}

artf cargo-lock = { path = "Cargo.lock", basis = true }
artf rules-code-style : markdown = { path = "project-rules/DEV-CODING.md", basis = true }

artf core = rust-crate!(package = "app-core", dir = "core")
artf cli = rust-crate!(
  package = "app-cli",
  dir = "cli",
  extra_evals = {
    eval packages : runtime(command = "cargo", args = ["package", "--locked", "-p", "app-cli"]) = {
      title = "The CLI packages"
    }
  },
)
```

## 문법 {#syntax}

- **주석**은 `//`로 시작해 그 줄 끝까지입니다. 레이아웃과 주석은 의미에 영향을 주지 않습니다.
- **문자열**은 한 줄에 쓰는 `"…"`이고 JSON과 같은 이스케이프(`\"`, `\\`, `\n`, `\t`, `\uXXXX` 등)를 씁니다. 여러 줄에 걸친 문자열은 `"""…"""`로 씁니다. 여는 따옴표 바로 뒤의 줄바꿈, 닫는 따옴표의 들여쓰기만 있는 마지막 줄, 모든 줄에 공통인 들여쓰기는 빠집니다. 줄 끝의 백슬래시는 다음 줄과 이어 붙입니다. 문자열 안의 `@{…}`는 [slot](#slots)이고, `@{`를 글자 그대로 쓰려면 `@@{`로 씁니다. 그 밖의 중괄호(`{#id}`, `{"json": true}`, `${HOME}`)는 일반 텍스트입니다. `'''…'''`는 이스케이프도 slot도 없는 raw 문자열입니다.
- **값**은 문자열, 정수, `true`/`false`, 목록 `[a, b]`, 블록 `{ key = value }`(항목은 줄바꿈이나 쉼표로 나눔), 이름(Artifact, toolset, profile, 매개변수), 호출 `name(key = value, …)`, 매크로 호출 `name!(key = value, …)`입니다. 선택 필드는 빈 값을 주지 말고 아예 쓰지 않습니다.
- Artifact, toolset, profile, 매크로의 **이름**은 글자로 시작하고 글자, 숫자, `-`, `_`로 이루어집니다. 이 이름들은 프로젝트의 모든 파일에 걸쳐 이름 공간 하나를 함께 씁니다. 이름 하나는 Artifact, toolset, 매크로, profile 중 한 번만 선언하고, 대소문자를 무시해도 서로 달라야 합니다. `con`이나 `nul` 같은 Windows 장치 이름은 피하세요. eval ID는 그 Artifact 안에서만 겹치지 않으면 됩니다.

## 선언 {#declarations}

| 선언 | 선언하는 것 |
|---|---|
| `artf NAME : TOOLSETS = { … }` | eval을 담은 Artifact. `: TOOLSETS`는 선택입니다. |
| `artf NAME = macro!(…)` | `artf!` 매크로로 만드는 Artifact. |
| `artf! macro(params) : TOOLSETS = { … }` | Artifact를 만드는 매크로. |
| `tool NAME = { … }` | toolset. 이름이 곧 namespace입니다. |
| `profile NAME : agent = …` | `.artfprofiles` 파일에 쓰는 Agent profile([Agent profile](#project-settings)). |

## Artifact 필드 {#declaration-validation}

```rust
artf app : files & html = {
  path = "crates/app/"
  select = ["src", "Cargo.toml"]          // 선택: 폴더 대상을 aspect로 만듦
  parts = [app-ui, app-db]                // 선택: 명시적 구성
  basis = false                           // 선택: eval이 없는 Artifact만 true
  tags = ["type:code", "tier:core"]       // 선택: 표시용 메타데이터
  fingerprint = { files = ["src"], ignore = ["*.snap"] }  // 선택: 기본값은 artifactsum

  eval review : reviewer = { … }
}
```

| 필드 | 규칙 |
|---|---|
| `path` | **필수.** 프로젝트 루트 기준 상대 경로로, 구분자는 `/`입니다. 폴더는 `"dir/"`나 `"./"`처럼 끝에 `/`를 붙이고, 일반 파일 하나는 `"file.ext"`로 씁니다. Live URI(예: `"https://example.com"`)도 쓸 수 있습니다. HTTP(S)만 여는 내장 `open` 도구와 달리 Live 대상은 다른 URI 스킴도 받습니다. |
| `: TOOLSETS` | Artifact가 조합하는 [toolset](../guides/tools.md)으로, `&`로 잇습니다. 내장 toolset(`files`, `markdown`, `artifactize`), 설치한 [플러그인](../guides/viewer-plugins.md)의 toolset, 프로젝트의 `tool` 선언을 쓸 수 있습니다. 조합한 도구 중 정규 이름이 같은 두 도구는 내용도 같아야 합니다. |
| `select` | 선택. 폴더 `path` 안의 상대 경로 목록으로, **aspect**를 정의합니다. 파일이나 Live Artifact에는 쓸 수 없습니다. |
| `parts` | 선택. 이 전체를 이루는 Artifact 이름 목록입니다. 자기 자신을 가리키거나 순환을 만들 수 없습니다. |
| `basis` | 선택. 불리언입니다. `true`는 이 Artifact 자신의 리뷰만 면제하고, eval이 있으면 쓸 수 없습니다. |
| `tags` | 선택. 비어 있지 않은 표시용 문자열 목록으로, `status`와 `config graph`에 나옵니다. |
| `fingerprint` | 선택:<br>• **폴더**: 생략하면 기본 `artifactsum`입니다. `{ files? = [...], ignore? = [...] }`(`files` 기본값 `["."]`, `ignore` 기본값 `[]`), `script(command = "...", args = [...], files? = [...], timeout_ms? = N)`, `false`를 쓸 수 있습니다.<br>• **Aspect**: 생략하면 `select`에 대한 기본 `artifactsum`입니다. `{ files = [...], ignore? = [...] }`(`files`는 필수이고 `select` 안에 있어야 함), `script(…)`, `false`를 쓸 수 있습니다.<br>• **파일**: 생략하거나 `{ files = ["<대상 파일 이름>"] }`(`ignore` 없음), `script(…)`, `false`를 씁니다.<br>• **Live**: 생략(스냅샷 없음), `script(…)`, `false`를 씁니다.<br>• **정의**(`.artifactize/` 아래의 대상): 생략하면 의미 기반 정의 fingerprint를 씁니다.<br>[Fingerprint와 재사용](../concepts/reuse.md#fingerprints-artifactsum-script-false)을 보세요. |
| `eval ID …` | 이 Artifact의 eval로, 쓴 순서를 지킵니다([eval 필드](#eval-fields-evalsid)). `...block`은 매크로 매개변수 같은 `eval` 선언 블록을 펼쳐 넣습니다. |

## eval 필드 {#eval-fields-evalsid}

Artifact 안의 `eval ID : EVALUATOR = { … }`는 `<artifact>/<ID>`를 정의합니다. evaluator는 다음 중 하나입니다.

- `runtime(command = "...", args = [...], timeout_ms? = N)`: 종료 코드가 판정이 되는 프로그램.
- profile 이름: [profile](#project-settings)대로 실행하는 Agent 리뷰.
- `human`: 사람의 승인.

| 필드 | 규칙 |
|---|---|
| `title` | **필수.** 무엇이 통과인지 요약한, 비어 있지 않은 문자열입니다. 그대로 적어야 하고, 매크로 매개변수로 채울 수 없습니다. |
| `instruction` | Agent와 Human eval에서 **필수**입니다. runtime eval은 종료 코드가 판정을 정하므로 쓸 수 없고, 설명은 `//` 주석으로 남깁니다. 평가 기준을 그대로 적고, 읽는 Artifact는 `@{artf …}` [slot](#slots)으로 가리킵니다. |
| `deps` | runtime eval 전용. 명령이 읽지만 `args`에서 `@{path …}`로 가리키지 않는 Artifact 목록입니다(lockfile, 공용 검사기 등). 목록의 Artifact는 slot과 마찬가지로 이 eval의 입력이 됩니다. |
| `red_at` | 선택. Agent eval에만 쓰는 우선순위 기준입니다(`"P0"`, `"P1"`, `"P2"`, `"P3"`, `P0`이 가장 높음). `red_at` 이상인 issue가 하나라도 있으면 `RED`입니다. 생략하면 issue가 하나만 있어도 `RED`입니다. `red_at`보다 낮은 issue는 eval이 `GREEN`이어도 남겨 둡니다. |
| `mutex` | 선택. runtime이나 Agent eval에 다는 계층형 잠금 태그 목록입니다(`"use-computer"` 또는 `"use-computer:<segment>..."`, 예: `"use-computer:display"`). 두 eval의 태그가 같거나 한쪽이 다른 쪽의 상위이면 충돌합니다(`use-computer`는 모든 하위 태그와 충돌). 충돌하는 eval은 동시에 실행되지 않고 차례를 기다립니다. |

## Slot {#slots}

slot은 문자열 안에서 무언가를 가리킵니다. Artifactize는 slot을 풀어 쓰고, slot이 선언하는 관계를 기록합니다.

| Slot | 쓰는 곳 | 바뀌는 값 |
|---|---|---|
| `@{artf NAME}` | Agent와 Human `instruction`, 도구 `description`(`@{artf self}`만) | Artifact 이름. 다른 Artifact를 가리키면 그 Artifact가 eval의 입력이 되고, 그 도구도 리뷰에 들어옵니다. |
| `@{path NAME}`, `@{path NAME/sub/path}` | runtime `args`, 도구 `args` | Artifact나 그 안 경로의 절대 경로(또는 Live URI). 파일 Artifact의 slot에는 `/sub/path`를 붙일 수 없습니다. 다른 Artifact를 가리키면 입력이 됩니다. |
| `@{param NAME}` | 매크로 안의 `path`와 인자 | 매크로 매개변수의 값(텍스트). |

`self`는 지금 선언하는 Artifact입니다. toolset이나 매크로 안의 `self`는 나중에 정해집니다. 그 toolset을 조합하는 Artifact, 또는 그 매크로가 만드는 Artifact가 `self`입니다. 없는 Artifact나 매개변수를 가리키는 slot은 그 줄에서 오류가 납니다.

## Toolset {#toolsets}

```rust
tool review = {
  agent coverage = command(command = "python3", args = ["coverage.py", "@{path requirements/requirements.md}"], protocol = "json",
    description = "List the spec.md lines that cite each requirement.", input_schema = { type = "object" })
  agent section = builtin(op = "section", args = ["spec.md"], description = "Print one section of spec.md.")
  human notes = builtin(op = "read", args = ["@{path self/notes.md}"], description = "Print the review notes of @{artf self}.")
  human open = command(command = "open-notes", args = ["@{path self/notes.md}"], kind = "launch", description = "Open the notes.")
}
```

toolset은 두 대상을 위한 도구와 session을 선언합니다.

- `agent NAME = builtin(op = "...", args? = [...], description? = "...")` 또는 `command(description, protocol = "json"|"plain", command, args, input_schema?, execution_paths?, timeout_ms?, session?)`: Agent 리뷰가 호출하는 도구입니다. 내장 도구 중 `declarations`, `graph`, `help`, `section`은 `args`가 필수이고, 대상이 고정된 `read`/`list`는 선택입니다. [내장 도구](../guides/tools.md#builtin-tools)를 보세요.
- `human NAME = builtin(op = "...", args = [...], description?, kind?)` 또는 `command(description, kind = "launch"|"output", command, args, timeout_ms?)`(`timeout_ms`는 `output` 명령에만 적용되고 `launch`에는 적용되지 않음), 또는 viewer가 있는 플러그인 안에서 `viewer(description = "...")`. `viewer`는 데스크톱 앱에서 그 플러그인의 Artifact Viewer를 엽니다.
- `session NAME = session(start = command(command, args, execution_paths?), stop = command(command, args, execution_paths?))`: Agent 도구가 `session = "NAME"`으로 묶는 하네스 session입니다(`start`와 `stop`에는 `timeout_ms`가 없고, 묶인 도구가 각자 선언합니다).

도구의 정규 이름은 `NAMESPACE.TOOL`(`review.coverage`)입니다. Artifact에 조합되면 리뷰어는 점을 `_`로 바꾼 `NAMESPACE_TOOL_ARTIFACT`(`review_coverage_spec`)로 도구를 부릅니다. Artifact는 toolset을 조합해 도구를 얻습니다(`artf spec : files & review`). Agent나 Human eval은 자기 Artifact의 도구와 instruction이 가리키는 모든 Artifact의 도구로 리뷰합니다. 내장 toolset과 같은 도구라면 따로 선언하지 말고 내장 toolset을 조합하세요.

## Agent profile {#project-settings}

profile은 Agent eval을 어떻게 실행할지 정합니다. `.artf` 파일이 아니라 `.artifactize/` 아래의 `.artfprofiles` 파일에 선언하고, profile 하나는 완결된 설정 하나입니다.

```rust
// .artifactize/profiles.artfprofiles
profile reviewer : agent = match TIER {
  default => { backend = "codex", model = "gpt-6-luna", reasoning = "max" },
  heavy => { backend = "codex", model = "gpt-6.2-sol", reasoning = "high" },
}
profile translator : agent = { backend = "codex", model = "gpt-6-luna" }
```

- **필드**: 설정에는 `backend`와 `model`이 들어가고, `reasoning`, `timeout_ms`, `max_tool_calls`, `max_tokens`는 선택입니다. 각 필드는 [Agent eval](../concepts/evals.md#agent-evals)을 보세요. 분기마다 완결된 설정을 쓰고, `default`에서 물려받는 값은 없습니다.
- **환경에 따라 고르기**: `match VARIABLE { default => { … }, LABEL => { … } }`는 환경 변수 값이 가리키는 분기를 실행합니다. `default`는 꼭 있어야 하고, 변수가 없거나 비어 있을 때, 또는 다른 profile에는 있지만 이 profile에는 없는 이름일 때 실행됩니다. 어떤 profile의 `match`에도 없는 값은 오류이므로, 철자를 틀린 값이 조용히 `default`로 실행되는 일은 없습니다. 지금 고르지 않은 분기까지 모두 검사합니다. Run은 읽은 변수 값을 저장된 그래프에 남깁니다.
- **쓰는 법**: Agent eval은 profile 이름을 씁니다(`eval review : reviewer = { … }`).
- **재사용과 fingerprint**: profile은 eval을 어떻게 실행할지 정할 뿐 무엇을 판단할지는 바꾸지 않습니다. 그래서 어떤 profile이나 분기도 Eval 정의 해시, 재사용 키, 정의 fingerprint에 영향을 주지 않습니다.

## 매크로 {#macros}

`artf!`로 Artifact를 만드는 매크로를 선언하고, Artifact를 바인딩하는 자리에서 `name!(…)`으로 부릅니다. 매크로 매개변수는 이름이고, 기본값을 줄 수 있습니다(`extra_evals = {}`). 호출할 때는 매개변수를 이름으로 넘기고, 기본값이 없는 매개변수는 꼭 넘겨야 합니다. 매크로 안에서 매개변수는 값 자리에서는 값으로(`parts = parts`), `path`나 인자 문자열 안에서는 `@{param NAME}`으로 씁니다. `...NAME`은 eval 블록을 Artifact 안에 펼쳐 넣습니다.

매크로는 무엇을 검사할지 바꾸는 값만 받습니다. 경로, 패키지, 한도, 추가 eval이 그런 값입니다. title, instruction, 도구 설명은 매크로 안에 그대로 적고 매개변수로 채우지 않습니다. 그래야 매크로를 어디서 부르든 같은 글로 읽힙니다.

매크로는 정적으로 펼쳐지고, 직접이든 다른 매크로를 거쳐서든 자기 자신을 부를 수 없습니다. `artifactize config expand ARTIFACT`는 바인딩이 만드는 선언을 모든 매개변수를 채워 보여 주고, `artifactize config symbols`는 모든 전역 이름과 매크로 시그니처를 나열합니다.

## 결과 {#owner-result-schemas}

모든 eval 결과는 `{ verdict, kind, issues }` 한 가지 모양이고, `kind`는 `runtime`, `agent`, `human` 중 하나입니다.

- **Agent**: issue마다 `{ priority, reason }`이고, 판정은 `red_at`이 정합니다.
- **Human**: issue마다 `{ reason }`이고, Artifact에서 고쳐야 할 것 하나를 적습니다. 사람은 issue 없이 `GREEN`으로 답하거나, issue를 하나 이상 적어 `RED`로 답합니다.
- **Runtime**: `RED`이면 issue 하나 `{ exitCode, reason }`에 명령이 마지막으로 출력한 부분을 남깁니다. 전체 출력은 실행 시도의 로그에 남습니다(`artifactize request logs REQUEST_ID`). runtime 결과에는 `durationMs`도 들어 있습니다.

## 정의 마이그레이션과 이름 바꾸기 {#upgrading-an-in-tree-project}

- **`artifactize config migrate [--repo PATH]`**: 예전 정의를 `.artf` 언어로 바꿉니다. 트리 안의 `index.artf`, 사이드카 `.artf`, 루트 `.artf` 파일, TOML로 쓴 `.artifactize/<name>.artf` 파일, `.artifactize/settings.toml`의 profile이 대상이고, profile은 `profiles.artfprofiles`로 옮깁니다. 내용이 같은 도구 테이블은 어떤 Artifact와도 이름이 겹치지 않는 공유 toolset 하나가 되고, `type = "…"`은 조합한 toolset이 됩니다. `{name}` 참조는 slot이 됩니다. runtime eval의 참조는 `@{path …}` 인자나 `deps`가 되고, instruction은 `//` 주석이 됩니다. 주석은 그대로 옮깁니다. `pass_schema`/`fail_schema`는 안내와 함께 빠집니다. 이제 사람은 `GREEN`으로, 또는 issue를 적어 `RED`로 답하기 때문입니다. 저장된 결과는 이어지지 않으므로 모든 eval이 새 재사용 키로 한 번 더 실행됩니다.
- **`artifactize config rename OLD NEW`**: 프로젝트를 검사한 뒤, 코드에서 그 Artifact를 가리키는 모든 곳의 이름을 바꿉니다. 바인딩, `parts` 목록, slot, `deps`가 모두 대상입니다. 그 Artifact 하나만 바인딩하고 이름도 같은 파일은 파일 이름도 함께 바뀝니다.

## 운영체제 규칙 {#operating-systems}

선언은 논리 경로에 `/`를 써서 한 번만 쓰면 됩니다. Artifactize는 Linux, macOS, Windows, WSL 2에서 같은 선언을 실행합니다.

- **프로그램 찾기**: 경로 없는 실행 파일 이름(`python3`, `cargo`)은 `PATH`에서 찾습니다(Windows에서는 `PATHEXT`를 써서 `npx`로 `npx.cmd`를 찾습니다). `/`가 들어간 상대 실행 파일(`./tools/check`)은 폴더와 aspect라면 Artifact가 허용한 범위 안에서, 파일 Artifact라면 그 파일이 있는 폴더 아래에서, Live 대상이라면 프로젝트 루트 아래에서 찾습니다. `..`와 심볼릭 링크는 언제나 거부합니다.
- **경로 대소문자**: 논리 경로는 모든 OS에서 대소문자를 구분해 맞춥니다. 대소문자를 구분하지 않는 macOS와 Windows 볼륨에서는 디스크에 실제로 적힌 철자까지 같은지 확인합니다.
- **파일과 URL 열기**: `artifactize login codex`는 로그인을 마치도록 시스템 브라우저를 엽니다.
