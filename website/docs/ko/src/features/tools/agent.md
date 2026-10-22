# Agent 도구 {#agent-tools}

사용자 정의 도구 Artifact를 선언하고 붙이는 방법은 [도구](index.md#build-your-own-tool-artifact)에 있습니다.

명령 필드 `description`, `protocol`, `command`, `args`는 필수입니다.
`input_schema`의 기본값은 `{"type":"object","additionalProperties":false}`입니다
(빈 인자만 허용). 직접 준 스키마는 루트가 `type: "object"`여야 하며,
파일/HTTP 해석 기능을 끈 `jsonschema`로 컴파일됩니다. 로컬 `$ref`,
조합, 그 밖의 표준 스키마 기능이 동작하며, 사용자 정의 키워드
부분 집합은 없습니다. 스키마 기본값은 결코 인자를 바꾸지 않습니다. 선언/스키마는
8 MiB로 제한됩니다. 호출에는 프로세스나 출력 디렉터리가 만들어지기 전에
최대 64 KiB인 JSON 객체가 필요합니다. 검증 오류는 크기가 제한되고 이스케이프된
인스턴스/스키마 경로를 최대 다섯 개(4 KiB 미만)까지 보여 주며, 인자 값은 결코 보여 주지 않습니다.

설명은 비어 있지 않아야 하고, UTF-16 코드 단위 4000개 이하여야 하며,
`{artifactName}` 치환(정규 Artifact ID)만 지원합니다. 내장 도구 참조는
`builtin`, 선택적인 `args`, 선택적인 `description`을 받습니다. `args`가 없으면 `read`,
`list`, `glob`, `grep`, `view_image`는 아래의 모델 입력을 받습니다. `args`가 있으면
`read`, `list`, `section`, `help`는 고정 대상에 대해 동작하고, `args = []`인 `graph`는
[내장 도구](builtin.md)에 설명된 대로 현재 프로젝트 구조 전체를 출력합니다.
명시적으로 선언한 도구만 나열됩니다.
내장 도구는 출력 디렉터리 없이 프로세스 안에서 실행됩니다(`help`만 자기
프로그램을 시작). `view_image`는 `json` 이미지 결과와 같은 이미지 검사를 적용합니다.

| 내장 도구 | 인자 | 결과 |
|---|---|---|
| `graph` | 고정 선언 `args = []`. 호출 `{}` | 이름 기준의 전체 Artifact/eval 트리, 추출된 기준, 그래프 fingerprint, 입력 → 소비자 관계 |
| `read` | `{path, offset?, limit?}`. offset은 1부터 세는 줄(기본 1), limit 기본 80, 최대 500 | `artifactId`, `resolvedArtifactId`, `path`, `lines: [{number, text}]`, `startLine`, null 가능한 `endLine`, `lineCount`, EOF를 알 때만 `totalLines`, `truncated`, null 가능한 `nextOffset` |
| `list` | `{path?, offset?, limit?}`. offset은 0부터 셈(기본 0), limit 기본값이자 최대 200 | `artifactId`, `path`, 정렬된 `entries: [{name, path, kind}]`, `totalEntries`, `truncated`, null 가능한 `nextOffset` |
| `glob` | `{pattern, path?}`. `path`는 디렉터리여야 함 | 검색한 `path`, 정렬된 논리적 `files` 최대 200개, `truncated` |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}`. 기본은 대소문자 구분, maxResults 기본값이자 최대 200 | 검색한 `path`, `matches: [{path, line, text}]`(일치하는 줄마다 하나, 경로 다음 줄 번호 순), `truncated` |

`read.artifactId`는 **도구를 선언한** Artifact를 가리키고,
`resolvedArtifactId`는 자식 폴더를 거쳐 닿은 Artifact를 가리킵니다.
예를 들어 app이 `spec`으로 선언된 자식 폴더 `rules`를 소유한다면, app의 read 도구를
`{"path":"rules/requirements.md"}`로 호출하면 줄 데이터와 함께 소유 메타데이터
`{"artifactId":"app","resolvedArtifactId":"spec","path":"rules/requirements.md"}`를
돌려줍니다. `path`는 물리적 머신 경로가 아니라 요청한 논리적 경로로 남습니다.
list의 최상위 `artifactId`도 마찬가지로 선언한 Artifact를 가리킵니다.
항목 kind는 `file`, `directory`, `symlink`, `other`입니다. glob/grep은
검색한 논리적 `path`를 돌려주며, grep의 일치하는 줄 번호는 1부터 시작합니다.

경로는 도구를 선언한 Artifact(그 자식 포함) 기준의 상대 논리적 경로이며,
경로를 생략하면 그 루트를 뜻합니다. 다른 Artifact의 파일은 그
Artifact의 도구로 읽으며, eval이 그 Artifact를 참조하면 그 도구를 쓸 수 있습니다. 도구는 결코
절대 경로, 경로 탈출 구성 요소, 심볼릭 링크를 받지 않습니다. 논리적 경로는 최대
UTF-16 코드 단위 4096개이며, ASCII 제어 문자, 백슬래시, 콜론, 빈
구성 요소나 `.`/`..` 구성 요소를 거부합니다. 단독 `.`은
list/glob/grep에서 파일 Artifact의 가상 루트로 받아들이며, 다른 점 구성 요소는
거부합니다. 레지스트리 범위는 다른 eval의 참조가 아니라 그 eval의
eval 범위에 들어간 Artifact로 남습니다. 경로는 구성 요소마다 링크를 따라가지 않는지 확인하며
고정된 디렉터리 디스크립터를 통해 읽기 전용으로 열리고, 특수 파일은
읽을 수 없습니다. `list`는 `symlink`/`other` 항목을 보고할 수 있지만, 검색은 이를 건너뜁니다.
파일 Artifact는 작업 디렉터리의 관련 없는 형제 파일이 아니라 대상 파일만
노출합니다. 경로 없이 또는 `path: "."`로 `list`를 호출하면 그 가상 루트의
대상 파일을 나열합니다. 대상은 파일 이름으로 읽으세요. 파일 경로는
들어 있는 폴더 기준 상대 경로로 남습니다.

모든 호출은 내장 도구나 명령 도구를 부르기 전에, 재사용이 꺼져 있더라도
eval 범위의 파일 대상을 검증합니다. 대상이 없거나, 일반 파일이 아니거나, 심볼릭 링크로 바뀌었으면
명령을 실행하지 않고 오류를 돌려줍니다.

read는 **원래 줄의 바이트 전체**를 최대 64 KiB까지 돌려주며, 각 줄의 `text`에
LF, CRLF, UTF-8 BOM을 보존합니다. 요청한 줄만 디코딩합니다.
잘못된 UTF-8/NUL은 오류이며, 처음 요청한 줄이 너무 커도 오류입니다. 빈
파일은 줄 0개와 `totalLines: 0`을 돌려주고, EOF를 넘은 페이지는 줄 0개와
실제 0이 아닌 전체 줄 수를 돌려줍니다. 돌려받은 `text` 값을 이어 붙이면
줄 번호 접두사가 덧붙지 않은 원본 범위가 그대로 재현됩니다.

`glob` 내장 도구는 디렉터리 `path`가 필요합니다. 내용을 확인하지 않고
일치하는 파일 이름을 고르며, 바이너리 파일 이름도 포함됩니다. `grep`은 파일이나
디렉터리를 받으며, 바이너리(NUL) 내용이나 잘못된 UTF-8 내용은 통째로 건너뜁니다.
선택적인 `glob` 인자는 파일 이름 필터이지, `glob` 도구를 호출하는 것이 아닙니다.

glob 패턴은 `path` 기준으로, 구성 요소 안에서는 `*`를, 디렉터리를 가로질러서는 `**`를 씁니다.
glob과 grep 패턴(grep의 선택적인 `glob` 포함)은 비어 있지 않아야 하고 최대
4096자입니다. glob은 상대 경로여야 하며, 앞의 슬래시,
백슬래시, 콜론, NUL, 빈/`.`/`..` 경로 구성 요소가 없어야 합니다. grep은 Rust `regex`
문법을 쓰며, 컴파일된 정규식 크기 상한은 2 MiB입니다. 비어 있거나, 잘못되었거나, 너무 큰
정규식은 오류입니다. grep의 선택적인 `glob` 필터는 상대 파일 경로에
일치합니다(`path`가 파일을 가리키면 기본 이름에). 숨김 파일도 포함되며, git 무시
규칙은 결과를 거르지 않습니다.
한도는 탐색 항목 10,000개, grep 파일당 8 MiB, 검색 전체 64 MiB,
JSON 결과당 512 KiB입니다. 검색 한도에 닿거나 너무 큰 파일을 건너뛰면
`truncated: true`가 됩니다. 계속하려면 경로/패턴을 좁히세요. 이와 별개로,
항목이 10,000개를 넘는 디렉터리를 하나라도 만나면 `list`, `glob`, `grep`은
잘린 검색 결과가 아니라 오류로 실패합니다. 목록은 결과
바이트 상한에 닿으면 일찍 페이지를 나눌 수 있습니다. 내장 도구 인자는 명령과 같은, 크기가 제한된 JSON Schema 검사를 씁니다.

`json` 프로토콜은 stdin으로 정확히 요청 하나를 받습니다.

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

범위 항목에는 정규화된 물리적 경로, `kind`(`file`, `folder`, `graph`),
논리적 자식 맵이 들어 있습니다. 파일 Artifact에서는 `context.artifactPath`와 그
범위 항목의 `path`가 프로세스 cwd가 아니라 파일을 가리킵니다. cwd는 그 파일이 들어 있는
폴더입니다. 파일 Artifact 참조는 `/path` 접미사를 거부합니다.
이 프로토콜 키는 camelCase로 남으며, 선언은 `execution_paths`와
`input_schema`를 씁니다. `executionPaths`는
비어 있으면 생략됩니다. 선언된 실행 경로는 각각 UTF-16 코드 단위 1024개 이하인
워크스페이스 기준 상대 파일/디렉터리 최대 64개(고유)이며, 심볼릭 링크나 복사 없이 해석됩니다. 파일 Artifact에서도
워크스페이스 기준 상대 경로로 남으며, 그 cwd나 대상 기준이 아닙니다.

Agent 리뷰가 시작되면 artifactize는 eval 도구의 각 실행 경로를 고정합니다.
파일은 그 바이트의 SHA-256으로, 디렉터리는 각
파일이나 심볼릭 링크의 상대 경로와 다이제스트에 대한 SHA-256으로 고정합니다(안의 심볼릭 링크는 대상 경로를
해시에 반영하되 따라가지는 않음). 디렉터리는 탐색만 합니다. 다이제스트에 반영되거나
항목 카운터를 늘리지 않으므로 빈 디렉터리는 세지 않습니다. 고정 값은
결과의 출처에
`executionPaths: {"TOOL": {"PATH": "sha256"}}`(`coverage_spec` 같은 등록된 도구 이름과
선언된 경로 기준)로 기록되고, 원격 기록과 함께 전달되며, 결과가 재사용될 때도
함께 남으므로 재사용된 판정에서 어떤 바이너리가 그것을 만들었는지 알 수 있습니다.
고정할 수 없는 경로(없음, 특수 파일, 파일 바이트 1 GiB 초과)는
Agent를 호출하기 전에 `PREPARATION_FAILED`로 리뷰를 실패시킵니다.
고정은 파일/심볼릭 링크 항목을 최대 10,000개까지 보존합니다. 그만큼 보존한 뒤에
파일 시스템 항목을 하나라도 더 만나면 준비가 실패합니다. 이는 파일 시스템 전체
탐색 한도가 아닙니다. 빈 디렉터리를 포함해 디렉터리 방문만으로는
카운터가 늘지 않습니다. 고정 값은 출처일 뿐
[재사용 키](../fingerprint.md#the-reuse-key)의 일부가 아닙니다. 바이너리가
바뀔 때 다시 리뷰하려면 그것을 fingerprint에 포함하세요.
성공한 stdout은 콘텐츠 블록 1–32개를 가진 JSON 객체 하나입니다:
`{"content":[{"type":"text","text":"..."},{"type":"json","data":{}}]}`.
이미지 블록은 [아래](#image-results)에서 설명합니다.
텍스트는 블록당 최대 64 KiB, 압축한 JSON 데이터는 블록당 최대 512 KiB,
정규화된 결과는 최대 8 MiB입니다. 두 프로세스 스트림은 각각 16 MiB로 제한됩니다.
결과 객체는 `content`와 선택적인 불리언 `isError`만 받습니다.
모든 블록도 닫혀 있습니다. 텍스트는 `type`과 `text`만, JSON은
`type`과 `data`만, 이미지는 `type`, `mimeType`, 그리고 `data`나 `path` 중
정확히 하나만 허용합니다. 결과나 블록에 필드가 더 있으면 리뷰어에게 전달되는 메타데이터가 아니라
잘못된 출력 오류입니다. 도구가 종료 코드 0으로 오류를 보고할 때는
`{"isError":true,"content":[{"type":"text","text":"Choose a smaller range."}]}`처럼
비어 있지 않은 텍스트 블록이 정확히 하나여야 합니다.
이런 오류는 리뷰어에게 그대로 전달됩니다. 0이 아닌 종료, 비정상 종료, 형식이 잘못되었거나 잘린
JSON, 프로세스 실패는 일반적인 오류가 되며, stderr는 결코 전달되지 않습니다.
도구로 관찰한 내용을 따로 증명하는 기록은 남기지 않습니다.

### 이미지 결과 {#image-results}

JSON 명령은 `mimeType`과, `data`(표준 base64 바이트)나 `path`(출력 파일) 중
**정확히 하나**를 가진 이미지 블록을 돌려줄 수 있습니다.

```json
{"content":[{"type":"image","mimeType":"image/png","data":"<base64-encoded PNG bytes>"}]}
```

```json
{"content":[{"type":"image","mimeType":"image/png","path":"screenshots/board.png"}]}
```

두 번째 형식의 파일은 요청의 `context.outputDir` 아래에 쓰세요. 상대
경로는 그곳에서 시작하고, 절대 경로는 여전히 그 디렉터리 아래일 때만 받아들입니다.
경로 탈출, 구성 요소 어디에든 있는 심볼릭 링크, 특수 파일, 그
출력 디렉터리 밖의 파일은 거부됩니다. Artifactize는 호출의 출력을
제거하기 전에 바이트를 읽어 내장하므로, 돌려받는 결과는 정리 뒤 더 이상 유효하지 않은 로컬 파일 경로가 아니라
자체적으로 완결된 base64입니다.

이미지는 비어 있지 않은 PNG, JPEG, WebP여야 하며, base64 디코딩 뒤
이미지 파일 바이트가 **4 MiB 이하**여야 합니다(디코딩된 픽셀 기준이 아님). 형식은 시그니처와 컨테이너 구조만 보고 판별하며, 이미지 전체가 유효한지는 검사하지 않습니다. 형식을 정하는 것은
파일 이름 확장자가 아니라 그 바이트이며, 각각
`image/png`, `image/jpeg`, `image/webp`와 일치해야 합니다. 잘못된 base64, MIME
불일치, GIF/BMP 같은 지원하지 않는 형식, 애니메이션 PNG는 거부됩니다.
정규화된 도구 결과 전체에 대한 8 MiB 제한은 base64 인코딩 뒤에도 그대로 적용됩니다.
[`view_image`](builtin.md#view_image)는 같은 내장 이미지 형식을 돌려주고
같은 바이트/형식 제한을 쓰지만, 파일은 eval 범위에서 읽습니다.

`plain`에서는 선언된 최상위 `input_schema.properties`만 argv
자리 표시자로 쓸 수 있습니다. 토큰 전체인 `{query}`와 끼워 넣은 `--query={query}` 모두 동작합니다.
문자열은 그대로 치환하고, 다른 JSON 값은 압축한 JSON을 씁니다. 값이 없거나
NUL 바이트가 있으면 프로세스를 시작하기 전에 실패합니다. `{{`와 `}}`는 리터럴 중괄호로 이스케이프합니다. 치환은
한 번만 합니다. 중괄호, 따옴표, 공백, `$()`, 세미콜론이 든 값도
리터럴 argv 요소 하나로 남으며, 결코 셸 코드가 되지 않습니다. plain 도구는 빈
stdin을 받습니다. 정리하고 손실 허용 방식으로 디코딩한 stdout이 텍스트 블록 하나가 되며, 최대 64 KiB이고
잘리면 명시적인 잘림 표시가 붙습니다. 0이 아닌 종료는 그 크기 제한된 stdout을
도구 오류로 표시합니다. stderr는 포함되지 않습니다. 일치하는 것이 없을 때 0이 아닌 값으로 종료하는
`rg` 같은 명령은, 그것을 성공한 빈 출력으로 쳐야 한다면 소유자가 래퍼를 만들어야 합니다.

명령 실행 파일은 파일 Artifact 예외와 범위 경로 탈출 거부를 포함해 공용
[프로그램 찾기](../others/platforms.md#program-lookup) 규칙을 따릅니다.
JSON 프로토콜 argv는 기존의 범위 지정 Artifact 참조를 쓸 수 있지만,
eval 범위 밖의 Artifact를 추가할 수는 없습니다. plain argv는 자기
스키마 속성 자리 표시자만 씁니다. 명령 자체에는 결코 값을 끼워 넣지 않습니다.
명령 호출은 소유자의 작업 디렉터리(파일 Artifact라면 들어 있는 폴더)를
cwd로, 작성자의 환경(
[runtime 환경](../declarations.md#runtime-execution-library): 실제 홈, 호출자의
`ARTIFACTIZE_*` 변수 없음)을 전용 외부 TMP/출력과 함께 쓰며, 기본 마감 시간은 120000 ms(1–2147483647)이고,
프로세스 그룹 단위로 취소/정리합니다. 호출별 디렉터리는 성공,
실패, 취소 때 제거되며 버려진 호출 future도 포함됩니다. 호출자가 소유한 출력
루트는 남습니다. 명령은 샌드박스가 아니라 신뢰하는 읽기 전용 프로그램입니다.

내부 호출자는 `tools::Registry::new(&config, "artifact/eval")`, `list()`,
`call(name, args, output_root, cancellation).await`를 씁니다. Agent eval에서만
쓸 수 있습니다. eval 범위의 각 Artifact는 자기 Agent 선언을
`<name>_<artifactId>`로 제공하며, 도구 이름과 세션 출처 이름을 이어 붙였을 때
충돌하면 거부됩니다. Human 도구는
결코 나열되지 않습니다. 나열은 디렉터리를 만들지 않고 소유자 코드도 실행하지 않습니다. 호출은
성공, 작성된 오류, 시스템 오류 결과에 대해 정규화된 `ToolResult`(`Success` 또는 `Error` 변형. `content()`와
`is_error()`로 읽음)를 돌려줍니다. 레지스트리 호출은 payload나 선언을 바꾸지 않습니다.
Agent 루프가 이 레지스트리를 호출합니다.

## 도구 세션 {#tool-sessions}

**도구 세션**은 리뷰 하나 동안 여러 호출에 걸쳐 하네스를 실행해 둡니다. 이는
저장된 Agent 대화가 아닙니다. 그 시작 명령과 중지 명령은 세션에 묶인 명령 도구와
같은 Artifact에 선언하세요.

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

`start`와 `stop`에는 `command`와 `args`가 필요하며, 유일한 선택 필드는
`execution_paths`입니다. 실행 파일 검색과 argv 범위 해석은 일반 명령과
같고, stdin으로 받는 JSON의 `args`는 `{}`입니다. `description`, `protocol`,
`timeout_ms`, 동시 실행 옵션은 받지 않습니다. 명령 도구의 `session`은
자기 Artifact 안의 세션을 가리켜야 하며, 내장 도구와 Human 도구는 이를 받지 않습니다.

하네스는 다음 시작 계약을 구현합니다.

- stdin에서 평소와 같은 JSON 객체를 읽습니다. `ARTIFACTIZE_SESSION_DIR`은 artifactize가
  제공하는 전용 디렉터리이며, 묶인 도구와 stop이 공유할 상태에 쓰세요.
- 준비되면 stdout에 줄바꿈으로 끝나는 줄 하나를 출력합니다. 그 내용은 무시됩니다.
- 세션 내내 시작 프로세스를 계속 실행해 두세요. 그 프로세스가 종료하면 세션이 끝납니다.

`json`이든 `plain`이든 묶인 도구와 stop은 같은
`ARTIFACTIZE_SESSION_DIR`을 받으며, 다른 모든 명령처럼 실제 홈을 포함한 작성자의 환경에서
실행됩니다.
호출마다 여전히 별도의 출력 디렉터리와 임시 디렉터리를 씁니다.
동시에 실행되는 리뷰는 같은 Artifact에서도 서로 독립적인 세션 디렉터리와 환경을
가집니다. 포트/디스플레이는 표준 OS 방식으로 고르세요
(예: 포트 0이나 자동으로 선택된 디스플레이). 데스크톱 하네스는
자기 헤드리스 디스플레이를 직접 시작합니다. Agent는 인스턴스 ID를 보지 못하며
start나 stop을 호출할 수 없습니다.

실행 경로 고정은 여전히 리뷰 시작 때 이루어집니다. 도구 세션은
Agent가 그 세션에 묶인 도구를 처음 호출할 때만 시작하며, 이후의 묶인 호출은 그 세션을 재사용합니다.
eval 범위에 들어 있지만 그 도구가 한 번도 호출되지 않은 세션은 프로세스를 시작하지 않고
세션 디렉터리도 만들지 않습니다. 시작과 준비 완료까지의 시간은 리뷰 시간 제한에 포함됩니다.
프로세스를 띄우지 못하거나 준비 완료 전에 종료하면 `PREPARATION_FAILED`가 아니라
그 도구 호출의 오류로 Agent에 돌아갑니다. 같은 세션에 대한 이후 호출은
다시 시작하지 않고 같은 시작 오류를 돌려줍니다. 준비 완료에는 따로 고정된 한도가 없습니다. start는
리뷰가 시간 초과되거나 취소될 때까지, 프로세스가 실행 중인 동안 준비할 수 있습니다.
준비 완료 뒤 stdout과 stderr는 읽어 버립니다.

리뷰 중에 시작 프로세스가 종료하면 artifactize는 남은
프로세스 그룹/Job을 즉시 죽이고 진행 중인 묶인 호출을 취소합니다. 프로세스 그룹의 종료를 확인한 뒤 관리 대상에서 해제하므로, 이후 리뷰를 정리할 때 예전 프로세스 그룹 ID로 신호를 보내지 않습니다. 저장된 프로세스 식별 정보를 해제하지 못하면 이를 보고하되,
묶인 호출의 취소/합류나 stop은 건너뛰지 않습니다. 이후의 묶인 호출은
``The tool session `app` ended.``를 돌려주며, 다시 시작하지 않습니다. 이는 저장된 대화에서
일반 도구 오류이며 새로운 이벤트 종류가 아닙니다.

시작된 모든 세션은 리뷰가 어떻게 끝나든 stop과 정리를 거칩니다.
판정, 오류, 시간 초과, 취소, 버려진 리뷰 future, 그리고 리뷰
패닉도 포함됩니다. 준비 완료 전에 종료한 start는 즉시 정리되고, 그렇지 않으면
Agent 루프 뒤, 입력 재검증, fingerprint
재확인, 결과 확정 전에 stop을 시도합니다. 정리는 stop을 실행하기 전에 진행 중인 묶인 호출을
취소하고 기다립니다. stop은 별도의 취소 토큰과 내부
30초 한도를 쓰며, 그 뒤 artifactize가 세션 그룹/Job을 강제 종료하고 회수합니다. 그
디렉터리와 실행 허가 기록은 stop 자체가 실패하거나 멈추더라도, 소유한 프로세스의 정리가
모두 확인된 뒤에만 제거됩니다. Windows 정리는 모든
Job 자손을 기다리며, 별도의 내부 30초 종료 한도를 가집니다.

정리는 리뷰를 요청한 프로그램에 의존하지 않습니다. 그 호출자가
비동기 런타임을 종료해도 artifactize의 백그라운드 실행기가 여전히 stop과
정리를 실행합니다. artifactize 프로세스 자체가 죽으면 stop을 실행할 수 없으므로,
다음 도구 세션이 먼저 정리합니다. 각 세션 디렉터리에는 그것을 소유한
프로세스와, 정리가 아직 진행 중이라는 표시인 **임대(lease)** 가 기록됩니다.
시작하기 전에 artifactize는 두 가지 디렉터리를 제거합니다. 하나는 관리하던 프로세스가
종료된 디렉터리(종료 후 아직 회수하지 않은 경우 포함)이고, 다른 하나는 정리에 실패한 뒤
임대가 해제된 디렉터리로, 이쪽은 관리하던 프로세스가 아직 실행 중이어도 제거합니다. 활성 임대가 있는 디렉터리는 실행 중인 리뷰가 쓰고 있으므로
그대로 둡니다.

Unix에서 artifactize는 각 start, stop, 묶인 호출의 프로세스 그룹을 실행 허가하기 전에
기록하고, 신호를 보내기 전에 저장된 프로세스 ID와 시작 시각을 확인하므로,
그룹의 첫 프로세스가 이미 종료했더라도 재사용된 프로세스 ID에 결코 신호를 보내지 않습니다.
Windows Job은 소유자가 죽으면 끝나며, 복구는 Windows가 열린 파일 핸들을
놓을 때까지 최대 30초 기다립니다.

정리 실패는 성공한 결과 대신 보고됩니다. 프로세스 정리가
확인되지 않으면 artifactize는 **저널**(어떤 프로세스를 시작했는지에 대한 기록)을 보관해
다음에 시작할 때 남은 정리를 마칠 수 있게 합니다. 제거하지 못한 디렉터리는
임대가 해제되면 다음 시작이 정리합니다. 그 정리에 실패하면
저널을 보관하고, 새 세션은 실행을 허가하지 않고 거부합니다. 재사용되었거나
기다린 결과는 아무것도 시작하지 않습니다.

start/stop 실행 경로 고정은 원격 기록을 포함해 결과의 기존 `executionPaths` 출처에
`<session>_<artifact>.start`와 `.stop` 키를 씁니다.
`tools check`는 참조된 세션의 실행 파일과 피연산자를 실행하지 않고 정적으로 확인합니다.
묶인 도구에 대한 `tools check --execute`와 `session send`는
각각 새 지연 세션을 따로 씁니다. 첫 묶인 호출 때 시작하고 끝나면 정리합니다.
묶인 도구를 호출하지 않는 후속 질문은 아무것도 시작하지 않습니다.

내부 호출자는
`registry.sessions(state, selected_tool_or_none, cancellation)`로 지연 세션을 등록하고,
`registry.with_sessions(&sessions)`로 붙인 다음, 호출을 마친 뒤 놓고
`sessions.close().await`를 기다립니다. 가드를 버려도 정리가 시작됩니다.
세션 명령은 여전히 **신뢰하는 코드로 실행되며, 샌드박스로 격리되지 않습니다**. 명령은
워크스페이스를 고치거나 자손을 일부러 분리해 정리를 피할 수 있습니다.

## 도구 진단 {#tool-diagnostics}

```sh
artifactize tools check                         # every Agent/Human eval's scope
artifactize tools check app/review              # same as --eval app/review
artifactize tools check --artifact app --audience agent
artifactize tools check --execute --artifact app --audience agent --tool read --args '{"path":"README.md"}'
artifactize tools check --execute --artifact app --audience human --tool inspect
artifactize tools check --execute --eval app/review --tool coverage_app --args '{}'
```

`tools check`는 선언을 탐색하고 검증하며, 실행 파일 사용 가능 여부,
범위 지정 argv 피연산자, 선언된 실행 경로를 해석하고, JSON으로 범위, 스키마,
도구별 준비 확인을 출력합니다. 기본으로 정적입니다. 소유자 프로세스, fingerprint
스크립트, 데이터베이스, 출력 디렉터리를 만들지 않습니다. 위치 인자 선택자나 `--eval`은
Agent/Human eval 하나를 고르며 `--artifact`나
`--audience`와 함께 쓸 수 없습니다. 명시적 실행에는 `--tool`과, eval 선택자 또는
`--artifact`와 `--audience` 둘 다가 필요합니다. `--artifact`에서는 짧은 작업 이름이나
게시된 이름을 받아들이며, 범위는 그 Artifact와 그 자식입니다.
eval에서는 그 eval이 게시하는 이름(`<tool>_<artifact>`)으로 도구를 지정하세요. 도구는
그 eval의 범위에서 실행되므로, 피연산자가 다른 Artifact를 가리키는 도구는
그 eval의 리뷰에서처럼 실행됩니다.
`--args`는 Agent 실행에만 쓸 수 있습니다. Human 스키마는 빈
객체만 받고 Human 명령은 자유 인자를 받지 않습니다. 확인은 결코 Run,
판정, 캐시 항목을 만들지 않습니다. Agent 호출은 전용 임시 출력(실행 후 제거)과 함께 작성자의 환경에서
실행되고, Human 호출은 리뷰어의 실제 환경을 씁니다. 종료 코드는
준비/성공이면 0, 선언/사전 확인/도구/정리 실패면 1,
잘못된 호출이면 2입니다. `--repo`, `--state-dir`, `--json`을 받으며, 보고서는
`--json`이 없어도 JSON입니다.
