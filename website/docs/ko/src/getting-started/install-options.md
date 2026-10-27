# 다른 설치 방법 {#other-install-methods}

OS별 기본 설치 방법은 [설치](install.md)에 있습니다. 이 페이지에서는 각 설치 방법의 세부 절차와
대안을 설명합니다. 한 가지 방법으로만 설치하세요. 어떤 방법으로 설치하든 `artifactize`가
설치되며, 그 [내장 도구](../features/tools/builtin.md#artifactize-tools-cli)는
`artifactize tools`로 쓸 수 있습니다. Store 이외의 방법으로 설치하면 이 도구가
`artifactize-tools` 명령으로도 함께 설치됩니다. 사용 중인 시스템에 맞는 설치 파일이 릴리스에
아직 없다면 [Cargo](#cargo-all-platforms)로 설치하거나
[소스에서 빌드](#build-from-source-all-platforms)하세요.

| OS | 기본: 앱 | 명령줄 | 기타 |
|---|---|---|---|
| macOS | [디스크 이미지](#the-app) | [Homebrew 또는 설치 스크립트](#macos) | [Cargo](#cargo-all-platforms), [소스에서 빌드](#build-from-source-all-platforms) |
| Windows | [Microsoft Store](#the-app) | [winget 또는 PowerShell 설치 스크립트](#windows) | [Cargo](#cargo-all-platforms), [소스에서 빌드](#build-from-source-all-platforms) |
| Linux | [`.deb`](#the-app) (x86_64) | [설치 스크립트](#linux) | [AppImage](#desktop-app), [Cargo 바이너리](#cargo-all-platforms), [소스에서 빌드](#build-from-source-all-platforms) |

## 앱 {#the-app}

앱에는 `artifactize` 명령이 함께 들어 있습니다. 설치 스크립트와 Homebrew
cask도 앱을 함께 설치합니다. Homebrew formula, Cargo, 소스 빌드는
명령만 설치합니다.

**macOS.** 앱을 한 번 연 뒤에도 명령을 찾을 수 없다면 관리자 권한 요청을 승인하세요. 그러면
새 터미널에서 `artifactize`를 쓸 수 있습니다([자세히](#desktop-app)). Apple의 공증을 받은
디스크 이미지 하나로 macOS 11 이상의 Apple 실리콘 Mac과 Intel Mac을 모두 지원합니다.

**Windows.** Microsoft가 Store 패키지에 서명하므로 스마트 앱 컨트롤이 실행을 허용하고,
Store가 패키지를 최신 상태로 유지합니다. 시작 메뉴에서 **Artifactize**를 선택하면
[데스크톱 앱](#desktop-app)이 열립니다. Store 버전의
[시작 메뉴 항목](../features/others/platforms.md#desktop-console-launches)과
[Store 버전만 따로 쓰는 상태 저장 위치](../features/others/state.md#state)는 Windows에서 다르게
동작하는 부분과 함께 설명합니다.

**Linux.** `.deb`는 세 명령을 모두 `/usr/bin`에 설치하며, apt가 함께 설치하는
WebKitGTK 4.1과 GTK 3가 필요합니다. aarch64나 다른 배포판에서는 [설치 스크립트](#linux)를 쓰세요.

## 명령줄 {#command-line}

### Linux {#linux}

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
artifactize --version
```

이 [스크립트](https://artifactize.dev/install.sh)는 OS와 CPU에 맞는 최신 안정 릴리스를
`~/.local/bin`에 설치합니다. 내려받은 파일의 체크섬이 일치하지 않거나 바이너리를 실행할 수 없으면 스크립트는 기존 파일을 교체하지 않고 종료하므로, 이미 설치한 프로그램은 계속 사용할 수 있습니다. `curl`이나 `wget`을 쓰며, `sudo`를 쓰지 않고
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

### macOS {#macos}

[Homebrew](https://brew.sh/)는 macOS 패키지 관리자로, 명령줄 도구를
대신 설치하고 업데이트해 줍니다. 터미널에서 `brew`를 쓸 수 없다면 먼저
[Homebrew 설치 안내](https://docs.brew.sh/Installation)를 따르세요. 그다음 아래 명령을 실행하세요.

```sh
brew install lhj6102/tap/artifactize
artifactize --version
```

이 명령을 실행하면 프로젝트의 Homebrew 패키지 저장소(*tap*)인 `lhj6102/tap`에서 Mac의 CPU에 맞는 두 명령이
설치되며, 이후에는 안정 릴리스로만 업데이트됩니다. Homebrew는 이런 명령줄 패키지를 *formula*, 앱 패키지를
*cask*라고 부릅니다. 데스크톱 앱은 같은 tap의 cask로 제공됩니다. cask는
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

### Windows {#windows}

**터미널에서 Microsoft Store로 설치(기본).** 같은
[Store 패키지](install.md#download-the-app)를 winget으로 설치합니다.

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
스마트 앱 컨트롤이 켜진 머신에서는 이전 릴리스가 실행되었더라도 Windows가 새 릴리스를
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

## Cargo (모든 플랫폼) {#cargo-all-platforms}

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall)로 앞의 설치 방법과
같은 사전 빌드 바이너리를 `~/.cargo/bin`에 내려받습니다.

```sh
cargo binstall artifactize artifactize-tools
```

또는 crates.io에서 최신 안정 릴리스를 빌드합니다. 빌드하려면
[rustup](https://rustup.rs)으로 받은 Rust 최신 안정 버전과 C 컴파일러가 필요합니다(SQLite를 소스에서 빌드합니다).

```sh
cargo install artifactize artifactize-tools --locked
cargo install artifactize@<version> artifactize-tools@<version> --locked   # a particular release
```

`--locked`는 릴리스를 테스트할 때 쓴 의존성 버전으로 빌드합니다.
바이너리는 `~/.cargo/bin`에 설치되며, 이 디렉터리가 `PATH`에 있어야 합니다. `--root DIR`을 쓰면 대신
`DIR/bin`에 설치합니다. 두 cargo 명령 모두 macOS와 Windows에서도 동작합니다.

## 소스에서 빌드 (모든 플랫폼) {#build-from-source-all-platforms}

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

## 데스크톱 앱 {#desktop-app}

[데스크톱 앱](../desktop/index.md)은 현재 브랜치 목록과 각 브랜치의 Run, 지금 verify하면
실행할 작업, 대기 중인 Human 리뷰를 한 창에 보여 줍니다. 디스크 이미지나 AppImage로
실행하면 앱이 첫 실행 때 `artifactize` 명령을 설정합니다(아래 참고).

[내려받기](install.md#download-the-app)에서 설치하세요. 각 릴리스에는 AppImage
`artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage`도 첨부됩니다. `VERSION`은 릴리스 버전이며,
내려받은 파일 이름에 들어 있습니다. Linux의 시스템 C 라이브러리인 glibc 2.35 이상이 필요하며,
`ldd --version`을 실행하면 첫 줄에 설치된 버전이 나옵니다. 파일을 내려받은 폴더에서 아래 명령의 파일 이름을 실제 파일 이름으로 바꾼 뒤, 그 파일에 실행 권한을 주고 실행하세요.

```sh
chmod +x artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage
./artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage
```

각 파일 옆에는 `.sha256` 체크섬이 있습니다. `artifactize-desktop` 명령으로 앱을 실행하거나
응용 프로그램 메뉴 또는 시작 메뉴에서 **Artifactize**를 여세요.

앱은 첫 실행 때 터미널이 이미 `artifactize`를 찾을 수 있는지 확인합니다. Homebrew, 설치
스크립트, `.deb`, Store로 설치했다면 아무것도 바꾸지 않습니다. 그렇지 않으면 명령을 설정하며,
그다음 새 터미널을 열고 `artifactize --version`을 실행하면 됩니다.

- **macOS:** 터미널이 앱의 명령을 찾을 수 있도록 관리자 권한 확인 창에서 승인하세요.
  명령은 설치된 앱에서 실행되므로 앱을 업데이트하면 명령도 업데이트됩니다. 앱을 다른
  위치로 옮겼다면 한 번 더 실행해 명령을 다시 설정하세요. 권한 요청을 취소하면 실행할
  때마다 다시 묻지 않습니다.
- **Linux AppImage:** 앱이 명령을 `~/.local/bin`에 설치합니다. AppImage를 닫은 뒤에도
  명령은 계속 동작하며, 더 새 AppImage를 실행하면 명령도 업데이트됩니다. 앱이 만들지 않은
  파일은 그곳에서 교체하지 않습니다. 셸이 `~/.local/bin`을 검색하지 않는다면 셸 시작 파일에
  `export PATH="$HOME/.local/bin:$PATH"`를 추가하고 새 터미널을 여세요.

설정 오류는 기록만 하고 앱을 멈추지 않습니다. Windows 설치 프로그램, `.deb`,
`install.sh`가 풀어 놓은 Linux 앱은 이미 명령을 제공하므로 설정이 필요 없습니다.

### 소스에서 데스크톱 앱 빌드 {#build-the-desktop-app-from-source}

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
