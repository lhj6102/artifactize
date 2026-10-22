# 운영체제 {#operating-systems}

artifactize는 Linux(x86_64와 aarch64), macOS(Apple 실리콘과 Intel),
Windows(x64)에서 실행됩니다. WSL 2에서는 Linux 빌드가 실행됩니다. 각 OS의 설치 채널은
[설치](../../getting-started/install.md)에 있습니다. 프로젝트, 그 선언, 그 리뷰는
모든 OS에서 같으며, 이 페이지는 다른 점을 정리합니다.

## 프로젝트 명령은 선언된 그대로 실행 {#project-commands-run-as-declared}

Runtime eval, fingerprint 스크립트, 명령 Agent 도구, 명령 Human 도구는 모든 OS에서
`command`를 `args`와 함께 인자 벡터 하나로 실행합니다. 셸도 없고,
선언의 OS별 변형도 없습니다. 프로젝트에는 리뷰를 실행하는 환경에 있는 명령을 선언하세요.
`python3`나 `grep`을 실행하는 eval이라면 모든 리뷰어의 `PATH`에 그 명령이 있어야 합니다.
파일을 읽고, 검색하고, 여는 데는 [내장 도구](../tools/builtin.md)가
외부 프로그램 없이 어디서나 동작합니다.

Runtime eval, fingerprint 스크립트, 명령 Agent 도구는 작성자의 환경에서도
실행됩니다. `artifactize`가 받은 환경 변수 전체(각 OS의 홈과 캐시 변수, Windows에서는
시스템 변수까지)를 그대로 넘겨주므로, 버전 관리자로 설치한 툴체인도
터미널에서와 같은 방식으로 찾을 수 있습니다. `ARTIFACTIZE_*` 변수만 빠지고, 실행마다
전용 `TMPDIR`(`TMP`와 `TEMP`도)를 받습니다.
[runtime 환경](../declarations.md#runtime-execution-library)을 보세요. 명령 Human 도구는
`ARTIFACTIZE_*` 변수를 포함한 리뷰어의 환경 전체를 물려받으며,
전용 임시 디렉터리나 출력 디렉터리를 받지 않습니다([Human 도구](../tools/human.md)).

Linux에서 AppImage로(직접 또는 `install.sh`의
실행 파일을 통해) 시작한 데스크톱 앱은, 번들에 든 라이브러리를 불러오도록
번들의 디렉터리를 `PATH`, `LD_LIBRARY_PATH` 같은 목록의 앞에 두고
번들의 변수와 함께 실행됩니다. 앱이 시작하는 모든 프로그램은 앱이 시작될 때의 환경을 받습니다.
그 항목과 변수는 빠지므로, 시스템 프로그램은 자기 라이브러리를 불러옵니다.

## 프로그램 찾기 {#program-lookup}

선언된 프로그램은 `tools check`, runtime eval,
fingerprint 스크립트, 명령 도구에서 같은 방식으로 찾습니다.

- `python3` 같은 경로 없는 이름은 `PATH`에서 찾습니다. 상대 경로와 빈 `PATH`
  항목은 명령의 작업 디렉터리 기준으로 해석됩니다. 그래서 Unix에서는 `PATH`가 설정되지 않았거나
  비어 있으면 그 디렉터리를 찾습니다. Windows에서는 확장자 없는 이름에 `PATHEXT`의 각 확장자를 차례로 붙여 보므로,
  `npx`가 `npx.cmd`를 찾습니다. `PATHEXT`가 없으면 순서는 `.COM;.EXE;.BAT;.CMD`입니다.
- `./tools/check`처럼 `/`가 들어간 상대 이름은
  폴더 Artifact의 eval 범위 안에서 해석됩니다. 파일 Artifact에서는 상대 경로 실행 파일을
  들어 있는 폴더 아래에서 해석하며, 관련 없는 형제 파일을 가리켜도 됩니다(예:
  `./inspect`). 두 경우 모두 `..`과 심볼릭 링크는 거부됩니다. 이 실행 파일 찾기 때문에
  형제 파일이 내장 도구나 범위 지정 argv 참조에 보이게 되지는 않습니다.
- 절대 경로는 적힌 그대로 실행합니다.

내장 **help**는 다릅니다. 그 리터럴 실행 파일은 절대 경로, 상위 경로 기준 상대 경로,
실행 파일 심볼릭 링크를 포함해 일반적인 cwd/PATH 검색을 씁니다.
하위 명령 인자는 범위 지정 파일 피연산자로 해석하지 않고 그대로 전달합니다.
help는 Artifact 파일에 갇힌 작업이 아니라 신뢰하는 프로그램을 실행합니다.

Windows 배치 파일(`.cmd`, `.bat`)은 셸 문자열 없이 바로 실행됩니다. Rust
표준 라이브러리가 각 인자를 `cmd.exe`용으로 따옴표 처리하며, 안전하게 전달할 수 없는 인자는
거부합니다. 오류 메시지는 `Process arguments could not be passed safely`이고,
아무것도 실행되지 않습니다.

도구 세션은 Unix에서는 프로세스 그룹을, Windows에서는 닫으면 종료되는 Job을 씁니다.
정리와 복구는 Artifactize가 맡으며, 플랫폼 공통
[도구 세션 계약](../tools/agent.md#tool-sessions)이 그 보장과
신뢰하는 명령의 한계를 설명합니다.

## 경로 {#paths}

논리적 경로는 선언, 도구 인자, 결과, 기록에서 모든 OS에 걸쳐 `/`를 씁니다.
Artifactize는 모든 OS에서 경로를 대소문자를 구분해 맞춥니다. Artifact
참조, 도구 경로, `.artfignore`, artifactsum `files` 패턴은
정확한 표기와 일치해야 합니다.

디스크의 표기를 확인하는 방식은 OS마다 다릅니다.

| OS | 디스크의 표기 |
|---|---|
| macOS, Windows | Artifactize가 강제합니다. 대소문자를 구분하지 않는 볼륨(둘 다 기본값)에서는 경로가 디스크 항목의 정확한 표기를 써야 하며, 자식 Artifact와 대소문자만 다른 이름의 파일이나 폴더는 읽지 않고 거부합니다. |
| Linux | Artifactize는 파일 시스템에 맡깁니다. 대소문자를 구분하는 파일 시스템(보통의 경우)은 정확한 표기만 열고, 대소문자를 구분하지 않는 파일 시스템(casefold 폴더나 WSL에 마운트한 Windows 드라이브 등)에서는 대소문자만 다른 경로가 같은 항목을 열 수 있습니다. |

## 파일과 브라우저 열기 {#opening-files-and-the-browser}

내장 [`open`](../tools/builtin.md#tools) 도구와 `login codex` 로그인은
데스크톱의 기본 애플리케이션으로 대상을 엽니다.

| OS | 여는 프로그램 |
|---|---|
| Linux | `xdg-open` |
| WSL | Linux 배포판 안의 `xdg-open` |
| macOS | `open` |
| Windows | Windows 셸(ShellExecute) |

WSL은 Linux처럼 동작합니다. `wslview`나 Windows 프로그램으로 대체하지 않습니다.
`xdg-open`이 없거나 아무것도 열지 않으면 배포판의 `xdg-utils`를 설치하고
기본 애플리케이션을 설정하세요. WSL에서 `xdg-open`은 `explorer.exe`를 통해 파일을 여는데,
`explorer.exe`는 파일을 연 뒤에도 실패를 보고하므로, WSL에서 artifactize는
`xdg-open`의 종료 상태 4도 받아들입니다. `login codex`는 어떤 브라우저에서든 쓸 수 있도록 로그인 URL도 출력합니다.

## 데스크톱에서 콘솔 실행 {#desktop-console-launches}

Windows에서 시작 메뉴나 탐색기로 인자 없이 `artifactize`를 실행하면,
이 프로세스가 콘솔에 연결된 유일한 프로세스이고 stdin과 stdout이 모두 콘솔일 때
도움말과 터미널 안내를 보여 준 뒤 Enter를 기다립니다. 인자가 있거나,
기존 터미널에서 실행했거나, 입출력을 리디렉션했으면 결코 멈추지 않습니다. Linux와
macOS는 평소처럼 인자 없이 도움말을 보여 주고 바로 종료합니다.

Microsoft Store 패키지에는 보이는 항목이 두 개 있습니다. 데스크톱
앱용 **Artifactize**와 이 도움말 실행용 **Artifactize CLI**이며, 명령 별칭은
`artifactize` 하나입니다. 그 도구는 `artifactize tools`로 쓸 수 있습니다. Store가 아닌 채널은
독립 실행형 `artifactize-tools` 명령도 함께 제공합니다.

## 상태 {#state}

기본 상태 디렉터리는 OS마다 다릅니다. Windows에서는 `%LOCALAPPDATA%\artifactize`,
Linux와 macOS에서는 `~/.local/state/artifactize`입니다. Store 버전은
패키지 전용 상태를 따로 유지하며 Windows가 제거 시 이를 지웁니다. WSL은 별도의 Linux
상태를 유지합니다. 전체 우선순위와 Windows 오버레이 주의 사항은
[상태](state.md#state)를 보세요.
