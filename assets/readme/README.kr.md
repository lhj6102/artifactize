[![CI](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml/badge.svg)](https://github.com/lhj6102/artifactize/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/artifactize.svg)](https://crates.io/crates/artifactize)
[![License](https://img.shields.io/github/license/lhj6102/artifactize)](../../LICENSE)
[![Download for macOS](https://img.shields.io/badge/Download-macOS-black)](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg)
[![Get it on Windows](https://img.shields.io/badge/Get%20it-Windows-0078D4)](https://apps.microsoft.com/detail/9PB6W4LL165D)
[![Download for Linux](https://img.shields.io/badge/Download-Linux%20.deb-E95420)](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb)

[English](../../README.md) · 한국어 · 홈페이지: [artifactize.dev](https://artifactize.dev)

# Artifactize: Evaluation-driven development for people and AI agents.

Artifactize는 프로젝트의 코드, 문서, 이미지, 실행 중인 앱을 검사합니다. 테스트를 실행하거나 AI 에이전트에 리뷰를 맡기거나 사람이 직접 확인해 승인할 수 있습니다. 검사 대상을 **Artifact**, Artifact마다 설정하는 검사를 **eval**이라고 부릅니다. 검증을 한 번 실행할 때마다 **Run**을 만들고, 실행한 검사와 그 결과를 Run에 기록합니다. 팀원들은 AI 에이전트의 작업 과정을 일일이 지켜보지 않아도 결과를 믿을 수 있습니다.

기존 테스트 러너는 코드만 따로 검사하고, 리뷰 봇은 풀 리퀘스트만 따로 봅니다. Artifactize는 프로젝트의 모든 Artifact와 각 검사에 필요한 입력을 하나의 **Artifact Graph**로 연결합니다. 덕분에 테스트, AI 에이전트 리뷰, 사람의 승인을 하나의 워크플로에서 관리할 수 있습니다. 검사마다 어떤 Artifact를 입력으로 쓰는지 지정해 둡니다. 입력이 바뀌면 그 검사를 다시 실행합니다. 다른 브랜치나 팀원의 개발 환경, CI에서도 검사 입력이 그대로라면 이전 결과를 재사용합니다.

## 한눈에 보기

`.artifactize/<name>.artf` 파일에 각 Artifact와 검사를 선언한 뒤 `artifactize verify`를 실행합니다.

```toml
# .artifactize/api.artf
path = "crates/api/"

[evals.tests]
title = "API tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"] }
payload.instruction = "Run the test suite of {api}."

[evals.matches-spec]
title = "API implementation matches the specification"
profile = { kind = "agent", backend = "codex", model = "gpt-6.2-sol", reasoning = "high" }
payload.instruction = "Review {api} against {spec}. Submit GREEN only when every route matches."
```

```sh
$ artifactize verify --all --wait
  spec/approved    [run-8fK2a1-1]: GREEN (reused from run-3bX9q0)
  api/tests        [run-8fK2a1-2]: GREEN
  api/matches-spec [run-8fK2a1-3]: GREEN
Summary: executed 2 (runtime 1, agent 1, human 0), reused 1 (human 1)
```

<p align="center">
  <img src="../demos/review-loop.gif" width="960"
       alt="Artifactize 데스크톱 앱에서 예제 프로젝트를 네 번 검증하는 과정을 녹화했습니다. 검사를 통과하지 못한 Artifact가 빨간색으로 표시되면 다음 검증 전에 고칩니다. 입력이 바뀌지 않은 검사는 이전 결과를 재사용합니다. 네 번째 검증에서는 예제 프로젝트가 모든 검사를 통과합니다.">
</p>

## 빠르게 시작하기

### 1. 설치

**데스크톱 앱 내려받기** (`artifactize` CLI가 함께 설치됩니다):
- **macOS** (Apple 실리콘 및 Intel): [Artifactize.dmg](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize.dmg)를 응용 프로그램 폴더로 옮긴 뒤 한 번 실행하세요.
- **Windows** (x64): [Microsoft Store의 Artifactize](https://apps.microsoft.com/detail/9PB6W4LL165D)를 설치한 뒤 새 터미널을 여세요.
- **Linux** (x86_64 Ubuntu 22.04 / Debian 12 이상): [Artifactize-amd64.deb](https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb)를 내려받아 `sudo apt install ./Artifactize-amd64.deb`로 설치하세요.

명령줄에서 바로 설치할 수도 있습니다.

| 시스템 | 명령 |
|---|---|
| Linux(x86_64, aarch64, WSL 2 포함), x86_64에서는 데스크톱 앱 포함 | `curl -fsSL https://artifactize.dev/install.sh \| sh` |
| [Homebrew](https://brew.sh/)를 쓰는 macOS | `brew install lhj6102/tap/artifactize` (데스크톱 앱까지 설치하려면 `--cask`를 붙이세요) |
| Homebrew를 쓰지 않는 macOS | `curl -fsSL https://artifactize.dev/install.sh \| sh` |
| Windows (winget으로 Microsoft Store 버전 설치) | `winget install 9PB6W4LL165D --source msstore` |

PowerShell 스크립트(`irm https://artifactize.dev/install.ps1 | iex`), AppImage, Cargo로 설치하는 방법과 소스 코드를 직접 빌드하는 방법은 [설치 가이드](https://artifactize.dev/docs/ko/getting-started/install.html)에서 확인할 수 있습니다.

**Claude Code나 Codex에 플러그인 추가하기** — 플러그인을 추가하면 조회·검증 도구와 **Arti** 스킬을 쓸 수 있습니다. Arti는 프로젝트에 Artifactize를 적용하는 방법을 안내합니다.

```sh
claude plugin marketplace add lhj6102/artifactize && claude plugin install artifactize@artifactize   # Claude Code
codex plugin marketplace add lhj6102/artifactize && codex plugin add artifactize@artifactize         # Codex
```

또는 아래 문장을 Claude Code나 Codex에 붙여 넣어 CLI와 플러그인 설치를 에이전트에게 바로 맡길 수 있습니다.

```text
Install Artifactize on this machine following https://artifactize.dev/docs/ko/getting-started/install.html, then add its plugin to yourself following https://artifactize.dev/docs/ko/agent-plugin/plugin.html.
```

### 2. 첫 작업 해 보기

프로젝트 폴더에서 Arti에게 다음과 같이 요청하세요(Codex에서는 아래 예시의 명령만 `$artifactize:hey-arti`로 바꿔 입력하세요).

```text
/artifactize:hey-arti Artifactize를 설명하고 이 프로젝트에 어떻게 적용할 수 있을지 알려 줘
```

Arti는 저장소를 살펴보고 팀에서 중요하게 여기는 기준을 물은 뒤, 프로젝트에 맞는 Artifact와 검사를 제안합니다. 제안에 동의하면 Arti가 `.artifactize/<name>.artf` 선언 파일을 작성하고 `verify`로 첫 검증을 실행합니다. AI 계정 없이 먼저 체험해 보려면 [5분 빠른 시작](https://artifactize.dev/docs/ko/getting-started/guide.html)을 따라 해 보세요.

## 동작 방식

1. 평가를 설정할 때 결과물이 충족해야 할 기준을 적습니다. AI 에이전트는 평가를 통과할 때까지 결과물을 고칩니다.
2. 테스트, AI 에이전트, 사람 모두 대등한 위치에서 평가할 수 있으며, 방식마다 강점이 다릅니다.
3. 평가를 마치면 그 결과물을 다음 평가의 입력으로 넘깁니다. 이 과정을 반복해 최종 결과물을 완성합니다.

## 주요 특징

<p align="center">
  <img src="../others/graph-example.ko.png" width="800"
       alt="예제 프로젝트의 Artifact Graph입니다. .artifactize/ 폴더의 .artf 파일에 PROBLEM.md, SPEC.md, code/, docs/를 선언했습니다. 어떤 Artifact가 서로 연결되고 그 연결에 명령 실행, AI 에이전트 리뷰, 사람의 승인 중 어떤 검사 방식이 쓰이는지 그래프에서 확인할 수 있습니다.">
</p>

| 기능 | 설명 |
|---|---|
| **Artifact Graph** | `.artifactize/<name>.artf`에서 `path`와 `parts`로 대상 파일이나 폴더, Live URI를 지정합니다. `{input}`으로 어떤 입력에 의존하는지 표시합니다. |
| **세 종류의 검사** | 한 워크플로에 세 가지 검사 방식을 모두 넣을 수 있습니다. `runtime`에서는 테스트나 린터를 실행하고, `agent`에서는 LLM이 읽기 전용 도구로 검사 대상을 살펴보고 판정하며, `human`에서는 사람이 승인합니다. |
| **입력 기반 재사용** | 검사에 쓰는 입력의 fingerprint(기본값 `artifactsum`)를 비교해 입력이 바뀌었는지 확인합니다. 다른 브랜치에서도 검사 입력이 바뀌지 않았다면 이전 결과를 그대로 재사용합니다. 팀원의 개발 환경과 CI에서도 마찬가지입니다. |
| **백그라운드에서 계속 실행되는 Run** | `artifactize verify`로 시작한 Run은 백그라운드에서 실행됩니다. 에이전트가 검사에서 드러난 문제를 고치는 동안에도 Run이 멈추지 않으며, 모든 자동 검사를 통과해야 사람에게 승인을 요청합니다. |
| **데스크톱 앱과 터미널 인터페이스** | 데스크톱 앱과 터미널 인터페이스 모두 Artifact Graph에서 Run 진행 상황을 실시간으로 보여 줍니다. 선언을 수정하거나, 직접 승인할 검사를 선택하거나, 검사 결과를 제출할 수 있습니다. |

## 더 알아보기

- [문서](https://artifactize.dev/docs/ko/): 핵심 개념, 가이드, 예제, CLI 및 MCP 레퍼런스를 확인할 수 있습니다.
- [Discord 커뮤니티](https://discord.gg/VswgKkPkS9): 질문을 올리고, 아이디어를 나누고, 도움을 요청할 수 있습니다.
- [라이선스](../../LICENSE): Apache License 2.0에 따라 소스 코드를 공개합니다.
