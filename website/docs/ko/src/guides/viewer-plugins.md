# Artifact 유형과 뷰어 플러그인 {#artifact-types-and-viewer-plugins}

Artifact에 `type`을 선언하면 플러그인에서 제공하는 Agent 도구와 Human 도구를 다시 적지 않고 쓸 수 있습니다. 평가 대상은 그대로 `path`에 지정합니다. Aspect의 파일 범위도 여전히 `select`로 정합니다. `type`을 생략하면 **Custom Artifact**로 취급하며, 도구는 `[views.*]`에 직접 선언합니다.

```toml
# .artifactize/guide.artf
path = "guide.md"
type = "markdown"
```

Agent eval의 대상과 `{name}`으로 참조한 Artifact에는 각각 **Agent 도구**가 하나 이상 있어야 합니다. Human eval에는 같은 방식으로 **Human 도구**가 필요합니다. `basis = true`인 입력도 예외가 아닙니다. Runtime eval에는 도구가 없어도 됩니다. verify를 실행하기 전에 `artifactize config check`로 빠진 도구를 찾으세요. 기존 로컬 Artifact에 `type = "files"`만 추가해 문제를 해결할 수 있다면, `artifactize config migrate`로 사용자 지정 도구를 바꾸지 않고 유형을 추가할 수 있습니다.

유형을 지정한 Artifact에도 도구를 추가할 수 있습니다. 다만 같은 테이블 안에서 플러그인 도구나 세션과 같은 이름을 쓸 수 없습니다. Custom Artifact와 유형을 지정한 Artifact 모두 `type`과 도구 선언은 eval 정의 해시에 들어가지 않습니다. 도구 이름과 접근 범위는 [도구와 하네스 세션](tools.md)을 참고하세요.

## 내장 플러그인 {#built-in-plugins}

내장 플러그인은 별도로 설치할 필요가 없습니다. 데스크톱 앱에서 Artifact의 **Tools** 목록이나 해당 Artifact가 포함된 Human 리뷰에서 뷰어 도구를 실행하세요. 앱의 별도 **Artifact Viewer** 창이 열립니다. 같은 Artifact에 같은 뷰어 도구를 다시 실행하면 이미 열린 창이 앞으로 나옵니다. 사용자 지정 `launch`와 `output` 도구는 기존과 같이 프로그램을 실행하거나 결과를 **Tool log**에 출력합니다.

| 유형 | Agent 도구 | Human 도구 |
|---|---|---|
| `files` | `list`, `glob`, `grep`, `read` | **File Explorer**: Artifact의 파일을 찾아 문서를 읽습니다. |
| `markdown` | `read` | **Markdown**: 전용 뷰어에서 Markdown을 읽습니다. |
| `artifactize` | `declarations` | **Artifactize Viewer**: Artifact 노드와 참조 관계를 살펴봅니다. 노드를 클릭하면 그래프 오른쪽에 Artifact 패널이 겹쳐 열리고 해당 eval을 읽을 수 있습니다. |

`.artifactize/front-design/` 같은 폴더를 대상으로 `type = "artifactize"`를 지정하면 그 폴더에 있는 선언을 리뷰할 수 있습니다. Agent와 Human 리뷰어가 보는 범위는 같습니다. 뷰어는 eval의 기준과 참조하는 Artifact 등 프로젝트에 **선언한 내용**을 보여 줍니다. Run 결과를 보여 주는 화면은 아닙니다. 기존 내장 `graph` 도구는 여전히 프로젝트 전체를 다룹니다.

## 프로젝트에 플러그인 설치하기 {#install-a-plugin}

```sh
artifactize plugin add PATH
artifactize plugin list
artifactize plugin remove TYPE
```

`PATH`에는 `plugin.toml`과 플러그인에 필요한 파일을 담은 폴더를 지정합니다. 플러그인을 추가하면 해당 폴더가 `.artifactize/plugins/<type>/`에 복사됩니다. 다른 체크아웃에서도 선언이 유형을 찾을 수 있도록 이 폴더를 선언 파일과 함께 커밋하세요. `list`는 내장 유형과 프로젝트에 설치한 유형을 모두 보여 줍니다. 플러그인을 교체하려면 기존 플러그인을 먼저 제거하세요. 제거한 유형을 선언에서 계속 쓰면 플러그인을 다시 설치하거나 선언을 고칠 때까지 `config check`에서 오류가 납니다. 내장 유형은 제거하거나 교체할 수 없습니다.

신뢰할 수 있는 플러그인만 설치하세요. 뷰어는 아래 브리지를 통해 Artifact를 읽을 수 있습니다. 플러그인에서 선언한 Agent·Human 명령 도구는 사용자 환경에서 프로그램을 실행할 수 있습니다. 설치 명령은 [MCP](../reference/cli-and-mcp.md#mcp-stdio-server)에서도 쓸 수 있습니다.

## 플러그인 매니페스트 작성하기 {#write-a-plugin-manifest}

[HTML 예제](https://github.com/lhj6102/artifactize/tree/main/examples/plugins/html)는 그대로 설치할 수 있는 플러그인입니다. Agent는 HTML 소스를 읽고, 사람은 외부 파일 없이 동작하는 HTML 페이지를 뷰어에서 엽니다. 설치한 뒤 `.artf` 파일에 `type = "html"`을 지정하세요.

```toml
# plugin.toml
type = "html"
title = "HTML Viewer"

[viewer]
entry = "viewer.html"

[views.agent_tools]
inspect = { builtin = "read", description = "Inspect the HTML source of {artifactName}." }

[views.human_tools]
view = { kind = "viewer", description = "Open {artifactName} in the HTML Viewer." }
```

`type`에는 Artifact에서 선언할 고유 이름을 적습니다. `title`은 뷰어 창에 표시할 플러그인 이름입니다. `[views.agent_sessions]`를 포함한 세 도구 테이블은 [Artifact 선언](../reference/declarations.md#declaration-validation)과 형식이 같습니다. 명령과 도구 경로는 플러그인 패키지가 아닌 Artifact를 기준으로 해석합니다. 뷰어를 여는 Human 도구가 없다면 `[viewer]`를 생략하세요. 뷰어를 제공하려면 `kind = "viewer"`인 Human 도구를 하나 이상 선언하고, `viewer.entry`에 패키지 안에서 외부 파일 없이 동작하는 HTML 파일을 지정하세요. 데스크톱 앱이 이 파일을 직접 띄웁니다. 웹 서버를 실행하거나 브라우저를 열지 않습니다.

## 뷰어 브리지 {#viewer-bridge}

플러그인 페이지는 샌드박스에서 실행됩니다. `artifactize-viewer/v1` 브리지만 사용할 수 있으며 데스크톱 앱의 명령은 직접 호출할 수 없습니다. 페이지가 시작할 때 `context`를 요청하세요. 첫 브리지 요청을 받기 전까지 뷰어의 도구 막대에 **Loading {plugin title}…**가 표시됩니다. 페이지를 다 불러왔는데도 요청이 없으면, 요청이 도착할 때까지 플러그인이 연결되지 않았다고 표시합니다.

`parent.postMessage`로 호스트에 호출을 전달하세요.

```js
parent.postMessage({
  channel: 'artifactize-viewer/v1',
  id: 1,
  method: 'context',
  args: {},
}, '*');
```

응답을 아직 받지 못한 호출끼리는 서로 다른 `id`를 쓰세요. `parent`가 보내는 응답에는 요청과 같은 `channel`과 `id`가 들어 있습니다. 성공하면 `result`, 실패하면 `error` 문자열이 옵니다. 메시지를 처리하기 전에 출처와 채널을 확인하세요. 호출에 응답을 연결하는 코드는 HTML 예제에서 볼 수 있습니다.

| `method` | `args` | 결과 |
|---|---|---|
| `context` | `{}` | `{artifact, type, title, target, targetKind, review}`. `target`은 선언한 대상입니다. `review`에는 담당 중인 활성 리뷰의 ID가 들어갑니다. 해당하는 리뷰가 없으면 `null`입니다. |
| `files` | `{path}` | 해당 폴더 바로 아래의 항목을 `{name, path, kind}` 형식으로 반환합니다. `kind`는 `file`이나 `directory`입니다. |
| `readText` | `{path}` | 파일의 UTF-8 텍스트를 반환합니다. 파일 하나를 대상으로 하는 Artifact에서는 `''`를 넘겨 대상 파일을 읽습니다. |
| `status` | `{text}` | `null`을 반환합니다. 뷰어의 도구 막대에 일반 텍스트 한 줄을 표시합니다. 새 메시지가 앞선 메시지를 대신하며, 창을 닫으면 사라집니다. |
| `pointing` | `{text}` | 텍스트를 추가한 **Notes**를 반환합니다. 이 리뷰어가 담당 중인 활성 리뷰가 없으면 호출을 거부합니다. |

브리지 호출의 `path`는 Artifact 기준 상대 경로입니다. 구분자는 `/`이며, 루트는 `''`로 지정합니다. 파일 Artifact와 같은 폴더에 있는 다른 파일, Aspect 범위 밖의 경로, 별도 폴더 `part`에 속한 파일은 읽을 수 없습니다. 심볼릭 링크도 따라가지 않습니다. `status`로 페이지에 표시하는 내용이나 불러오지 못한 이유를 알리세요. HTML 예제는 Artifact 페이지를 다 불러온 뒤 `checkout/index.html · Checkout`처럼 파일 경로와 문서 제목을 알립니다.

호스트는 다음 이벤트도 보냅니다.

- `{channel: 'artifactize-viewer/v1', event: 'activeReviewChanged'}`: 이전 리뷰에서 위치를 고르는 중이었다면 취소하세요. 페이지에 리뷰 정보를 표시한다면 `context`를 다시 읽으세요.
- `{channel: 'artifactize-viewer/v1', event: 'point', armed: true}`: 사용자가 페이지에서 위치를 고르면 해당 위치를 `pointing`으로 한 번 보냅니다. 호출하면 Point가 끝납니다. `armed`가 `false`라면 `pointing`을 호출하지 않고 선택을 중단하세요. 응답과 마찬가지로 출처와 채널을 확인하세요.

## Point로 활성 리뷰에 위치 남기기 {#point-into-the-active-review}

맡은 Human 리뷰가 활성 상태라면 **Point**를 클릭하고 Artifact에서 위치를 고르세요. 뷰어는 도구 막대에 활성 리뷰의 eval을 표시합니다. 위치 선택을 지원하는 뷰어는 같은 막대에 **Point**도 표시합니다. 위치는 File Explorer에서 `notes.md L42:C9`, Markdown에서 `L42:C9`가 됩니다. Artifactize Viewer에서는 Artifact 이름이나 `front-design/matches-layout` 같은 eval ID, HTML 예제에서는 `#checkout .total` 같은 CSS 선택자를 사용합니다. **Point**를 다시 클릭하거나 Esc를 누르면 아무 내용도 추가하지 않고 취소합니다. Artifactize는 기존 **Notes**를 그대로 두고 새 줄에 위치를 추가합니다. 판정을 선택하거나 리뷰를 제출하지는 않습니다. 리뷰 ID는 플러그인이 고르지 않고, 앱의 활성 리뷰를 사용합니다.
