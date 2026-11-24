# 도구와 하네스 세션 {#tools-and-harness-sessions}

도구를 사용하면 Agent나 Human 리뷰어가 빌드 세션을 따로 열지 않고도 Artifact를 살펴볼 수 있습니다. 리뷰어는 eval의 대상 Artifact와 `{name}`으로 참조한 입력 Artifact에 선언된 도구를 사용할 수 있습니다. 실제로 제공되는 도구 이름은 `<도구이름>_<artifact>` 형태입니다(예: `spec`에 선언한 `read`는 `read_spec`). 도구 선언(`views`)은 Artifact를 조회하는 수단이므로 [eval 정의 해시](../concepts/reuse.md#the-reuse-key)에 포함되지 않습니다.

로컬 Artifact에 자주 쓰는 도구를 하나씩 선언할 필요는 없습니다. [Artifact 유형과 뷰어 플러그인](viewer-plugins.md)을 고르세요. Custom Artifact는 `[views.*]`에 도구를 직접 선언합니다.

## 내장 도구 {#builtin-tools}

`[views.agent_tools]` 또는 `[views.human_tools]` 아래에 `builtin`으로 내장 도구를 선언합니다.

```toml
[views.agent_tools]
read = { builtin = "read" }                                      # 모델이 path, offset, limit 선택
list = { builtin = "list" }
glob = { builtin = "glob" }
grep = { builtin = "grep" }
view_image = { builtin = "view_image" }                          # PNG, JPEG, WebP 이미지 확인
spec_sec = { builtin = "section", args = ["spec.md"] }           # 모델이 {"heading": "..."} 전달
fixed_doc = { builtin = "read", args = ["README.md"] }           # 고정 파일 읽기 (모델 인자 없음)
structure = { builtin = "graph", args = [] }                     # 전체 프로젝트 Artifact/eval 트리
cli_help = { builtin = "help", args = ["cargo", "build"] }       # `cargo build --help` 실행

[views.human_tools]
open = { builtin = "open", args = ["{artifactPath}/notes.md"] }  # kind = "launch" (기본 앱 실행)
viewer = { builtin = "graph", kind = "launch", args = [] }       # 데스크톱 앱에서 Current 열기
notes = { builtin = "read", args = ["notes.md"] }                # kind = "output" (Tool log에 출력)
```

| 내장 도구 | Agent (모델 입력) | Agent (고정 `args`) | Human (`args`) | 동작 |
|---|---|---|---|---|
| `read` | `{path, offset?, limit?}` | `[file]` | `[file]` | UTF-8 텍스트를 줄 단위로 읽으며, `.artf` 파일은 정규화한 선언 내용을 보여 줍니다. |
| `list` | `{path?, offset?, limit?}` | `[folder]` | `[folder]` | 정렬된 디렉터리 항목(`file`, `directory`, `symlink`, `other`)을 나열합니다. |
| `glob` | `{pattern, path?}` | — | — | 글로브 패턴(`*`, `**`, `?`, `[abc]`, `{a,b}`)에 맞는 상대 파일 경로를 찾습니다. |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}` | — | — | Rust 정규표현식으로 UTF-8 파일을 검색합니다. |
| `view_image` | `{path}` | — | — | PNG, JPEG, WebP 이미지를 모델에 전달합니다. |
| `section` | `{heading}` (`args = [file]`일 때) | `[file]` 또는 `[file, heading]` (둘 다 고정이면 모델 입력 없음) | `[file, heading]` | 지정한 Markdown 제목부터 같은 단계나 상위 단계의 제목이 다시 나오기 전까지 한 섹션을 출력합니다. |
| `help` | — | `[program, subcommand...]` | `[program, subcommand...]` | `program subcommand... --help`를 실행하고 출력을 정리해 UTF-8 텍스트로 반환합니다. |
| `graph` | — | `[]` | `[]` (`launch`) | Agent: 현재 프로젝트의 Artifact/eval 트리와 기준·관계를 출력합니다. Human: `artifactize-desktop`에서 프로젝트의 Current 화면을 엽니다. |
| `declarations` | — | `[]` | — | Artifact가 대상으로 삼은 `.artifactize/` 경로에 선언된 Artifact, eval 기준, 참조 관계를 출력합니다. `artifactize` 유형에 포함된 도구입니다. |
| `open` | — | — | `[파일, 폴더, 또는 http(s) URL]` (`launch`) | OS 기본 프로그램으로 대상을 엽니다. |

**범위 제한**: 내장 파일 도구(`read`, `list`, `glob`, `grep`, `view_image`, `section`)는 허용된 Artifact 범위 안에서만 파일과 폴더에 접근할 수 있습니다. `..`와 절대 경로는 거부하고 심볼릭 링크는 따라가지 않습니다. `list`는 심볼릭 링크 항목을 `symlink`로 표시하고, 다른 파일 도구는 심볼릭 링크를 거부하거나 건너뜁니다. 파일 Artifact에서는 그 파일 하나만 보여 줍니다. 상위 폴더 Artifact에 선언한 내장 파일 도구 가운데 호출할 때 경로를 받는 도구가 있습니다. 이런 도구는 해당 eval이 `{part}`를 직접 참조할 때만 폴더 `parts`에 접근할 수 있으며, 그렇지 않으면 폴더 `parts`를 건너뜁니다. `read = { builtin = "read", args = [...] }`, `list`, `section`처럼 경로를 미리 지정한 Agent용 내장 도구는 그 경로가 도구를 선언한 Artifact의 범위 안에 있는지 확인합니다. 따라서 폴더 `part`의 경로를 지정하는 도구는 그 `part`에 선언하세요. 파일이나 Aspect로 선언한 하위 Artifact의 파일은 상위 폴더를 통해 읽을 수 있습니다. 다만 하위 Artifact에 선언된 도구를 사용하거나 해당 Artifact를 선행 의존성으로 추가하려면 `{part}`를 직접 참조해야 합니다. 내장 `help`와 아래의 사용자 지정 명령 도구는 OS 샌드박스 없이 실행됩니다. 따라서 직접 작성한 프로그램 중 읽기 전용으로 동작하고 신뢰할 수 있는 것만 선언하세요.

### 단독 `artifactize tools` CLI (`artifactize-tools`) {#artifactize-tools-cli}

스크립트나 Runtime eval에서는 `artifactize tools`(Microsoft Store 외 채널에서는 `artifactize-tools`도 사용 가능)로 현재 디렉터리에서 같은 내장 도구를 실행할 수 있습니다.

```sh
artifactize tools read README.md
artifactize tools list [PATH]
artifactize tools glob "**/*.rs" [PATH]
artifactize tools grep [--glob "*.rs"] [--case-insensitive] "fn main" [PATH]
artifactize tools section spec.md "Install"
artifactize tools help cargo build
artifactize tools open notes.md
```

## 사용자 지정 Agent 명령 도구 {#agent-tools}

`[views.agent_tools.<name>]`에 `json` 또는 `plain` 프로토콜을 쓰는 읽기 전용 명령 도구를 선언합니다.

```toml
# JSON 프로토콜: stdin으로 {version, context, args}를 받고 stdout으로 JSON content 블록 출력
# (이 도구를 쓰는 eval은 payload.instruction에서도 {requirements}를 참조해야 함)
[views.agent_tools.coverage]
description = "Check requirement coverage for {artifactName}."
protocol = "json"
command = "python3"
args = ["coverage.py", "{requirements}/requirements.md"]
input_schema = { type = "object", properties = { id = { type = "string" } }, additionalProperties = false }
execution_paths = ["coverage.py"]

[evals.review]
title = "The spec covers every requirement"
profile = { kind = "agent", backend = "codex", model = "gpt-6.2-sol" }
payload.instruction = "Review {spec} against {requirements} using coverage_spec."

# Plain 프로토콜: input_schema 속성(예: {id})을 개별 argv 인자로 치환
[views.agent_tools.lookup]
description = "Look up a requirement by ID in {artifactName}."
protocol = "plain"
command = "python3"
args = ["lookup.py", "--id", "{id}"]
input_schema = { type = "object", properties = { id = { type = "string" } }, required = ["id"], additionalProperties = false }
```

- **`protocol = "json"`**: 스크립트는 표준 입력(`stdin`)으로 JSON 객체 하나(`{version: 1, context: {artifactId, artifactPath, outputDir, tmpDir, scope, executionPaths?}, args}`)를 받아 표준 출력(`stdout`)에 JSON 객체 하나를 내보낸 뒤 종료 코드 `0`으로 끝나야 합니다.
  - 정상 결과의 `content` 배열에는 `text`, `json`, `image` 블록을 넣습니다. `image` 블록에는 `mimeType`과 함께 base64 `data` 또는 `path` 중 **정확히 하나**를 넣습니다. `path`에는 `context.outputDir` 안에 저장한 이미지의 경로를 적습니다. 이 디렉터리를 기준으로 한 상대 경로나 이미지의 절대 경로를 쓸 수 있습니다.
    ```json
    {"content": [{"type": "text", "text": "All IDs cited."}, {"type": "json", "data": {"missing": []}}]}
    ```
  - 도구가 직접 알리는 오류 (종료 코드 `0`, `isError: true`, 그리고 비어 있지 않은 `text` 블록 **단 하나**):
    ```json
    {"isError": true, "content": [{"type": "text", "text": "Unknown requirement ID: R9"}]}
    ```
- **`protocol = "plain"`**: `args` 각 항목의 자리 표시자(예: `{id}`)를 `input_schema`에 선언한 속성의 값으로 바꾼 뒤 명령을 실행하고, 표준 출력(`stdout`)을 텍스트로 돌려줍니다.
- **`execution_paths`**: 도구 또는 세션의 `start`/`stop`에 선언하며, 작업 공간을 기준으로 파일이나 디렉터리의 상대 경로를 적습니다. 리뷰가 시작될 때 각 경로의 SHA-256 해시를 계산해 요청 기록의 `executionPaths`에 저장합니다.

## 하네스 세션 (지속 실행 하네스) {#tool-sessions}

Agent가 도구를 여러 번 호출해 실행 중인 앱이나 헤드리스 게임을 살펴봐야 한다면 `[views.agent_sessions.<name>]`에 세션을 선언한 뒤 해당 도구의 `session`에 세션 이름을 적습니다.

```toml
[views.agent_sessions.game]
start = { command = "python3", args = ["harness.py", "start"] }
stop = { command = "python3", args = ["harness.py", "stop"] }

[views.agent_tools.board]
session = "game"
description = "Read the current game board."
protocol = "json"
command = "python3"
args = ["harness.py", "board"]
```

세션에 연결된 도구가 처음 호출될 때 Artifactize가 세션을 시작합니다. `start`와 `stop` 모두 표준 입력(`stdin`)으로 빈 `args`(`{}`)가 담긴 표준 JSON 요청을 받으며 전용 `ARTIFACTIZE_SESSION_DIR` 환경 변수를 공유합니다. `start`는 하네스 준비가 끝났을 때 표준 출력(`stdout`)에 첫 줄바꿈 문자를 출력하고 플러시한 뒤 계속 실행합니다. 이후 연결된 도구를 호출할 때와 `stop`을 실행할 때도 같은 `ARTIFACTIZE_SESSION_DIR`이 전달됩니다. 리뷰가 끝나면 Artifactize는 앞서 시작한 세션의 `stop`을 호출하고 세션 디렉터리를 정리합니다.

## 사용자 지정 Human 도구와 도구 점검 {#human-tools}

Human eval에서 리뷰어가 쓸 명령 도구는 `[views.human_tools.<name>]`에 선언합니다.

```toml
[views.human_tools]
preview = { description = "Launch preview for {artifactName}.", kind = "launch", command = "python3", args = ["preview.py", "{artifactPath}"] }
summary = { description = "Print summary.", kind = "output", command = "python3", args = ["summarize.py", "{artifactPath}"], timeout_ms = 60000 }
```

- 리뷰어가 Human 도구를 실행할 때는 인자를 임의로 추가할 수 없으며, 선언한 `args`에는 `{artifactPath}`와 허용된 `{name}` 참조를 쓸 수 있습니다.
- `kind = "output"` 도구는 명령이 끝나거나 제한 시간(`timeout_ms`)에 도달할 때까지 대기합니다. 그동안 `stdout`과 `stderr`를 리뷰어의 **Tool log**에 실시간으로 표시합니다. 출력을 복사해 판정 양식에 붙여 넣을 수 있습니다. `kind = "launch"` 도구는 데스크톱 프로그램이나 뷰어를 띄운 뒤 프로그램이 닫힐 때까지 대기하지 않습니다. `timeout_ms`는 앱이 실행되는 시간을 제한하지 않습니다.
- 옵션 없는 `artifactize tools check`는 프로젝트 선언을 정적으로 검증하고 모든 Agent eval과 Human eval에서 사용할 수 있는 도구를 미리 점검합니다. 특정 eval의 범위(`{name}` 참조 포함)를 점검하려면 `--eval ID`를 넘기세요. 다른 Artifact를 참조하지 않는 Artifact의 도구를 점검할 때는 `--artifact NAME`을 넘깁니다. Run을 만들지 않고 개별 도구를 시험 실행하려면 `--execute`를 사용하세요.
  ```sh
  artifactize tools check
  artifactize tools check --eval spec/review
  artifactize tools check --artifact notes
  artifactize tools check --execute --eval spec/review --tool coverage_spec --args '{}'
  artifactize tools check --execute --artifact spec --audience human --tool notes
  ```
