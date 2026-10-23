# Agent 도구 {#agent-tools}

도구 Artifact의 `[views.agent_tools.NAME]` 아래에 명령 도구를 선언하고, eval에서 그 Artifact를
참조하세요. 그러면 그 eval의 Agent는 이 도구를 `NAME_ARTIFACT`라는 이름으로 쓸 수 있습니다. 리뷰하기 전에
[`tools check`](../../cli/index.md#tool-diagnostics)로 확인하세요. 단계별 예제는
[도구](index.md#build-your-own-tool-artifact)에 있습니다.

명령 필드 `description`, `protocol`, `command`, `args`는 필수입니다. `input_schema`의 기본값은
`{"type":"object","additionalProperties":false}`(빈 인자만 허용)입니다. 스키마를 직접 지정할 때는 루트가 `type: "object"`여야 합니다. 로컬 `$ref`와 조합을 포함해 표준 JSON Schema가 동작하지만,
파일이나 URL 참조는 동작하지 않습니다. 스키마 기본값이 인자를 바꾸지는 않습니다. 선언과 스키마는
8 MiB까지입니다. 호출 인자는 최대 64 KiB인 JSON 객체여야 하며, 프로세스나 출력 디렉터리를
만들기 전에 이를 확인합니다. 인자 검사에 실패하면 오류 메시지에 이스케이프 처리한 인스턴스·스키마 경로를 최대 다섯 개(총 4 KiB 미만)
표시하며, 인자 값은 표시하지 않습니다.

설명은 비어 있지 않아야 하고, 4000자 이하여야 하며(이모지처럼 기본 다국어 평면 밖의 문자는
2자로 셉니다), `{artifactName}`
치환(정규 Artifact ID)만 지원합니다. 내장 도구 참조는 `builtin`, 선택 항목인 `args`와
`description`을 받습니다. `args`가 없으면 `read`, `list`, `glob`, `grep`, `view_image`는 아래의
모델 입력을 받습니다. `args`가 있으면 `read`, `list`, `section`, `help`는 고정된 대상에 대해
동작하고, `args = []`인 `graph`는 [내장 도구](builtin.md)에서 설명한 대로 현재 프로젝트 구조
전체를 출력합니다. 직접 선언한 도구만 목록에 나옵니다. 내장 도구에는 별도의 프로그램이 필요 없습니다. 다만 `help`는 요청한 프로그램을 시작합니다. `view_image`는 `json` 이미지 결과와 같은 방식으로 이미지를 검사합니다.

| 내장 도구 | 인자 | 결과 |
|---|---|---|
| `graph` | 고정 선언 `args = []`. 호출 `{}` | 이름 기준의 전체 Artifact/eval 트리, 추출한 기준, 그래프 fingerprint, 입력 → 사용처 관계 |
| `read` | `{path, offset?, limit?}`. offset은 1부터 세는 줄(기본 1, 최대 9007199254740991), limit 기본 80, 최대 500 | `artifactId`, `resolvedArtifactId`, `path`, `lines: [{number, text}]`, `startLine`, null일 수 있는 `endLine`, `lineCount`, EOF를 알 때만 `totalLines`, `truncated`, null일 수 있는 `nextOffset` |
| `list` | `{path?, offset?, limit?}`. offset은 0부터 셈(기본 0, 최대 9007199254740991), limit 기본값이자 최대 200 | `artifactId`, `path`, 정렬된 `entries: [{name, path, kind}]`, `totalEntries`, `truncated`, null일 수 있는 `nextOffset` |
| `glob` | `{pattern, path?}`. `path`는 디렉터리여야 함 | 검색한 `path`, 정렬된 논리적 `files` 최대 200개, `truncated` |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}`. 기본은 대소문자 구분, maxResults 기본값이자 최대 200 | 검색한 `path`, `matches: [{path, line, text}]`(일치하는 줄마다 하나, 경로 다음 줄 번호 순), `truncated` |

`read.artifactId`는 **도구를 선언한** Artifact를 가리키고, `resolvedArtifactId`는 자식 폴더를
거쳐 찾은 Artifact를 가리킵니다. 예를 들어 app 안에 `spec`으로 선언된 자식 폴더 `rules`가
있을 때 app의 read 도구를 `{"path":"rules/requirements.md"}`로 호출하면, 줄 데이터와 함께 소유
정보 `{"artifactId":"app","resolvedArtifactId":"spec","path":"rules/requirements.md"}`를
돌려줍니다. `path`는 실제 머신 경로가 아니라 요청한 논리적 경로 그대로입니다. list의 최상위
`artifactId`도 마찬가지로 선언한 Artifact를 가리킵니다. 항목 종류는 `file`, `directory`,
`symlink`, `other`입니다. glob/grep은 검색한 논리적 `path`를 돌려주며, grep이 찾은 줄 번호는
1부터 셉니다.

경로는 도구를 선언한 Artifact를 기준으로 쓰는 상대 논리적 경로이며, 자식 폴더 안도 가리킬 수 있습니다. 경로를 생략하면 해당 Artifact의 루트를 가리킵니다. 다른 Artifact의 파일은 그 Artifact의 도구로 읽으며, eval이 그 Artifact를 참조하면
그 도구를 쓸 수 있습니다. 절대 경로, 경로를 거슬러 올라가는 구성 요소, 심볼릭 링크는 받지
않습니다. 논리적 경로는 4096자까지이고(이모지처럼 기본 다국어 평면 밖의 문자는 2자로 셉니다), ASCII 제어 문자, 백슬래시, 콜론, 빈
구성 요소나 `.`/`..` 구성 요소는 거부합니다. `.` 하나는 list/glob/grep에서 파일 Artifact의
가상 루트로 해석하고, 그 밖의 점 구성 요소는 거부합니다. 도구는 그 eval 범위의 Artifact에만
접근할 수 있으며, 다른 eval의 범위에는 접근할 수 없습니다. 파일은 읽기 전용으로 열고, 심볼릭 링크는 따라가지
않으며, 특수 파일은 읽을 수 없습니다. `list`는 `symlink`/`other` 항목을 보여 줄 수 있지만,
검색에서는 건너뜁니다. Windows에서는 junction과 그 밖의 reparse point도 링크로 간주해 대상으로 쓸 수
없으며, `list`는 junction을 `symlink`로, 그 밖의 reparse point를 `other`로 보여 줍니다. 파일 Artifact는 대상 파일만 보여 주고, 작업 디렉터리에 함께 있는 다른 파일은 보여 주지 않습니다. 경로 없이 또는 `path: "."`로 `list`를 호출하면 그 가상 루트의 대상
파일을 보여 줍니다. 대상 파일을 읽을 때는 파일 이름을 지정하며, 경로는 그 파일이 있는 폴더를 기준으로 씁니다.

재사용을 끈 경우에도 내장 도구나 명령 도구를 호출하기 전에 허용 범위에 포함된 대상 파일을 매번 확인합니다. 대상이 없거나, 일반 파일이 아니거나, 심볼릭 링크로 바뀌었으면 명령을 실행하지 않고
오류를 돌려줍니다.

read는 원래 줄을 중간에서 자르지 않고 **줄 전체**를 최대 64 KiB까지 돌려주며, 각 줄의 `text`에 줄 끝(`\n`이나 Windows의
`\r\n`)과 UTF-8 바이트 순서 표시(일부 편집기가 파일 맨 앞에 넣는 보이지 않는 표시)를 그대로
둡니다. 요청한 줄만 디코딩합니다. 잘못된 UTF-8이나 NUL은 오류이고, 요청한 첫 줄이 너무
커도 오류입니다. 빈 파일은 줄 0개와 `totalLines: 0`을 돌려주고, 파일 끝을 넘는 페이지는 줄 0개와 0이
아닌 실제 전체 줄 수를 돌려줍니다. 돌려받은 `text` 값을 이어 붙이면 줄 번호가 붙지 않은 그 범위의 원문을
그대로 복원할 수 있습니다.

`glob` 내장 도구는 디렉터리 `path`가 필요하며, 내용을 보지 않고 일치하는 파일 이름을
고릅니다(바이너리 파일 이름 포함). 파일이나 폴더 이름이 올바른 UTF-8이 아니면, 일치하지 않는
이름이라도 `list`, `glob`, `grep` 호출 전체가 실패합니다. 경로를 좁히거나 이름을 바꾸세요.
`grep`은 파일이나 디렉터리를 받으며, 바이너리(NUL)나 잘못된 UTF-8 내용은 통째로 건너뜁니다.
선택 인자 `glob`은 파일 이름 필터이지 `glob` 도구를 부르는 것이 아닙니다.

glob 패턴은 `path` 기준입니다. `*`는 폴더 이름 하나 안에서, `**`는 여러 폴더에 걸쳐 일치하고,
`?`는 한 글자, `[abc]`나 `[a-z]`는 집합이나 범위에서 한 글자, `{md,txt}`는 둘 중 하나에
일치합니다. `*`, `?`, `[`, `{`를 글자 그대로 찾으려면 `[*]`처럼 대괄호에 넣으세요. 백슬래시
이스케이프는 받지 않습니다. glob과 grep 패턴(grep의 선택 인자 `glob` 포함)은 비어 있지 않고
4096자 이하여야 합니다. glob은 상대 경로여야 하고, 앞의 슬래시, 백슬래시, 콜론, NUL, 빈 구성
요소나 `.`/`..` 구성 요소가 없어야 합니다. grep의 패턴은 Rust `regex` 문법의 정규 표현식이며,
예를 들어 `fn \w+_test`는 `fn `과 `_test`로 끝나는 이름이 있는 줄을 찾습니다. 비어 있거나,
잘못됐거나, 너무 큰 패턴은 오류입니다. grep의 선택 인자 `glob` 필터는 상대 파일 경로(`path`가
파일이면 파일 이름)에 맞춰 봅니다. 숨김 파일도 포함하며, git ignore 규칙으로 결과를 거르지
않습니다. 탐색할 수 있는 항목은 최대 10,000개이며, grep은 파일당 8 MiB, 검색 전체에서 64 MiB까지 읽습니다. JSON 결과 하나의 크기는 최대 512 KiB입니다. 검색 한도에 도달하거나 너무 큰 파일을 건너뛰면 `truncated: true`가 되니, 경로나
패턴을 좁혀서 이어 가세요. 이와 별개로, 항목이 10,000개 넘는 디렉터리를 하나라도 만나면 `list`,
`glob`, `grep`은 잘린 검색 결과가 아니라 오류로 실패합니다. list는 결과의 바이트 수가 한도에 이르면
항목 수 제한에 못 미쳐도 결과를 다음 페이지로 나눌 수 있습니다. 내장 도구의 인자에도 명령 도구와 같은 크기 제한과 JSON Schema
검사를 적용합니다.

`json` 프로토콜은 stdin으로 요청을 정확히 하나 받습니다.

```json
{
  "version": 1,
  "context": {
    "artifactId": "example",
    "artifactPath": "/workspace/example",
    "outputDir": "/external/private/output",
    "tmpDir": "/external/private/tmp",
    "scope": {
      "example": {"path": "/workspace/example", "kind": "folder", "children": {}}
    },
    "executionPaths": {"shared/rules.json": "/workspace/shared/rules.json"}
  },
  "args": {"section": "summary"}
}
```

scope 항목에는 정규화된 실제 경로, `kind`(`file`, `folder`, `graph`), 논리적 자식 맵이 들어
있습니다. 파일 Artifact라면 `context.artifactPath`와 그 scope 항목의 `path`는 프로세스 작업
디렉터리가 아니라 그 파일을 가리키며, 작업 디렉터리는 파일이 들어 있는 폴더입니다. 파일
Artifact 참조 뒤에는 `/path`처럼 하위 경로를 붙일 수 없습니다. 이 프로토콜 키는 camelCase를 유지하며,
선언에서는 `execution_paths`와 `input_schema`를 씁니다. `executionPaths`는 비어 있으면
생략합니다. 실행 경로로는 작업 공간 기준 파일이나 디렉터리를 중복 없이 최대 64개 선언할 수 있으며, 각 경로는
1024자까지입니다(이모지처럼 기본 다국어 평면 밖의 문자는 2자로 셉니다). 경로는 심볼릭 링크를 거치지 않고 해석하며, 대상을 복사하지 않습니다. 파일
Artifact에서도 작업 공간 기준이며, 작업 디렉터리나 대상 기준이 아닙니다.

Agent 리뷰가 시작되면 artifactize는 eval 도구의 각 실행 경로에 대한 SHA-256(디렉터리라면 그
안의 파일과 심볼릭 링크)을 `coverage_spec` 같은 공개 도구 이름별로
`executionPaths: {"TOOL": {"PATH": "sha256"}}` 형태로 결과에 기록합니다. 이 기록은 원격 결과에도
함께 저장되고 재사용한 결과에도 남으므로, 재사용한 판정을 어느 바이너리가 만들었는지 알 수 있습니다.
경로가 없거나, 특수 파일이거나, 1 GiB나 파일 10,000개를 넘으면 Agent를 부르기 전에
`PREPARATION_FAILED`로 리뷰가 실패합니다. 이 해시는 기록일 뿐
[재사용 키](../fingerprint.md#the-reuse-key)에 들어가지 않습니다. 바이너리가 바뀔 때 다시
리뷰하려면 fingerprint에 그 바이너리를 포함하세요.
명령이 성공하면 stdout에 내용 블록 1–32개를 담은 JSON 객체 하나를 출력해야 합니다.
`{"content":[{"type":"text","text":"..."},{"type":"json","data":{}}]}`.
이미지 블록은 [아래](#image-results)에서 설명합니다. 텍스트는 블록당 최대 64 KiB, 압축한 JSON
데이터는 블록당 최대 512 KiB, 정리한 결과 전체는 최대 8 MiB입니다. stdout과 stderr는 각각
16 MiB까지입니다. 결과 객체는 `content`와 선택 항목인 불리언 `isError`만 받습니다. 각 블록에도
정해진 필드 외의 필드를 넣을 수 없습니다. 텍스트 블록은 `type`과 `text`만, JSON 블록은 `type`과 `data`만, 이미지 블록은
`type`, `mimeType`, 그리고 `data`나 `path` 중 정확히 하나만 받습니다. 결과나 블록에 허용하지 않은 필드가
있으면 이를 메타데이터로 리뷰어에게 전달하지 않고 출력 형식 오류로 처리합니다. 명령이 종료 코드 0으로 오류를 알릴 때는 `{"isError":true,"content":[{"type":"text","text":"Choose a smaller range."}]}`처럼 비어
있지 않은 텍스트 블록이 정확히 하나여야 하며, 리뷰어에게 그대로 전달됩니다. 0이 아닌 종료 코드,
비정상 종료, 잘못되거나 잘린 JSON, 프로세스 실패는 일반 오류가 되며, stderr는 전달하지
않습니다. 관찰 기록은 따로 남기지 않습니다.

### 이미지 결과 {#image-results}

JSON 명령은 `mimeType`과, `data`(표준 base64 바이트)나 `path`(출력 파일) 중 **정확히 하나**를
담은 이미지 블록을 돌려줄 수 있습니다.

```json
{"content":[{"type":"image","mimeType":"image/png","data":"<base64-encoded PNG bytes>"}]}
```

```json
{"content":[{"type":"image","mimeType":"image/png","path":"screenshots/board.png"}]}
```

두 번째 형식으로 파일 경로를 반환하려면 해당 파일을 요청의 `context.outputDir` 아래에 저장하세요. 상대 경로는 `context.outputDir`을 기준으로 해석하며,
절대 경로도 이 디렉터리 안을 가리킬 때만 허용합니다. 상위 경로로 올라가는 경로, 어느
구성 요소에서든 심볼릭 링크를 거치는 경로, 특수 파일, 그 출력 디렉터리 밖의 파일은 거부합니다. artifactize는
호출의 출력을 지우기 전에 이미지 바이트를 읽어 결과에 담으므로, 돌려받는 결과는 로컬 파일 경로가 아니라 이미지
데이터가 모두 담긴 base64 문자열입니다.

이미지는 비어 있지 않은 PNG, JPEG, WebP 파일이며 최대 **4 MiB**입니다. 형식 검사를 통과한 파일도
손상되어 표시되지 않을 수 있습니다. 형식은 확장자가 아니라 파일 내용으로 판단하며, 각각
`image/png`, `image/jpeg`, `image/webp`와 맞아야 합니다. 잘못된 base64, MIME 불일치, GIF/BMP 같은
지원하지 않는 형식, 애니메이션 PNG는 거부합니다. base64로 인코딩한 뒤에도 정리한 도구 결과 전체
8 MiB 한도가 그대로 적용됩니다. [`view_image`](builtin.md#view_image)는 같은 내장 이미지 형태를
돌려주고 같은 바이트/형식 한도를 쓰지만, 파일은 허용된 Artifact 범위에서 읽습니다.

`plain`에서는 선언한 `input_schema.properties`의 최상위 항목만 argv 자리 표시자로 쓸 수
있습니다. 인자 전체를 `{query}`로 쓸 수도 있고, `--query={query}`처럼 인자 일부에 넣을 수도 있습니다. 문자열은 그대로
치환하고, 다른 JSON 값은 압축한 JSON으로 치환합니다. 값이 없거나 NUL 바이트가 있으면 프로세스를
시작하기 전에 실패합니다. `{{`와 `}}`를 쓰면 중괄호 자체를 넣을 수 있습니다. 치환은 한 번만 하므로,
중괄호, 따옴표, 공백, `$()`, 세미콜론이 든 값도 셸 코드가 아니라 글자 그대로의 argv 요소 하나로
남습니다. plain 도구는 빈 stdin을 받습니다. stdout은 UTF-8로 읽어 텍스트 블록 하나로 반환하며, 잘못된 바이트는 대체 문자로 바꿉니다. 터미널 색 코드와 ASCII 제어 문자는 지우되 탭, LF, CR, DEL은 남깁니다.
64 KiB에서 자르고 잘렸다는 표시를 붙이며, 0이 아닌 종료 코드면 그 stdout이 도구 오류로
표시됩니다. stderr는 포함하지 않습니다. `rg`처럼 일치하는 것이 없을 때 0이 아닌 코드로 끝나는
명령이 빈 출력을 정상 결과로 돌려주게 하려면 소유자가 래퍼를 만들어야 합니다.

명령의 실행 파일을 찾을 때도 공통 [프로그램 찾기](../others/platforms.md#program-lookup)
규칙을 따르며, 파일 Artifact에 적용되는 예외도 같고, 허용 범위를 벗어나 위로 올라가는 경로도 똑같이 거부합니다. JSON 프로토콜 argv는
범위가 정해진 기존 Artifact 참조를 쓸 수 있지만, eval이 허용한 범위 밖의 Artifact를 더할 수는
없습니다. plain argv는 스키마 속성 자리 표시자만 씁니다. 명령 자체는 치환하지 않습니다. 명령은 작성자의 환경에서 해당 Artifact 폴더를 작업 디렉터리로 삼아 실행됩니다. 파일 Artifact라면 그 파일이 있는 폴더에서 실행됩니다. [Runtime 환경](../declarations.md#runtime-execution)과 마찬가지로 실제 홈 디렉터리를 쓰며, 호출한 쪽의 `ARTIFACTIZE_*` 변수는 전달하지 않습니다. 호출마다 별도의 임시·출력 폴더를 주고, 호출이 끝나면 그 폴더를 지웁니다. 기본 제한 시간은 120000ms(1–2147483647)이며, 호출이 정상 종료되거나 취소·시간 초과로 끝나면 명령 프로세스와 그 하위 프로세스를 모두 종료합니다.
계속 실행되어야 하는 보조 프로그램에는 [도구 세션](#tool-sessions)을 쓰세요.
명령은 사용자가 신뢰하는 읽기 전용 프로그램으로 보고 샌드박스 없이 실행합니다.

Agent eval은 그 eval의 범위에 있는 모든 Artifact의 Agent 도구를 `coverage_spec`처럼
`<name>_<artifactId>`라는 이름으로 쓸 수 있습니다. 두 도구가 같은 이름이 되면 `config check`는 통과하지만
리뷰는 `PREPARATION_FAILED`로 실패하며, `artifactize tools check`가 이를 알려 줍니다. 이름이
겹치지 않을 때까지 도구나 Artifact의 이름을 바꾸세요. Human 도구는
Agent에게 주지 않습니다.

## 도구 세션 {#tool-sessions}

**도구 세션**을 쓰면 리뷰 하나가 진행되는 동안 하네스를 계속 실행해 두고 여러 도구 호출에서 재사용할 수 있습니다. 저장된 Agent
대화와는 다릅니다. 세션의 start와 stop 명령은 세션에 묶이는 명령 도구와 같은 Artifact에
선언하세요.

```toml
[views.agent_sessions.app]
start = { command = "harness", args = ["start"], execution_paths = ["tools/harness"] }
stop = { command = "harness", args = ["stop"], execution_paths = ["tools/harness"] }

[views.agent_tools.click]
session = "app"
description = "Click an element of the running app."
command = "harness"
args = ["act"]
protocol = "json"
```

`start`와 `stop`에는 `command`와 `args`가 필요하고, 선택 필드는 `execution_paths`뿐입니다. 실행 파일을 찾는 방식과 argv에 적용하는 범위는 명령 도구와 같습니다. stdin으로는 명령 도구와 같은 형식의 JSON 요청을 받으며, `args: {}`를 씁니다. `description`,
`protocol`, `timeout_ms`, 동시 실행 옵션은 받지 않습니다. 명령 도구의 `session`은 그 명령
도구와 같은 Artifact에 선언한 세션을 가리켜야 하며, 내장 도구와 Human 도구는 `session`을 받지 않습니다. 도구
이름처럼, eval이 쓸 수 있는 세션의 `<session>_<artifact>` 이름도 겹치면 안 됩니다. 겹치면
`config check`는 통과하지만 리뷰는 `PREPARATION_FAILED`로 실패하니, 세션이나 Artifact의 이름을
바꾸세요.

하네스의 start 명령은 다음 규칙을 따라야 합니다.

- stdin으로 명령 도구와 같은 형식의 JSON 요청을 읽습니다. `ARTIFACTIZE_SESSION_DIR`은 artifactize가 주는 별도의
  디렉터리이며, 세션에 묶인 도구와 stop이 함께 쓰는 상태를 여기에 둡니다.
- 준비되면 stdout에 줄바꿈으로 끝나는 줄 하나를 출력합니다. 그 내용은 무시합니다.
- 세션 내내 start 프로세스를 계속 실행해 둡니다. 이 프로세스가 끝나면 세션도 끝납니다.

세션에 묶인 도구(`json`과 `plain` 모두)와 stop 명령은 같은 `ARTIFACTIZE_SESSION_DIR`을 받으며,
다른 명령과 마찬가지로 실제 홈 디렉터리를 포함한 작성자의 환경에서 실행됩니다. 호출마다 별도의 출력·임시
디렉터리를 받습니다. 같은 Artifact를 동시에 리뷰하더라도 리뷰마다 별도의 세션 디렉터리와 환경을 씁니다. 포트나 디스플레이는 OS의 표준 방식(예: 포트 0이나 자동으로 고른 디스플레이)으로
고르세요. 데스크톱 하네스는 헤드리스 디스플레이를 직접 시작합니다. Agent는 인스턴스 ID를
보지 못하고 start나 stop을 부를 수 없습니다.

실행 경로는 이전과 마찬가지로 리뷰를 시작할 때 고정합니다. 도구 세션은 Agent가 그 세션에 묶인 도구를
처음 호출할 때 시작하고, 이후의 호출은 그 세션을 다시 씁니다. eval에서 쓸 수 있는 세션이라도 그 세션의 도구를 한 번도 호출하지 않으면 프로세스를 시작하지 않고 세션 디렉터리도 만들지 않습니다. 하네스를 시작하고 준비하는 데 걸린 시간도 리뷰 제한 시간에 포함됩니다. start를 실행할 수 없거나 준비되기 전에 끝나면 Agent에는
`PREPARATION_FAILED`가 아니라 그 도구 호출의 오류가 전달되며, 이후 그 세션을 호출해도 세션을 다시 시작하지 않고
같은 오류를 돌려줍니다. 준비 단계의 제한 시간은 따로 없으며, 준비가 끝나지 않으면 리뷰가 시간 초과되거나
취소될 때까지 start 프로세스가 계속 실행됩니다. 준비 완료 줄 뒤의 출력은 버립니다.

리뷰 중에 start 프로세스가 끝나면 artifactize는 그 프로세스의 하위 프로세스를 종료하고 실행 중인 세션
호출을 취소합니다. 이후의 세션 호출은 ``The tool session `app` ended.``를 돌려주며, 다시
시작하지 않습니다. 이 오류는 다른 도구 오류와 마찬가지로 저장된 대화에 기록됩니다.

리뷰가 어떻게 끝나든(판정, 오류, 시간 초과, 취소) 시작된 세션은 모두 Agent가 끝난 뒤, 결과를
확인하고 저장하기 전에 멈춥니다. 먼저 실행 중인 세션 호출을 취소하고, 그다음 stop을 실행하며,
최대 30초 뒤에는 artifactize가 세션의 프로그램을 직접 끝냅니다. 프로그램이 끝나면 stop이
실패하거나 멈춰 있더라도 세션 폴더를 지웁니다. artifactize 자체가 강제로 종료되면 stop을 실행할
수 없습니다. 정리에 실패하면 성공 결과 대신 그 실패를 알리며, 이전 세션이 정리되지 않으면 새 세션을 시작할 수 없습니다. 결과를 재사용하거나 실행 중인 리뷰에 합류할 때는 아무것도 시작하지 않습니다.

start/stop에서 고정한 실행 경로는 원격 기록을 포함해 결과의 기존 `executionPaths` 출처 정보에
`<session>_<artifact>.start`와 `.stop` 키로 기록됩니다. `tools check`는 명령을 실행하지 않고,
참조한 세션의 실행 파일과 인자만 확인합니다. 세션에 묶인 도구에 대한 `tools check --execute`와
`session send`는 각각 처음 세션 호출을 할 때 새 세션을 시작하고 끝나면 멈춥니다. 세션 도구를
부르지 않는 follow-up은 아무것도 시작하지 않습니다.

세션 명령은 사용자가 신뢰하는 명령으로 보고 **샌드박스 없이** 실행합니다. 작업 공간을 바꿀 수 있고, 부모 프로세스와
일부러 분리해 실행한 프로그램은 계속 실행될 수 있습니다.

도구 선언을 확인하고 도구를 직접 실행하려면
[`tools check`](../../cli/index.md#tool-diagnostics)를 쓰세요.
