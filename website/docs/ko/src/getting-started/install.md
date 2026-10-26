# artifactize 설치 {#install-artifactize}

Artifactize는 Linux(x86_64 또는 aarch64), macOS(Apple 실리콘 또는 Intel), Windows(x64)에서
실행되며, WSL 2에서는 Linux 빌드가 실행됩니다. 한 가지 방법으로 설치한 다음 새 터미널을 열고
`artifactize --version`을 실행하세요. `artifactize 0.15.0`처럼 설치된 버전이 출력됩니다.
Artifactize는 [Apache License 2.0](https://github.com/lhj6102/artifactize/blob/main/LICENSE)으로
공개된 오픈 소스입니다.

## 앱 내려받기 {#download-the-app}

사용하는 운영체제에 맞는 앱을 내려받으세요. 앱에 `artifactize` 명령이 함께 들어 있으므로
Rust 빌드 도구는 필요 없습니다.

- **macOS**: [Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg).
  디스크 이미지 하나로 Apple 실리콘 Mac과 Intel Mac에 모두 설치할 수 있습니다. Artifactize를
  응용 프로그램 폴더로 끌어다 놓고 한 번 열면 터미널에서 artifactize 명령을 쓸 수 있습니다.
- **Windows**: [Microsoft Store의 Artifactize](https://apps.microsoft.com/detail/9PB6W4LL165D)
  (x64). 설치한 뒤 새 터미널을 여세요.
- **Linux**: x86_64 Ubuntu 22.04, Debian 12 이상용
  [Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb).
  소프트웨어 설치 프로그램으로 열거나 `sudo apt install ./Artifactize-amd64.deb`를 실행하세요.

## 명령줄에서 설치 {#from-the-command-line}

**Linux** (x86_64와 aarch64, WSL 2 포함). x86_64에서는 데스크톱 앱도 함께 설치됩니다.

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

**macOS**, [Homebrew](https://brew.sh/) 사용. 명령과 함께 데스크톱 앱도 설치하려면 `--cask`를 붙이세요.

```sh
brew install lhj6102/tap/artifactize
```

**Windows**, Microsoft Store에서 설치:

```powershell
winget install 9PB6W4LL165D --source msstore
```

[다른 설치 방법](install-options.md)에서 이 방법들을 자세히 설명하고, PowerShell 설치 스크립트,
AppImage, Cargo, 소스에서 빌드하는 방법도 다룹니다.

## 플러그인으로 빠르게 시작하기 {#quick-start-with-the-plugin}

Arti와 대화하며 시작해 보세요. Arti는 코딩 어시스턴트에서
현재 작업 중인 프로젝트를 예로 들어 Artifactize 사용법을 설명하고, 기존 테스트와 리뷰를 옮기거나
새 프로젝트를 준비하도록 돕습니다.
[플러그인](../agent-plugin/plugin.md) 하나만 설치하면 Claude Code와 Codex에서
[Hey Arti](../agent-plugin/hey-arti.md)의 안내를 받으며 Artifactize 도구를 쓸 수 있습니다.

```text
/plugin marketplace add lhj6102/artifactize          # Claude Code, 세션 안에서
/plugin install artifactize@artifactize
codex plugin marketplace add lhj6102/artifactize     # Codex, 터미널에서
codex plugin add artifactize@artifactize
```

**코딩 에이전트에게** 이렇게 말을 걸어 보세요(Codex에서는 `$artifactize:hey-arti`로
쓰세요).

```text
/artifactize:hey-arti Artifactize에 대해 설명해 주고 이 프로젝트에 어떻게 적용하면 좋을지 이야기해 줘
/artifactize:hey-arti 지금 있는 테스트와 리뷰를 Artifactize로 옮기도록 도와줘
/artifactize:hey-arti 이 프로젝트를 Artifactize로 처음부터 시작하도록 도와줘
```

## 다음 단계 {#next}

- [가이드](guide.md): 모델 없이 예제 프로젝트를 실행해 보는 5분짜리 실습입니다.
- [백엔드와 로그인](backends.md): Agent eval을 모델 서비스에 연결합니다.
- [업데이트와 제거](update.md): 업데이트, 업그레이드 안내, 상태가 저장되는 곳, 제거 방법을 다룹니다.
