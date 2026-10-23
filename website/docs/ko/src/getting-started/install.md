# artifactize 설치 {#install-artifactize}

아래 표에서 OS를 고르고 한 가지 방법으로 설치한 다음, 새 터미널을 열고 `artifactize --version`을
실행하세요. `artifactize 0.15.0`처럼 설치된 버전이 출력됩니다.

artifactize는 [Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE)으로
공개된 오픈 소스입니다.

사용 중인 시스템에 맞는 설치 파일이 릴리스에 아직 없다면 Cargo로 설치하거나
[소스에서 빌드](#build-from-source-all-platforms)하세요.

## 사전 요구 사항 {#prerequisites}

- Linux(x86_64 또는 aarch64), macOS(Apple 실리콘 또는 Intel), 또는 Windows(x64). WSL 2에서는
  Linux 빌드가 실행됩니다.
- 예제 프로젝트는 Python 검사를 실행하는 데 `python3`를, 텍스트를 검색하는 데 `grep`을 씁니다.
  두 명령 모두 터미널에서 쓸 수 있어야 합니다. artifactize는 모든 OS에서
  프로젝트에 선언된 명령을 적힌 그대로 실행합니다. [운영체제](../features/others/platforms.md)를 보세요.
- Agent eval을 쓰려면 백엔드마다 `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, Codex가 포함된 ChatGPT 요금제 중 필요한 것을 준비하세요(선택 사항). Runtime eval과 Human eval에는 이런 인증 정보가 필요하지 않습니다.

`PATH`는 터미널이 명령을 찾는 디렉터리 목록입니다.
예제를 시작하기 전에 필요한 도구를 찾을 수 있는지 확인하세요.

```sh
python3 --version
grep --help
```

Python 버전과 grep 도움말이 출력되어야 합니다. 둘 중 하나라도 명령을 찾을 수 없다는 오류가
나오면, 그 도구를 설치하거나 도구가 설치된 디렉터리를 `PATH`에 추가한 뒤 계속하세요.

Rust 툴체인은 `cargo install`에만 필요합니다.
[rustup](https://rustup.rs)으로 받은 Rust 최신 안정 버전과 C 컴파일러가 필요합니다(SQLite를 소스에서 빌드합니다).

## 설치 {#install}

OS를 고르고 한 가지 방법으로만 설치하세요. 어떤 방법으로 설치하든 `artifactize`가 설치되며, 그
[내장 도구](../features/tools/builtin.md#artifactize-tools-cli)는 `artifactize tools`로 쓸 수
있습니다. Store 이외의 방법으로 설치하면 이 도구가 `artifactize-tools` 명령으로도 함께 설치됩니다.

먼저 OS에 맞는 앱을 내려받으세요. 명령줄이 더 편하거나
앱이 시스템에 맞지 않는다면 다른 방법을 고르세요.

| OS | 기본: 앱 | 명령줄 | 기타 |
|---|---|---|---|
| macOS | [디스크 이미지](#download-the-app) | [Homebrew 또는 설치 스크립트](#macos) | [Cargo](#cargo-all-platforms), [소스에서 빌드](#build-from-source-all-platforms) |
| Windows | [Microsoft Store](#download-the-app) | [winget 또는 PowerShell 설치 스크립트](#windows) | [Cargo](#cargo-all-platforms), [소스에서 빌드](#build-from-source-all-platforms) |
| Linux | [`.deb`](#download-the-app) (x86_64) | [설치 스크립트](#linux) | [AppImage](#desktop-app), [Cargo 바이너리](#cargo-all-platforms), [소스에서 빌드](#build-from-source-all-platforms) |

앱에는 `artifactize` 명령이 함께 들어 있습니다. 설치 스크립트와 Homebrew
cask도 앱을 함께 설치합니다. Homebrew formula, Cargo, 소스 빌드는
명령만 설치합니다.

### 앱 내려받기 {#download-the-app}

**macOS.**
[Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg)를 내려받아
열고 **Artifactize**를 응용 프로그램 폴더로 끌어다 놓은 뒤 앱을 한 번 여세요.
명령을 찾을 수 없다면 관리자 권한 요청을 승인한 뒤 새 터미널에서 `artifactize`를 쓰세요
([자세히](#desktop-app)). Apple의 공증을 받은 이 디스크 이미지 하나로
macOS 11 이상의 Apple 실리콘 Mac과 Intel Mac을 모두 지원합니다.

**Windows.**
[Microsoft Store 페이지](https://apps.microsoft.com/detail/9PB6W4LL165D)(x64)에서 Artifactize를 설치한 뒤
새 터미널을 열고 `artifactize --version`을 실행하세요. Microsoft가 Store 패키지에 서명하므로
스마트 앱 컨트롤이 실행을 허용하고, Store가 패키지를 최신 상태로 유지합니다. 시작 메뉴에서 **Artifactize**를 선택하면 [데스크톱 앱](#desktop-app)이 열립니다. Store 버전의
[시작 메뉴 항목](../features/others/platforms.md#desktop-console-launches)과
[별도로 유지하는 상태](../features/others/state.md#state)는 Windows에서 달라지는 다른 동작과 함께
설명합니다.

**Linux.** x86_64 Ubuntu 22.04, Debian 12 이상에서는
[Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb)를 내려받아
소프트웨어 설치 프로그램으로 열거나, 다음을 실행하세요.

```sh
sudo apt install ./Artifactize-amd64.deb
artifactize --version
```

세 명령을 모두 `/usr/bin`에 설치하며, apt가 함께 설치하는 WebKitGTK 4.1과 GTK 3가
필요합니다. aarch64나 다른 배포판에서는 [설치 스크립트](#linux)를 쓰세요.

### 명령줄 {#command-line}

대신 터미널에서 설치하려면 OS별 기본 명령을 쓰세요.

#### Linux {#linux}

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version
```

이 [스크립트](https://artifactize.dev/install.sh)는 GitHub의 최신 안정 릴리스에서 OS와
CPU에 맞는 압축 파일을 고르고, 그 SHA-256을 릴리스의 체크섬과 비교해
일치하지 않으면 설치하지 않습니다. 기존 파일을 교체하기 전에 새 바이너리 두 개가 실행되는지
확인한 뒤 `~/.local/bin`에 설치합니다. `curl`이나 `wget`을 쓰며, `sudo`를 쓰지 않고
셸 시작 파일도 고치지 않습니다. `~/.local/bin`이 `PATH`에 없다면
추가할 줄을 출력합니다. 대신 실행될 수 있는 다른 `artifactize`나 `artifactize-tools`가
`PATH`에 있으면 경고도 합니다. 대부분의 x86_64 Linux 배포판(Ubuntu, Debian, Fedora 등)에서는 같은
릴리스의 [데스크톱 앱](#desktop-app)도 같은 방식으로 확인해
`~/.local/share/artifactize/desktop`에 설치하고, `artifactize` 명령과 같은 디렉터리에 `artifactize-desktop` 실행 파일을
추가하며, 애플리케이션 메뉴에 **Artifactize** 항목을 만듭니다. ARM Linux와 Alpine에서는 명령만 설치합니다. 두
환경 변수로 동작을 바꿀 수 있습니다.
사전 릴리스는 `ARTIFACTIZE_VERSION`으로 그 버전을 지정했을 때만 설치됩니다.

```sh
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_VERSION=0.5.2 sh             # this release
curl -fsSL https://artifactize.dev/install.sh | ARTIFACTIZE_INSTALL_DIR="$HOME/bin" sh   # this directory
```

#### macOS {#macos}

[Homebrew](https://brew.sh/)는 macOS 패키지 관리자로, 명령줄 도구를
대신 설치하고 업데이트해 줍니다. 터미널에서 `brew`를 쓸 수 없다면 먼저
[Homebrew 설치 안내](https://docs.brew.sh/Installation)를 따르세요. 그다음 아래 명령을 실행하세요.

```sh
brew install lhj6102/tap/artifactize
artifactize --version
```

`lhj6102/tap` Homebrew tap의 formula는 Mac의 CPU에 맞는 두 명령을 설치하며
안정 릴리스로만 업데이트됩니다. 데스크톱 앱은 같은 tap의 cask로 제공됩니다. cask는
앱에 들어 있는 `artifactize`와 `artifactize-tools`를 Homebrew의 PATH에 연결하므로, 이 명령 하나로 앱과 두 CLI 명령을 모두 설치할 수 있습니다. cask가 formula를 대신하므로, formula가 설치되어 있다면 먼저 제거하세요.

```sh
brew install --cask lhj6102/tap/artifactize
```

macOS에서도 설치 스크립트는 Linux와 같은 확인 과정을 거치고 같은 환경
변수를 씁니다. 명령은 `~/.local/bin`에 설치되는데, 새 Mac에서는 이 디렉터리가
`PATH`에 없습니다. 스크립트가 `~/.zshrc` 같은 셸 시작 파일에 추가할 줄을
출력합니다. 데스크톱 앱도 `~/Applications/Artifactize.app`으로 설치합니다.

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

#### Windows {#windows}

**터미널에서 Microsoft Store로 설치(기본).** 같은
[Store 패키지](#download-the-app)를 winget으로 설치합니다.

```powershell
winget install 9PB6W4LL165D --source msstore
artifactize --version   # in a new terminal
```

**PowerShell 설치 스크립트.** Windows PowerShell 5.1 또는 PowerShell 7에서
관리자 권한 없이 실행합니다.

```powershell
irm https://artifactize.dev/install.ps1 | iex
artifactize --version   # in a new terminal
```

이 [스크립트](https://artifactize.dev/install.ps1)는 zip의 SHA-256을 확인하고 새
바이너리 두 개를 실행해 본 다음, `artifactize.exe`, `artifactize-tools.exe`, 데스크톱
앱의 `artifactize-desktop.exe`를 `%LOCALAPPDATA%\Programs\artifactize`에 설치합니다. 그리고 그
디렉터리를 사용자 `Path`에 추가하고 앱의 **Artifactize** 시작 메뉴 바로 가기를 추가합니다.
설치 후에는 새 터미널을 여세요. 쓸 수 있는 환경 변수는 `install.sh`와 같습니다.

**스마트 앱 컨트롤.** 설치 스크립트가 쓰는 릴리스 zip은 코드 서명이 되어 있지 않습니다.
스마트 앱 컨트롤이 켜진 컴퓨터에서는 이전 릴리스가 실행되었더라도 Windows가 새 릴리스를
차단할 수 있습니다. Windows가 새 바이너리를 차단하면 스크립트는 기존 파일을 하나도
교체하지 않고 멈춥니다. 이전 `artifactize`와 `artifactize-tools`는 설치된 채로
실행할 수 있고, `Path`도 바뀌지 않습니다. 애플리케이션 제어
정책을 우회하지 말고 Microsoft Store에서 설치하세요.

**winget 자체 패키지**는 아직 없습니다. `lhj6102.Artifactize` 패키지가
[winget](https://learn.microsoft.com/windows/package-manager/winget/)
저장소에 아직 등록되지 않았습니다. 위의 Store 패키지나 설치 스크립트를 쓰세요.

**WSL.** WSL 2 안에서는 artifactize의 Linux 빌드를 씁니다. 배포판 안에
[Linux](#linux) 안내대로 설치하세요. Windows 설치와는 별개이며
상태도 따로 저장합니다. 내장 `open`과 `login codex`는 Windows 프로그램이 아니라
배포판의 `xdg-open`을 씁니다.
[파일과 브라우저 열기](../features/others/platforms.md#opening-files-and-the-browser)를 보세요.

### Cargo (모든 플랫폼) {#cargo-all-platforms}

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall)로 앞의 설치 방법과
같은 사전 빌드 바이너리를 `~/.cargo/bin`에 내려받습니다.

```sh
cargo binstall artifactize artifactize-tools
```

또는 crates.io에서 최신 안정 릴리스를 빌드합니다.

```sh
cargo install artifactize artifactize-tools --locked
cargo install artifactize@<version> artifactize-tools@<version> --locked   # a particular release
```

`--locked`는 릴리스를 테스트할 때 쓴 의존성 버전으로 빌드합니다.
바이너리는 `~/.cargo/bin`에 설치되며, 이 디렉터리가 `PATH`에 있어야 합니다. `--root DIR`을 쓰면 대신
`DIR/bin`에 설치합니다. 두 cargo 명령 모두 macOS와 Windows에서도 동작합니다.

### 소스에서 빌드 (모든 플랫폼) {#build-from-source-all-platforms}

소스에서 빌드하려면 저장소를 복제한 뒤 그 디렉터리에서 다음 명령으로 설치하세요. 복제한 저장소의 `rust-toolchain.toml`에 따라
Rust 안정 버전이 쓰입니다. 예제 프로젝트와 [빠른 시작](guide.md)을
따라 하려면 복제한 저장소가 필요합니다.

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
cargo install --path crates/artifactize-tools --locked
```

릴리스 빌드는 태그에서 버전을 가져오며, 복제한 저장소에서 빌드하면 임시 버전 `0.0.0-dev`가
출력됩니다.

### 데스크톱 앱 {#desktop-app}

[데스크톱 앱](../desktop/index.md)은 현재 브랜치 목록과 각 브랜치의 Run, 지금 verify하면
실행할 작업, 대기 중인 Human 리뷰를 한 창에 보여 줍니다. 디스크 이미지나 AppImage로
실행하면 앱이 첫 실행 때 `artifactize` 명령을 설정합니다(아래 참고).

[내려받기](#download-the-app)에서 설치하세요. 각 릴리스에는 AppImage
`artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage`도 첨부됩니다. AppImage에
실행 권한을 주고 실행하세요. glibc 2.35 이상이 필요합니다.

각 파일 옆에는 `.sha256` 체크섬이 있습니다. `artifactize-desktop` 명령으로 앱을 실행하거나
응용 프로그램 메뉴 또는 시작 메뉴에서 **Artifactize**를 여세요.

앱은 첫 실행 때 터미널이 이미 `artifactize`를 찾을 수 있는지 확인합니다. Homebrew, 설치
스크립트, `.deb`, Store로 설치했다면 아무것도 바꾸지 않습니다. 그렇지 않으면 명령을 설정하며,
그다음 새 터미널을 열고 `artifactize --version`을 실행하면 됩니다.

- **macOS:** 앱이 시스템 관리자 권한 확인 창을 통해 앱 내부 `Contents/MacOS` 디렉터리의 절대 경로를
  `/etc/paths.d/artifactize`에 기록합니다. 그다음 새
  터미널을 열고 `artifactize --version`을 실행하세요. 두 명령 모두
  설치된 앱에서 실행되므로 앱을 업데이트하면 명령도 업데이트됩니다. 앱을 옮겼다면
  다시 실행해, 사라진 이전 위치를 가리키는 항목을 고치세요. 권한 요청을
  취소하면 앱이 설치된 위치별로 취소 여부를 앱 상태에 저장하므로, 같은 위치에서 다시
  실행할 때는 묻지 않습니다.
- **Linux AppImage:** 명령은
  `${XDG_DATA_HOME:-~/.local/share}/artifactize/bin`에 복사되고 `~/.local/bin`에서 링크됩니다.
  임시 AppImage 마운트가 사라진 뒤에도 계속 동작하며, 더
  새 AppImage를 실행하면 앱의 복사본이 갱신됩니다. 앱이 만들지 않은 `~/.local/bin`의
  파일이나 링크는 절대 교체하지 않습니다. 셸이 아직
  `~/.local/bin`을 검색하지 않는다면 셸 시작 파일에 `export PATH="$HOME/.local/bin:$PATH"`를 추가하고
  새 터미널을 여세요. 앱은 셸 시작 파일을 고치지 않습니다.

설정 오류는 기록만 하고 앱을 멈추지 않습니다. Windows 설치 프로그램, `.deb`,
`install.sh`가 풀어 놓은 Linux 앱은 이미 명령을 제공하므로 설정이 필요 없습니다.

#### 소스에서 데스크톱 앱 빌드 {#build-the-desktop-app-from-source}

위에서 설명한 소스 빌드에 필요한 Rust 도구 외에 Node.js 24,
`crates/artifactize-desktop/ui/package.json`에 적힌 버전의 pnpm, 그리고
OS별로 [Tauri 빌드에 필요한 도구와 라이브러리](https://v2.tauri.app/start/prerequisites/)를 설치하세요.
Linux에는 WebKitGTK 4.1, GTK 3, librsvg 개발 패키지가 필요하고, macOS에는
Xcode 명령줄 도구가, Windows에는 C++ 빌드 도구와 WebView2가 필요합니다.
저장소 루트에서 실행하세요.

```sh
pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile
pnpm -C crates/artifactize-desktop/ui build
cargo install --path crates/artifactize-desktop --locked --bin artifactize-desktop
artifactize-desktop
```

이 순서대로, Cargo로 설치하기 전에 프런트엔드를 빌드하세요.
앞에서처럼 같은 저장소에서 CLI도 설치하세요. 앱은
`artifactize verify`가 시작한 Run을 보여 줍니다. 앱은 Store 앱의 전용 상태가 아니라
패키지되지 않은 CLI와 같은 상태 디렉터리와 자격 증명을 씁니다.

### Claude Code와 Codex용 플러그인 {#plugin-for-claude-code-and-codex}

[플러그인](../agent-plugin/plugin.md)은 Claude Code나 Codex에 Hey Arti 스킬과
artifactize MCP 서버를 추가합니다. 먼저 위 설치 방법 중 하나로 `artifactize` 프로그램을
설치한 다음
[플러그인 페이지](../agent-plugin/plugin.md#add-the-plugin-to-your-assistant)를 따르세요.

## 업데이트 {#update}

| 설치 방법 | 업데이트 |
|---|---|
| `install.sh` 또는 `install.ps1` | 같은 명령을 다시 실행하세요. 데스크톱 앱도 함께 업데이트됩니다 |
| Homebrew | `brew upgrade artifactize`. 데스크톱 앱은 `brew upgrade --cask artifactize` |
| Microsoft Store | 자동. 또는 Store 앱의 **라이브러리 → 업데이트 받기** |
| 디스크 이미지, `.deb` 또는 AppImage | 릴리스의 더 새 파일을 같은 방법으로 설치하세요 |
| `cargo binstall` | `cargo binstall artifactize artifactize-tools` |
| `cargo install` | `cargo install artifactize artifactize-tools --locked`. 체크아웃에서는 `git pull` 후 `cargo install --path`를 반복하세요 |

설치 스크립트나 Cargo를 쓰면 릴리스가 게시되는 즉시 그 버전을 설치할 수 있습니다. Homebrew와
Microsoft Store는 안정 릴리스만 제공하며, Microsoft가 검토하는 동안 Store 업데이트가
늦어질 수 있습니다. 업데이트해도 상태는 유지됩니다.

플러그인 업데이트 방법은 [플러그인 페이지](../agent-plugin/plugin.md#updates)에 있습니다.

### 0.11에서 업그레이드 {#upgrading-from-011}

저장된 리뷰 데이터의 형식(스키마)은 처음 사용할 때 스키마 6~8에서 현재 스키마 9로 자동으로
업그레이드되며, 기존 Run, 캐시 기록, Human 승인은 그대로 남습니다. 이전 버전의 artifactize는
업그레이드된 상태를 열 수 없으니, 버전을 바꾸기 전에 백업해 두세요.

이제 `artifactize verify`는 터미널을 닫아도 계속 실행되는 Run을 제출하고, Run ID를 출력한 뒤 접수되면 종료합니다.
검증 종료 코드가 필요한 스크립트와 CI에는 `--wait`를 추가하세요. 터미널을 닫거나
지켜보는 명령을 중단해도 Run은 계속 실행됩니다. Run을 취소하려면
`artifactize run cancel RUN_ID`를 실행하세요. 포그라운드 Human 제한 시간과
`verify --timeout-ms`는 제거되었습니다. `--request-key`를 쓰면 재시도해도 같은 Run이 반환됩니다.

게이트 우회(`review_policy.dependency_gates`를 포함한 `--ignore-gates`와 `review_policy`)는
제거되었습니다. 빈 `review_policy` 테이블도 설정 검사에 실패하니 지우세요. 요청마다 고르는
`--speculate`와 `--defer-human`이 이를 대신하며, 둘 다 기본으로 꺼져 있고 어느 쪽도 최종 승인을
면제하지 않습니다. 승인을 미루면 Run은 `SIGNOFFS_PENDING`으로 끝나고 `--wait`는 5로 종료합니다. [Run과 status](../features/run.md)를 보세요.
인자 없이 실행한 `artifactize monitor`는 ALL에서 시작합니다. `monitor .`, 위치 인자 경로, 또는 `--repo`는
처음에 그 저장소를 선택합니다.

### 0.9로 업그레이드 {#upgrading-to-09}

선언을 폴더에는 TOML `index.artf`로, 파일에는 `<file>.artf`로 다시 작성하세요. 키는
snake_case이고 eval은 `[evals.<id>]` 테이블입니다. `.artfignore`가 예전 무시 파일을
대신합니다. 변환기는 없습니다. 에이전트에게 파일을 다시 작성해 커밋하게 하고,
예전 표시를 제거한 뒤 `artifactize config check`를 실행하세요. Family는 제거되었습니다. 템플릿이
필요하다면 직접 만든 생성기를 쓰고 그 `.artf` 출력을 커밋하세요. [선언 (.artf)](../features/declarations.md)을 보세요.

0.9 이전의 상태(스키마 1~5)는 거부합니다. `ARTIFACTIZE_STATE_HOME`이나 `--state-dir`을 새
디렉터리로 설정하거나, 이전 디렉터리를 다른 곳으로 옮기세요. 그 뒤의 상태는 자동으로
업그레이드됩니다([상태](../features/others/state.md#state)).
재사용 키 v2는 0.9 이전의 어떤 결과와도 일치하지 않습니다.
팀 리뷰 저장소에 있거나 스크립트 fingerprint를 쓰는 경우도 마찬가지입니다. 업그레이드 후 처음으로 전체 `verify`를
실행하면 Human 승인을 포함해 모든 eval을 한 번씩 리뷰합니다. [0.9로 업그레이드](../features/fingerprint.md#upgrading-to-09)를 보세요.

이제 Monitor는 eval의 Detail을 모달이 아니라 전용 패널에서 엽니다. 독립 실행형
Review도 같은 Human Detail을 씁니다. 요청은 직접 맡아야 하며, 도구를 실행하거나
결과를 제출해도 요청을 맡은 것으로 처리되지 않습니다. 판정 양식, 제출 키, 편집기 단축키가 바뀌었습니다.
현재 조작 방법은 [Human 리뷰와 Monitor](../features/human.md)를 보세요.

0.5.0은 `chatgpt`와 `claude` Agent 백엔드를 `login chatgpt`,
`logout chatgpt`, 내부 `mcp` 명령과 함께 제거하고, ChatGPT 요금제
사용자를 위한 `codex`를 추가합니다. `config check`는 제거된 백엔드를 아직 쓰는
eval이나 프로필 변형을 찾아 대신 쓸 백엔드를 안내합니다. artifactize는 이전 릴리스가 저장한 ChatGPT
토큰을 더 이상 쓰지 않습니다. ChatGPT 설정에서 artifactize 연결을 끊은 다음
상태 디렉터리에서 `auth/chatgpt.json`, `auth/chatgpt-registration.json`, `auth/chatgpt.lock`을
삭제하세요.

## 상태 {#state}

Run, 저장된 Agent 대화, 로그인을 포함한 모든 상태는 한
디렉터리에 있습니다. 기본값은 `~/.local/state/artifactize`(Windows에서는 `%LOCALAPPDATA%\artifactize`)입니다.
[상태](../features/others/state.md#state)에 그 내용,
상태 위치를 옮기는 환경 변수, Store 버전의 전용 사본이 정리되어 있습니다.

`--state-dir PATH`는 그 명령 하나에서만 상태 위치 전체를 바꿉니다. `login`, `verify`, `request`, `run`,
`monitor`에 같은 값을 쓰세요. 상태는 리뷰 대상 저장소 밖에 있어야 합니다.
`auth/` 아래의 토큰도 git 워크트리나
artifactize 프로젝트 안에는 둘 수 없습니다. `$HOME`에 있는 dotfiles 저장소나 체크아웃 안의 gitignore된 폴더가 그런 예입니다.
`ARTIFACTIZE_CODEX_AUTH_FILE`을 쓰면 토큰을 따로 저장할 곳이 필요 없습니다
([상태](../features/others/state.md#state)).

## 백엔드 {#backends}

Agent eval에는 백엔드 하나와 사용할 모델의 정확한 ID를 지정하세요. artifactize는
모델 목록을 따로 제공하지 않으며, 지정한 백엔드나 모델을 다른 것으로 바꾸지도 않습니다.

**백엔드**는 artifactize가 Agent 리뷰를 모델 서비스로 보낼 때 쓰는
연결입니다. **API 키**는 그 서비스에서 계정을 인증할 때 쓰는 비공개 인증 정보입니다.
API 키 백엔드를 쓰려면 [OpenAI 계정](https://platform.openai.com/api-keys)이나
[Anthropic Console](https://console.anthropic.com/settings/keys)에서 키를 만든 다음, 해당 명령의
`...`를 그 키로 바꾸세요. 키를 커밋하거나 Artifact 선언에 넣지 마세요.
Codex는 대신 아래의 별도 브라우저 로그인을 씁니다.

| 백엔드 | 설정 | 모델 목록 |
|---|---|---|
| `openai` | `export OPENAI_API_KEY=...` | `artifactize models openai` |
| `anthropic` | `export ANTHROPIC_API_KEY=...` | `artifactize models anthropic` |
| `codex` | `artifactize login codex` | `artifactize models codex` |

PowerShell에서는 `$env:OPENAI_API_KEY = "..."`로 키를 설정하세요.

`login codex`는 Codex CLI와 같은 방식으로 ChatGPT 계정에 로그인합니다.
로그인 URL을 출력하고 기본 브라우저로 엽니다(Linux와 WSL에서는 `xdg-open`, macOS에서는 `open`,
Windows에서는 기본 브라우저). 로그인은 같은 머신의 브라우저에서 마치세요.
로그인이 끝나면 브라우저가 `localhost:1455`로 이동합니다. WSL 2에서는 Windows 브라우저도 쓸 수 있습니다. 아무것도 열리지 않으면
출력된 URL을 브라우저에 복사하세요. Codex CLI나 앱 로그인 같은 다른 프로그램이 포트 1455를 쓰고
있으면, artifactize는 주소를 붙여 넣으라고 요청합니다. 다른 머신에서 로그인할 때도 URL을 붙여 넣는 방법을 쓰세요.
`http://localhost:1455/auth/callback?code=`로 시작하는 전체 URL을
터미널이나 데스크톱 로그인의 붙여 넣기 칸에 넣으세요. 로그인은 5분 안에 마쳐야 하며, 그 뒤에는
`login codex`를 다시 실행하세요. 페이지 로드에 실패했다면 브라우저 주소 표시줄에서,
페이지가 계속 로딩 중이라면 개발자 도구 → Network에서 이 URL을 찾을 수 있습니다.
`logout codex`는 artifactize의 토큰을 삭제하고
서버에 토큰 폐기를 요청합니다. 폐기가 확인되지 않으면 경고합니다. 대신 기존 Codex 로그인의 토큰을 쓰려면
`ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json`을 설정하세요. artifactize는 그 파일을 읽기만 합니다.
[Codex](../features/agent.md#codex)를 보세요.

```sh
artifactize doctor                 # state and its schema, API-key presence, Codex sign-in, review store, desktop app
artifactize doctor --repo PROJECT  # also validates PROJECT's declarations
```

`doctor`는 어떤 provider도 호출하지 않고 Run도 만들지 않습니다. 모든 백엔드가 선택 사항이므로
키나 로그인이 없으면 경고로 처리합니다. 데스크톱 앱이 없을 때도 설치 링크와 함께 경고만 합니다. 쓸 수 없는 상태나 잘못된 설정 같은
심각한 오류가 있을 때만 1로 종료합니다.

## 정리와 제거 {#cleanup-and-uninstall}

```sh
artifactize prune --dry-run           # list removable scratch output of finished Runs
artifactize prune --older-than 7d     # remove it; saved results stay readable
artifactize logout codex              # if you signed in with Codex
artifactize remote logout             # if you signed in to a review store
```

그다음 설치한 방법대로 명령을 제거하세요.

```sh
rm ~/.local/bin/artifactize ~/.local/bin/artifactize-tools   # install.sh (or your ARTIFACTIZE_INSTALL_DIR)
brew uninstall artifactize                                   # Homebrew
cargo uninstall artifactize artifactize-tools                # cargo install or cargo binstall
```

데스크톱 앱도 제거합니다.

```sh
rm -rf ~/Applications/Artifactize.app                        # install.sh on macOS
rm -rf ~/.local/share/artifactize/desktop ~/.local/bin/artifactize-desktop \
  ~/.local/share/applications/artifactize.desktop             # install.sh on Linux
brew uninstall --cask artifactize                            # Homebrew cask, with its commands
sudo apt remove artifactize                                  # .deb
```

디스크 이미지로 설치했다면 응용 프로그램 폴더의 **Artifactize**를 휴지통으로 끌어다 놓으세요.
첫 실행 때 명령 설정을 승인했다면 그 터미널 경로 항목도 제거하세요.

```sh
sudo rm -f /etc/paths.d/artifactize
```

AppImage를 단독으로 실행했다면 AppImage를 삭제하세요. 앱의 고정 복사본을 가리키는
명령 링크만 제거한 다음, 그 복사본을 제거하세요.

```sh
bin="${XDG_DATA_HOME:-$HOME/.local/share}/artifactize/bin"
for command in artifactize artifactize-tools; do
  link="$HOME/.local/bin/$command"
  [ "$(readlink "$link")" != "$bin/$command" ] || rm "$link"
done
rm -rf "$bin"
```

명령의 경로 항목을 제거한 뒤에는 새 터미널을 여세요.

Windows에서는:

```powershell
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\artifactize"   # install.ps1
Remove-Item "$([Environment]::GetFolderPath('Programs'))\Artifactize.lnk"   # its Start menu shortcut
```

Store 버전은 **설정 → 앱 → 설치된 앱**에서 제거하거나, 시작 메뉴에서
Artifactize를 마우스 오른쪽 버튼으로 클릭해 제거하세요. `install.ps1` 설치를 제거한 뒤에는
그 디렉터리를 사용자 `Path`에서도 제거하세요(시작 메뉴에서 "계정의 환경 변수 편집"을
검색하세요).

**Store 앱**을 제거하면 Run, 저장된 결과, 로그인, 캐시를 포함한
패키지 전용 상태가 제거됩니다. 다른 설치 방법은 상태 디렉터리를 남겨 두므로,
나중에 패키지되지 않은 버전을 설치하면 그 Run과 결과를 다시 볼 수 있습니다. `prune`은 활성 Run,
데이터베이스 행, 저장소를 건드리지 않습니다. 패키지되지 않은 버전이 남긴 상태를 지우려면
`artifactize doctor`가 첫 줄에 출력하는 상태 디렉터리를 삭제하세요.

제거해도 프로젝트 파일, 내보낸 파일, `--state-dir`이나 환경 변수로 경로를 지정해 AppData 밖에 명시적으로 둔 상태는 지워지지 않습니다. eval의 명령이 만든 파일은 AppData 안에 있더라도 남으니,
더 이상 필요하지 않으면 따로 지우세요. 가상화되지 않은 기존
AppData 파일도 Store 패키지의 정리 대상이 아닙니다.
[Windows 동작과 오버레이 주의 사항](../features/others/state.md#state)을 보세요.

다음으로 [빠른 시작](guide.md)을 해 보세요. 모델이 필요 없는 5분짜리 둘러보기입니다.
