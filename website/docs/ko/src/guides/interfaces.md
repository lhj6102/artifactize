# 데스크톱 앱 {#desktop-app}

데스크톱 앱(`artifactize-desktop`)과 CLI는 같은 상태 디렉터리를 사용합니다. 앱은 Run의 진행 상황, 워크트리 추가·삭제, 팀원이 공유한 결과를 화면에 바로 반영합니다. `artifactize monitor`와 `artifactize review`는 데스크톱 앱을 여는 CLI 단축 명령입니다.

## 앱 열기 {#open-the-app}

```sh
artifactize-desktop                                       # 모든 프로젝트 목록으로 열기
artifactize-desktop monitor --repo PATH                   # PATH의 브랜치를 펼쳐 가장 최근 항목 표시
artifactize-desktop review [REQUEST_ID] [--reviewer NAME] # 가장 최근 대기 중인 Human 리뷰(또는 REQUEST_ID) 열기
artifactize-desktop graph --repo PATH                     # PATH의 Current 선언 그래프 열기
artifactize monitor [PATH | --repo PATH | --all]          # 데스크톱 앱을 여는 CLI 단축 명령
artifactize review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]  # Review 화면을 여는 CLI 단축 명령
```

`artifactize-desktop`은 `--state-dir PATH`, `--all`, `--repo PATH`(`--all`과 함께 쓸 수 없음), `--reviewer NAME` 옵션도 받습니다.

어느 화면에서든 상단에서 **Settings**를 열거나 명령을 검색할 수 있습니다.

- **Pane** 스위치: 메인 영역은 그대로 두고 왼쪽 Pane을 숨기거나 다시 표시합니다.
- **Commands** 버튼: 현재 화면에서 실행할 수 있는 명령을 검색하는 명령 팔레트를 엽니다. 공통 명령으로 **Keyboard shortcuts**, **Show Settings**, **Show or hide the Pane**, **Search the branches**, **Open a request by ID**, **Refresh**, **Quit**을 나열합니다. 현재 열어 둔 Artifact Graph·Human review·Settings 화면에 맞는 전용 명령도 함께 표시합니다.
- **Settings** 버튼: **Settings** 모달(**Providers**, **Remote**, **Limits**)을 엽니다.
- **업데이트 알림** (`{version} available`): 새 정식 릴리스가 나오면 **Settings** 앞에 표시됩니다. 프리릴리스 버전은 무시하며, Microsoft Store 설치 버전에는 표시하지 않습니다. 클릭하면 설치 방식에 맞는 업데이트 안내 창이 열리며, 앱이 직접 파일을 다운로드하거나 설치하지는 않습니다.

마우스 대신 키보드 단축키를 쓸 수도 있습니다. 지원하는 단축키는 **Commands**의 각 명령 옆과 **Help**(**Commands → Keyboard shortcuts**, 그래프 범례 포함)에서 확인할 수 있습니다. 요청을 맡았지만 아직 판정을 제출하지 않은 경우가 있습니다. 그 상태에서 창을 닫으려 하거나 **Quit**을 선택하면 종료 확인 창이 뜹니다. **Stay**를 누르면 창을 닫지 않고 계속 리뷰할 수 있습니다. **Release and quit**을 누르면 요청의 담당 지정을 해제하고 앱을 종료합니다.

## 왼쪽 Pane: 검색, 브랜치, 대기 중인 리뷰 {#pane}

- **검색**: Pane 상단의 검색 입력란을 클릭하거나 **Commands**에서 **Search the branches**를 실행합니다. 일반 검색어와 `repo:`, `branch:` 조건에 맞는 브랜치와 대기 중인 리뷰만 보여 줍니다. 브랜치 목록은 `sort:recent`(기본값)나 `sort:name`으로 정렬하며, 대기 중인 리뷰는 항상 최신순으로 표시합니다. 검색어를 입력하면 자동 완성 목록이 뜹니다. 목록에서 검색 조건에 쓸 키나 값을 클릭해 넣을 수 있습니다.
- **브랜치**: 이 컴퓨터에 Run 기록이 있는 활성 Git 워크트리(`project / branch`)와 Git을 쓰지 않는 프로젝트 폴더를 나열합니다. `--repo`나 `graph --repo`로 지정해 연 프로젝트는 첫 Run 전이라도 목록에 표시됩니다. 브랜치 옆 별표를 클릭하면 목록 맨 위에 고정됩니다. 브랜치 이름을 클릭하면 타임라인이 펼쳐지고 가장 최근 항목이 열립니다. 왼쪽 화살표를 클릭하면 메인 영역을 바꾸지 않고 타임라인만 접거나 펼칩니다. 타임라인 첫 줄은 **Current**이며 진행 중인 Run이 있으면 그 Run으로 바뀝니다. 아래에는 저장된 Run이 최신순으로 이어집니다. **Current**나 저장된 Run을 클릭해 메인 영역에 띄우고, 타임라인 맨 아래의 **Older runs**를 클릭해 이전 Run을 더 불러옵니다.
- **대기 중인 리뷰**: 모든 저장소의 대기 중인 Human 승인 요청을 Pane 하단에 모아 보여 줍니다(비어 있으면 `(0)` 표시). 항목을 클릭하면 메인 영역에 **Human review** 화면이 열립니다.
- **크기 조절**: Pane과 메인 영역 사이의 경계선이나 Detail 패널 왼쪽 가장자리를 드래그해 너비를 조절합니다. 바꾼 너비는 화면을 전환하거나 앱을 다시 실행해도 유지됩니다.

## 메인 영역: Run, Current, Artifact Graph, Detail {#current}

- **Run 화면**: Run ID와 상태 요약은 Artifact Graph의 도구 막대 위에 표시합니다. 상태 요약을 클릭하거나 **Commands**에서 **Open the Run's Detail**을 실행하면 **Outcome**, **Waits for**, **What**, **Provenance**, **Technical** 항목을 담은 Run Detail이 열립니다.
- **Current 화면과 편집**: `artifactize status`처럼 워크트리를 스캔하면서 확인을 마친 Artifact부터 예상 동작(`reused`, `would run`, `waiting`, `blocked`)을 색으로 표시합니다. 헤더 오른쪽 끝에는 **Refresh** 버튼이 있습니다.
  - **선언 생성과 편집**: 도구 막대에서 **New Artifact**를 클릭하고 **Name**과 **Path**를 입력한 뒤 **Create**를 누르면 `.artifactize/<name>.artf` 파일이 만들어집니다. Artifact의 **Detail** 패널에서는 **Definition**의 세 항목(`Path`, `Parts`, `Comment`)을 수정할 수 있습니다.
    - `Path`: 새 경로를 입력하고 `Enter`를 눌러 저장합니다.
    - `Parts`: `+ Add`를 클릭해 제안 목록에서 고르거나 `Enter`로 추가하고, `✕`를 클릭해 제거합니다.
    - `Comment`: 노드 아래에 인라인 메모 입력란을 엽니다.
    이름 변경과 삭제는 **Rename**과 **Delete** 버튼으로 실행합니다. **Source**에서는 `.artf` 파일 전체를 직접 고치고 **Save**나 **Revert** 버튼으로 저장·복원합니다. 앱에서 파일을 연 뒤 다른 곳에서 파일이 바뀌면, 앱은 변경 사항을 덮어쓰지 않도록 저장을 막고 **Current** 화면을 갱신합니다.
  - **노드, `parts`, 메모 배치**: 노드, 접힌 상위 Artifact, 또는 상위 Artifact의 **제목 줄**을 드래그해 `[annotations.style]`에 위치를 저장합니다. 노드를 드래그해도 화면은 따라 움직이지 않으며, 상위 Artifact의 내부 빈 공간이나 배경을 드래그하면 화면을 옮길 수 있습니다. 노드를 원래 속한 상위 Artifact 안의 빈 공간에 놓으면 위치만 바뀝니다. 다른 상위 Artifact 안이나 다른 노드 위에 놓으면 대상 Artifact의 `parts`에 노드를 추가하고 기존 상위 Artifact의 `parts`에서는 뺍니다. 노드를 어느 박스에도 속하지 않는 빈 공간에 놓으면 상위 Artifact의 `parts`에서 빠집니다. 박스 제목 줄이나 도구 막대의 **Reset layout**을 클릭하면 자동 배치로 되돌아갑니다. 노드 아래 메모를 클릭해 `[annotations.comment]`를 수정합니다(`Enter` 저장, `Shift+Enter` 줄 바꿈). 메모를 다른 Artifact 위로 드래그하면 옮길 수 있습니다. 단, 대상에 기존 메모가 없을 때만 그 Artifact의 선언 파일에 저장합니다.
- **Artifact Graph 탐색**: Artifact Graph 위 도구 막대에서 **Find**(Artifact, eval, 즐겨찾기로 이동), **Zoom out**, **Zoom in**, **Fit View**를 쓸 수 있습니다. 배경 드래그로 이동하고 스크롤로 확대·축소합니다. 노드를 클릭하면 오른쪽에 **Detail** 패널이 열리고, 상위 Artifact의 **Minimize**/**Expand** 버튼(또는 **Commands**의 **Minimize or expand the whole**)으로 박스를 접거나 펼칩니다. Artifact Graph 일부가 화면 밖에 있으면 오른쪽 아래에 **Navigator** 미니맵이 나타납니다. 큰 Artifact Graph를 축소해 이름을 읽기 어려워지면 노드를 상태별 색상으로 구분한 블록으로 표시합니다. 블록을 클릭해도 **Detail** 패널을 열 수 있습니다.
- **Detail 패널**:
  - **Artifact 탭**: 접고 펼 수 있는 **Evals**, **Neighbours**(왼쪽에 입력, 오른쪽에 사용처), 선언된 Human **Tools**(워크트리에서 바로 실행하며 실시간 **Tool log** 표시), **Path**(Current에서는 **Definition**과 **Source**), **Debug** 섹션을 보여 줍니다. 제목 옆 별표를 클릭하면 해당 프로젝트의 모든 브랜치에서 즐겨찾기로 표시됩니다.
  - **Eval 탭**: eval 행이나 칩(`[icon] {name} / [eval]`)을 클릭해 엽니다. Run에 기록된 Human eval 항목을 클릭하면 탭에서 **Human review** 화면이 바로 열립니다. 다른 eval을 열면 **Summary**가 먼저 표시되며, 로그가 있으면 일반 텍스트 전환 버튼으로 **Agent log**(실시간·저장 대화)나 **Runtime log**(저장된 `stdout`/`stderr`)를 볼 수 있습니다.
- **Human review 화면**: **Claim to review**를 클릭해 요청을 맡은 뒤 선언된 **Tools**를 실행하고 실시간 **Tool log**를 확인합니다. 이어서 **GREEN**이나 **RED**를 클릭하고 판정 양식에 필요한 내용을 입력한 뒤 **Submit**을 클릭합니다. 판정을 제출하지 않고 담당 지정을 해제하려면 **Release claim**을 클릭하세요.
- **Settings**: 헤더의 **Settings**를 열어 세 탭에서 각각 설정을 확인하거나 바꿀 수 있습니다. **Providers**에서는 `codex`나 `claude-agent`에 로그인·로그아웃할 수 있고, `openai`·`anthropic`·`gemini` API 키를 저장하거나 삭제할 수 있습니다. 별칭이나 엔드포인트를 지정해 사용자 지정 프로바이더를 추가하고, **List models**를 클릭해 모델 목록을 확인할 수도 있습니다. **Remote**에서는 팀 리뷰 저장소에 로그인하거나 로그아웃할 수 있습니다. **Check status** 버튼을 눌러 연결 상태를 확인하세요. 업로드할 항목을 미리 확인(dry-run)하거나 결과를 업로드할 수도 있습니다. **Limits**에서는 `signoffIdleGraceSeconds`를 설정합니다.
