# 도구와 하네스 세션 {#tools-and-harness-sessions}

도구를 사용하면 Agent나 Human 리뷰어가 빌드 세션을 따로 열지 않고도 Artifact를 살펴볼 수 있습니다. 도구는 **toolset** 단위로 묶입니다. Artifact가 toolset을 조합하면(`artf spec : files & review = { … }`) 리뷰어는 eval의 대상 Artifact와 eval이 가리키는 Artifact에 조합된 도구를 쓸 수 있습니다. 도구의 정규 이름은 `NAMESPACE.TOOL`(`files.read`)이고, 리뷰어는 점을 `_`로 바꾼 `NAMESPACE_TOOL_ARTIFACT` 이름으로 도구를 부릅니다(예: `spec`의 `files.read`는 `files_read_spec`). instruction에서는 [slot](../reference/declarations.md#slots) `@{tool spec agent files.read}`로 도구를 가리킵니다. toolset은 Artifact를 조회하는 수단이므로 [eval 정의 해시](../concepts/reuse.md#the-reuse-key)에 포함되지 않습니다.

내장 toolset을 쓰면 로컬 Artifact에 자주 쓰는 도구를 하나씩 선언하지 않아도 됩니다. `files`(File Explorer: `list`, `glob`, `grep`, `read`와 뷰어), `markdown`(`read`와 Markdown 뷰어), `artifactize`(`declarations`와 선언을 보여 주는 Artifactize Viewer)가 있습니다. [뷰어 플러그인](viewer-plugins.md)은 자기 toolset을 더합니다. 직접 만들 때는 `tool NAME = { … }`으로 선언하며, 이름이 곧 namespace입니다.

## 내장 도구 {#builtin-tools}

toolset 안에서 `builtin(op = …)`으로 내장 도구를 선언합니다.

```rust
tool docs = {
  agent read = builtin(op = "read")                              // 모델이 path, offset, limit 선택
  agent list = builtin(op = "list")
  agent glob = builtin(op = "glob")
  agent grep = builtin(op = "grep")
  agent view_image = builtin(op = "view_image")                  // PNG, JPEG, WebP 이미지 확인
  agent spec_sec = builtin(op = "section", args = ["spec.md"])   // 모델이 {"heading": "..."} 전달
  agent fixed_doc = builtin(op = "read", args = ["README.md"])   // 고정 파일 읽기 (모델 인자 없음)
  agent structure = builtin(op = "graph", args = [])             // 전체 프로젝트 Artifact/eval 트리
  agent cli_help = builtin(op = "help", args = ["cargo", "build"])  // `cargo build --help` 실행

  human open = builtin(op = "open", args = ["@{path self/notes.md}"])  // kind = "launch" (기본 앱 실행)
  human viewer = builtin(op = "graph", kind = "launch", args = [])     // 데스크톱 앱에서 Current 열기
  human notes = builtin(op = "read", args = ["notes.md"])              // kind = "output" (Tool log에 출력)
}
```

| 내장 도구 | Agent (모델 입력) | Agent (고정 `args`) | Human (`args`) | 동작 |
|---|---|---|---|---|
| `read` | `{path, offset?, limit?}` | `[file]` | `[file]` | UTF-8 텍스트를 줄 단위로 읽습니다. `.artf` 파일은 `//` 주석을 뺀 내용을 줄 번호 그대로 보여 줍니다. |
| `list` | `{path?, offset?, limit?}` | `[folder]` | `[folder]` | 정렬된 디렉터리 항목(`file`, `directory`, `symlink`, `other`)을 나열합니다. |
| `glob` | `{pattern, path?}` | — | — | 글로브 패턴(`*`, `**`, `?`, `[abc]`, `{a,b}`)에 맞는 상대 파일 경로를 찾습니다. |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}` | — | — | Rust 정규표현식으로 UTF-8 파일을 검색합니다. |
| `view_image` | `{path}` | — | — | PNG, JPEG, WebP 이미지를 모델에 전달합니다. |
| `section` | `{heading}` (`args = [file]`일 때) | `[file]` 또는 `[file, heading]` (둘 다 고정이면 모델 입력 없음) | `[file, heading]` | 지정한 Markdown 제목부터 같은 단계나 상위 단계의 제목이 다시 나오기 전까지 한 섹션을 출력합니다. |
| `help` | — | `[program, subcommand...]` | `[program, subcommand...]` | `program subcommand... --help`를 실행하고 출력을 정리해 UTF-8 텍스트로 반환합니다. |
| `graph` | — | `[]` | `[]` (`launch`) | Agent: 현재 프로젝트의 Artifact/eval 트리와 기준·관계를 출력합니다. Human: `artifactize-desktop`에서 프로젝트의 Current 화면을 엽니다. |
| `declarations` | — | `[]` | — | Artifact가 대상으로 삼은 `.artifactize/` 경로에 선언된 Artifact, eval 기준, 관계를 출력합니다. `artifactize` toolset에 포함된 도구입니다. |
| `open` | — | — | `[파일, 폴더, 또는 http(s) URL]` (`launch`) | OS 기본 프로그램으로 대상을 엽니다. |

**범위 제한**: 내장 파일 도구(`read`, `list`, `glob`, `grep`, `view_image`, `section`)는 허용된 Artifact 범위 안에서만 파일과 폴더에 접근할 수 있습니다. `..`와 절대 경로는 거부하고 심볼릭 링크는 따라가지 않습니다. `list`는 심볼릭 링크 항목을 `symlink`로 표시하고, 다른 파일 도구는 심볼릭 링크를 거부하거나 건너뜁니다. 파일 Artifact에서는 그 파일 하나만 보여 줍니다. 상위 폴더 Artifact에 선언한 내장 파일 도구 가운데 호출할 때 경로를 받는 도구가 있습니다. 이런 도구는 해당 eval이 그 part를 직접 가리킬 때만 폴더 `parts`에 접근할 수 있으며, 그렇지 않으면 폴더 `parts`를 건너뜁니다. `builtin(op = "read", args = [...])`, `list`, `section`처럼 경로를 미리 지정한 Agent용 내장 도구는 그 경로가 도구를 조합한 Artifact의 범위 안에 있는지 확인합니다. 따라서 폴더 `part`의 경로를 지정하는 도구는 그 `part`에 조합하세요. 파일이나 Aspect로 선언한 하위 Artifact의 파일은 상위 폴더를 통해 읽을 수 있습니다. 다만 하위 Artifact에 조합된 도구를 사용하거나 해당 Artifact를 선행 의존성으로 추가하려면 eval이 그 part를 직접 가리켜야 합니다. 내장 `help`와 아래의 사용자 지정 명령 도구는 OS 샌드박스 없이 실행됩니다. 따라서 직접 작성한 프로그램 중 읽기 전용으로 동작하고 신뢰할 수 있는 것만 선언하세요.

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

`agent NAME = command(…)`로 `json` 또는 `plain` 프로토콜을 쓰는 읽기 전용 명령 도구를 선언합니다.

```rust
profile sol = agent(backend = "codex", model = "gpt-6.2-sol")

tool review = {
  // JSON 프로토콜: stdin으로 {version, context, args}를 받고 stdout으로 JSON content 블록 출력.
  // 이 도구는 requirements를 읽으므로, 이 도구를 쓰는 eval도 requirements를 가리켜야 함.
  agent coverage = command(
    description = "Check requirement coverage for @{artf self}.",
    protocol = "json",
    command = "python3",
    args = ["coverage.py", "@{path requirements/requirements.md}"],
    input_schema = { type = "object", properties = { id = { type = "string" } }, additionalProperties = false },
    execution_paths = ["coverage.py"],
  )
  // Plain 프로토콜: input_schema 속성(예: {id})을 개별 argv 인자로 치환.
  agent lookup = command(
    description = "Look up a requirement by ID in @{artf self}.",
    protocol = "plain",
    command = "python3",
    args = ["lookup.py", "--id", "{id}"],
    input_schema = { type = "object", properties = { id = { type = "string" } }, required = ["id"], additionalProperties = false },
  )
}

artf requirements : files = { path = "requirements/", basis = true }

artf spec : files & review = {
  path = "spec/"

  eval review : sol = {
    title = "The spec covers every requirement"
    instruction = "Review @{artf self} against @{artf requirements} using @{tool self agent review.coverage}."
  }
}
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

Agent가 도구를 여러 번 호출해 실행 중인 앱이나 헤드리스 게임을 살펴봐야 한다면 toolset 안에 `session NAME = session(start = …, stop = …)`으로 세션을 선언하고, 해당 도구의 `session`에 세션 이름을 적습니다.

```rust
tool game = {
  session game = session(
    start = command(command = "python3", args = ["harness.py", "start"]),
    stop = command(command = "python3", args = ["harness.py", "stop"]),
  )
  agent board = command(
    session = "game",
    description = "Read the current game board.",
    protocol = "json",
    command = "python3",
    args = ["harness.py", "board"],
  )
}
```

세션에 연결된 도구가 처음 호출될 때 Artifactize가 세션을 시작합니다. `start`와 `stop` 모두 표준 입력(`stdin`)으로 빈 `args`(`{}`)가 담긴 표준 JSON 요청을 받으며 전용 `ARTIFACTIZE_SESSION_DIR` 환경 변수를 공유합니다. `start`는 하네스 준비가 끝났을 때 표준 출력(`stdout`)에 첫 줄바꿈 문자를 출력하고 플러시한 뒤 계속 실행합니다. 이후 연결된 도구를 호출할 때와 `stop`을 실행할 때도 같은 `ARTIFACTIZE_SESSION_DIR`이 전달됩니다. 리뷰가 끝나면 Artifactize는 앞서 시작한 세션의 `stop`을 호출하고 세션 디렉터리를 정리합니다.

## 사용자 지정 Human 도구와 도구 점검 {#human-tools}

Human eval에서 리뷰어가 쓸 명령 도구는 `human NAME = command(…)`로 선언합니다.

```rust
tool preview = {
  human open = command(description = "Launch preview for @{artf self}.", kind = "launch", command = "python3", args = ["preview.py", "@{path self}"])
  human summary = command(description = "Print summary.", kind = "output", command = "python3", args = ["summarize.py", "@{path self}"], timeout_ms = 60000)
}
```

- 리뷰어가 Human 도구를 실행할 때는 인자를 임의로 추가할 수 없습니다. 선언한 `args`에는 `@{path self}`와, eval이 가리키는 Artifact의 `@{path …}`를 쓸 수 있습니다.
- 리뷰 화면은 도구를 정규 이름(`preview.open`)으로 보여 주고, 도구를 조합한 Artifact별로 묶습니다.
- `kind = "output"` 도구는 명령이 끝나거나 제한 시간(`timeout_ms`)에 도달할 때까지 대기합니다. 그동안 `stdout`과 `stderr`를 리뷰어의 **Tool log**에 실시간으로 표시합니다. 출력을 복사해 issue에 붙여 넣을 수 있습니다. `kind = "launch"` 도구는 데스크톱 프로그램이나 뷰어를 띄운 뒤 프로그램이 닫힐 때까지 대기하지 않습니다. `timeout_ms`는 앱이 실행되는 시간을 제한하지 않습니다.
- 옵션 없는 `artifactize tools check`는 프로젝트 선언을 정적으로 검증하고 모든 Agent eval과 Human eval에서 사용할 수 있는 도구를 미리 점검합니다. 특정 eval의 범위(eval이 가리키는 Artifact 포함)를 점검하려면 `--eval ID`를 넘기세요. 다른 Artifact를 가리키지 않는 Artifact의 도구를 점검할 때는 `--artifact NAME`을 넘깁니다. Run을 만들지 않고 개별 도구를 시험 실행하려면 `--execute`를 사용하세요. `--eval`에서는 리뷰어가 부르는 이름으로, `--artifact`에서는 Artifact 이름을 뺀 이름(`review_coverage`)으로 도구를 지정합니다.
  ```sh
  artifactize tools check
  artifactize tools check --eval spec/review
  artifactize tools check --artifact notes
  artifactize tools check --execute --eval spec/review --tool review_coverage_spec --args '{}'
  artifactize tools check --execute --artifact spec --audience human --tool preview_summary
  ```
