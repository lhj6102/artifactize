# Artifactize {#artifactize}

Evaluation-driven development for people and AI agents.

Artifactize로 프로젝트의 각 부분을 어떻게 검증할지 선언합니다. 평가 기준과 입력이 일치하면 재사용할 수 있는 리뷰 결과를 다시 씁니다.

- **Artifact**: 프로젝트에서 리뷰할 작업 단위입니다. 폴더, 단일 파일, 폴더 안에서 고른 경로(**Aspect**), 외부 서비스 URI(**Live**)를 대상으로 삼을 수 있으며 `.artifactize/` 아래 `.artf` 파일에 `artf` 문으로 선언하고 `path`를 적습니다.
- **eval**: Artifact 하나를 검증해 `GREEN` 또는 `RED` 판정을 내리는 평가입니다. `runtime` eval은 테스트나 린터를 실행하고, `agent` eval은 읽기 전용 도구로 대상을 살펴 판정합니다. `human` eval에서는 리뷰어가 직접 판정합니다.
- **Artifact Graph**: eval이 `@{artf name}` 같은 slot이나 runtime eval의 `deps`로 가리키는 입력과 eval의 의존 관계, 그리고 `parts`로 묶은 Artifact의 구성 관계를 보여 줍니다.
- **fingerprint와 재사용**: 재사용 키가 일치하면 이전 판정을 다시 쓸 수 있습니다. 자세한 동작은 [fingerprint와 재사용](/docs/ko/concepts/reuse.html)을 보세요.

## 기본 작업 흐름 {#how-a-project-flows}

1. **평가 기준을 먼저 선언합니다.** `.artifactize/` 아래에 작은 `.artf` 언어로 각 Artifact를 선언하고(반복되는 부분은 매크로로 씁니다) eval 종류(`runtime`, `agent`, `human`)와 평가에 사용할 입력 Artifact를 지정합니다.
2. **만들면서 반복 검증합니다.** `artifactize verify --all --wait` 명령을 직접 실행하거나 플러그인을 쓰는 코딩 에이전트에게 검증을 맡깁니다. 검증을 시작하면 Artifactize가 백그라운드에서 **Run**을 실행하고 결과를 기록합니다.
3. **결과를 재사용하고 승인합니다.** 입력이 그대로라면 로컬 환경, 팀 리뷰 저장소, CI에서도 이전 평가 결과를 그대로 재사용할 수 있습니다. 같은 Artifact의 자동 평가를 모두 통과해야 사람에게 승인을 요청합니다.

## 다음으로 볼 문서 {#where-to-go-next}

- **[설치](/docs/ko/getting-started/install.html)**: Linux, macOS, Windows에서 데스크톱 앱과 CLI를 설치하고 AI 백엔드를 연결합니다.
- **[5분 만에 첫 프로젝트 만들어 보기](/docs/ko/getting-started/guide.html)**: AI 계정 없이 작은 프로젝트를 만들고 `verify`와 결과 재사용을 직접 체험합니다.
- **[핵심 개념](/docs/ko/concepts/artifacts.html)**: Artifact, eval, fingerprint, Run이 서로 어떻게 맞물리는지 살펴봅니다.
- **[코딩 에이전트와 함께 쓰기](/docs/ko/agent-plugin/plugin.html)**: Claude Code나 Codex에 플러그인을 설치해 **Hey Arti**와 **Focus then sweep**을 활용합니다.
- **[가이드](/docs/ko/guides/tools.html)**: 리뷰 도구 설정, 데스크톱 앱 사용법, 팀 리뷰 저장소 및 CI 연동 방법을 다룹니다.
- **[레퍼런스](/docs/ko/reference/declarations.html)**: `.artf` 필드는 [선언(.artf) 레퍼런스](/docs/ko/reference/declarations.html)에서, 명령과 종료 코드 및 MCP 도구는 [CLI와 MCP 레퍼런스](/docs/ko/reference/cli-and-mcp.html)에서 찾아봅니다.
