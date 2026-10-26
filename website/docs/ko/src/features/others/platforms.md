# 운영체제 {#operating-systems}

모든 OS에서 공통으로 쓸 명령과 경로를 한 번만 선언하세요. artifactize는 그 선언을
Linux(x86_64와 aarch64), macOS(Apple 실리콘과 Intel), Windows(x64)에서 실행하며, WSL 2에서는
Linux 빌드가 실행됩니다. 리뷰하기 전에는 `artifactize tools check`가 프로그램을 찾을 수 없는
도구를 알려 줍니다. 리뷰 중에 Runtime eval의 프로그램을 시작하지 못하면 그 eval은 RED가 아니라 ERROR가 되고, 도구의 프로그램을 시작하지 못하면 도구 오류를 돌려주며 리뷰는 계속됩니다. 각
OS의 설치 채널은 [설치](../../getting-started/install.md)에 있습니다. 이 페이지는 OS마다 다른
점을 정리합니다.

## 프로젝트 명령은 선언된 그대로 실행 {#project-commands-run-as-declared}

Runtime eval, fingerprint 스크립트, 명령을 실행하는 Agent 도구와 Human 도구는 모든 OS에서
`command`와 `args`를 인자 목록 하나로 실행합니다. 셸을 거치지 않으며 OS별로 선언을 따로 작성하지도 않습니다.
리뷰어의 실행 환경에서 쓸 수 있는 명령을 프로젝트에 선언하세요. `python3`나 `grep`을 실행하는 eval이라면 모든
리뷰어의 `PATH`에 그 명령이 있어야 합니다. 파일을 읽고 검색하는 데는
[내장 도구](../tools/builtin.md)가 외부 프로그램 없이 어디서나 동작합니다. `open`은 OS가 제공하는
열기 프로그램을 씁니다. macOS에서는 `open`, Windows에서는 기본 애플리케이션, Linux와 WSL에서는
`xdg-open`이며, `xdg-open`은 설치되어 있어야 합니다.

명령은 [Runtime 실행](../declarations.md#runtime-execution)에서 설명한 대로 내 환경에서
실행됩니다. Windows에서는 다른 OS와 달리 시스템 변수도 전달합니다.

Linux에서 AppImage로 실행한 데스크톱 앱은 다른 프로그램을 시작할 때 앱이 시작될 때의 환경 변수를
전달하되, AppImage가 추가한 설정은 제거합니다. 직접 설정해 둔 변수를 AppImage가 덮어썼다면(`GTK_THEME` 등) 원래
값으로 되돌리지 않고 지웁니다. 명령에 그 변수가 필요하면 다시 설정하세요.

## 프로그램 찾기 {#program-lookup}

선언된 프로그램은 `tools check`, Runtime eval,
fingerprint 스크립트, 명령 도구에서 같은 방식으로 찾습니다.

- `python3` 같은 경로 없는 이름은 `PATH`에서 찾습니다. 상대 경로와 빈 `PATH`
  항목은 명령의 작업 디렉터리 기준으로 해석됩니다. 그래서 Unix에서 `PATH`가 설정되지 않았거나
  비어 있으면 명령의 작업 디렉터리에서 실행 파일을 찾습니다. Windows에서는 확장자 없는 이름에 `PATHEXT`의 각 확장자를 차례로 붙여 보므로,
  `npx`를 지정하면 `npx.cmd`도 찾습니다. `PATHEXT`가 없으면 순서는 `.COM;.EXE;.BAT;.CMD`입니다.
- `./tools/check`처럼 `/`가 들어간 상대 이름은
  폴더 Artifact의 허용된 범위 안에서 해석됩니다. 파일 Artifact에서는 상대 경로로 지정한 실행 파일을
  대상 파일이 있는 폴더 아래에서 찾으며, 같은 폴더의 다른 파일을 가리켜도 됩니다(예:
  `./inspect`). 두 경우 모두 `..`과 심볼릭 링크는 거부됩니다. 실행 파일을 이렇게 찾을 수 있다고 해서 그 파일을 내장 도구나 범위가 정해진 argv 참조의 대상으로 쓸 수 있는 것은 아닙니다.
- 절대 경로는 적힌 그대로 실행합니다.

내장 **help**는 다릅니다. 인자로 지정한 실행 파일은 일반적인 cwd/PATH 검색으로 찾으며, 절대 경로,
상위 폴더 기준 상대 경로, 실행 파일 심볼릭 링크도 받습니다.
하위 명령 인자는 범위가 정해진 파일 인자로 해석하지 않고 그대로 전달합니다.
help는 Artifact 범위 안의 파일을 읽는 작업에 한정되지 않고, 신뢰하는 프로그램을 실행합니다.

Windows 배치 파일(`.cmd`, `.bat`)도 명령으로 쓸 수 있지만, 모든 문자를 인자로 넘길 수는
없습니다. artifactize는 그런 인자를 바꿔서 넘기지 않고 거부합니다. 오류 메시지는
`Process arguments could not be passed safely`이고, 아무것도 실행되지 않습니다.

도구 세션은 모든 OS에서 같은 방식으로 멈추고 정리합니다. 세션을 끝낼 때 어떤 프로세스까지 정리하는지,
샌드박스 없이 실행하는 명령에서 무엇을 조심해야 하는지는 [도구 세션 계약](../tools/agent.md#tool-sessions)에서 설명합니다.

## 경로 {#paths}

선언, 도구 인자, 결과, 기록에 쓰는 논리적 경로의 구분자는 OS와 관계없이 `/`입니다.
artifactize는 모든 OS에서 경로를 비교할 때 대소문자를 구분합니다. Artifact
참조, 도구 경로, `.artfignore`, artifactsum `files` 패턴은
정확한 표기와 일치해야 합니다.

디스크의 표기를 확인하는 방식은 OS마다 다릅니다.

| OS | 디스크의 표기 |
|---|---|
| macOS, Windows | artifactize가 강제합니다. 대소문자를 구분하지 않는 볼륨(둘 다 기본값)에서는 경로의 대소문자 표기가 디스크에 저장된 파일·폴더 이름과 정확히 일치해야 하며, 자식 Artifact와 대소문자만 다른 이름의 파일이나 폴더는 읽지 않고 거부합니다. |
| Linux | artifactize는 파일 시스템에 맡깁니다. 대소문자를 구분하는 파일 시스템(보통의 경우)에서는 디스크에 기록된 이름과 정확히 일치하는 경로로만 열 수 있고, 대소문자를 구분하지 않는 파일 시스템(casefold 폴더나 WSL에 마운트한 Windows 드라이브 등)에서는 대소문자만 다른 경로로도 같은 항목을 열 수 있습니다. |

## 파일과 브라우저 열기 {#opening-files-and-the-browser}

내장 [`open`](../tools/builtin.md#tools) 도구와 `login codex` 로그인은
데스크톱의 기본 애플리케이션으로 대상을 엽니다.

| OS | 여는 프로그램 |
|---|---|
| Linux | `xdg-open` |
| WSL | Linux 배포판 안의 `xdg-open` |
| macOS | `open` |
| Windows | 대상에 지정된 기본 애플리케이션 |

WSL은 Linux처럼 동작합니다. `wslview`나 Windows 프로그램으로 대체하지 않습니다.
`xdg-open`이 없거나 아무것도 열지 않으면 배포판의 `xdg-utils`를 설치하고
기본 애플리케이션을 설정하세요. WSL에서 `xdg-open`은 `explorer.exe`를 통해 파일을 여는데,
`explorer.exe`는 파일을 연 뒤에도 실패를 보고하므로, WSL에서는 `xdg-open`이
종료 상태 4를 돌려줘도 성공으로 처리합니다. `login codex`는 어떤 브라우저에서든 쓸 수 있도록 로그인 URL도 출력합니다.

## 데스크톱에서 콘솔 실행 {#desktop-console-launches}

Windows에서 시작 메뉴나 탐색기로 인자 없이 `artifactize`를 실행하면, 창이 바로 닫히지 않도록
도움말과 터미널 안내를 보여 준 뒤 Enter를 누를 때까지 창을 열어 둡니다. 인자가 있거나, 직접 연 터미널이나
스크립트에서 실행하면 멈추지 않습니다. Linux와 macOS에서는 인자 없이 실행하면 평소처럼 도움말을 보여 주고 바로 종료합니다.

Microsoft Store 패키지를 설치하면 항목 두 개가 표시됩니다. 데스크톱 앱을 여는 **Artifactize**와 도움말을 여는 **Artifactize CLI**이며, 명령 별칭은
`artifactize` 하나입니다. 도구 명령은 `artifactize tools`로 쓸 수 있습니다. Store 이외의 방법으로
설치하면 독립 실행형 `artifactize-tools` 명령도 함께 설치됩니다.

## 상태 {#state}

기본 상태 디렉터리는 OS마다 다릅니다. Windows에서는 `%LOCALAPPDATA%\artifactize`,
Linux와 macOS에서는 `~/.local/state/artifactize`입니다. Store 버전은
패키지 전용 상태를 따로 유지하며 패키지를 제거하면 Windows가 이 상태를 지웁니다. WSL은 별도의 Linux
상태를 유지합니다. 전체 우선순위와 Windows 오버레이 주의 사항은
[상태](state.md#state)를 보세요.
