# Artifactize {#artifactize}

Build together on an Artifact Graph.

artifactize는 여러분과 팀원이 프로젝트의 각 부분을 리뷰하되, 바뀌지 않은 작업은
다시 검사하지 않도록 도와줍니다. **Artifact**는 리뷰할 수 있는 작업 단위입니다.
파일이나 폴더뿐 아니라, 그 부분들 사이의 연결로 이루어진 그래프도 Artifact가 될 수 있습니다.
**eval**은 Artifact에 대한 검사입니다. eval은 명령 실행, AI 에이전트의 리뷰, 사람의 승인 가운데 하나입니다.

- 맡은 부분은 각자 편한 방식으로 작업하세요. 각 eval이 쓰는 다른 부분을 선언해 두면
  그 부분이 바뀔 때 영향을 받는 리뷰만 다시 실행됩니다.
- 입력이 그대로라면 리뷰를 재사용하세요. **fingerprint**가 그 입력을 식별하며,
  `artifactize status`는 무엇을 다시 리뷰해야 하는지 보여 줍니다.
- 팀 리뷰 저장소를 통해 팀원과 CI에 결과를 공유하세요. Monitor에서 진행 상황을
  확인하고, Review에서 승인 대기 중인 요청을 차례로 처리할 수 있습니다.

Linux, macOS, Windows에서 실행됩니다. [설치](/docs/ko/getting-started/install.html)부터 시작한 뒤
[가이드](/docs/ko/getting-started/guide.html)를 따라 첫 GREEN 결과를 얻고, 모델이나 API 키 없이
그 결과를 재사용해 보세요.

## 다음으로 볼 곳 {#where-to-go-next}

- **[기능](/docs/ko/features/artifact.html):** Artifact와 eval을 선언하고, 무엇을
  재사용할지 정하고, 검증을 실행하고 진행 상황을 확인하며, 리뷰어에게 도구를 주고, 팀과 결과를 공유합니다.
  CLI, 데스크톱 앱, 에이전트 플러그인에서 똑같이 동작합니다.
- **[데스크톱](/docs/ko/desktop/index.html):** 창에서 Run을 살펴보고 Human 리뷰를 처리합니다.
- **[에이전트 플러그인](/docs/ko/agent-plugin/hey-arti.html):** 여러 코딩 어시스턴트에서 쓸 수 있는 스킬인 Arti를 어시스턴트에 추가하고
  [MCP](/docs/ko/agent-plugin/mcp.html)로 코딩 호스트를 연결합니다.
- **[CLI](/docs/ko/cli/index.html):** 명령 문법, 출력, 종료 코드.
- **[TUI](/docs/ko/tui/index.html):** 터미널에서 쓰는 Monitor와 Review.
- **[소스](https://github.com/lhj6102/artifactize):** Apache 2.0으로 공개된 오픈 소스.
