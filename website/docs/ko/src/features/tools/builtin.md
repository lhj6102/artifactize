# 내장 도구 {#built-in-tools}

스크립트를 쓰거나 `cat`, `xdg-open` 같은 OS 프로그램을 부르는 대신 `builtin`에 이름을 적어
리뷰어에게 내장 도구를 제공하세요. `[views.agent_tools]` 아래에 `read = { builtin = "read" }`를
적으면 Agent가 그 Artifact의 파일을 무엇이든 읽을 수 있고, `args`를 주면 정해진 파일이나 섹션
하나를 보여 줍니다. 다른 형태는 다음과 같습니다.

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

내장 도구는 Linux, macOS, Windows에서 똑같이 동작합니다. 다른 점은 디스크에 있는 경로 표기를
확인하는 방식([경로](../others/platforms.md#paths) 참고)과, Windows의 junction과 그 밖의 reparse
point를 심볼릭 링크처럼 거부한다는 점뿐입니다. 파일이나 URL을 기본 애플리케이션으로 여는 OS
명령(Linux에서는 `xdg-open`)을 쓰는 `open`과, 요청한 프로그램을 실행하는 `help`를 빼면 따로
설치할 것이 없으며, [`artifactize-tools`](#artifactize-tools-cli) 명령도 필요 없습니다. 그 명령과
`artifactize tools`는 스크립트와 Runtime eval에 파일 시스템과 프로그램 도구를 제공합니다. 현재
프로젝트의 구조를 보여 주는 `graph` 도구는 artifactize의 리뷰 도구로 선언해야 쓸 수 있으며, 독립
실행형 도구 CLI에는 없습니다.

내장 도구 선언은 `builtin`, `args`(모델 입력을 받는 Agent 도구에서는 선택), 선택 항목인
`description`을 받으며, 도구마다 기본 설명이 있습니다. Human 내장 도구는 `kind`도 적을 수
있지만 도구와 맞아야 합니다(`open`과 `graph`는 실행, 나머지는 출력). 맞지 않으면
`config check`가 거부합니다. 내장 도구는 `timeout_ms`, `command`, `protocol`, `input_schema`를
받지 않습니다. 리뷰어는 [명령 도구](agent.md#agent-tools)처럼 각 도구를
`<name>_<artifactId>`라는 이름으로 씁니다.

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
| `graph` | — | `[]` | `[]`(실행) | Agent: eval, 기준, 방향 있는 관계를 담은 트리. Human: 데스크톱 뷰어 |
| `open` | — | — | `[file, folder 또는 URL]`(실행) | 없음. 기본 애플리케이션으로 엽니다 |

`args`가 없으면 Agent 도구는 모델의 입력을 받습니다. `read`, `list`, `glob`, `grep`,
`view_image`의 입력, 결과, 한도는 [Agent 도구](agent.md#agent-tools)에 있습니다. `args`를 고정하면 인자를 하나만 지정한 `section`을 제외하고는 모델에서 인자를 받지 않습니다. 인자를 고정한 Agent 도구 중 `list`는 구조화된 JSON 내용 블록을 돌려주고, `read`, `section`, `help`는 텍스트 블록을 돌려줍니다.

- **`read`**는 UTF-8 파일을 최대 64 KiB까지 출력하며, 더 긴 파일은 `[output truncated]`로
  끝납니다. 파일 앞부분에서 실제로 읽은 범위만 검사하므로, 그 안에 NUL이나 잘못된 UTF-8이 있으면 거부하지만 출력에서 잘린 뒷부분에 있으면 감지하지 못할 수 있습니다. CLI의 고정 read도 한도가 같습니다.
- **`list`**는 모델 입력을 받는 `list`처럼 폴더 항목을 정렬한 목록의 첫 페이지를 JSON으로 출력하지만, offset이나
  limit을 받지 않아 다음 페이지를 가져올 수 없습니다. 항목이 더 많은 폴더라면 모델 입력을 받는
  `list`나 직접 만든 명령을 선언하세요.
- **`section`**은 Markdown 파일의 섹션 하나를 출력합니다. 수준과 상관없이 텍스트가 `heading`과 정확히 같은 첫 제목을 찾습니다. `#` 제목과 밑줄 제목을 모두 찾되, 코드 블록 안의 제목은 제외합니다.
  섹션은 그 제목부터 같거나 더 높은 수준의 다음 제목까지이므로 하위 섹션도 포함합니다. 일치하는
  제목이 없으면 오류에 그 파일의 제목 목록이 나옵니다. 모델이 넘기는 제목은 1–4096자여야 하고,
  선언한 제목은 비어 있으면 안 됩니다. 파일은 최대 8 MiB, 출력하는 섹션은 최대 64 KiB입니다.
- **`help`**는 `program subcommand... --help`를 실행하고 stdout을 먼저 출력한 뒤 stderr를 이어서 출력하며,
  UTF-8로 읽고 터미널 색 코드와 탭, LF, CR을 뺀 ASCII 제어 문자를 지웁니다(DEL은 남깁니다).
  program은 글자 그대로의 프로그램 이름이나 경로입니다. 경로 없는 이름은 `PATH`(Windows에서는
  `PATHEXT`도)로 찾습니다. 범위가 정해진 명령 도구 실행 파일과 달리 help는 일반적인 프로그램
  찾기를 쓰므로 절대 경로, 상위 폴더 기준 상대 경로, 실행 파일 심볼릭 링크를 받습니다. 상대
  경로는 선언한 Artifact 폴더에서 시작하고, 하위 명령 인자는 범위 해석 없이 그대로 넘깁니다.
  프로그램은 그 폴더에서 빈 stdin으로 실행되며, 별도의 출력·임시 디렉터리를 받지 않습니다.
  Agent용 `help`는 작성자의 환경을 쓰되, `ARTIFACTIZE_*` 변수는 대소문자를 가리지 않고 모두 뺍니다. Human용 `help`는 `ARTIFACTIZE_REMOTE_TOKEN` 같은 자격 증명과 `ARTIFACTIZE_*`
  변수를 포함한 리뷰어의 전체 환경에서 실행됩니다. 10초 안에 끝나고, 종료 코드 0이어야 하며, stdout과
  stderr를 합쳐 최대 64 KiB까지 출력해야 합니다. 종료 코드가 0이 아니면 stdout 대신 종료 상태와 프로그램의 stderr를 그대로 담은 오류를 돌려줍니다. 실제 프로그램을 실행하므로 신뢰하는
  프로그램만 적으세요.
- **`open`**(Human 전용)은 파일, 폴더, `http://`/`https://` URL 하나를 데스크톱의 기본
  애플리케이션으로 엽니다. Linux와 WSL에서는 `xdg-open`, macOS에서는 `open`, Windows에서는
  Windows 셸(ShellExecute)을 씁니다. 대상은 셸을 거치지 않고 인자 하나로 넘깁니다. `file:`이나
  `mailto:` 같은 다른 URL 스킴은 거부합니다. 결과에는 애플리케이션을 시작했다는 사실만 기록되며,
  실행한 애플리케이션은 자동으로 종료되지 않으므로 리뷰어가 직접 관리합니다. Linux, WSL, macOS에서는 여는 프로그램 자체가
  3초 안에 성공으로 끝나야 하며, 여는 프로그램이 없거나, 시간이 초과되거나, 0이 아닌 코드로
  끝나면 도구 오류입니다. WSL에서는 `xdg-open`의 종료 상태 4도 성공으로 간주합니다. `explorer.exe`를
  통해 파일을 여는데, `explorer.exe`는 파일을 연 뒤에도 실패를 보고하기 때문입니다. 3초 제한은 애플리케이션을 여는 명령에만 적용되며, 열린 애플리케이션의 실행 시간에는 적용되지 않습니다.

### view_image {#view_image}

`view_image`는 Agent 전용 도구이며 모델 입력 `{"path":"board.png"}`가 필요합니다. 경로는 허용된
Artifact 범위 안의 논리적 경로이며, 다른 내장 도구와 마찬가지로 거슬러 올라가는 경로와 심볼릭 링크를 확인합니다.
비어 있지 않은 일반 파일을 읽어서, 파일 이름을 출력하는 대신 내장 이미지 블록(`type`, base64
`data`, `mimeType`)을 돌려줍니다.

형식은 확장자가 아니라 파일 내용으로 정합니다. PNG, JPEG, WebP를 지원하고, GIF, BMP, 애니메이션
PNG와 그 밖의 형식은 거부합니다. 파일은 최대 **4 MiB**입니다.
[이미지 결과 규약](agent.md#image-results)에서 명령 도구의 정리 방식과 같은 MIME 유형·크기 검사를
설명합니다. 호출별 출력 디렉터리의 파일을 이미지 결과로 돌려줄 수 있는 것은 명령 도구뿐입니다.

### 현재 프로젝트 그래프 {#current-project-graph}

어느 Artifact에든 `builtin = "graph", args = []`를 선언하세요. 그래프 Artifact는 필요 없습니다.
이 도구는 고른 리뷰의 Run 기록이나 범위와 관계없이 항상 현재 프로젝트의 선언 전체를 읽으며,
eval을 시작하거나 의존성을 더하지 않습니다.
[그래프 fingerprint](../fingerprint.md#graph-fingerprint)가 포함하는 것과 같은 그래프를
보여 줍니다.

- **Agent:** 모델 입력이 없습니다(`{}`만). 소스 파일은 읽지 않고 선언만 읽습니다. Artifact와
  그 eval을 이름 기준으로 정리한 트리를 출력하며, 순환 참조가 있어도 같은 항목을 끝없이 반복하지 않습니다. 추출한
  정의와 평가 기준 전체, 그래프 fingerprint, 입력에서 사용처로 향하는 화살표와 각 화살표의 이름도 함께 출력합니다.
- **Human:** `kind = "launch"`. 다른 launch 도구와 마찬가지로 애플리케이션을 호출 프로세스와 분리해 실행하는 방식으로
  `artifactize-desktop graph --repo PATH`를 시작합니다. `artifactize-desktop`이 설치되어 있고
  프로그램 검색 경로에서 찾을 수 있어야 합니다. 앱은 그 브랜치의
  [Current](../../desktop/index.md#current)로 열리며, 워크트리의 파일을 읽어 무엇을
  재사용하거나 실행할지 미리 보여 줍니다. 그래프 렌더러와 키보드 조작, 확대·축소 방식은 Run 화면과 같습니다. 다만 Run 판정 대신 현재 선언을 보여 주며, 각 Artifact에 기존 결과를 재사용할지 새로 실행할지 표시합니다.
  **Structure and evaluation criteria**를 펼치면 Agent가 보는 것과 같은 전체 텍스트 트리가
  나옵니다. **Refresh**나 Ctrl+R/F5로 다시 읽습니다. Run이 하나도 없어도 동작합니다. 도구 실행 결과는 승인이 아니라 앱을 시작했다는 확인일 뿐이며, 도구 호출이 끝나도 앱은 계속 실행됩니다.

### `args`의 경로 {#paths-in-args}

파일 시스템 대상은 실제 리뷰에 허용된 범위 안에 있어야 합니다.

- **고정 Agent 도구**는 선언한 Artifact 기준의 논리적 경로를 쓰며, 자식 폴더 안을 가리킬 수도
  있습니다(`reqs/requirements.md`). 선언을 찾거나 `config check`를 실행할 때 대상이 존재하고 그 Artifact의
  자식 범위 안에 있는지 확인하며, 조건에 맞지 않으면 거부합니다. 호출할 때도 다시 확인합니다.
- **Human 도구**는 같은 논리적 경로와 함께 `{artifactPath}`나 `{artifactPath}/path`,
  `{name}`이나 `{name}/path`(`name`은 Artifact)도 받습니다.
  [Human 도구](human.md#human-tools)를 보세요. `--flag=` 접두어는 받지 않습니다. 선언을 찾거나 `config check`를 실행할 때는 리뷰 범위보다 넓은 작업 공간 전체를 기준으로 참조 이름과 대상이 존재하는지 확인합니다. 그래서 `other`가 자식 Artifact가 아니더라도 `{other}/file`이 존재하면 설정 검사를
  통과할 수 있습니다. 사전 확인과 도구 호출 때 `other`가 그 Human eval의 범위에 들어 있어야 하므로, 그
  eval에서 `{other}`를 직접 참조해야 합니다. Artifact만 대상으로 하는 도구 확인은 그 Artifact의 자식
  범위만 씁니다. 도구 인자는 의존성을 더하거나 범위를 넓히지 않습니다.

설정을 검사하거나 도구를 호출할 때마다 파일 시스템 대상이 절대 경로, `..`, 심볼릭 링크, 특수
파일이면 거부합니다. `open`은 HTTP(S) URL도 받습니다. `help`는 자리 표시자 없이 글자 그대로의 프로그램
이름이나 경로와 하위 명령 인자를 받습니다. 이 인자는 프로그램 인자이지, 이런 범위 확인을 받는
파일 시스템 대상이 아닙니다.

### 재사용 {#reuse}

내장 도구의 이름과 `args`를 포함한 도구 선언은 [재사용 키](../fingerprint.md#the-reuse-key)에
들어가지 않습니다. 도구는 Artifact를 보는 방법일 뿐이기 때문입니다. 도구를 바꿔도 이전 판정을
재사용하며, 내장 도구의 동작을 바꾼 새 artifactize 릴리스도 마찬가지입니다. 도구가 읽는 파일이
바뀔 때 다시 리뷰하려면 그 파일을 fingerprint에 포함하세요. 기본 artifactsum 계산에는 그 Artifact에
속한 파일이 들어갑니다.

## `artifactize-tools` CLI {#artifactize-tools-cli}

Microsoft Store를 포함해 어떤 방법으로 설치하든 `artifactize tools`로 현재 디렉터리에서 같은 도구를
쓸 수 있습니다. Store 이외의 방법으로 설치하면 `artifactize-tools`도 `artifactize`와 같은 디렉터리에 설치됩니다. 어느 명령을 쓰든 인자, 출력, 종료 코드는 같으며, `artifactize tools --version`도 같은 방식으로 동작합니다. 기존의
`artifactize tools check`는 현재 디렉터리 도구를 실행하는 것이 아니라 선언한 리뷰 도구를
확인합니다.

Runtime eval, fingerprint 스크립트, 직접 만든 스크립트에서 다음처럼 씁니다.

```sh
artifactize tools read DEV-CODING.md             # also: artifactize-tools read DEV-CODING.md
artifactize-tools list [PATH]
artifactize-tools glob PATTERN [PATH]
artifactize-tools grep [--glob GLOB] [--case-insensitive] PATTERN [PATH]
artifactize-tools section spec.md "Install"
artifactize-tools help cargo build          # cargo build --help
artifactize-tools open notes.md             # or an http(s) URL
```

리뷰 중이 아닐 때는 Artifact 범위가 없으므로 현재 디렉터리를 범위로 삼습니다. read/list/glob/grep/section/open의
파일 시스템 대상은 상대 경로여야 하고 그 아래에 있어야 합니다. `.`은 그 디렉터리 자체를 가리키며
list/glob/grep의 기본값입니다. 절대 경로 대상, `..`, 심볼릭 링크는 링크가 안쪽을 가리키더라도
거부합니다. `help`의 프로그램과 인자에는 이 규칙이 적용되지 않으며, [`help`](#tools)에서처럼
찾습니다. URL은 `open`만 받으며, `http://`나 `https://`만 됩니다.

`read`, `section`, `help`는 [내장 도구](#tools)에 설명된 한도까지 텍스트를 출력합니다. `section`은 제목이 정확히 일치해야 합니다. `read`와 `section`의 출력이 64 KiB를 넘으면 잘렸다는 표시를 끝에 붙입니다. `help`의 출력이 64 KiB를 넘으면 `Help output exceeds 64 KiB.`를 출력하고 종료 코드 1로 실패합니다.
`list`, `glob`, `grep`은 [Agent 도구](agent.md#agent-tools)의 JSON 결과를 그 패턴 문법과 한도대로
출력합니다. 현재 디렉터리는 선언된 Artifact가 아니므로 `artifactId`는 `"cwd"`입니다. `notes.md`만
있는 폴더에서 `artifactize-tools list .`를 실행하면 다음이 출력됩니다.

```json
{"artifactId":"cwd","path":"","entries":[{"kind":"file","name":"notes.md","path":"notes.md"}],"totalEntries":1,"truncated":false,"nextOffset":null}
```

명령줄에서 `help`는 터미널 환경에서 `ARTIFACTIZE_*` 변수를 모두 뺀 환경으로, 별도의
임시·출력 디렉터리 없이 프로그램을 실행합니다. 실행이 끝나면 성공했더라도 그 프로그램이 시작한 뒤 아직
실행 중인 프로그램을 모두 멈추므로, `help`로 보조 프로그램을 계속 띄워 둘 수는 없습니다. 계속 실행해야 한다면
[도구 세션](agent.md#tool-sessions)을 쓰세요. 도구 오류는 stderr에 출력하고 종료 코드 1을 돌려주며, 명령을 잘못 사용한 경우에는 종료 코드 2를 돌려줍니다. `artifactize tools --help`(또는 `artifactize-tools --help`)는
도구 목록을, `artifactize tools SUBCOMMAND --help`는 도구 하나의 설명을 보여 줍니다.
`artifactize tools help PROGRAM SUBCOMMAND...`는 다릅니다. `PROGRAM SUBCOMMAND... --help`를
실행해서 그 프로그램의 도움말을 출력합니다.

Artifact 범위가 없으면 `list`는 항목의 첫 페이지만 돌려주고 offset이나 limit을 받지 않습니다.
항목이 더 많은 폴더에는 `glob`이나 직접 만든 명령을 쓰세요.
