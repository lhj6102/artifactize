# 설치 {#installation}

Artifactize는 [Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE)으로 공개되어 있으며 Linux(`x86_64`, `aarch64`, WSL 2 포함), macOS(Apple silicon, Intel), Windows(`x64`)에서 실행됩니다. 한 가지 설치 방법을 골라 설치하세요. 새 터미널에서 `artifactize --version`을 실행해 버전을 확인합니다. 이어서 [5분 만에 첫 프로젝트 만들어 보기](guide.md)로 이동하세요.

## 1. 데스크톱 앱 또는 CLI 설치 {#1-install-the-app-or-cli}

어떤 방법으로 설치하든 `artifactize` 명령과 내장 도구(`artifactize tools`, Microsoft Store 외 채널에서는 `artifactize-tools`도 포함)가 함께 설치됩니다.

### 데스크톱 앱 다운로드 {#download-the-desktop-app}

데스크톱 앱에는 `artifactize` CLI가 함께 들어 있습니다.

- **macOS**: [Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg)(Apple silicon·Intel 공용)를 다운로드해 **Artifactize**를 응용 프로그램 폴더로 옮긴 뒤 한 번 실행하세요. 권한을 묻는 창에서 허용하면 터미널에서 `artifactize` 명령을 쓸 수 있습니다.
- **Windows**: [Microsoft Store의 Artifactize](https://apps.microsoft.com/detail/9PB6W4LL165D)(`x64`)를 설치한 뒤 새 터미널을 여세요.
- **Linux**: `x86_64` Ubuntu 22.04 또는 Debian 12 이상에서는 [Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb)를 다운로드해 `sudo apt install ./Artifactize-amd64.deb`로 설치하세요.

### 명령줄에서 설치하기 {#or-install-from-the-command-line}

- **Linux** (`x86_64`, `aarch64`, WSL 2; `sudo` 없이 `~/.local/bin`에 설치하며, `x86_64` glibc 환경에서는 데스크톱 앱도 함께 설치):
  ```sh
  curl -fsSL https://artifactize.dev/install.sh | sh
  ```
- **[Homebrew](https://brew.sh/)를 쓰는 macOS**:
  ```sh
  brew install lhj6102/tap/artifactize          # CLI만 설치
  brew install --cask lhj6102/tap/artifactize   # 데스크톱 앱과 CLI 함께 설치
  ```
  Homebrew가 없다면 `curl -fsSL https://artifactize.dev/install.sh | sh`를 실행하세요(`~/.local/bin`에 CLI를, `~/Applications/Artifactize.app`에 데스크톱 앱을 설치합니다).
- **Windows**:
  ```powershell
  winget install 9PB6W4LL165D --source msstore  # Microsoft Store 패키지 (코드 서명 포함)
  irm https://artifactize.dev/install.ps1 | iex # %LOCALAPPDATA%\Programs\artifactize에 설치
  ```
  Windows Smart App Control이 서명되지 않은 PowerShell 설치 파일을 차단하면 Microsoft Store 패키지를 설치하세요.

### 기타 설치 방법 (AppImage, Cargo, 소스 빌드) {#other-channels-appimage-cargo-source}

- **Linux AppImage**: 릴리스 페이지에서 `artifactize-desktop-vVERSION-x86_64-unknown-linux-gnu.AppImage`를 다운로드해 `chmod +x`로 실행 권한을 주고 실행하세요. 앱을 처음 실행하면 `~/.local/bin`에 `artifactize` 명령용 심볼릭 링크를 만듭니다.
- **설치 스크립트 옵션**: `ARTIFACTIZE_VERSION=0.23.0` 또는 `ARTIFACTIZE_INSTALL_DIR="$HOME/bin"` 환경 변수를 지정해 특정 버전이나 설치 경로를 고를 수 있습니다.
- **Cargo**: `cargo binstall artifactize artifactize-tools` 또는 `cargo install artifactize artifactize-tools --locked`를 실행합니다.
- **소스에서 빌드**:
  ```sh
  git clone https://github.com/lhj6102/artifactize && cd artifactize
  cargo install --path crates/artifactize --locked
  cargo install --path crates/artifactize-tools --locked
  ```
  `artifactize-desktop`까지 소스에서 빌드하려면 `pnpm -C crates/artifactize-desktop/ui install --frozen-lockfile && pnpm -C crates/artifactize-desktop/ui build`를 먼저 실행한 뒤 `cargo install --path crates/artifactize-desktop --locked --bin artifactize-desktop`을 실행하세요.

## 2. AI 백엔드 연결 (선택 사항) {#2-connect-ai-backends-optional}

Runtime eval과 Human eval에는 모델 서비스가 필요하지 않으므로 바로 [5분 만에 첫 프로젝트 만들어 보기](guide.md)로 넘어가도 됩니다. Agent eval을 쓸 때는 해당 eval에 지정한 백엔드만 터미널 명령이나 데스크톱 앱의 **Settings → Providers**에서 연결하세요.

| 백엔드 | 로그인 및 API 키 설정 | 모델 목록 확인 |
|---|---|---|
| `codex` | `artifactize login codex` (브라우저에서 ChatGPT 계정으로 로그인, 또는 `ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json` 지정) | `artifactize models codex` |
| `claude-agent` | `artifactize login claude-agent` (`PATH`의 `claude` 2.1.248 이상에서 `claude auth login` 실행) | `artifactize models claude-agent` |
| `openai` | `artifactize login openai` (표준 입력에 키 붙여 넣기) 또는 `OPENAI_API_KEY` 설정 | `artifactize models openai` |
| `anthropic` | `artifactize login anthropic` 또는 `ANTHROPIC_API_KEY` 설정 | `artifactize models anthropic` |
| `gemini` | `artifactize login gemini` 또는 `GEMINI_API_KEY` 설정 | `artifactize models gemini` |

`artifactize doctor`를 실행하면 상태 디렉터리, 자격 증명, 데스크톱 앱 설치 여부를 한 번에 점검합니다. 새 릴리스가 있으면 함께 알려 줍니다. 사용자 지정 프로바이더와 `logout`은 [Agent eval](../concepts/evals.md#agent-evals)을 참고하세요.

### Chat 전용 Antigravity {#antigravity-for-chat-only}

Google의 Antigravity CLI(`agy` 1.5.1 이상)를 설치하고 `PATH`에 추가하세요. Antigravity에 Google 계정으로 이미 로그인했다면 Artifactize에서도 바로 쓸 수 있습니다. 로그인하지 않았다면 `artifactize login antigravity`를 실행하세요. Google 로그인 링크를 열고 Antigravity 페이지에 표시된 코드를 붙여 넣으세요. 로그인 정보는 Antigravity CLI에서만 보관합니다.

Chat에서 Antigravity 모델을 고르세요. 계정에서 쓸 수 있는 모델은 `artifactize models antigravity`로 확인할 수 있습니다. MCP에서는 `models`에 `provider: "antigravity"`를 지정하세요. 로그아웃하려면 `artifactize logout antigravity`를 실행하세요. 사용 중인 CLI가 이 명령을 지원하지 않으면 Antigravity에서 `/logout`을 실행하세요. Agent eval에는 API 키를 쓰는 `gemini` 백엔드를 사용하세요. Antigravity는 사용자 지정 프로바이더의 별칭 대상으로 지정할 수 없습니다.

## 업데이트와 제거 {#3-update-and-uninstall}

| 설치 채널 | 업데이트 | 제거 |
|---|---|---|
| `install.sh` / `install.ps1` | 같은 설치 스크립트 다시 실행 | `~/.local/bin`의 실행 파일을 지운 뒤 설치 방식에 따라 `~/.local/share/artifactize/desktop` 또는 `~/Applications/Artifactize.app`도 삭제(Windows에서는 `%LOCALAPPDATA%\Programs\artifactize` 삭제) |
| Homebrew | `brew upgrade artifactize` (또는 `--cask`) | `brew uninstall artifactize` (또는 `--cask`) |
| Microsoft Store | Store에서 자동 업데이트 | Windows **설정 → 앱**에서 제거 (Store 전용 상태 디렉터리도 함께 삭제됨) |
| `.deb` / `.dmg` | 새 릴리스 파일을 같은 방식으로 설치 | `sudo apt remove artifactize`, 또는 `Artifactize.app`을 휴지통으로 옮기고 `sudo rm -f /etc/paths.d/artifactize` 실행 |
| AppImage | 새 AppImage 파일을 다운로드해 실행 | AppImage 파일 삭제, 처음 실행할 때 CLI용 심볼릭 링크를 만들었다면 `~/.local/bin`의 `artifactize`·`artifactize-tools` 심볼릭 링크와 `${XDG_DATA_HOME:-$HOME/.local/share}/artifactize/bin`의 실행 파일 삭제 |
| Cargo | `cargo binstall` 또는 `cargo install --locked` 다시 실행 | `cargo uninstall artifactize artifactize-tools` |

Store 외 채널로 설치한 경우 로컬 상태는 `~/.local/state/artifactize`(Windows는 `%LOCALAPPDATA%\artifactize`)에 남습니다. 모든 로컬 기록과 캐시를 지우려면 `artifactize doctor` 첫 줄에 나오는 디렉터리를 삭제하세요.
