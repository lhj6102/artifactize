# 가이드 (모델 불필요) {#guide-no-model-needed}

이 가이드에서는 저장소에 포함된 예제 프로젝트로 실습합니다. 저장소를 아직 로컬에 복제하지 않았다면, 먼저 버전 관리 프로그램인 **Git**으로 내려받으세요
(`git --version`이 응답하지 않으면 [Git 받기](https://git-scm.com/downloads)).
`git clone`으로 저장소를 새 `artifactize` 폴더에 내려받은 다음, `cd`로 그 폴더에 들어가세요.

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
```

예제는 Python으로 된 eval을 실행하는 데 `python3`를, 텍스트를 검색하는 데 `grep`을 쓰므로 두 명령
모두 터미널에서 쓸 수 있어야 합니다. `PATH`는 터미널이 명령을 찾는 디렉터리 목록입니다.
터미널이 두 명령을 찾는지 확인하세요.

```sh
python3 --version
grep --help
```

Python 버전과 grep 도움말이 출력되어야 합니다. 둘 중 하나라도 명령을 찾을 수 없다는 오류가
나오면, 그 도구를 설치하거나 도구가 설치된 디렉터리를 `PATH`에 추가한 뒤 계속하세요.
artifactize는 모든 OS에서 프로젝트에 선언된 명령을 적힌 그대로 실행합니다.
[운영체제](../features/others/platforms.md)를 보세요.

리뷰할 작업 단위마다 **Artifact**를 선언합니다. Artifact의 `index.artf`는 텍스트 설정 파일이며, 여기에 **eval**이라는 리뷰 항목을 나열합니다. 이 파일은 읽기 쉬운 `key = value` 형식의 **TOML**로 작성합니다. **fingerprint**는 리뷰된 입력을 식별합니다. 내장 **artifactsum**은 파일 내용을 해시해 변경 여부를 감지합니다.

artifactize는 Run과 결과를 상태 디렉터리에 저장하며, 이 기록을 **상태**(state)라고 합니다.
이 실습의 리뷰 기록은 평소 쓰는 상태 디렉터리와 다른 디렉터리에 따로 두세요. POSIX 셸(예를 들어
Bash나 zsh)에서 아래 첫 줄은 `mktemp -d`로 새 임시 디렉터리를 만들고
artifactize가 그곳에 상태를 저장하도록 합니다. `export`는 이 터미널에서 실행하는
명령에도 그 설정을 넘겨 줍니다. 그다음 예제로 들어가 선언을 확인하세요.

```sh
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)
cd examples/runtime-relations
artifactize config check
artifactize status
artifactize config graph
```

PowerShell에서는 첫 줄을
`$env:ARTIFACTIZE_STATE_HOME = (New-Item -ItemType Directory (Join-Path $env:TEMP (New-Guid))).FullName`으로 바꾸세요.
이 명령은 Windows 임시 디렉터리 아래에 고유한 이름의 디렉터리를 만들고
그 경로를 위와 같은 환경 변수에 저장합니다. `artifactize` 명령은 그대로입니다.

`config check`는 프로젝트 프로그램을 실행하지 않고 선언을 검증합니다.
`status`는 eval 두 개가 실행되고, 세 번째 eval은 앞선 결과가 나올 때까지 실행을 미룬다고 보여 줍니다.
프로젝트에 필요한 eval이 아직 모두 통과하지 않았으므로 종료 코드는 1입니다. `config graph`는
Artifact들과 그 입력 관계를 보여 줍니다. 이제 세 eval을 모두 실행하고
결과를 기다리세요.

```sh
artifactize verify --all --wait
```

세 결과 모두 GREEN이고 종료 코드는 0(성공)이어야 합니다. 출력에는
저장된 Run의 Run ID가 `run-Abc123` 같은 형태로 나옵니다. 다음 명령에서는 `RUN_ID`를
그대로 입력하지 말고 **화면에 출력된 Run ID**를 넣으세요.

```sh
artifactize run show RUN_ID
```

이 명령은 저장된 Run을 JSON(구조화된 텍스트 기록) 형식으로 출력합니다. `argv`는
프로그램에 전달된 인자 목록이고, `stdout`은 그 프로그램이 출력한 텍스트입니다.
기록에는 리뷰된 입력을 식별하는 fingerprint도 들어 있습니다.

같은 eval을 다시 실행하세요.

```sh
artifactize verify --all --wait
artifactize status
```

이번에는 아무것도 실행되지 않습니다. 세 결과가 모두 재사용됩니다. 두 명령 모두 0으로 종료하고,
status는 재사용할 수 있는 결과를 알려 줍니다. 이후 단계에서도 같은 임시 상태
디렉터리를 쓰도록 이 터미널을 열어 두세요.

[runtime-relations README](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations)는 eval이 RED가 되는 상황을 만드는 방법도 보여 줍니다. 각 폴더는
그 안의 `index.artf` 파일에서 Artifact로 선언됩니다. `fingerprint`를 생략하면 파일 변경 여부를 확인하는 내장 artifactsum이 쓰입니다. 재사용을 끄려면 `fingerprint = false`로 설정하세요. snake_case 키와 JSON/출력 이름의
구분은 [선언 (.artf)](../features/declarations.md)을 보세요.

## 파일과 eval 의존성 {#files-and-eval-dependencies}

`hero.png.artf` 같은 사이드카는 옆에 있는 파일을 독립된
Artifact로 선언합니다. 사이드카에는 `name`이 필요하고, 폴더와 같은 TOML 스키마를 쓰며, 기본적으로
그 파일만 해시합니다. 내장 도구는 대상 파일과 eval에 명시적으로 제공한 입력에만
접근할 수 있으며, 같은 폴더의 다른 파일에는 접근할 수 없습니다. 공용 검사기처럼 다른 Artifact를 쓰는 eval은
그 Artifact를 `{house-style}`처럼 이름으로 가리킵니다.
[eval 의존성](../features/artifact.md#eval-dependencies)을 보세요. [posts 예제](https://github.com/lhj6102/artifactize/tree/main/examples/posts)는
Markdown 사이드카 세 개와 공용 스타일 검사기 하나를 씁니다. 이전
예제의 디렉터리에서 다음을 실행하세요.

```sh
cd ../posts
artifactize config check   # four Artifacts, three evals
artifactize verify --all --wait   # three GREEN results
artifactize verify tip/style --wait # qualified file eval; reused
artifactize status         # all three post reviews are reused
cd ../runtime-relations    # continue the tour below
```

파일 Artifact의 기본 fingerprint는 그 파일 하나만 해시해 계산합니다. 참조된 `house-style` Artifact가
검사기와 금지어를 담고 있으므로, 공용 부분이 바뀌면 세 글 모두 기존 리뷰의
[재사용 키](../features/fingerprint.md#the-reuse-key)가 무효가 됩니다. 재사용 키는
검사 기준과 리뷰한 입력을 구분하는 값이며, 이 값을 기준으로 이전 결과를 계속 쓸 수 있는지 판단합니다.
이 예제의 README는 글 하나를 RED로 만들고 나머지 두 개는 재사용하는 방법을 보여 줍니다.

Runtime eval은 지시문이나 인자에서 입력을 직접 참조합니다. 예를 들어
애플리케이션의 eval은 `{e2e-tests}`를 입력으로 삼아, 실제로 동작하는 애플리케이션을 검사하는 종단 간(E2E) 테스트 모음을 실행합니다. 테스트 코드
Artifact의 eval은 코드 스타일을 리뷰하고, 테스트가 각 기능을 충분히 검증하는지도 확인합니다. 그 애플리케이션 eval만
테스트 코드 리뷰가 끝난 뒤에 실행되고, 관련 없는 애플리케이션 eval은 그동안에도 실행할 수 있습니다.
[eval 의존성](../features/artifact.md#eval-dependencies)을 보세요.

## 선택 실습: Agent 도구와 Human 리뷰 절차 {#optional-agent-tools-and-the-human-flow}

앞선 실습에는 모델이 필요 없지만, 이 선택 실습을 진행하려면 모델이 필요합니다. 먼저
[agent-tools README](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools)에 설명된 대로
`examples/agent-tools/spec/index.artf`에서 쓰는 프로필에 정확한 모델 ID를 넣고 자격 증명을 설정하세요.
다른 백엔드를 쓰려면 `verify`에 `--profile anthropic` 또는 `--profile codex`를 덧붙입니다.
자격 증명이 없으면 리뷰는 ERROR로 기록되고 대기 명령은 2로 종료합니다.
Human eval은 같은 Artifact의 Human이 아닌 다른 eval이 모두
GREEN이 된 뒤에만 요청되므로 `spec/review`가 먼저 실행됩니다. 두 터미널은 같은 임시 상태 디렉터리를 써야 합니다. 아래의
대기 명령을 시작하기 전에 첫 번째 터미널의 값을
`echo "$ARTIFACTIZE_STATE_HOME"`(PowerShell: `$env:ARTIFACTIZE_STATE_HOME`)으로 출력하세요.
두 번째 터미널에서는 아래 `STATE_DIRECTORY`를 출력된 그 경로로 바꾸세요.
`RUN_ID`에는 대기 명령이 출력한 새 Run ID를 쓰고, `REQUEST_ID`는
`request list`가 출력한 대기 중인 요청 식별자로 바꾸세요.
PowerShell에서는 `export` 대신 `$env:ARTIFACTIZE_STATE_HOME = "STATE_DIRECTORY"`를
쓰세요. 그다음 같은 예제 디렉터리를 여세요.

```sh
cd ../agent-tools
artifactize tools check     # static: schemas, executables and scoped paths
artifactize tools check --execute --eval spec/review --tool coverage_spec --args '{}'
artifactize verify --eval spec/review --wait    # the machine review comes first
artifactize verify --eval spec/signoff --wait   # records a sign-off request and waits
# In a second terminal, in the same directory:
export ARTIFACTIZE_STATE_HOME="STATE_DIRECTORY"  # copy the first terminal's path
artifactize request list --run RUN_ID    # shows REQUEST_ID
artifactize request claim REQUEST_ID
artifactize request tool REQUEST_ID notes_spec
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true,"comment":"Both questions are answered."}'
artifactize run show RUN_ID
```

`claim`, `tool`, `submit`은 `--reviewer NAME`을 넘기지 않으면 내 사용자 이름(`$USER`)을 리뷰어로
씁니다. `request unclaim REQUEST_ID`는 담당을 해제합니다. `artifactize
review`는 같은 일을 터미널 UI에서 합니다. 대기 중인 요청을 나열하고
지시문을 미리 보여 줍니다. Enter를 누르면 Detail이 열립니다. `c`를 눌러 요청을 직접 맡고, Tab으로 도구에 초점을 옮긴 뒤 Enter를 누르거나 클릭하면 도구 하나가 바로 실행됩니다. `g`/`r`을 누르면 GREEN/RED 입력 칸이 열리고, Ctrl-S를 누르면 제출됩니다. Esc는 목록으로 돌아가기 전에 편집을 멈추며 초안은 유지됩니다.
옵션 없는 `verify`는 Run을 제출하고 바로 끝납니다. `verify --wait`는 Run이 끝날 때까지 포그라운드에서 진행 상황을 표시합니다. 승인만
남았을 때 담당자가 2분 안에 요청을 맡으면, 그 담당자가 승인을 맡고 있는 동안 Run이 계속 열려 있습니다. 앞선
자동 리뷰가 여전히 유효하면 GREEN 승인이 제출되는 즉시 `verify --wait`가 종료 코드 0으로 끝납니다. Run이
이미 YELLOW(종료 코드 5)로 끝난 뒤에 요청을 맡더라도, 승인을 제출하면 다음 `verify`에서 그 승인을 재사용합니다.

## Monitor {#monitor}

```sh
artifactize monitor .      # Initially select this repository
artifactize monitor        # Initially select ALL repositories in this state
```

Run이 실행되는 동안 두 번째 터미널에서 시작하세요. Enter나 →로
Scope에서 Run과 트리를 거쳐 eval의 Detail까지 이동하고, Esc로 한 단계 돌아갑니다. `j`/`k`로 항목을 고르고, `?`로 도움말을 확인하고, `q`로 종료하세요. Detail은 Agent 대화, Runtime
로그, 또는 eval을 실행하기 전에 충족해야 할 조건을 보여 줍니다. 화면을 살펴보기만 해서는 프로젝트에 선언된 명령이 실행되지 않습니다.

Human 요청 담당 지정, 도구, 판정 양식은
[Human 리뷰와 Monitor](../tui/index.md#monitor)를 보세요.

## 팀 리뷰 저장소 (선택) {#team-review-store-optional}

팀 리뷰 저장소는 재사용할 수 있는 판정을 여러 머신과 CI가 공유하게 해 줍니다. 각 머신은
토큰으로 로그인합니다. 토큰은 어떤 동작이 허용되는지 정하는 비밀 값입니다. CI가 승인을 만들 수 없도록
CI에는 읽기 권한만 주세요. [팀 가이드](../features/team/index.md)와
[워크스루](../features/team/walkthrough.md)에서 서버 설정, 토큰, 그리고
두 번째 머신에서 같은 리뷰를 시도하는 방법을 다룹니다.

## 다음 단계 {#next-steps}

[예제](examples.md)는 각 기능을 작은 프로젝트로 보여 주고,
[Artifact와 eval](../features/artifact.md)은 방금 실행한 선언을
설명합니다.
