# 데스크톱 앱과 TUI {#desktop-app-and-tui}

데스크톱 앱(`artifactize-desktop`)과 터미널 인터페이스(`artifactize monitor`, `artifactize review`)는 CLI와 같은 로컬 상태를 공유하며, Run이 진행되거나 팀원의 결과가 도착하면 화면에 실시간으로 반영합니다.

## 데스크톱 앱 (`artifactize-desktop`) {#open-the-app}

```sh
artifactize-desktop                         # 모든 프로젝트 목록으로 앱 열기
artifactize-desktop monitor --repo PATH     # PATH의 브랜치를 펼쳐 가장 최근 항목 표시
artifactize-desktop review [REQUEST_ID]     # 가장 최근 대기 중인 Human 리뷰(또는 REQUEST_ID) 열기
artifactize-desktop graph --repo PATH       # PATH 브랜치의 Current 선언 그래프 열기
```

창 전체 단축키 (macOS에서는 `Ctrl` 대신 `Command` 사용):

| 키 | 동작 |
|---|---|
| `Ctrl+K`, `F1` | 검색 가능한 **Commands** 팔레트 열기 |
| `?` | 현재 초점 영역의 **Help** 및 그래프 기호 안내 |
| `Ctrl+,` | **Settings** 모달 열기/닫기 (**Providers**, **Remote**, **Limits**) |
| `Ctrl+B` | 왼쪽 **Pane** 숨기기/보이기 |
| `/` | Pane의 브랜치·대기 리뷰 검색 칸으로 이동 |
| `Ctrl+F` | 그래프가 보이고 초점이 Pane 밖에 있으면 그래프 안 **Find** 열기, 그 외에는 Pane 검색 칸으로 이동 |
| `Ctrl+O` | **Open a request by ID** (대기 중인 Human 리뷰는 바로 처리 가능, 나머지는 읽기 전용) |
| `Ctrl+R`, `F5` | 상태 다시 읽기 및 브랜치/Current 재스캔 (Human review 화면에서는 `Ctrl+R`이 RED 선택이므로 `F5` 사용) |
| `q` | `q`를 누르면 앱을 종료합니다. **Settings**가 열려 있으면 먼저 **Settings**를 닫으세요. 텍스트 입력 칸에 초점이 있으면 입력 칸 밖으로 초점을 옮기세요. **Human review**에서 양식을 편집하거나 작업을 실행 중이면 먼저 `Esc`를 누르세요. 다른 영역에서 종료할 때도 이 창에서 맡은 요청 중 아직 판정을 제출하지 않은 것이 있으면 **Stay** 또는 **Release and quit**(`u`)을 고릅니다 |

### 왼쪽 Pane: 검색, 브랜치, 대기 중인 리뷰 {#pane}

- **검색 (`/`)**: 일반 단어, `repo:`, `branch:` 조건은 브랜치와 대기 중인 리뷰를 모두 필터링합니다. `sort:recent`(기본값)와 `sort:name`은 브랜치 목록 정렬 순서를 바꾸며, 대기 중인 리뷰는 항상 최신순으로 나열됩니다.
- **브랜치**: 이 머신에 Run 기록이 있는 활성 Git 워크트리(`프로젝트 / 브랜치`)나 일반 폴더, 그리고 `--repo`나 `graph --repo`로 지정한 프로젝트(첫 Run 실행 전이라도 표시)를 나열합니다. 별표를 눌러 즐겨찾기한 브랜치는 맨 위에 고정됩니다. 브랜치 이름을 클릭하면 타임라인이 펼쳐지며 가장 최근 항목이 열립니다. 타임라인 첫 항목은 **Current**이고 그 아래에 저장된 Run이 최신순으로 나옵니다. 진행 중인 Run이 있으면 타임라인에서 **Current** 대신 그 Run을 표시합니다. **Current**나 저장된 Run을 클릭해 Main area에 띄우고, 타임라인 맨 아래의 **Older runs**를 클릭해 이전 Run 항목을 더 불러옵니다.
- **대기 중인 리뷰(Waiting reviews)**: 모든 저장소의 대기 중인 Human 승인 요청이 Pane 아래쪽에 쌓입니다(`Waiting (N)`). 항목을 클릭하면 Main area에 **Human review** 화면이 열립니다.

### Main area: Run, Current, 그래프, Detail {#current}

- **Run 화면**: 상단 헤더 행에 Run 요약을 보여 주고 아래에 Artifact Graph를 그립니다. 헤더의 요약을 클릭하거나 팔레트에서 **Open the Run's Detail**을 실행하면 Run의 Detail(**Outcome**, **Waits for**, **What**, **Provenance**, **Technical**)이 열립니다. Detail에 초점을 옮긴 뒤 `t`를 누르면 Technical이 펼쳐지거나 접힙니다.
- **Current 화면과 선언 편집**: `artifactize status`처럼 현재 워크트리를 스캔해 지금 `verify`를 실행하면 어떻게 될지(`reused`, `would run`, `waiting`, `blocked`) 그래프로 보여 줍니다.
  - **선언 추가 및 편집**: 도구 모음의 **New Artifact**를 클릭하고 **Name**과 **Path**를 입력한 뒤 `Enter`를 누르면 `.artifactize/<name>.artf` 파일이 만들어집니다. Artifact Detail 탭의 **Definition**에서는 `Path`를 수정한 뒤 `Enter`를 눌러 반영합니다. `Comment`를 누르면 그래프 노드 위의 코멘트 편집기가 열리며, 대규모 그래프를 축소해 둔 상태에서는 노드별 조작이 나타날 때까지 확대하거나 **Source**에서 `[annotations.comment]`를 편집하세요. `Parts`에서는 `+ Add`를 누르고 이름을 입력해 후보를 좁힙니다. 이어서 방향키로 원하는 Artifact를 고르고 `Enter`를 누르거나(`Enter`는 강조된 후보를 선택) 후보를 직접 클릭해 추가하며, 제거할 때는 `✕`를 누릅니다. **Rename**이나 **Delete**로 Artifact 이름을 바꾸거나 삭제할 수 있으며, **Source**에서는 `.artf` 파일 전문을 편집합니다(`Ctrl+S`로 저장, **Revert**로 되돌리기).
  - **노드 배치, 부분 구성, 코멘트**: 노드나 상위 상자의 **제목 줄**을 끌어 놓으면 `[annotations.style]`에 위치가 저장됩니다(상자 안쪽 빈 곳이나 배경을 끌면 화면 이동). 노드를 원래 속한 상자 안의 빈 곳에 놓으면 위치만 바뀝니다. 노드를 다른 상위 상자 안의 빈 곳에 놓으면 그 상자가 새 상위 Artifact가 되어 놓은 노드가 `parts`에 들어갑니다. 같은 상자 안에서 한 노드를 다른 노드 위에 놓으면, 옮긴 노드가 대상 노드의 `parts`에 추가됩니다. 모든 상자 바깥의 빈 그래프 영역에 놓으면 상위 Artifact의 `parts`에서 빠집니다. 노드 아래 코멘트를 클릭해 `[annotations.comment]`를 수정하거나(`Enter` 저장, `Shift+Enter` 줄바꿈), 코멘트가 없는 다른 Artifact로 끌어 옮길 수 있습니다.
- **그래프 탐색**: 노드를 클릭하거나 `Tab`+`Enter`를 누르면 오른쪽에 **Detail**이 열리고, `m`을 누르면 상위 상자를 접거나(**Minimize**) 펼칩니다(**Expand**). `Ctrl+F`(**Find**)로 Artifact, eval, 별표 즐겨찾기를 찾고, 그래프 일부가 화면 밖에 있을 때는 오른쪽 아래 **Navigator** 미니맵으로 이동합니다. 노드가 많은 대규모 그래프에서 글자가 보이지 않을 만큼 축소하면 노드를 상태별로 색이 다른 사각형으로 표시하며, 노드를 클릭하면 **Detail**이 열립니다. 이름이 읽힐 만큼 확대하고 화면 안의 노드 수가 충분히 적어지면 노드별 조작 기능(`Tab`+`Enter`, **Minimize**/**Expand**, 코멘트 편집)을 쓸 수 있습니다. 노드별 조작이 꺼져 있을 때도 노드를 클릭하면 **Detail**이 열립니다. 대규모 그래프에서는 확대해도 노드를 끌어 배치하는 기능은 비활성화되므로 위치는 **Source**의 `[annotations.style]`에서, 부분 소속은 **Definition → Parts**에서 편집하세요.
- **Detail 탭 구성**:
  - **Artifact 탭**: **Evals**, **Neighbours**(왼쪽에 입력, 오른쪽에 이 Artifact를 입력으로 쓰는 항목), 선언된 Human **Tools**(워크트리에서 바로 실행하고 실시간 **Tool log** 확인), **Path**(Current에서는 **Definition**과 **Source**), **Debug**(Detail에 초점을 두고 `t`를 눌러 전환)를 보여 줍니다. 제목 옆 별표를 누르면 즐겨찾기에 등록됩니다.
  - **eval 탭**: eval 행이나 칩(`[아이콘] {name} / [eval]`)을 클릭하면 해당 eval 탭이 열립니다. Run에 기록된 요청이 있는 Human eval이면 탭에 **Human review** 화면이 바로 열립니다. 다른 eval은 기본으로 eval 요약을 보여 줍니다. Run에 기록된 요청이 있으면 **Summary** 전환 버튼과 함께 **Agent log**(로컬 대화가 있으면 실시간 대화 표시) 또는 **Runtime log**(저장된 `stdout`/`stderr` 또는 출력이 없는 이유 표시) 탭도 나타납니다.
- **Human review 화면**: **Claim to review**(`c`)로 요청을 맡은 뒤 선언된 **Tools**를 실행하고 **Tool log**를 확인합니다. 이어서 **GREEN**(`g` / `Ctrl+G`) 또는 **RED**(`r` / `Ctrl+R`)를 골라 양식을 채우고 **Submit**(`Ctrl+S`)으로 제출합니다. 제출하지 않고 담당을 해제하려면 **Release claim**을 클릭하거나 입력 칸 밖에서 `u`를 누르세요.
- **Settings (`Ctrl+,`)**: **Providers**(`codex`·`claude-agent` 로그인/로그아웃, `openai`·`anthropic`·`gemini` API 키 저장/삭제, 별칭·엔드포인트 사용자 지정 프로바이더 관리, 모델 목록 조회), **Remote**(팀 리뷰 저장소 로그인/로그아웃, 상태 확인, 푸시), **Limits**(`signoffIdleGraceSeconds`)를 설정합니다.

## 터미널 인터페이스 (`artifactize monitor`와 `artifactize review`) {#monitor}

```sh
artifactize monitor [PATH | --repo PATH | --all]   # ALL 범위로 시작하거나 PATH 범위 선택
artifactize review [REQUEST_ID] [--repo PATH]      # REQUEST_ID를 바로 열거나 요청 목록 검색
```

- **Monitor**: 경로 없이 실행하거나 `--all`을 주면 `ALL` 범위에서 시작하고, `PATH`나 `--repo PATH`를 주면 해당 저장소·워크트리를 먼저 선택합니다. **Scope → Runs → Run tree → Detail** 순서로 이동할 수 있습니다. 현재 단계는 초점이 있는 **Full** 패널에 표시하고, 이전 단계가 있으면 왼쪽 **Compact** 패널에, 다음 단계가 있으면 오른쪽 **Preview** 패널에 함께 표시합니다(**Scope**에서는 **Compact**를, **Detail**에서는 **Preview**를 표시하지 않습니다).
  - 조작 키: 탐색 패널에서는 `↑`/`↓` 또는 `j`/`k`로 행을 선택하고 초점이 있는 읽기 전용 Detail 패널 안에서는 내용을 스크롤합니다. 로컬 대화가 있는 Agent 요청은 먼저 실시간·저장된 **Agent 대화**를 열고 `d`로 대화와 저장된 결과의 상세 화면 사이를 오갑니다. Agent 대화가 없을 때 읽기 전용 Detail은 **Sections**에 초점을 두고 시작합니다. **Sections** 옆이나 아래에 출력 패널이 있으면 `Tab`이나 `Shift+Tab`으로 **Sections**와 출력 패널 사이에서 초점을 옮겨 저장된 Runtime 출력을 스크롤할 수 있습니다. `Enter`/`→`로 다음 단계에 들어가고(트리에서는 전체 Detail 열기), `Esc`/`←`로 이전 단계로 돌아갑니다. `d`로 연 Agent 결과 상세 화면에서 `Esc`를 누르면 Agent 대화로 돌아가고, `←`를 누르면 Detail을 닫고 **Run tree**로 돌아갑니다. Human 판정 양식을 편집하거나 Human 도구를 실행하는 동안에는 `←`와 `q`가 작동하지 않으므로 먼저 `Esc`를 누르세요. `Space`(`h`/`l`)로 트리 행을 접거나 펼칩니다. `!`로 다음 `ERROR`·`RED`·대기 중 Human eval로 이동하고, `b`로 선택 행의 상위 의존성을 순회합니다(`Backspace`로 복귀). 읽기 전용 Detail에서는 `t`로 Technical을 전환하고(Agent 대화에서는 먼저 `d`를 눌러 결과 상세를 연 뒤 `t` 입력), `F2`로 마우스 캡처를 전환하며, `?`로 도움말을 열고, 편집이나 Human 작업 중이 아닐 때 `q`로 종료합니다.
- **Review 및 공통 Human Detail**: `artifactize review`는 `kind:human status:waiting_human` 검색으로 요청 목록을 엽니다. `--repo PATH`를 넘기면 `repo:` 조건을 추가하고, `REQUEST_ID`를 넘기면 Detail을 바로 엽니다. 목록에서는 `/`를 눌러 검색어를 편집합니다(`repo:`, `eval:`, `request:`, `reviewer:`, `tag:`, `kind:`, `status:`, `is:mine|claimed|unclaimed`; `Tab` 자동 완성, `Enter` 확정, `Esc` 취소). `↑`/`↓` 또는 `j`/`k`로 요청을 고른 뒤 `Enter`나 `→`로 Detail을 열며, 목록에서 `q`를 누르면 종료합니다. 이 세션에서 맡았지만 아직 판정을 제출하지 않은 요청이 있을 때 `q`를 누르면 종료 방식을 묻습니다. `k`는 담당을 유지한 채 종료하고, `u`는 담당을 해제한 뒤 종료하며, `Esc`는 종료를 취소합니다. Human Detail(`review`와 `monitor` 공통) 안에서는 다음과 같이 조작합니다.
  - **담당 지정과 도구 실행**: `c`를 눌러 담당자로 지정합니다. 양식 편집 전에는 `Tab`으로 Tools와 Fields 사이를 오가며(양식 편집 중에는 `Tab`이 입력 칸 사이를 이동하므로 `Shift+Tab`으로 Tools로 이동), Tools에서 `↑`/`↓`로 도구를 고르고 `Enter`로 실행합니다.
  - **양식 편집과 제출**: `g`나 `r`(`Ctrl+G`/`Ctrl+R`)로 GREEN/RED 양식을 열고 `Ctrl+S`로 제출합니다. `u`(편집 중에는 `Ctrl+U`)로 담당 지정을 해제합니다. 양식을 편집하지 않을 때는 `i`로 지시문을 펼치고, `t`로 Technical을 펼치거나 접습니다. 텍스트 입력 칸에서 이 키를 누르면 글자가 입력됩니다.
  - **화면에서 나가기**: `Esc`를 누르면 실행 중인 작업 취소 → 양식 편집 종료 → Detail에서 요청 목록(또는 트리)으로 복귀 순서로 한 단계씩 돌아갑니다.
