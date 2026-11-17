# 데스크톱 {#desktop}

앱을 열면 이 머신에서 Run을 실행한 모든 프로젝트의 현재 브랜치와, 승인이 필요한 모든 Human
리뷰를 한 화면에서 볼 수 있습니다. Run을 살펴보고, 각 브랜치에서 지금 검증하면 실행될 작업과 증거를
확인한 뒤 Human 승인을 처리하세요. 앱은 CLI, TUI와 상태를 공유하므로 한쪽에서 변경한 내용이
다른 쪽에도 반영됩니다. 앱은 릴리스마다 명령과 함께 배포되며, 설치 방법은 [데스크톱 앱](../getting-started/install-options.md#desktop-app)을 보세요.

첫 실행 때의 명령 설정, PATH 세부 사항, 업데이트, 정리는
[설치 안내](../getting-started/install-options.md#desktop-app)를 보세요.

## 앱 열기 {#open-the-app}

```sh
artifactize-desktop
artifactize-desktop monitor --repo PATH
artifactize-desktop review [REQUEST_ID]
artifactize-desktop graph --repo PATH
```

인자가 없으면 현재 디렉터리와 상관없이 아무것도 고르지 않은 채 앱이 열립니다.
`--repo PATH`는 그 폴더의 브랜치를 고르고 펼친 뒤 가장 최근 항목을 보여 줍니다. 기본값인 `--all`은
아무것도 고르지 않으며, 어느 쪽이든 모든 브랜치가 나열됩니다. `review`는
가장 최근에 승인 대기 상태가 된 Human 리뷰를, `review REQUEST_ID`는 지정한 요청을 엽니다. `review`는 `--repo`를 주어도 브랜치를 고르거나 펼치지 않습니다. `graph`에는
`--repo`가 필요하며 그 브랜치의 [Current](#current)를 보여 줍니다. `--state-dir PATH`로
상태 디렉터리를 지정하며, 지정하지 않으면 공유 상태 디렉터리를 씁니다. `--reviewer NAME`으로
협업할 때 리뷰어로 표시할 이름(보통 사용자 이름)을 지정합니다. `-h`나 `--help`(모드 뒤에도
쓸 수 있음, 예: `artifactize-desktop monitor --help`)와 `artifactize-desktop help [MODE]`는 잘못된 인자와
마찬가지로 stderr에 도움말을 출력하고 2로 종료합니다. 앱을 정상적으로 닫으면 종료 코드는 0이고, 시작하지 못하면
1입니다.

Ctrl+K나 F1을 누르거나 **Commands** 버튼을 클릭하면 지금 실행할 수 있는 명령과 단축키를 검색할 수 있는 명령 팔레트가 열립니다. Enter를 누르면 명령이 실행되고, Esc를 누르면 닫힙니다. `?`를 누르면 Help가 열려 지금 초점이 있는 곳의 키와, Artifact Graph에서는 각 표시의 뜻을 보여 줍니다. 창 전체에서 쓰는 키는 다음과 같습니다(macOS에서는 Ctrl 대신 Command).

| 키 | 동작 |
|---|---|
| Ctrl+K, F1 | 명령 팔레트 |
| `?` | Help |
| Ctrl+, | [Settings](#settings)를 열고 닫기 |
| Ctrl+B | [Pane](#pane)을 숨기거나 보여 주기 |
| `/` | 브랜치 [검색](#search) |
| Ctrl+O | [ID로 요청 열기](#requests-by-id) |
| Ctrl+R, F5 | 상태를 다시 읽고 브랜치(와 보이는 Current)를 다시 스캔. Human 리뷰에서는 Ctrl+R이 RED이므로 F5를 쓰세요 |
| `q` | [종료](#leave-and-quit) |

앱은 시스템 설정과 관계없이 항상 다크 모드로 표시됩니다.

[새 릴리스](../getting-started/update.md#newer-releases)가 나오면 헤더의 **Settings** 앞에
`VERSION available`이 나타납니다. 클릭하면 대화상자가 열리고, 이 앱의 버전과 앱을 설치한
방법에 맞는 업데이트 방법을 보여 줍니다. 실행할 명령은 선택해서 복사할 수 있는 텍스트로,
직접 내려받아 설치할 파일은 링크로 보여 주고, 설치 방법을 알 수 없으면 릴리스 페이지를
안내합니다. 링크는 브라우저에서 열리고, Esc나 **Close**로 대화상자를 닫습니다. 앱은 시작할 때와
실행 중 하루에 한 번 새 릴리스를 확인하며, 스스로 업데이트하지는 않습니다. Microsoft Store로
설치한 앱은 Store가 업데이트하므로 이 알림이 나타나지 않습니다.

마우스를 올리면 강조만 되고, 자세한 정보는 클릭해야 열립니다. 상자의 **Minimize**·**Expand** 버튼만 예외로, 마우스를 올리면 단축키 `m`을 알려 주는 툴팁이 나옵니다.

## Pane {#pane}

앱 화면은 왼쪽의 **Pane**과 그 옆의 **Main area**로 나뉩니다.
Pane의 항목은 세로로 한 줄씩 배치됩니다. 맨 위에 검색 칸, 그 아래에 브랜치, 그리고
맨 아래에 사람의 승인이 필요한 리뷰가 있습니다. Pane과
Main area 사이의 구분선을 끌어 크기를 바꾸세요. Main area에 무엇이 표시되든 직접 조절한 비율이 그대로 유지됩니다.

Ctrl+B를 누르거나 헤더의 **Pane** 버튼을 누르면 Pane이 숨겨지고 Main area가 창 너비 전체를 씁니다. 다시 누르면 Pane이 원래 너비로 돌아옵니다. Pane이 숨겨져 있으면 키보드 초점은 Main area에 있습니다. `/` 또는 Esc를 누르면 Pane으로 돌아가며, 숨겨진 Pane이 먼저 표시됩니다.

### 브랜치 {#branches}

브랜치는 Git 저장소의 작업 이력을 나누어 관리하는 단위입니다. 워크트리는 브랜치 하나를 체크아웃해 작업하는 폴더입니다. 한 저장소에 워크트리를 여러 개 둘 수 있습니다. 각 워크트리에서는 브랜치를 체크아웃해 작업하거나, 브랜치에 연결되지 않은 특정 커밋을 체크아웃할 수 있습니다. 각 항목은 `artifactize / feat/x`처럼
프로젝트와 브랜치로 표시됩니다. 각 항목에는 현재 워크트리와 체크아웃한 브랜치가 표시되며, 분리된 상태에서는 브랜치 대신 폴더 이름이 표시됩니다. Git으로 관리하지 않는 프로젝트는 폴더 이름으로 표시합니다.
나열되는 프로젝트는 이 머신에 Run이 있는 프로젝트와 `--repo`나 `graph --repo`로 앱을 시작한
프로젝트입니다. `--repo`로 시작한 프로젝트는 아직 Run이 없어도 나열됩니다. 제거된
워크트리나 폴더, 또는 Git이 더 이상 나열하지 않는 워크트리는 보이지 않습니다. 추가되거나 제거된 워크트리는
다시 읽지 않아도 몇 초 안에 나타나거나 사라집니다. Run을 시작한 주체가 에이전트인지 사람인지에 따라 항목을 나누어 표시하지 않습니다. 각 브랜치에는 실행한 폴더와 관계없이 그 프로젝트의 해당 브랜치에서 실행한 Run이 나옵니다.

접힌 브랜치의 이름 아래에는 가장 최근 Run의 상태를 나타내는 색 점과 상태 이름, 실행 후 경과 시간(실행 중이면 지금까지 걸린 시간)이 표시됩니다. 브랜치를 클릭하면
펼치고 가장 최근 항목을 보여 줍니다. 이름 앞의 꺾쇠를 클릭하면 Main area는 바뀌지 않고
그 항목만 펼쳐지거나 접힙니다. 브랜치를 펼치면 타임라인에 Run이 최신순으로 나옵니다.
각 Run의 상태와 경과 시간 아래에는 verify를 요청할 때 남긴
[코멘트](../cli/index.md#submission-and-watching)를 한 줄로 요약해 보여 줍니다. 코멘트가 없으면 Run ID를 보여 줍니다. 맨 앞 항목은 워크트리의 지금 상태인 **Current**입니다. Run이
진행 중이면 그 Run이 Current 자리에 오고, Run이 끝나면 Current가 다시 표시됩니다. 끝의 **Older runs**는
더 오래된 Run을 가져옵니다. 항목을 클릭하면 회색의 둥근 배지로 표시되며 Main area에 나옵니다. 브랜치는
몇 개든 펼칠 수 있습니다. `monitor`나 `graph` 명령에 브랜치를 지정하면 해당 브랜치가 처음부터 펼쳐져 있습니다.

브랜치 이름 끝의 별을 클릭하면 즐겨찾기에 추가되어 별이 채워집니다. 별표가 있는 브랜치가 먼저 나오고,
두 그룹은 각각 검색 순서대로 정렬됩니다. 별표는 앱을 다시 열어도 남아 있습니다.
별표를 떼려면 채워진 별을 다시 클릭하세요.

↑/↓ 또는 `j`/`k`로 브랜치, 펼친 브랜치의 타임라인, 대기 중인 리뷰 사이를 이동할 수 있습니다. 브랜치로 이동하면 그 브랜치의 가장 최근 항목을, 타임라인 항목이나 리뷰로 이동하면 해당 항목을 보여 줍니다. →를 누르면 선택한 브랜치가 펼쳐지거나 그 타임라인으로 들어가며, ←를 누르면 타임라인에서 빠져나오거나 브랜치가 접힙니다. 타임라인의
가장 오래된 항목에서 ↓를 누르면 더 오래된 Run을 가져옵니다. Enter를 누르면 Main area로 이동하고, 그곳에서 Detail이 열려 있지 않을 때 Esc를 누르면 Pane으로 돌아갑니다. Pane이 숨겨져 있으면 먼저 Pane을 보여 줍니다.

### 검색 {#search}

`/`를 누르면 검색 칸으로 이동하며, Pane이 숨겨져 있으면 먼저 보여 줍니다. Pane에 있거나 Artifact Graph가 보이지 않을 때는 Ctrl+F도 같습니다.
Artifact Graph가 보일 때 Ctrl+F를 누르면 [Artifact Graph에서 찾기](#find-in-the-graph)가 열립니다. 처음 검색어는 `sort:recent`입니다. 일반 검색어와
`key:value` 조건을 모두 만족하는 브랜치와 대기 중인 리뷰만
표시합니다. 일반 검색어는 대소문자를 구분하지 않으며, 프로젝트, 브랜치, 대기 중인 리뷰의 eval 이름에 그 검색어가 들어 있는 항목을 찾습니다. `repo:`와 `branch:`는 각각 저장소와 브랜치 필드에 그 검색어가 들어 있는 항목을 찾습니다. 값에 공백이 있으면 `repo:"my project"`처럼 따옴표로 묶으세요. 따옴표 안의 공백은 새 검색어를 시작하지 않고 그 값에 포함됩니다.
`sort:recent`는 가장 최근 Run이 있는 브랜치를 맨 앞에, Run이 없는 브랜치를
맨 뒤에 두고, `sort:name`은 프로젝트와 브랜치 순으로 정렬합니다. 자동 완성으로 검색 키와
값을 채울 수 있습니다. 검색어가 비어 있거나 커서가 공백 바로 뒤에 있을 때 ↓를 누르면 검색 키 후보가 표시됩니다. ↑/↓로 후보를 고른 뒤 Tab이나 Enter를 누르거나 후보를 클릭하면
검색어에 들어갑니다. 자동 완성 후보가 떠 있지 않을 때 Enter를 누르면 목록으로 돌아갑니다. 알 수 없는 키 등으로 검색어를 해석할 수 없으면 브랜치 목록 대신 오류를 보여 주고, 검색어를 고칠 때까지 두 목록을 비워 둡니다. Esc를 누를 때마다
자동 완성 후보를 닫고, 검색어를 `sort:recent`로 되돌리고, 목록으로 돌아가는 순서로 동작합니다.

### 대기 중인 리뷰 {#waiting-reviews}

Pane 아래쪽에는 제거된 브랜치의 리뷰를 포함해 이 머신의 모든 저장소에서
대기 중인 Human 리뷰가 최신순으로 나열되며, 각각
eval, 프로젝트와 브랜치, 대기 시간이 나오고, 누군가 맡았으면 맡은 사람도 나옵니다. 대기 중인 리뷰가 없으면 이
영역은 **Waiting (0)**이라고 적힌 얇은 막대로 줄어듭니다. 대기 중인 리뷰가 생길 때마다 행이 하나씩
추가되고, 영역은 아래에서 위로 늘어나 Pane 높이의 절반까지 커집니다. 그보다 많아지면 목록이
스크롤됩니다. 하나를 클릭하거나 Enter를 누르면 Main area에 [리뷰](#human-reviews)가 열립니다.
같은 Human 리뷰를 공유하는 요청은 하나의 리뷰로 묶어 표시합니다. 그런 요청을 열면
원래 리뷰가 열립니다. 제거된 브랜치의 리뷰는 그 저장소 폴더가 남아 있는 동안만 목록에 나옵니다. 폴더를
삭제하면 해당 행은 사라집니다. 다만 다른 워크트리에 같은 리뷰의 대기 중인 요청이 있으면 그 워크트리 아래에 계속 표시됩니다.

다른 곳에서 생긴 Run, 결과, 담당 지정, 제출과 워크트리 변경 사항은 직접 새로 고치지
않아도 화면에 반영됩니다.

## Main area {#main-area}

Main area에는 Pane에서 선택한 Run, 브랜치의 Current, 리뷰 또는
ID로 연 요청이 표시되며, 맨 위 헤더 행 하나가 무엇을 보여 주는지와 그 요약을 알려 줍니다. 아무것도 고르지 않았거나, 불러오는 중이거나,
오류가 났거나, Artifact Graph가 비어 있으면 맨 위에 그 상태를 알리는 한 줄이 나오고 Artifact Graph 자리는 비어 있습니다. 다른 Run을 읽을 수 없는 등
이 머신의 기록을 읽는 데 문제가 있으면, Artifact Graph를 옮기거나 가리지 않고 헤더 행 끝에
문제를 몇 단어로 빨갛게 표시합니다. 클릭하면 전체 내용을 볼 수 있고, 다시 클릭하면 접힙니다. 읽기에 실패하면 마지막으로 불러온 내용이 그대로 남아 실제 상태와
다를 수 있으니, 원인을 해결한 뒤 새로 고치세요(Ctrl+R). 선택한 Run 자체를
읽을 수 없게 되거나 그 Artifact Graph를 불러오지 못하는 경우는 다릅니다. 메시지가 Artifact Graph
자리를 대신하며, 다른 것을 고르면 됩니다. 손상된 기록은
검증을 통과했다는 근거로 삼지 않고 [읽을 수 없는 기록으로 표시합니다](../features/others/state.md#unreadable-records).

### Run {#run}

Pane에서 Run을 고르면 Main area에 그 Run의 Artifact Graph가 표시됩니다. 헤더 행에는 Run ID와 요약이 나옵니다. Run 요약에는 판정 표시와 이름이 해당 색으로 나옵니다. 이어서 RED·ERROR eval과 대기 중인 승인이 각각 표시와 이름으로 나오며, 공간이 부족하면 행 끝이 잘립니다. 요약을 클릭하거나 팔레트 명령 **Open the Run's Detail**을 쓰면 [Run의 Detail](#detail)이 열립니다. 그 Run의 결과(상태, 실패한 eval, 상태별 eval 수, 걸린 시간, 코멘트 전문)와 대기 중인 항목을 먼저 보여 주고, **What**, **Provenance**, **Technical**은 펼칠 때까지 접혀 있습니다. `t`를 누르면 **Technical**이 표시되거나 숨겨집니다. Run이 끝날 때의 검증 결과는 기록으로 남습니다. 나중에 Human 판정을 제출해도
이전 Run의 판정은 바뀌지 않고, 현재 요청의 상태 표시만 바뀔 수 있습니다.

### Current {#current}

브랜치의 **Current**는
[`artifactize status`](../features/run.md#status-and-static-graph)처럼 워크트리의 지금 상태를 스캔하고,
Run 없이 현재 선언의 Artifact Graph를 그립니다. 각 Artifact는 처음에 확인 중(checking)으로 표시되다가,
지금 verify를 실행하면 어떻게 될지를 보여 줍니다. reused(모든 eval에서 기존 결과를 재사용), would run,
waiting, blocked 중 하나이며, eval 중 몇 개를 재사용할지도 함께 나옵니다. 스크린 리더는 Artifact마다 이 상태와 실행될 eval 수를 읽어 줍니다.
스캔하는 동안 헤더 행에는 확인 중 표시와 아직 확인할 Artifact 수(`3 left`)가 나옵니다. 스캔이 끝나면 스캔한 시각을, 실패하면 원인을 표시합니다. **Refresh**는 그 행 끝에 있습니다.
Current를 다시 스캔하는 경우는 다음과 같습니다.

- Current를 다시 고르거나 그 브랜치의 Run이 끝날 때: 다시 스캔하며, 스캔 중이면 그 스캔을 이어 갑니다.
- 행 끝의 **Refresh**: 다시 스캔하며, 스캔 중에는 비활성화됩니다.
- 팔레트의 **Refresh**나 Ctrl+R/F5: 진행 중인 스캔이 끝난 뒤 다시 스캔합니다.
- 검색 때문에 브랜치가 Pane에 보이지 않을 때 Run이 끝나면: **Refresh**를 누르세요.

Human [`graph` 도구](../features/tools/builtin.md#current-project-graph)와
`artifactize-desktop graph --repo PATH`는 이 보기로 앱을 엽니다. 이 보기를 열기만
해서는 어떤 리뷰도 승인되지 않습니다.

### Current 편집 {#editing-the-current}

브랜치의 Current에서만 편집할 수 있습니다. Run Artifact Graph는 기록입니다. Artifact Graph 도구 줄의
**New Artifact**에서 Name과 Path를 입력하면 `.artifactize/<name>.artf`를 만들고
Detail을 엽니다. 대상이 아직 없어도 정의할 수 있습니다. 이름이나 경로가 잘못되면
이유를 보여 주고 아무것도 쓰지 않습니다.

Run 탭에는 **Path**가 표시되고, Current의 Artifact 탭에는 그 자리에 **Definition**이 표시됩니다. 정의 파일, 편집할 수
있는 Path, Parts, Comment, Rename, Delete를 보여 줍니다. 바꾼 Path는 Enter로 저장하고
Esc로 되돌립니다. Parts의 **+ Add**는 이름으로 Artifact를 제안합니다. Enter로 추가하고
Esc로 닫으며 **✕**로 part를 제거합니다. 순환이 생기는 추가는 거부합니다. 처음에는 접혀
있는 **Source**에서 파일 전체를 편집합니다. eval, 도구, fingerprint도 여기서 바꿉니다.
입력하면 **Save**와 **Revert**가 나타납니다. Save나 Ctrl+S로 저장하고, Revert로 파일을
다시 읽습니다.

Rename은 같은 폴더에서 파일 이름을 바꾸고 구조적 참조와 저장된 part 위치를 함께
고칩니다. Delete는 지울 파일을 보여 줍니다. 다른 Artifact가 이름을 참조하면 그 목록을
보여 주고 거부합니다. 모든 편집은 프로젝트를 검증한 뒤에만 씁니다. 잘못된 편집은
아무것도 쓰지 않고 입력을 남깁니다. Source 오류에는 파일, 행, 열이 표시됩니다. 앱의
실행 취소는 없습니다. 변경은 워크트리 파일이며 Git이 이력을 보관합니다.

#### 노드와 part 배치 {#placing-nodes-and-parts}

노드, 최소화한 whole, whole의 제목 줄을 끌어 배치합니다. whole은 part와 코멘트를 함께
옮깁니다. 움직이지 않고 클릭하면 Detail이 열리고, 배경을 끌면 화면이 이동합니다.
끌다가 Esc를 누르면 저장하지 않고 원래 자리로 돌아갑니다. Run에서는 끌어도 화면만
이동합니다.

상자와 최상위는 안의 노드를 처음 옮기기 전까지 자동 배치됩니다. 최상위에서 처음
옮길 때는 모든 최상위 노드의 현재 배치를 한 번에 저장합니다. whole은 part의 위치를
그 whole의 정의에 저장하고, 최상위 노드는 각자의 위치를 자기 정의에 저장합니다. 다시 열어도 유지되며
이 브랜치의 Run에도 적용됩니다. 새 노드는 빈 곳에 놓입니다. whole 제목 줄이나 Current
도구 줄의 **Reset layout**은 그 상자나 최상위를 자동 배치로 되돌립니다. 안쪽 상자의
위치는 그대로입니다.

노드 위나 whole 상자 안에 놓으면 part가 됩니다. 놓을 수 있는 대상이 강조됩니다.
상자 사이로 옮기면 이전 whole에서 빼고 새 whole에 넣는 작업을 한 번에 저장합니다.
모든 상자 밖에 놓으면 그 whole에서 빠집니다. 공유 part를 옮기면 끌어 옮긴 사본이 속한 whole만
바뀝니다. whole을 하나 더 추가하려면 Detail의 Parts를 쓰세요. 순환이 생기는 배치는 거부하고
노드를 원래 위치로 되돌립니다. 참조는 eval 안의 문구이므로 간선을 끌어 바꾸지 않고 Source에서
고칩니다.

#### 노드 코멘트 {#node-comments}

노드의 일반 텍스트 코멘트는 노드 아래, whole 상자의 제목 아래, 공유 part의 각 사본
아래에 표시됩니다. 처음 세 줄 뒤에는 말줄임표가 붙습니다. 최소화로 숨긴 part나 블록으로
그린 노드에는 표시되지 않습니다. Run을 요청할 때 쓰는 코멘트와 별개이며 검증과 재사용에
영향을 주지 않습니다.

Current의 Definition에서 **Comment**를 누르거나 코멘트를 클릭하면 그 자리에서 전체
텍스트를 편집합니다. Enter로 저장하고 Shift+Enter로 줄을 나눕니다. 빈 텍스트를 저장하면
삭제됩니다. Esc나 바깥 클릭은 저장하지 않고 닫습니다. 코멘트를 다른 Artifact로 끌면
두 파일 사이에서 한 번에 옮깁니다. 배경, 자기 노드, 이미 코멘트가 있는 대상에 놓으면
그대로이며 Esc로 취소할 수 있습니다. Run에서 코멘트를 클릭하면 노드의 Detail만 엽니다.

#### 파일 편집과 충돌 {#conflicts-with-file-edits}

Current를 스캔한 뒤 터미널이나 에이전트가 파일을 바꾸면, 앱은 아무 파일도 쓰지 않고
해당 파일 이름을 알린 뒤 다시 스캔합니다. 입력 중인 Source는 Save나 Revert 전까지 남으며
스캔으로 덮어쓰지 않습니다. 노드를 옮기거나 part 변경, Reset layout, 코멘트 저장을 하려면
그 작업으로 바뀔 파일의 Source를 먼저 저장하거나 되돌리세요. 그 파일에 저장하지 않은
Source가 남아 있으면 작업을 거부합니다.
손대지 않은 텍스트와 알 수 없는 annotation 값은 보존합니다. 파일 형식은
[위치와 코멘트](../features/declarations.md#positions-and-comments)를 보세요.

### Artifact Graph {#graphs}

노드에는 상태 기호와 색 띠가 표시되고, 이름 뒤에는 GREEN인 eval 수가 `2/3`처럼 표시됩니다(Current에서는 재사용될 eval 수). 각 표시의 뜻은 **Help**(`?`)에서 볼 수 있습니다. 간선은 입력에서 사용처로 이어지며, RED, ERROR, blocked인 Artifact에서 나가는 간선만 색이 있고 나머지는 회색입니다. 결과가 나오면 노드와 간선이 바로 갱신됩니다.

Run의 Artifact Graph와 Current Artifact Graph는 똑같이 조작합니다.

| 조작 | 동작 |
|---|---|
| 클릭, 또는 Tab으로 Artifact에 초점을 옮긴 뒤 Enter | Detail 열기 |
| **Find**, Ctrl+F | [Artifact나 eval 찾기, 즐겨찾기 보기](#find-in-the-graph) |
| `m` | 상자 최소화·펼치기(아래 설명) |
| **Zoom out**, **Zoom in**, 휠, 핀치, 배경 더블 클릭 | 확대·축소(최대 200%) |
| **Fit** | Artifact Graph 전체 보기(100%를 넘지 않음) |
| 배경 드래그 | 이동 |

Artifact Graph는 처음 나타날 때 전체를 보여 줍니다. 이후 보기 영역은 사용자가 직접 옮기지 않는 한 그대로 유지되며, 새 결과나 스캔·재배치·Detail 열기로는 바뀌지 않습니다.

Artifact Graph 일부가 화면 밖에 있으면 Artifact Graph 오른쪽 아래(Detail이 열려 있으면 그 왼쪽)에 **Navigator**가 나옵니다. Artifact Graph 전체를 작게 보여 주며, 각 Artifact는 그 상태 색의 사각형으로, 지금 보이는 부분은 테두리로 표시합니다. 그 안을 클릭하거나 드래그하면 확대 비율은 그대로 두고 그곳으로 보기 영역을 옮깁니다. Artifact Graph 전체가 화면에 들어오면 사라집니다.

모든 Artifact Graph는 입력이 왼쪽에 오도록 배치됩니다. **노드가 500개를 넘는** Artifact Graph를 이름을
읽을 수 없을 만큼 축소하면 노드는 상태 색의 블록으로, whole 상자는 테두리로 보입니다.
이름을 읽을 수 있는 배율이어도 화면에 노드가 500개 넘게 보이면 블록으로 표시합니다.
블록을 클릭하면 Detail이 열립니다. 이름을 읽을 수 있고 화면에 노드가 500개 이하일 때는
Tab과 Enter를 다시 쓸 수 있습니다. Detail에는 양쪽 이웃을 각각 최대 16개까지 보여 주고
나머지는 개수만 표시합니다.

[부분](../features/artifact.md#composition)으로 이루어진 Artifact는 부분들을 감싸는 상자로
그려집니다. 상자 위쪽의 제목 행에는 표시, 이름, 개수가 나오고, 그 아래를 상태 색의 띠가 닫습니다. 부분에 다시 부분이 있으면 그 안에 상자가 하나 더
들어갑니다. 상자가 곧 그 Artifact입니다. 부분이 없는 곳을 클릭하거나 Tab과 Enter를 쓰면 그
Artifact의 Detail이 열리고, 부분을 클릭하면 그 부분의 Detail이 열립니다. 여러 Artifact에 속한
부분은 각 Artifact 안에 하나씩 그려집니다. 그중 하나를 열면 모든 사본이 함께 강조되고, Detail에
몇 번 그려졌는지와 어느 Artifact 안에 있는지가 나옵니다.

상자 오른쪽 위에는 **Minimize** 버튼이 있습니다. Tab을 눌러 상자 다음에 있는 이 버튼으로 초점을 옮길 수 있으며, 툴팁에서 단축키 `m`을 확인할 수 있습니다. 누르면
부분을 숨기고 Artifact를 일반 노드 크기로 줄여 표시하며, 이름과 부분 개수, **Expand** 버튼을 보여 줍니다. 숨긴 부분에
이어진 간선은 이 노드에 이어지고, 숨긴 부분끼리 잇는 간선은 그리지 않습니다. 이 노드도 다른 노드처럼
Detail을 열 수 있고, 찾기 칸에는 숨긴 Artifact도 그대로 나옵니다. **Expand**를 누르면 상자가 다시
펼쳐집니다. `m`을 누르면 Tab으로 초점을 옮긴 상자를 최소화하거나 펼칩니다. 초점이 상자에 없으면 Detail이
열린 상자에서 같은 동작을 합니다. 입력하는 중이거나 키보드 초점이 Pane에 있으면 동작하지 않습니다. 최소화는 그 브랜치의
모든 Artifact Graph에 적용되고, 다시 펼치거나 앱을 끌 때까지 유지됩니다.

### Artifact Graph에서 찾기 {#find-in-the-graph}

Run의 Artifact Graph나 Current가 보일 때 도구 줄의 **Find**를 누르거나, 키보드 초점이 Pane 밖(Artifact Graph나 Detail)에 있을 때 Ctrl+F를 누르면
Artifact Graph 왼쪽 위에 찾기 칸이 열립니다. 입력한 글자가 이름에 들어 있는 Artifact와 eval을 대소문자 구분 없이
Artifact Graph 순서대로 보여 줍니다. Artifact는 전체 이름으로, eval은 `Artifact / eval` 형태로 나오고, 각각
판정 기호와 색이 붙습니다(Current에서는 verify가 할 일). 처음 50개까지 보여 주고, 나머지는 몇 개가 더
있는지만 알려 줍니다. 찾은 항목이 없으면 없다고 표시합니다.

아무것도 입력하지 않으면 이 Artifact Graph에서 [별표를 단 Artifact](#detail)를 즐겨찾기로 보여 줍니다.
찾은 항목과 같은 모양으로, Artifact Graph 순서대로 나옵니다. 이 Artifact Graph에 별표를 단 Artifact가 없으면 별표를 다는
방법을 알려 줍니다.

↑/↓로 항목을 고릅니다. Enter를 누르거나(고른 항목이 없으면 첫 항목) 항목을 클릭하면 그 Detail이
열리며, eval이면 그 eval의 탭으로 열립니다. 그 Artifact는 Detail 왼쪽 Artifact Graph의 한가운데로 오고,
이름을 읽을 수 있을 만큼 확대됩니다. Esc를 누르면 찾기 칸이 닫히고 키보드 초점이 돌아갑니다. 찾기 칸
바깥을 클릭해도 닫히며, 그 클릭은 평소처럼 동작합니다. Artifact Graph가 아무리 커도
검색하면 모든 Artifact를 찾을 수 있습니다. 여러 Artifact 안에 그려진 부분은 한 번만 나오고, 열면 첫
번째 사본이 가운데로 옵니다. 최소화된 Artifact 안의 부분은 계속 숨겨져 있습니다. 열면 그 부분의 Detail이
열리고, 대신 최소화된 Artifact가 가운데로 오며 강조됩니다. Artifact Graph에서 그 부분을 보려면 최소화된 Artifact를
펼치세요.

### Detail {#detail}

Artifact를 클릭하면 Artifact Graph 오른쪽에 해당 Artifact의 **Detail** 패널이 열립니다. 다른 Artifact를 클릭하면 그 Artifact의 Detail로 바뀌고, Artifact Graph
배경을 클릭하거나 **Close**, Esc, ←를 누르면 닫힙니다. Human 리뷰의 Detail에서 ←는 편집하거나 실행하는
것이 없을 때만 Detail을 닫고, 나머지는 [리뷰에서 Esc를 누를 때](#human-reviews)처럼 먼저 한 단계씩 되돌아갑니다. Detail의 왼쪽 가장자리를 드래그해, 내용을 읽기 편하면서도 Artifact Graph가 조금 보이는 너비로 조절하세요. 그 폭은 다음 실행에서도 유지됩니다.

섹션의 머리 행을 클릭하거나 그 위에서 Enter나 Space를 누르면 섹션을 접거나 펼칩니다. 섹션마다 마지막으로 선택한 접기·펼치기 상태를 저장해 두므로, 이후 열리는 Detail과 다음 실행에도 그 상태가 유지됩니다. 단, Human 리뷰의 Tools, Tool log, Result는 항상 펼쳐져 있습니다. 처음에 펼쳐진 섹션은 **Evals**, **Tools**, **Path**, **Result**, **Outcome**, **Waits for**이고, 처음에 접힌 섹션은 **Neighbours**, **Execution options**, **Execution metadata**, **Fingerprint**, **What**, **Provenance**, **Debug**, **Technical**입니다.

Artifact Detail은 해당 Artifact 탭에서 열리고, 제목에는 Artifact 이름이 나옵니다. **Evals**에는 eval마다 판정 표시, 그 색으로 쓴 상태가 나오고, 오른쪽 끝에는 Run에서는 걸린 시간이, Current에서는 선언된 제목이 나옵니다. 클릭하면 그 eval의 탭이 열립니다. **Neighbours**를 펼치면 그 Artifact를 중심으로 이웃 Artifact와의 관계를 Artifact Graph와 같은 노드와 간선으로 보여 줍니다. 왼쪽에는 이 Artifact의 eval이 사용하는 Artifact가, 오른쪽에는 이 Artifact를 사용하는 eval이 속한 Artifact가 있습니다. 이 그림은 이동하거나 확대할 수 없으며, 이웃을 클릭하면 그 이웃의 Detail이 열립니다. 그다음 **Tools**와 **Path**(Current에서는 **Definition**과 **Source**)가 나오고, 도구를 한 번 실행하면 **Tool log**도 표시됩니다. **Debug**에는 대기 중인 의존성을 비롯한 나머지 정보가 접힌 상태로 표시됩니다.

eval의 탭에서는 제목 아래의 칩이 경로처럼(`[icon] {name} / [eval] …`) 읽힙니다. Artifact의 탭으로 돌아가는 Artifact 칩 하나와 eval마다 판정의 표시와 색을 보여 주는 칩이 하나씩 있으며, Current에서는 실행될지, 재사용될지, 아직 확인 중인지를 보여 줍니다.

어느 탭을 보고 있든 Artifact Detail 제목 옆의 별을 클릭하면 그 Artifact에 별표가 붙어 **즐겨찾기**가
됩니다. 즐겨찾기는 Ctrl+F를 누르고 [아무것도 입력하지 않으면](#find-in-the-graph) 나옵니다. 별표는
그 프로젝트의 모든 브랜치에서 Run과 Current 모두에 적용되고, 앱을 다시 열어도 유지됩니다. 채워진 별을 다시
클릭하면 별표가 빠집니다.

**Tools**에는 Artifact가 선언한 [Human 도구](../features/tools/human.md)가
아래 리뷰 화면과 같이 operation과 설명이 붙은 버튼으로 나옵니다. 클릭하면 지금 보고 있는 Run과 무관하게, 그 브랜치의 현재 워크트리에서 도구 하나를 바로 실행합니다. 어떤 요청도 맡지 않으며, 실행 결과는 리뷰 증거로 남지 않습니다. 실행 내역은 Artifact의 **Tool log**에 나오며, Tool log는 첫 실행 때 나타나고 접어 두었다면 다시 펼쳐집니다. 정상 종료 여부는 로그의 마지막 줄에, 실패 내용은 도구 아래에 표시됩니다. 도구 하나가
실행되는 동안 같은 Artifact의 다른 도구 버튼은 비활성화되며, 실행 중 표시 옆의 **Cancel** 버튼으로
실행을 멈출 수 있습니다. Detail을 닫아도 멈추지 않으며, 출력은 계속
Artifact의 Tool log에 쌓여 Detail을 다시 열면 보입니다. 실행 기록은 앱이 실행되는 동안
그 Tool log에 남습니다. Artifact Graph의 노드에는 도구 버튼이 없습니다. Tools와 별 표시는 Pane의 검색 결과에
그 브랜치가 남아 있을 때만 보이므로, 검색 때문에 브랜치가 숨겨지면 검색어를 지우거나 넓혀야 다시 나타납니다.

eval 탭은 **Summary**로 열립니다. 상태, 지시문, 결과를 읽거나, Current에서는 verify가
할 일과 재사용할 결과를 확인하세요. 칩 아래에서 **Agent log**를 고르면 실시간 대화를,
**Runtime log**를 고르면 저장된 stdout/stderr를 볼 수 있습니다. 대화를 볼 수 없으면 이
머신에서 읽을 수 없는 이유를 알려 줍니다. **Debug**를 보려면 Summary로 돌아와 `t`를
누르세요. Artifact 탭과 Human 리뷰에서도 같습니다.

Run의 Human eval은 아래에서 설명하는 리뷰와 승인을 엽니다. 그 Run에서 선택하지 않은
eval에는 요청이 없으므로, 결과의 Summary, Human 컨트롤, Tool log 없이 섹션을 읽기 전용으로
보여 줍니다.

Agent log는 실시간으로 갱신됩니다. 초점이 있는 Agent log에서 ↑/↓ 또는 `j`/`k`는 행 단위로,
PgUp/PgDn은 페이지 단위로 이동하고, Home/End나 **Top/Bottom**은 양 끝으로 가며, 마우스 휠은
스크롤합니다. 위로 이동하면 자동 스크롤이 멈추고, Bottom으로 돌아가면 새 메시지에 맞춰 다시 자동으로 스크롤됩니다.
Space나 Enter로 화면에 보이는 묶음을 펼치거나 접을 수 있으며, 묶음 제목을 클릭해도
같습니다. 숨겨진 reasoning은 보이지 않고, 공개 reasoning 요약만 보입니다.

### ID로 요청 열기 {#requests-by-id}

Ctrl+O나 팔레트의 **Open a request by ID**를 쓰면 저장소에 관계없이
저장된 요청을 Main area에서 열 수 있습니다. 대기 중인 Human 리뷰는 Pane에서 열 때처럼 조작할 수 있고,
다른 요청은 읽기 전용이며 헤더 행의 제목에 read-only가 붙습니다. 완료된 Human 요청을 열면 Human 리뷰 화면이 읽기 전용으로 표시되며,
`t`를 누르면 **Debug**가 표시되거나 숨겨집니다. 로그가 있는 다른 읽기 전용 요청을 열면 eval의 탭에서처럼 Summary와 Agent log
또는 Runtime log를 전환해 볼 수 있고, Summary에서 `t`로 **Debug**를 표시하거나 숨길 수 있습니다. **Close**를 누르면 Pane으로 돌아갑니다. 다른 곳에서 일어난
요청의 변경도 화면에 반영되며, Ctrl+R이나 F5로 다시 열지 않고 다시 읽을 수 있습니다.

## Human 리뷰 {#human-reviews}

Pane에서 대기 중인 리뷰를 고르거나 ID로 열면 Main area에 Human
리뷰 화면이 표시되고, 그 위 헤더 행에 eval과 제목이 나옵니다. 브랜치나 타임라인 항목을 고르면 리뷰 상태를 유지한 채 그 화면으로 바뀝니다. 열린 양식은 초안을 유지하고, 실행 중인 리뷰 작업은 계속됩니다.
Esc는 한 번에 한 단계씩 되돌아갑니다. 실행 중인 작업을 취소하고, 다음에는 양식에서 나가고, 그다음에는
리뷰에서 나가 Pane으로 돌아갑니다(Pane이 숨겨져 있으면 먼저 보여 줍니다). Detail에서는 Detail을 닫습니다.

Human 리뷰는 **CLAIM** 단계로 열리며, 다른 사람이 맡았으면 그 이름을 보여 줍니다.
내가 맡으면 **REVIEW (yours)**로 바뀝니다. 지시문을 읽고 **Tools**를 사용한 뒤
**Tool log**를 확인하고 **Result**를 작성하세요. 기준이 길면 스크롤해 읽습니다. 넓은
창에서는 로그가 양식 옆에, 좁은 창이나 Detail 탭에서는 Result 위에 나옵니다.
**Debug**는 처음에 접혀 있습니다. `t`나 제목 줄을 누르면 요청 ID, 모드, 맡은 사람,
대기 시간을 보여 줍니다.

리뷰를 열기만 해서는 아무것도 맡지 않습니다. `c`나 **Claim to review**로 요청을 맡습니다. 텍스트 필드 밖에서 `u`를 누르거나 **Release claim**을 누르면 담당을 해제하며, 양식을 편집하는 중에도 초안을 남긴 채 해제할 수 있습니다. 다른 사람이 맡은 리뷰는 해제될 때까지 읽기 전용입니다. 담당 지정은 계정을 인증하는 기능이 아니라 리뷰어끼리 작업이 겹치지 않게 하는 잠금이며, 요청을 맡은 사람만 도구를 실행하거나 결과를 제출할 수 있습니다. Human 요청을 처리하는 공통 절차와 DEFERRED 상태의 승인 요청에 관한 설명은
[Human](../features/human.md)을 보세요.

도구는 그 도구를 선언한 Artifact별로 묶이며, 리뷰 대상 Artifact가 먼저 옵니다. 도구를 선언한 Artifact가 둘 이상일 때만 묶음에 Artifact 이름이 붙습니다.
도구마다 실행 표시로 시작하는 operation 버튼과 설명이 있으며, 실행할 수 없으면 그 이유도 표시됩니다. 요청을 맡기 전에는 버튼이 비활성화됩니다.
**클릭하거나, 초점이 있는 버튼에서 Enter나 Space를 누르면 바로 실행됩니다**. Tab으로 버튼 사이를 이동할 수 있습니다. launch 도구는 애플리케이션을 시작하자마자 호출을 마치며,
도구 실행에 실패하면 판정을 기록하지 않고 Result의 동작 행에 오류만 표시합니다. 도구를 실행하려면 현재 선언이 요청에
저장된 정의와 여전히 일치해야 합니다. 선언이 바뀌었으면 리뷰를 열거나, 도구를 실행하거나, 제출할 때
요청이 ERROR(INPUT_CHANGED)로 끝나며, 다음에 verify하면 리뷰를 다시 요청합니다.

이 리뷰에서 도구를 실행할 때마다 [Tool log](../features/tools/human.md#tool-log)에
실행 내역이 바로 추가되며, 시작 시각은 로컬 시간으로 표시됩니다. Tool log에서는 긴 줄이 화면 너비에 맞게 줄바꿈되고, 새 출력이 오면 최신 줄이 보이도록 자동으로 스크롤됩니다. 위로 스크롤해 읽으면 그 자리에 머물고,
맨 아래로 다시 스크롤하면 다시 자동으로 스크롤됩니다. 그 텍스트는 선택할 수 있는 일반
텍스트이므로, 도구가 출력한 내용을 결과에 복사하세요. 로그는 앱이 실행되는 동안 유지되며
이 창에서 제출하면 지워집니다. CLI나
다른 창에서 결과를 제출하면 이 창의 편집은 끝나지만 로그는 그대로 남습니다. 로그는 어디에도
저장되지 않습니다.

요청을 맡은 뒤 `g`나 **GREEN**을 누르면 GREEN 양식이, `r`이나 **RED**를 누르면 RED 양식이 열립니다. 각각 eval의
스키마로 만든 양식입니다([쓸 수 있는 형태](../features/declarations.md#human-result-schemas)).
`*`는 필수 필드를 표시하고, 비어 있는 선택 필드에는 **Optional**이 보이며, **Add**와 지우기 표시로 목록 항목과 그룹을 더하거나 지웁니다. Tab으로 컨트롤
사이를 이동하고 Ctrl+G/Ctrl+R이나 판정 버튼으로 판정을 바꿉니다. Result는 한 행으로 끝나며, 왼쪽에 **Release claim**, 오른쪽에 **Submit GREEN**이나 **Submit RED**가 있고, 그 사이에 실행 중인 작업이나 실패 내용이 나옵니다. Ctrl+S를 누르거나 **Submit**을 누르면
제출되며, 필드 안에서 Enter를 눌러서는 제출되지 않습니다. 스키마 오류가 나면 문제가 된 필드를
알려 주고 양식을 연 채로 두므로 고칠 수 있습니다. 판정마다 초안이 따로 남으며, 같은 창 안에서는 양식을 나가거나 다른
곳에서 같은 요청을 열어도 제출하기 전까지 유지됩니다. 입력기로 한국어 등을 입력하는 동안에는 단축키가
실행되지 않습니다. 백업을 복원하는 등으로 상태 디렉터리가 바뀌면 열린 리뷰가 닫히고, 그 초안과
Tool log는 버립니다.

완료된 결과는 화면에 열린 채로 남습니다. 결과를 **로컬에 저장한 뒤에도**
원격 게시에 실패할 수 있습니다. 그러면 양식이 닫히고, 결과는 로컬에 저장됐지만 원격 게시에는 실패했다는 알림이 나옵니다.
판정은 이미 기록되었으니, 다시 제출하지 말고 Settings의 **Push results**
(또는 `artifactize remote push`)로 게시를 다시 시도하세요.

### 나가기와 종료 {#leave-and-quit}

텍스트 필드 밖에서 `q`를 누르거나, Commands에서 **Quit**을 고르거나, 창을 닫으면 종료합니다. 키보드
초점이 편집이나 작업이 진행 중인 Human 리뷰에 있으면 `q`를 눌러도 종료되지 않으니, 먼저 그 작업을
마치거나 취소하세요. Settings가 열려 있을 때는 `q`가 동작하지 않습니다.

이 창에서 맡은 Human 요청 중 아직 제출하지도, 담당을 해제하지도 않은 요청이 있으면 그 요청을 알려 주고 먼저 묻습니다.

| 선택 | 키 | 담당한 요청 |
|---|---|---|
| **Stay** | Esc | 아무것도 바뀌지 않고 앱이 열린 채로 남습니다. |
| **Release and quit** | `u` | 담당을 해제하므로 다른 리뷰어가 맡을 수 있습니다. |

종료하면 아직 출력을 내고 있는 도구와 진행 중인 리뷰 작업을 취소하고, 제출하지 않은 초안은 버립니다. launch 도구가 연 프로그램은
계속 실행되니 직접 닫으세요.

## Settings {#settings}

Settings는 화면 위에 모달 창으로 열립니다. Esc를 누르거나 **Close**를 누르거나 **Settings**
버튼을 다시 누르면 닫히고, 아래 화면은 열기 전 그대로입니다. 열 때마다 계정, 키, 저장소 정보를
다시 불러오며, 팔레트의 **Refresh settings** 명령으로도 다시 불러올 수 있습니다. 탭은
**Providers**, **Remote**, **Limits** 세 개이며, 탭 목록에 초점이 있을 때 ←와 →로 차례로 오갈 수 있습니다.

**Providers** 탭 왼쪽에는 기본 provider(Codex, Claude Agent, OpenAI, Anthropic, Gemini)가 차례로 나오고, 그 아래에
직접 추가한 커스텀 provider의 이름과 **Custom provider**가 나옵니다. 하나를 고르면 오른쪽에 그
provider의 설정이 나오고, **List models**로 제공하는 모델을 볼 수 있습니다.

- **Codex:** 계정 상태를 항목별로 보여 줍니다. 로그인하지 않은 상태, artifactize가 저장한 토큰과 만료 시각, 읽기 전용
  `ARTIFACTIZE_CODEX_AUTH_FILE`, 또는 저장이 거부된 이유 중 하나입니다. 저장된 액세스 토큰이 만료되면 다음에 쓸 때 갱신되고, 외부 파일의 토큰이 만료되면 Codex에 다시 로그인해야 합니다.
  외부 파일 모드와 저장 거부 모드에서는 여기서 로그인할 수 없으며, 로그아웃은
  artifactize가 저장한 계정에서만 쓸 수 있습니다. **Sign in with Codex**는 브라우저로 로그인하며, 로그인하는 동안 계정 항목에 진행 중이라고 나오고 **Cancel sign-in**으로 멈출 수 있습니다.
  브라우저가 앱으로 돌아오지 못하면 **Finish by hand**를 펼치세요. 복사할 수 있는 로그인 주소와 리디렉션 URL을 넣을 칸, **Finish sign-in**이 나옵니다.
  포트 1455를 다른 프로세스가 사용 중이면 브라우저가 여기로 돌아올 수 없다고 알려 주니,
  페이지 로드에 실패한 뒤 브라우저 주소 표시줄에서 전체 `http://localhost:1455/auth/callback?code=…` URL을 복사해 붙여 넣으세요.
  **Sign out of Codex**는 로컬 토큰을 삭제하며, 서버에서 토큰이 폐기됐는지 확인할 수 없으면 경고합니다.
  로그인된 상태에서는 Commands의 **Sign in to Codex again**으로 로그아웃하지 않고 다시 로그인할 수 있습니다.
- **Claude Agent:** Claude Code의 로그인 상태를 Claude Code가 직접 알려 주는 그대로
  (`claude auth status`) 보여 줍니다. `PATH`에 `claude`가 없음, 로그인 안 됨, 로그인 방식과
  요금제(예: `claude.ai · max`)로 로그인됨, 또는 실패한 오류 중 하나입니다. 터미널에서 로그인한
  뒤처럼 상태를 다시 확인하려면 **Check again**을 누르세요. **Sign in with Claude Code**는
  `artifactize login claude-agent`처럼 `claude auth login`을 실행합니다. Claude Code가 브라우저를
  엽니다. 브라우저에서 마치세요. 브라우저가 열리지 않았거나 돌아오지 못하면 **Finish by hand**를 펼쳐 Claude Code가 출력한 URL과
  페이지에 나온 코드를 넣을 칸을 보고, 코드를 붙여 넣은 뒤 **Finish sign-in**을 누르세요. **Cancel sign-in**은 Claude Code를 멈춥니다.
  **Sign out of Claude Code**는 `claude auth logout`을 실행합니다. 어느 쪽이든 끝나면 상태를 다시
  보여 줍니다. 로그인 정보는 Claude Code가 보관하며, artifactize는 이를 읽거나 복사하거나 저장하지
  않습니다. **List models**를 누르면 Claude Code가 그 로그인으로 제공하는 모델이 나옵니다
  ([Claude Agent](../features/agent.md#claude-agent)).
- **OpenAI, Anthropic, Gemini:** 키가 환경 변수(그 이름과 함께)에서 오는지, 여기 저장되어 있는지, 없는지를 보여 주며,
  값 자체는 보여 주지 않습니다. 키를 붙여 넣고 **Save key**를 누르면 `artifactize login`처럼
  저장하고, **Remove saved key**로 지웁니다. 환경 변수가 설정되어 있으면 그 키를 먼저 씁니다. 이때
  함께 저장한 키는 보관되지만 쓰이지 않는다고 표시되며, 지울 수도 있습니다.
- **커스텀 provider:** **Custom provider**를 누르면 빈 양식이 열립니다. 이름을 정하고, 종류로 기본 provider의 별칭(그 provider의 로그인이나 키를 씀)과
  엔드포인트(API, Base URL, 키(선택 사항)) 중 하나를 고른 다음 **Save**를 누르세요. 필요한 만큼 추가할 수 있고, 목록에서 하나를 고르면 양식에서 바꾸거나
  **Remove provider**로 삭제할 수 있습니다. 엔드포인트에 저장한 키는 보여 주지 않습니다. 키 칸을 비워 두면
  저장된 키를 그대로 쓰고, **No key**를 체크하면 키 없이 저장합니다. eval에서는 `backend`에 이 이름을
  씁니다. [커스텀 provider](../features/agent.md#custom-providers)를 보세요.

**Remote** 탭에서는 리뷰 저장소 연결을 관리합니다. 로그인하지 않았을 때는 저장소 URL, 공유 수준, 토큰 칸과 **Sign in to the store**가 나옵니다. 로그인하면 저장소 URL과 공유 수준은 각각 항목으로, 현재 상태는 한 줄(로그인 상태와 리뷰어, 오늘 이 머신에서 푸시한 결과 수, 또는 푸시나 확인 결과)로 보여 주고, **Check status**, **Push (dry run)**, **Push results**, **Sign out of the store**가 나옵니다. 로그아웃하면 입력 칸이 다시 나옵니다. 저장소가 저장된 토큰을 거부하거나 저장소에 연결할 수 없으면 로그인 입력 칸이 저장소 URL, 문제 내용과 함께 다시 나오고, **Sign in to the store** 외에는 **Check status**만 있습니다. 유효한 토큰으로 다시 로그인하거나, 연결을 복구한 뒤 **Check status**를 고를 때까지 **Push results**는 없습니다. **Summary**는 호출 세부 사항,
캡처한 출력, 도구 호출 이력을 빼고, **Full**은 저장된 실행 기록도 보냅니다.
다만 Summary에도 리뷰 메타데이터, 추가 결과 필드, 선언된 Runtime 명령과
인자, Agent 실행 경로 메타데이터는 포함되어 공유됩니다. 그 선언에는 경로나 민감한 리터럴이 들어 있을 수 있으며, Summary를 골라도 이런 정보가 자동으로 가려지지는 않습니다. 팀에
필요한 개인정보 보호 수준에 맞춰 공유 수준을 고르세요. 로그아웃은 로컬 설정을 제거할 뿐, **서버
토큰은 제거하지 않습니다**. 저장소 관리자가 [그 토큰을 폐기](../cli/review-store.md#review-store-server)해야 합니다.
[공유와 푸시 동작](../cli/review-store.md#remote-review-store-client)을 보세요.

**Limits** 탭에서는 아무도 맡지 않은 승인만 남은 Run이 YELLOW로 종료되기까지의 대기 시간을 설정합니다. 초 단위 칸에 지금 적용 중인 값이 들어 있으며, 따로 바꾸지 않았다면 120입니다. 새 값(0–86400)을 넣고 **Save**를 누르면
그 뒤에 시작하는 Run에 적용됩니다. `artifactize limits`와 같은 머신 전체 [설정](../features/others/state.md#sign-off-idle-grace)입니다.

Settings에서 하는 계정과 저장소 관리 작업은 CLI와 같습니다. 자격 증명은
`.artf`에 넣지 않습니다.
