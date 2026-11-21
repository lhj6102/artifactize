# 데스크톱 앱 {#desktop-app}

데스크톱 앱(`artifactize-desktop`)은 CLI 및 MCP 서버와 같은 로컬 상태를 사용합니다. Run의 진행 상황, 리뷰 상태, 워크트리 목록은 새로고침하지 않아도 갱신됩니다. 아래쪽 막대는 어느 화면에서나 같은 자리에 있습니다. 왼쪽 로고를 클릭하면 명령 팔레트가 열리고, 가운데 **Chat**, **Verify**, **Review**로 화면을 전환합니다. 오른쪽에서는 **Settings**를 엽니다. 화면을 전환해도 각 화면의 선택 항목, 스크롤 위치, 열어 둔 패널, 입력 내용은 그대로 남습니다. **Review**는 현재 필터와 상관없이 대기 중인 Human 리뷰 수를 보여 줍니다.

## 앱 열기 {#open-the-app}

```sh
artifactize-desktop                                       # Verify에서 전체 프로젝트 목록 열기
artifactize-desktop monitor --repo PATH                   # Verify에서 PATH의 브랜치 열기
artifactize-desktop review [REQUEST_ID] [--reviewer NAME] # Review에서 가장 최근 대기 요청 또는 지정한 요청 열기
artifactize-desktop graph --repo PATH                     # Verify에서 PATH의 Current 열기
artifactize monitor [PATH | --repo PATH | --all]          # Verify를 여는 CLI 단축 명령
artifactize review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]  # Review를 여는 CLI 단축 명령
```

`artifactize-desktop`은 `--state-dir PATH`, `--all`, `--repo PATH`(`--all`과 함께 쓸 수 없음), `--reviewer NAME` 옵션도 받습니다.

로고를 클릭하거나 `Ctrl+K`를 눌러 명령 팔레트를 엽니다. **Show or hide the Pane**(`Ctrl+B`)으로 Pane을 숨기거나 표시합니다. **Open a request by ID**로 요청을 바로 열 수 있습니다. **Keyboard shortcuts**에서는 단축키를 확인합니다. Verify 명령은 Verify 화면을 열었을 때만 팔레트에 표시됩니다. Pane 옆 구분선을 드래그해 Pane 너비를, **Detail** 왼쪽 가장자리를 드래그해 Detail 너비를 조절합니다. 이 앱에서 맡은 리뷰가 남아 있는 상태로 종료하면 **Stay** 또는 **Release and quit**을 선택할 수 있습니다. 앱은 자신이 맡은 리뷰만 해제합니다. 담당을 해제했다가 CLI에서 다시 맡은 리뷰는 앱을 종료해도 담당이 유지됩니다. 담당을 해제해도 초안은 남습니다. 종료할 때 진행 중인 Chat 응답도 중단합니다.

## Chat: 에이전트와 작업하기 {#chat}

Chat은 외부 코딩 에이전트를 실행하는 화면이 아니라 Artifactize 자체 에이전트입니다. 세션은 처음 연 브랜치에 속하며, 그 워크트리에서 터미널을 열었을 때와 같은 환경을 사용합니다. 같은 워크트리에서 다른 브랜치로 바꾸면 Chat은 메시지를 보내거나 도구를 쓸 수 없는 이유를 알려 줍니다. 그 워크트리에서 원래 브랜치로 돌아가면 세션을 이어서 쓸 수 있습니다. 모델과 상관없이 도구는 같습니다. 파일을 읽고, 목록을 보거나 찾고, 내용을 검색하고, 파일을 쓰고 수정할 수 있습니다. 명령 실행도 할 수 있습니다. Verify 도구로 Run을 시작하거나 진행 상황을 확인하고 취소할 수도 있습니다. 선언과 저장한 결과도 읽을 수 있습니다. Chat은 도구를 호출할 때마다 사용자에게 승인을 요청하지 않습니다.

1. **New chat**을 클릭하고 활성 브랜치를 고릅니다. **Model**에는 해당 브랜치의 마지막 세션에서 쓴 모델이 표시됩니다. 이전 세션이 없다면 모든 프로바이더의 응답을 받은 뒤 연결된 프로바이더가 제공하는 첫 모델을 표시합니다. 모델이 아직 정해지지 않았다면 목록을 읽고 있다고 안내하며 **Create**를 누를 수 없습니다. 목록을 다 읽은 뒤에도 모델이 없다면 **Settings**에서 프로바이더를 연결하라고 안내합니다. 모델이 표시되면 **Create**를 누릅니다. Pane 맨 위에 **New chat**이 있고, 아래에는 프로젝트와 브랜치별로 세션을 묶어 최신순으로 표시합니다. 첫 메시지를 보내면 세션 제목이 생깁니다. 앱을 다시 열어도 세션과 대화 내용은 남아 있습니다.
2. 입력란 왼쪽 아래의 선택 메뉴에는 현재 모델이 표시됩니다. 메뉴를 열면 프로바이더별로 모델을 고를 수 있습니다. 프로바이더 연결이 필요하면 **Connect provider**를 눌러 **Settings → Providers**를 엽니다. 이후 답변에 쓸 모델을 바꿔도 Chat의 작업 방식은 그대로입니다.
3. 메시지를 입력하고 **Send**를 클릭하거나 Enter를 누릅니다. Shift+Enter는 줄을 바꿉니다. 에이전트의 답변이 진행되는 대로 표시됩니다. 도구는 호출마다 한 줄씩 어떤 작업을 어디에 했는지 보여 줍니다. 실패하면 이유가 나옵니다. **Stop**을 클릭하거나 입력란에서 Esc를 누르면 응답과 실행 중인 도구를 중단합니다. 대화 기록은 그대로 남고 Verify가 이미 접수한 Run은 계속 진행합니다. 응답을 받는 동안 쓴 글은 입력란에 남습니다. 응답이 끝난 뒤 **Send**를 눌러 보내세요. 응답이 중단되거나 실패해도 입력란을 계속 쓸 수 있습니다.
4. 에이전트에게 작업을 검증해 달라고 요청하세요. 접수한 Run마다 대화에 Run ID, 현재 상태, 실패 항목이나 사람이 확인할 항목이 표시됩니다. 원하는 Run의 **Open in Verify**를 클릭하세요. Verify에서는 Run 헤더 맨 앞에 **Chat · {session title}**이 나옵니다. 이 링크를 누르면 세션이 열리고 해당 Run 카드로 바로 이동합니다.

## Verify: 브랜치와 타임라인 {#pane}

- **검색**: 일반 검색어는 프로젝트와 브랜치 이름에서 찾습니다. `repo:`나 `branch:`를 쓰면 해당 항목에서만 찾습니다. `sort:recent`(기본값) 또는 `sort:name`으로 목록을 정렬합니다. 입력하는 동안 조건에 쓸 키와 값이 제안됩니다.
- **브랜치**: Run 기록이 있는 프로젝트의 활성 브랜치와 Git을 쓰지 않는 프로젝트 폴더를 표시합니다. `--repo`로 연 프로젝트도 목록에 나옵니다. 브랜치를 클릭하면 타임라인을 펼치고 가장 최근 항목을 엽니다. 왼쪽 화살표를 누르면 항목을 열지 않고 타임라인만 접거나 펼칩니다. 맨 위에는 **Current** 또는 실행 중인 Run이 표시되고, 그 아래에는 저장된 Run이 최신순으로 나옵니다. **Older runs**로 이전 Run을 더 불러옵니다. 별표를 누른 브랜치는 목록에서 별표 없는 브랜치보다 위에 표시됩니다.

## Verify: Run, Current, Detail {#current}

- **Run**: 선택한 Run의 상태, 실패하거나 대기 중인 eval, Artifact Graph를 확인합니다. Run의 상태 요약을 클릭하면 자세한 정보가 열립니다. Run을 시작한 곳이 CLI, MCP 클라이언트, Chat 세션 중 어디인지도 확인할 수 있습니다.
- **Current**: 현재 워크트리의 선언과 다음 verify에서 예상되는 결과를 보여 줍니다. `artifactize status`와 비슷합니다. **Refresh**를 클릭하면 다시 확인합니다. 선언을 편집할 때도 Current를 사용합니다.
- **Artifact Graph와 Detail**: **Find**로 Artifact, eval, 즐겨찾기로 이동합니다. 배경을 드래그해 화면을 옮기고, 스크롤로 확대·축소합니다. **Fit View**로 그래프 전체를 화면에 맞춥니다. 일부가 화면 밖에 있으면 **Navigator**가 나타납니다. 노드를 클릭하면 오른쪽에 **Detail**이 열립니다. eval을 클릭하면 요약과 Agent 대화 기록 또는 Runtime 로그를 확인할 수 있습니다. Artifact 탭에는 eval, 인접 Artifact, Human 도구가 나옵니다. 리뷰를 맡지 않고도 현재 워크트리에서 그 도구를 실행할 수 있습니다.
- **Human 리뷰 링크**: Run의 Artifact Detail에서 대기 중인 Human eval을 **Claim to review**로 맡거나 **Release claim**으로 담당을 해제합니다. **Open in Review**를 누르면 Review에서 해당 요청을 엽니다. 두 화면에서 같은 담당 상태와 초안을 사용합니다.

### Current에서 선언 편집하기 {#edit-definitions-in-current}

**New Artifact**를 클릭하고 **Name**과 **Path**를 입력한 뒤 **Create**를 누릅니다. Artifact Detail의 **Definition**에서 **Path**와 **Parts**를 바꾸고, 메모를 추가하거나 Artifact의 이름을 바꾸거나 삭제할 수 있습니다. `.artf` 파일을 직접 편집하려면 **Source**를 열고 **Save** 또는 **Revert**를 사용하세요. 창을 연 뒤 다른 곳에서 파일을 바꿨다면 Artifactize는 덮어쓰지 않고 Current를 갱신합니다.

노드나 상위 Artifact의 제목 줄을 드래그해 위치를 정합니다. 노드를 다른 상위 Artifact에 놓으면 그 Artifact의 `parts`에 추가합니다. 어느 상위 Artifact에도 속하지 않는 빈 공간에 놓으면 기존 구성이 해제됩니다. `parts`의 의미는 [구성](../concepts/artifacts.md#composition)을 참고하세요. **Reset layout**을 누르면 자동 배치로 돌아갑니다. 노드의 메모를 클릭해 편집하거나, 메모가 없는 다른 Artifact로 드래그해 옮길 수 있습니다.

## Review: Human 리뷰 찾기와 완료하기 {#review}

**Review**의 기본 필터는 `type:human waiting`입니다. 다른 요청을 찾으려면 입력란의 검색어를 고치세요. 모든 조건을 만족하는 요청만 표시됩니다. `type:human` 또는 `type:agent`로 평가 종류를, `waiting`으로 결과가 나올 때까지 대기 중인 요청을 선택합니다. `repo:`나 `branch:`는 목록에 표시되는 프로젝트나 브랜치 이름에서만 찾습니다. 일반 검색어는 eval, 프로젝트, 브랜치에서 찾습니다. 입력하는 동안 검색 조건에 쓸 키와 값이 제안됩니다. 리뷰는 프로젝트별로 묶어 최신순으로 표시합니다. 한 줄에 요청 하나가 나옵니다.

참조 링크로 열면 필터에 걸리지 않는 요청도 볼 수 있습니다. 예를 들어 이미 맡은 리뷰를 **Open in Review**로 열어도 필터는 바뀌지 않습니다. 열린 요청이 현재 필터에 해당하지 않으면 Review에서 그 사실을 알려 줍니다. 리뷰 헤더 맨 앞의 **Verify · {Run} › {Artifact}**를 누르면 해당 Run과 eval로 돌아갑니다. 다른 요청과 승인 결과를 공유하는 리뷰도 담당을 맡거나 다시 불러온 뒤에 처음 연 요청의 ID와 Verify 링크를 유지합니다.

1. **Claim to review**를 클릭해 요청을 맡습니다. 지시를 읽고 **Tools**로 Artifact를 살펴봅니다. 출력과 오류는 **Tool log**에서 확인하세요. 뷰어 도구는 [Artifact Viewer](viewer-plugins.md) 창을 엽니다.
2. **Notes**를 적고 **GREEN** 또는 **RED**를 선택합니다. eval이 요구하는 항목을 채우고 **Submit**을 클릭하세요. 입력이 잘못됐다면 초안은 그대로 열려 있습니다. 내용을 고친 뒤 다시 제출하세요. 제출하지 않고 담당을 해제하려면 **Release claim**을 누릅니다.

창이 넓으면 Review는 왼쪽에 지시, 도구, 도구 로그를 놓고 오른쪽에 **Evaluation**을 표시합니다. 초안은 리뷰와 리뷰어별로 저장합니다. Notes와 두 판정 양식에 입력한 값이 남습니다. 직접 고르기 전에는 판정이 선택되지 않습니다. 맡은 리뷰를 다시 열면 선택한 판정의 양식과 입력값이 표시됩니다. 앱을 다시 시작해도 같습니다. 담당을 해제해도 초안은 남습니다. 제출하면 초안은 지우고 Notes는 결과와 함께 저장합니다.

### 뷰어를 사용하면서 리뷰하기 {#review-while-using-a-viewer}

맡은 리뷰가 활성 상태에서 다른 모드로 바꾸거나 메인 창에서 포커스가 벗어나면 화면 오른쪽 아래에 작은 펫 오버레이가 나타납니다. 오버레이에서 같은 초안의 판정, **Notes**, 판정 양식을 사용할 수 있고 **Submit**으로 제출할 수 있습니다. 오버레이 헤더의 링크를 누르면 메인 창에서 해당 리뷰를 다시 엽니다. 평가 화면으로 돌아오거나 담당을 해제하거나 제출하면 오버레이가 사라집니다.

지원하는 뷰어에서 [**Point**](viewer-plugins.md#point-into-the-active-review)를 눌러 활성 리뷰의 **Notes**에 위치를 추가하세요. **Notes**를 다시 편집하기 전까지 Review와 펫 오버레이에서 추가한 텍스트를 강조해 보여 줍니다. 리뷰 대상 Artifact를 **Point**를 지원하는 뷰어에서 열어 둔 경우 오버레이의 **Point**를 쓸 수 있습니다. Point는 리뷰를 제출하지 않습니다.

## Settings {#settings}

- **Providers**에서는 내장 또는 사용자 지정 프로바이더를 연결하고 사용 가능한 모델을 확인합니다. Chat은 선택한 프로바이더와 모델을 사용합니다. Agent eval의 프로바이더와 모델은 프로필에 선언합니다.
- **Remote**에서는 [팀 리뷰 저장소](team-and-ci.md)에 로그인하고, 상태를 확인하고, 업로드할 결과를 미리 보거나 업로드합니다.
- **Limits**에서는 앞으로 실행할 Run에 적용할 승인 요청 대기 시간을 설정합니다. Settings 헤더에는 리뷰어 이름이 표시됩니다. 새 정식 버전이 나오면 이곳의 알림을 눌러 업데이트 방법을 확인할 수 있습니다. Artifactize가 직접 다운로드하거나 설치하지 않습니다. Microsoft Store 설치 버전은 Store에서 업데이트합니다.
