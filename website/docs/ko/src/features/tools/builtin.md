# 내장 도구 {#built-in-tools}

artifactize에는 Rust로 작성한 도구 모음이 들어 있으며, 디스크에서 경로 표기를 확인하는 방식을 빼면
Linux, macOS, Windows에서 똑같이 동작합니다([경로](../others/platforms.md#paths) 참조).
스크립트를 작성하거나 `cat`, `xdg-open` 같은 OS 프로그램을 호출하는 대신 `builtin`으로 이름을 지정해 선언하세요. 내장 도구는 artifactize 안에서 실행되므로
`PATH`에 [`artifactize-tools`](#artifactize-tools-cli) 명령이 없어도 됩니다.
그 명령과 `artifactize tools`는 스크립트와 runtime eval에 파일 시스템/프로그램 도구를
제공합니다. 프로젝트를 인식하는 `graph` 도구는 artifactize 안에서 선언한
리뷰 도구로만 쓸 수 있으며, 독립 실행형 도구 CLI에는 없습니다.

```toml
[views.agent_tools]
read = { builtin = "read" }                                       # the model picks the path
grep = { builtin = "grep" }
style = { builtin = "read", args = ["DEV-CODING.md"] }                 # one fixed file
spec = { builtin = "section", args = ["spec.md"] }                # the model passes only the heading
install = { builtin = "section", args = ["spec.md", "Install"] }  # one fixed section
reqs = { builtin = "read", args = ["reqs/requirements.md"] }      # in the child folder reqs
structure = { builtin = "graph", args = [] }                    # current project entity tree
cargo_help = { builtin = "help", args = ["cargo", "build"] }      # cargo build --help

[views.human_tools]
open = { builtin = "open", args = ["{artifactPath}/notes.md"] }   # kind = "launch"
viewer = { builtin = "graph", kind = "launch", args = [] }       # existing desktop graph view
docs = { builtin = "open", args = ["https://artifactize.dev/docs/"] }
notes = { builtin = "read", args = ["notes.md"] }                 # kind = "output"
summary = { builtin = "section", args = ["{reqs}/requirements.md", "Summary"] }
```

내장 도구 선언은 `builtin`, `args`(모델 입력을 받는 Agent 도구에서는
선택), 그리고 선택적인 `description`을 받습니다. 각 도구에는 기본 설명이 있습니다. Human 내장 도구는 `kind`도 지정할 수 있는데,
도구와 맞아야 합니다(`open`과 `graph`는 launch, 나머지는 모두 출력). 맞지 않으면 `config check`가
거부합니다. 내장 도구는 `timeout_ms`, `command`, `protocol`, `input_schema`를 받지 않습니다.
[명령 도구](agent.md#agent-tools)와 마찬가지로 리뷰어는 각 도구를
`<name>_<artifactId>`로 봅니다.

## 도구 {#tools}

| 도구 | Agent, 모델 입력 | Agent, 고정 `args` | Human `args` | 출력 |
|---|---|---|---|---|
| `read` | `{path, offset?, limit?}` | `[file]` | `[file]` | 파일 |
| `list` | `{path?, offset?, limit?}` | `[folder]` | `[folder]` | 정렬된 항목(JSON) |
| `glob` | `{pattern, path?}` | — | — | 일치하는 경로 |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}` | — | — | 일치하는 줄 |
| `view_image` | `{path}` | — | — | 이미지 |
| `section` | — | `[file]`(모델이 `heading`을 넘김) 또는 `[file, heading]` | `[file, heading]` | Markdown 섹션 하나 |
| `help` | — | `[program, subcommand...]` | `[program, subcommand...]` | `program subcommand... --help` |
| `graph` | — | `[]` | `[]` (launch) | Agent: eval, 기준, 방향이 있는 관계를 담은 개체 트리. Human: 데스크톱 뷰어 |
| `open` | — | — | `[file, folder or URL]` (launch) | 없음. 기본 애플리케이션을 엶 |

`args`가 없으면 Agent 도구는 모델의 입력을 받습니다.
`read`, `list`, `glob`, `grep`, `view_image`의 입력, 결과, 제한은
[Agent 도구](agent.md#agent-tools)에 있습니다. 고정 `args`는 모델 입력을 없애며,
인자 하나만 받은 `section`만 예외입니다. 고정 Agent `list`는 구조화된 JSON 콘텐츠
블록을 돌려주고, 고정 `read`, `section`, `help`는 텍스트 블록을 돌려줍니다.

- **`read`** 는 UTF-8 파일을 최대 64 KiB까지 출력합니다. 그보다 긴 파일은
  `[output truncated]`로 끝납니다. 확인하는 앞부분만 검증하므로, 그 안의 NUL이나
  잘못된 UTF-8은 거부하지만 잘린 파일의 뒷부분에 있는 그런 바이트는
  발견하지 못할 수 있습니다. CLI의 고정 read도 같은 제한을 가집니다.
- **`list`** 는 모델의 `list`처럼 폴더의 정렬된 항목 한 페이지를 JSON으로
  출력합니다.
- **`section`** 은 Markdown 파일의 섹션 하나를 출력합니다. 텍스트가 `heading`과
  정확히 같은 첫 제목을 수준과 상관없이 찾습니다(`#` 제목이나 밑줄 제목. 코드 블록 안의
  제목은 세지 않음). 섹션은 그 제목부터 같은 수준이거나 더 높은 수준의 다음 제목 전까지이므로,
  중첩된 섹션도 포함됩니다.
  일치하는 제목이 없으면 오류가 파일에 있는 제목들을 나열합니다. 파일은
  최대 8 MiB, 출력되는 섹션은 최대 64 KiB입니다.
- **`help`** 는 `program subcommand... --help`를 실행하고 그 출력을 보여 줍니다. 프로그램은
  리터럴 프로그램이나 경로입니다. 경로 없는 이름은 `PATH`(Windows에서는 `PATHEXT`도)를 씁니다.
  범위가 정해진 명령 도구 실행 파일과 달리 help는 평범한 프로그램 찾기를 씁니다.
  절대 경로, 상위 경로 기준 상대 경로, 실행 파일 심볼릭 링크를 받아들입니다.
  상대 경로는 선언한 Artifact의 폴더에서 시작하며, 하위 명령 인자는
  범위 해석 없이 그대로 전달됩니다. 프로그램은 그 폴더에서 빈 stdin으로 실행되며,
  전용 출력 디렉터리나 임시 디렉터리를 받지 않습니다. Agent에서는
  작성자의 환경에서 모든 `ARTIFACTIZE_*` 변수(대소문자 무시)를 뺀 환경에서 실행됩니다.
  Human에서는 리뷰어의 환경 전체에서 실행되며,
  `ARTIFACTIZE_REMOTE_TOKEN` 자격 증명 같은 `ARTIFACTIZE_*` 변수도 포함됩니다. 10초 안에 끝나고, 0으로 종료하고,
  stdout과 stderr를 합쳐 최대 64 KiB를 출력해야 합니다. 실제 프로그램을 실행하므로
  신뢰하는 프로그램만 지정하세요.
- **`open`**(Human 전용)은 파일, 폴더, 또는 `http://`/`https://` URL 하나를
  데스크톱의 기본 애플리케이션으로 엽니다. Linux와 WSL에서는 `xdg-open`, macOS에서는 `open`,
  Windows에서는 Windows 셸(ShellExecute)을 씁니다. 대상은 결코 셸을 거치지 않고
  인자 하나로 전달됩니다. `file:`이나 `mailto:` 같은 다른 URL 스킴은
  거부됩니다. 결과에는 애플리케이션이 시작되었다는 사실만
  기록됩니다. 애플리케이션은 리뷰어의 것이며 계속 실행됩니다.
  Linux, WSL, macOS에서는 여는 프로그램 자체가 3초 안에
  성공적으로 종료해야 합니다. 여는 프로그램이 없거나, 시간이 초과되거나, 0이 아닌 값으로 종료하면 도구 오류입니다.
  WSL에서는 `xdg-open`의 종료 상태 4도 성공으로 칩니다. `explorer.exe`를 통해 파일을 여는데,
  `explorer.exe`는 파일을 연 뒤에도 실패를 보고하기 때문입니다.
  이 확인 마감 시간은 열린 애플리케이션의 수명 제한이 아닙니다.

### view_image {#view_image}

`view_image`는 모델 입력 `{"path":"board.png"}`가 필수인 Agent 전용 도구입니다.
경로는 받아들여진 Artifact 범위 안의 논리적 경로이며, 평소처럼
경로 탈출 금지와 심볼릭 링크 금지 검사를 받습니다. 비어 있지 않은 일반 파일을 읽어,
출력된 파일 이름이 아니라 내장된 이미지 블록(`type`, base64 `data`, `mimeType`)을 돌려줍니다.

판별은 확장자가 아니라 시그니처와 컨테이너 구조를 확인합니다. 픽셀을
디코딩하거나 이미지 내용을 완전히 검증하지는 않습니다. 지원하는 시그니처는 PNG,
JPEG, WebP이며, 지원하지 않는 시그니처, GIF/BMP, 애니메이션 PNG는 거부됩니다.
이미지 파일 바이트는 base64 디코딩 뒤 **4 MiB 이하**여야 하며,
이는 디코딩된 픽셀 메모리에 대한 제한이 아닙니다.
[이미지 결과 계약](agent.md#image-results)에서 정규화와 명령 도구에 대한
같은 MIME/크기 검사를 설명합니다. 전용 출력 디렉터리의 파일을 가리킬 수 있는 것은
그 명령 도구뿐입니다.

### 현재 프로젝트 그래프 {#current-project-graph}

어떤 Artifact에든 `builtin = "graph", args = []`를 선언하세요. graph Artifact는
필요 없습니다. 이 도구는 선택된 리뷰의 과거 Run이나 그 범위만이 아니라
언제나 현재 프로젝트 전체의 선언을 읽습니다. 소스
내용을 읽거나, eval을 시작하거나, 의존성을 추가하지 않습니다. 대상별 두 도구는
그래프 fingerprint를 만드는 것과 정확히 같은 추출을 씁니다.

- **Agent:** 모델 입력 없음(`{}`만). 이름 기준이며 순환에 안전한
  Artifact와 그 eval의 트리, 추출된 정의/평가 기준 전체,
  그래프 fingerprint, 그리고 입력에서 소비자로 가는 라벨 붙은 화살표를 출력합니다.
- **Human:** `kind = "launch"`. 앱을 여는 일반적인 방식대로
  `artifactize-desktop graph --repo PATH`를 분리된 별도 프로세스로 시작합니다. `artifactize-desktop`이
  설치되어 있고 프로그램 검색 경로로 찾을 수 있어야 합니다. 앱은 그
  브랜치의 [Current](../../desktop/index.md#current)에서 열립니다. Run과 같은 그래프 렌더러와
  키보드/확대 조작을 쓰지만, Run 판정 대신 현재 선언과
  Artifact마다 재사용될지 실행될지 표시를 보여 줍니다. **Structure and
  evaluation criteria**를 펼치면 Agent가 보는 것과 같은 전체 텍스트 트리가 나오고,
  **Refresh** 또는 Ctrl+R/F5로 다시 스캔합니다. Run이 하나도 없어도 동작합니다. 도구는
  승인이 아니라 실행 확인을 돌려주며, 호출한 도구가 끝나도 앱은
  계속 실행됩니다.

### `args`의 경로 {#paths-in-args}

파일 시스템 대상은 실제 리뷰에 받아들여진 범위 안에 있어야 합니다.

- **고정 Agent 도구**는 선언한 Artifact 기준의 논리적 경로를 쓰며,
  이 경로는 자식 폴더(`reqs/requirements.md`)로 이어질 수 있습니다. 탐색과
  `config check`는 그 Artifact의 자식 범위에 대해 이 대상을 검증하며,
  없거나 범위 밖인 대상을 거부합니다. 호출 때도 다시 확인합니다.
- **Human 도구**는 같은 논리적 경로를 받으며,
  [Human 도구](human.md#human-tools)처럼 `{artifactPath}` 또는
  `{artifactPath}/path`, 그리고 `{name}` 또는 `{name}/path`(`name`은 Artifact)도 받습니다.
  `--flag=` 접두사는 받지 않습니다.
  탐색과 `config check`는 참조 이름과 대상 존재 여부를 더 좁은 리뷰 범위가 아니라
  워크스페이스에 대해 검증합니다. 그래서 존재하는 `{other}/file`은
  자식이 아니어도 설정 검증을 통과할 수 있습니다. 사전 확인과 호출은
  `other`가 실제 Human eval 범위에 받아들여질 것을 요구하므로,
  eval이 `{other}` 자체를 참조해야 합니다. Artifact만 대상으로 하는 도구 확인은 그
  Artifact의 자식 범위만 씁니다. 도구 피연산자는 결코 의존성을 추가하거나 그 범위를 넓히지 않습니다.

파일 시스템 대상은 설정 검증 때와 모든 호출 때 절대 경로, `..`, 심볼릭 링크, 특수 파일을
거부합니다. `open`은 추가로 HTTP(S)
URL을 받습니다. `help`는 자리 표시자 없이 리터럴 프로그램이나 경로와 하위 명령 인자를 받습니다.
이는 프로그램 인자이지, 이 범위 검사의 대상인 파일 시스템 대상이 아닙니다.

### 재사용 {#reuse}

내장 도구의 이름과 `args`를 포함한 도구 선언은
[재사용 키](../fingerprint.md#the-reuse-key)에 들어가지 않습니다. 도구는
Artifact를 보는 방법이기 때문입니다. 도구를 바꿔도 이전 판정을 재사용하며, 내장 도구의 동작을 바꾸는
새 artifactize 릴리스도 마찬가지입니다. 도구가 읽는 파일이 바뀔 때 다시 리뷰하려면
그 파일을 fingerprint에 포함하세요. 기본 artifactsum은 Artifact 자신의 파일을 포함합니다.

## `artifactize-tools` CLI {#artifactize-tools-cli}

`artifactize tools`는 Microsoft Store 패키지를 포함한 모든 설치에서 같은
현재 디렉터리 도구를 제공합니다. Store가 아닌 채널은 `artifactize` 옆에
`artifactize-tools`도 둡니다. 어느 표기든 `artifactize tools --version`을 포함해 같은 파서와 실행기, 출력,
종료 코드를 씁니다. 기존의 `artifactize tools check`는 현재 디렉터리 도구를 실행하는 대신
선언된 리뷰 도구를 확인합니다.

runtime eval, fingerprint 스크립트, 직접 만든 스크립트에서 쓰세요.

```sh
artifactize tools read DEV-CODING.md             # also: artifactize-tools read DEV-CODING.md
artifactize-tools list [PATH]
artifactize-tools glob PATTERN [PATH]
artifactize-tools grep [--glob GLOB] [--case-insensitive] PATTERN [PATH]
artifactize-tools section spec.md "Install"
artifactize-tools help cargo build          # cargo build --help
artifactize-tools open notes.md             # or an http(s) URL
```

리뷰 밖에는 Artifact 범위가 없으므로 현재 디렉터리가 범위입니다.
read/list/glob/grep/section/open의 파일 시스템 대상은 상대 경로여야 하며 그 아래에
있어야 합니다. `.`은 그 디렉터리 자체를 가리키며 list/glob/grep의 기본값입니다.
절대 경로 대상, `..`, 심볼릭 링크는 링크가 안쪽을 가리키더라도 거부됩니다.
이는 help의 리터럴 실행 파일이나 하위 명령 인자에 대한 제한이 **아닙니다**.
위에서 설명했듯이 help의 평범한 찾기는 절대/상위 경로와 실행 파일 심볼릭 링크를 받아들입니다.
URL을 받는 것은 `open`뿐이며,
그것도 `http://`나 `https://`만 받습니다.

`read`, `section`, `help`는 위의 제한으로 텍스트를 출력하고, `list`, `glob`,
`grep`은 Agent 도구의 JSON 결과를 출력합니다. 명령줄에서 `help`는
터미널 환경에서 모든 `ARTIFACTIZE_*` 변수를 뺀 환경으로, 전용 임시 디렉터리나
출력 디렉터리 없이 프로그램을 실행합니다. 도구 오류는 stderr로 가고 종료 코드는
1이며, 잘못된 사용법은 2로 종료합니다. `artifactize-tools --help`와 `artifactize-tools
SUBCOMMAND --help`가 각 하위 명령을 설명합니다. 진입점은 `artifactize tools
--help`와 `artifactize tools SUBCOMMAND --help`를 씁니다. `artifactize tools help PROGRAM
SUBCOMMAND...`는 clap의 도움말 탐색 명령이 아니라 내장 help 도구입니다.
