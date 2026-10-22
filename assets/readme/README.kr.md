[![CI](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml/badge.svg)](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/artifactize.svg)](https://crates.io/crates/artifactize)
[![License](https://img.shields.io/github/license/lhj6102/artifactize)](../../LICENSE)
[![README in English](https://img.shields.io/badge/README-English-blue)](../../README.md)

# Artifact 그래프 위에서 함께 만드세요.

Artifactize는 사람과 에이전트의 작업을 하나로 모으고 프로젝트의 일관성을 지켜 줍니다.

**누구를 위한 도구인가요?**

- 다른 사람과 함께 만들면서 효율적으로 리뷰하고 모듈 단위로 개발하고 싶은 사람.
- AI 에이전트가 만든 변경을 모든 단계를 지켜보지 않고도 믿고 싶은 사람.
- 프로젝트가 커져도 코드, 문서, 이미지, 실행 중인 애플리케이션이 서로 어긋나지 않기를 바라는 사람.

<p align="center">
  <a href="https://github.com/user-attachments/assets/6aafdfbe-e41c-4b90-b435-dab7bfdd28b6">
    <img src="https://raw.githubusercontent.com/lhj6102/artifactize/main/website/demo/media/promo.gif" width="640"
         alt="Artifactize가 프로젝트를 검사하고 이전 리뷰를 다시 사용하는 실제 터미널 녹화.">
  </a>
  <br>
  <sub>27초 소개 영상, 소리 없음. 선택하면 원본 화질 영상을 볼 수 있습니다.</sub>
</p>

**우리의 원칙**

- 만드는 방식을 제한하지 않습니다. 원하는 도구와 에이전트 하네스(AI 어시스턴트를 실행하는 앱)를 그대로 쓰세요.
- 개성을 제한하지 않습니다. 느슨한 가드레일 안에서 자유롭게 만드세요.
- AI가 모든 것을 결정하지 않습니다. 비전, 취향, 방향에 관한 중요한 결정은 사람의 손에 남습니다.

**Artifact**는 프로젝트의 한 조각입니다. 코드, 글, 디자인이나 이미지가 될 수 있습니다.
**그래프**는 이 조각들이 어떻게 연결되는지 보여 줍니다. 조각마다 자기만의 검사를 붙이세요. 테스트,
AI 리뷰, 또는 사람의 승인입니다. 검사가 살펴본 작업이 바뀌지 않았다면
Artifactize는 이전 결과를 다시 사용할 수 있습니다. 프로젝트 버전이 달라도, 팀원의 사본이어도 마찬가지입니다.

자리를 비운 동안에도 검사는 계속 실행됩니다. 검증마다 Run ID라는 추적 코드가
붙고, 명령 창을 닫아도 검증은 멈추지 않습니다.

## 설치

사용하는 운영체제에 맞는 앱을 내려받으세요. 앱에 `artifactize` 명령이
함께 들어 있으므로 Rust 빌드 도구는 필요 없습니다.

- **macOS**: [Artifactize.dmg 내려받기](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg).
  Apple 실리콘과 Intel에서 모두 쓰는 디스크 이미지 하나입니다. Artifactize를 응용 프로그램 폴더로 끌어다 놓고
  한 번 열면 터미널 명령이 설정됩니다.
- **Windows**: [Microsoft Store에서 Artifactize 받기](https://apps.microsoft.com/detail/9PB6W4LL165D)
  (x64). 설치한 뒤 새 터미널을 여세요.
- **Linux**: x86_64 Ubuntu 22.04, Debian 12 이상용 [.deb 내려받기](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb).
  소프트웨어 설치 프로그램으로 열거나
  `sudo apt install ./Artifactize-amd64.deb`를 실행하세요.

### 명령줄에서 설치

**Linux** (x86_64와 aarch64, Windows의 WSL 2 포함). x86_64에서는 데스크톱 앱도 함께 설치됩니다.

```sh
curl -fsSL https://artifactize.dev/install.sh | sh
```

**macOS**, [Homebrew](https://brew.sh/) 사용:

```sh
brew install lhj6102/tap/artifactize
```

명령과 함께 데스크톱 앱도 설치하려면 대신 `brew install --cask lhj6102/tap/artifactize`를
쓰세요. Homebrew가 없다면 설치 스크립트가 둘 다 설치합니다.
`curl -fsSL https://artifactize.dev/install.sh | sh`

**Windows**, Microsoft Store에서 설치한 뒤 새 터미널을 여세요.

```powershell
winget install 9PB6W4LL165D --source msstore
```

[전체 설치 안내](https://artifactize.dev/docs/kr/getting-started/install.html)에서
다른 방법(PowerShell 설치 스크립트, AppImage, Cargo, 소스에서 빌드)과
사전 요구 사항, 업데이트, 제거 방법을 설명합니다.

설치를 마쳤다면
[5분 빠른 시작](https://artifactize.dev/docs/kr/getting-started/guide.html)을 따라 해 보세요.
예제 프로젝트를 쓰므로 AI 계정이나 제공자 접근 키(API 키)가 필요 없습니다.
[Hey Arti](https://artifactize.dev/docs/kr/agent-plugin/hey-arti.html)를 쓰면 코딩
어시스턴트에게 평소 말투로 이런 검사를 설정하고 실행해 달라고 요청할 수 있습니다.
[플러그인](https://artifactize.dev/docs/kr/agent-plugin/plugin.html) 하나만 설치하면 Claude Code와
Codex가 이 안내와 Artifactize의 도구를 함께 갖추게 됩니다.

```text
/plugin marketplace add lhj6102/artifactize          # Claude Code, 세션 안에서
/plugin install artifactize@artifactize
codex plugin marketplace add lhj6102/artifactize     # Codex, 터미널에서
codex plugin add artifactize@artifactize
```

## 더 알아보기

- [문서](https://artifactize.dev/docs/kr/): 개념, 가이드, 예제, 명령 레퍼런스.
- [Discord 커뮤니티](https://discord.gg/VswgKkPkS9): 질문, 아이디어, 도움.
- [소스와 라이선스](../../LICENSE): Apache License 2.0으로 공개된 오픈 소스입니다.

## 실제로 동작하는 제품

아래는 데스크톱 앱의 실제 녹화와 스크린샷입니다. 스크립트
([데모](../capture/demos.mts), [스크린샷](../capture/screenshots.mts))가 실제로 검증하는
프로젝트에서 만들었습니다. 예시 리뷰어 "alice"는 리뷰 양식을 보여 주려고 승인 요청을
맡았을 뿐이며, 아무것도 제출되지 않았습니다. 텍스트 기반 앱(CLI)에도 Monitor와
Review 화면이 있습니다.

**통과할 때까지 고치기:** 한 예제의 Run 네 번을 8배속으로 보여 줍니다. Run 사이에
녹화 스크립트가 실패한 부분을 제거해 AI가 코드를 고치는 과정을 대신합니다. 바뀌지 않은
작업은 바로 재사용됩니다.

<img src="../demos/review-loop.gif" width="1200" alt="Artifactize 데스크톱 앱에서 한 브랜치의 Run 네 번을 실제로 녹화한 화면: 실패는 빨간색이 되고 Run 사이에 고쳐지며, 바뀌지 않은 작업은 재사용되고, 네 번째 Run은 충족됩니다.">

**이 프로젝트가 자라 온 과정:** 릴리스마다의 Artifactize 자신의 Artifact 그래프를
0.7.0부터 지금까지 보여 줍니다.

<img src="../demos/graph-growth.gif" width="1200" alt="이 저장소의 현재 Artifact 그래프가 0.7.0부터 현재 main까지 릴리스마다 자라는 모습을 보여 주는 Artifactize 데스크톱 앱의 실제 녹화.">

**브랜치와 Run:** 왼쪽 패널에 Run 칩과 대기 중인 승인이 있는 예제 브랜치, 그리고
그 Run입니다. core는 통과했고, cli의 테스트가 실패해 docs는 막혔으며,
release는 사람의 승인을 기다립니다.

<img src="../screenshots/monitor.png" width="1200" alt="실제 Artifactize 데스크톱 앱: 왼쪽 패널의 예제 브랜치와 대기 중인 승인, 그리고 core는 통과, cli는 실패, docs는 막힘, release는 승인 대기인 Run 그래프.">

**Detail:** cli가 사용하는 core와 cli를 사용하는 docs 사이에 있는 cli와, 그 검사들입니다.

<img src="../screenshots/detail.png" width="1200" alt="실제 Artifactize 데스크톱 앱의 cli Artifact Detail: 이웃인 core와 docs, 그리고 lint와 tests 검사.">

**Review:** 대기 중인 리뷰에서 연 release 승인 요청입니다. 예시 리뷰어가 맡아
notes 도구를 실행한 뒤 GREEN 양식을 열어 둔 상태입니다.

<img src="../screenshots/review.png" width="1200" alt="실제 Artifactize 데스크톱 Review: 예제의 맡은 승인 요청과 그 지시, notes 도구, 도구 로그, GREEN 양식.">
