# 코딩 에이전트와 함께 쓰기 {#working-with-coding-agents}

`artifactize` 플러그인을 설치하면 **Claude Code**와 **Codex**에 세 가지 구성 요소가 한 번에 추가됩니다.

- **[Hey Arti](#hey-arti-shape-and-audit-your-project)**: 저장소를 분석하고 팀에 맞는 Artifact와 eval 구성을 제안하며, 검증 결과를 보고 문제의 원인을 찾는 프로젝트 안내 스킬입니다. Claude Code에서는 `/artifactize:hey-arti`, Codex에서는 `$artifactize:hey-arti`로 호출합니다.
- **[Focus then sweep](#focus-then-sweep-iterate-on-red-evals)**: 실패한 eval을 고치는 동안에는 해당 eval만 반복 실행하고, 수정이 끝나면 Artifact Graph 전체를 한 번 검증합니다. Claude Code에서는 `/artifactize:focus-then-sweep`, Codex에서는 `$` 스킬 선택기에서 `$artifactize:focus-then-sweep`을 선택합니다.
- **[MCP 서버](../reference/cli-and-mcp.md#mcp-stdio-server)**: 에이전트가 Artifact Graph를 조회하고 Run을 시작하거나 진행 상황을 확인할 수 있도록 `artifactize mcp` 도구를 제공합니다.

## 플러그인 추가 및 업데이트 {#updates}

플러그인은 `artifactize` 실행 파일을 호출하므로 먼저 [`artifactize`를 설치](../getting-started/install.md)하세요.

**Claude Code** (세션 안에서 실행하거나 셸에서 `claude plugin ...` 실행):

```text
/plugin marketplace add lhj6102/artifactize
/plugin install artifactize@artifactize
```

`/reload-plugins`를 실행하거나 프로젝트 폴더에서 새 세션을 시작한 뒤 `/mcp`에서 `artifactize` 서버 연결을 확인하세요. 업데이트할 때는 `/plugin marketplace update artifactize`를 실행합니다.

**Codex** (셸에서 마켓플레이스를 추가한 뒤 `codex plugin add`를 실행하거나 `/plugins`에서 `artifactize` 선택):

```sh
codex plugin marketplace add lhj6102/artifactize
codex plugin add artifactize@artifactize
```

프로젝트 폴더에서 새 Codex 세션을 시작하세요. 업데이트할 때는 `codex plugin marketplace upgrade artifactize`를 실행합니다.

**기타 호스트 (수동 설정)**: 표준 입출력(stdio)으로 `artifactize mcp`를 등록하고([MCP 서버 등록](../reference/cli-and-mcp.md#register-the-mcp-server)), [`hey-arti/SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md)와 [`focus-then-sweep/SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/focus-then-sweep/SKILL.md) 원본 파일을 대화에 첨부하세요. 플러그인을 설치한 호스트에는 MCP 서버를 따로 등록하지 마세요.

## Hey Arti: 프로젝트 구조 설계와 점검 {#hey-arti-shape-and-audit-your-project}

Claude Code에서는 `/artifactize:hey-arti`를 입력하고, Codex에서는 `$` 스킬 선택기에서 `$artifactize:hey-arti`를 고릅니다.

```text
/artifactize:hey-arti Artifactize를 설명하고 이 프로젝트에 어떻게 적용할 수 있을지 알려 줘
/artifactize:hey-arti 기존 테스트와 리뷰 절차를 Artifactize로 옮기는 것을 도와줘
```

Arti와 함께 작업하는 순서:

1. **저장소 파악과 인터뷰**: Arti가 먼저 코드, 문서, 명세, 테스트 명령을 읽습니다. 이어서 코드만으로는 알 수 없는 목표, 보통 함께 수정하는 파일이나 모듈, 외부 명세나 공용 정책, 사람이 직접 판단해야 할 기준을 묻습니다.
2. **근거와 함께 구성 제안**: 저장소 전체를 하나로 묶지 않고 따로 수정하고 리뷰할 수 있는 단위를 찾습니다. 각 단위에 맞는 Artifact와 eval을 제안하고, 사용자가 동의할 때까지 `.artifactize/` 아래의 `.artf` 파일은 바꾸지 않습니다.
3. **선언 작성 및 검증**: 동의한 내용을 바탕으로 선언을 작성하고 `config_check`로 문법을 확인합니다. 이어서 `status`로 실행할 eval과 재사용할 결과를 확인한 뒤 `verify_start`로 검증을 시작합니다.
4. **사람의 권한 존중**: Arti는 팀이 리뷰하지 않기로 합의한 입력에만 `basis = true`를 씁니다. 사람 대신 Human 승인 요청을 맡거나 판정을 제출하지는 않습니다. 대기 중인 승인 요청은 [자동 검증을 마치고 승인 요청 넘기기(`YELLOW`)](#working-toward-yellow)에 따라 사람에게 넘깁니다.

그 밖에도 큰 저장소에 **단계적으로 도입**하거나, 기존 선언을 **점검(`config_check`, `config_graph`)**하거나, `status`로 **평가가 왜 다시 실행됐는지 알아볼 때** Arti에게 요청할 수 있습니다.

## Focus then sweep: RED eval 반복 수정 {#focus-then-sweep-iterate-on-red-evals}

Claude Code에서는 `/artifactize:focus-then-sweep`을 호출하고, Codex에서는 실패 원인을 찾아 고쳐 달라고 요청하기 전에 `$` 스킬 선택기에서 `$artifactize:focus-then-sweep`을 선택합니다.

입력 Artifact의 fingerprint가 바뀌면 그 입력을 쓰는 eval의 이전 결과를 재사용할 수 없습니다. 수정할 때마다 `verify --all`을 실행하는 데 시간과 토큰이 많이 듭니다. 그래서 `focus-then-sweep` 스킬은 상황에 맞춰 세 가지 계획 중 하나를 고릅니다.

CLI에서는 `--wait` 없이 검증을 시작하세요. 출력된 Run ID로 `artifactize run show RUN_ID --json`을 실행하면 진행 중인 Run도 확인할 수 있습니다. 진행 상황을 계속 보려면 `artifactize run watch RUN_ID`를 쓰세요. Ctrl+C는 관찰만 멈출 뿐 Run을 취소하지 않습니다. MCP에서는 `verify_start`가 반환한 ID로 `verify_wait`를 호출하고 `run_show`에서 요청을 살핍니다. 호스트가 프로젝트 루트를 전달하지 않으면 모든 호출에 프로젝트의 절대 경로를 `project`로 넘기세요.

- **Plan A: 실패한 eval을 하나씩 고친 뒤 전체 검증 (기본값).** `verify_result`나 `run_show`에서 실패한 eval 하나와 요청 ID를 찾습니다. CLI에서는 `artifactize run show RUN_ID --json`을 사용합니다. 저장된 판정은 `request_show`나 `artifactize request show REQUEST_ID --json`으로, runtime 출력은 `request_logs`나 `artifactize request logs REQUEST_ID`로 확인하세요. 평가 대상 Artifact를 고친 뒤 `artifactize verify --eval app/review --speculate`로 해당 eval만 실행합니다. MCP에서는 `verify_start`에 `scope: {"kind":"eval","evalId":"app/review"}`와 `speculate: true`를 넘깁니다. Run 전체 결과와 별개로 해당 eval의 판정을 확인하세요. 실패한 eval을 하나씩 고친 뒤 `artifactize verify --all` 또는 `scope: {"kind":"all"}`로 전체를 검증합니다. 새 실패가 나오면 다시 그 eval에 집중합니다.
- **Plan B: 수정의 영향을 받는 eval부터 실행한 뒤 전체 검증.** 실제로 바꾼 Artifact마다 영향을 확인하세요. `app/review`가 실패했고 `spec`을 바꿨다면 `artifactize config graph spec --impact`를 실행합니다. MCP에서는 `config_graph`에 `scope: {"kind":"artifact","artifactId":"spec"}`와 `impact: true`를 넘깁니다. `status`로 재검증할 eval을 확인하세요. 실패한 eval과 영향을 받는 eval을 같이 실행합니다(`artifactize verify --evals app/review,docs/matches-app --speculate` 또는 MCP의 `scope: {"kind":"evals","evalIds":["app/review","docs/matches-app"]}`와 `speculate: true`). 대상이 참조 순환에 속하면 그 순환에 속한 Artifact의 모든 eval을 함께 실행합니다. 이후 Plan A처럼 전체 검증합니다.
- **Plan C: 매번 전체 검증.** eval이 적거나 빠르거나, 수정한 내용을 대부분의 eval이 읽는다면 수정할 때마다 `artifactize verify --all` 또는 MCP의 `scope: {"kind":"all"}`로 전체를 검증하세요.

### 자동 검증을 마치고 승인 요청 넘기기 (`YELLOW`) {#working-toward-yellow}

Run이 `WAITING_HUMAN`이어도 저장된 요청과 검증 근거를 다음 순서로 확인하세요.

1. `RED`나 `ERROR`가 있으면 다른 작업을 기다리지 말고 먼저 해결합니다. 새 Run이 대체한 승인 요청은 실패가 아니라 미응답으로 보고 새 요청을 확인하세요.
2. 실패가 없고 실행할 자동 평가가 남았다면 계속 관찰합니다. MCP에서는 `verify_wait`를 다시 호출하고 `run_show`를 확인하세요. 승인이 남아 있으면 `verify_result`에서 아직 결과를 받지 못할 수 있습니다.
3. 자동 평가가 끝나면 Run의 `validation.obligations`를 확인합니다. Run이 아직 열려 있으면 `config_graph`나 `status`로 eval, `parts`, `basis = true` 설정이 모두 없는 Artifact도 확인하세요. 승인 외에 남은 요건을 해결한 뒤 전체 검증을 다시 실행합니다. 해결할 수 없다면 입력이 그대로인 Run을 반복하지 말고 문제를 알리세요.
4. 자동 평가가 통과하고 승인만 남았다면 담당자에게 요청 ID를 전달합니다. 데스크톱 앱이 있으면 `artifactize review REQUEST_ID`로 열 수 있습니다. 앱 없이도 [`artifactize request` 명령](../reference/cli-and-mcp.md#command-reference)으로 리뷰할 수 있습니다. 수정 작업을 맡은 에이전트가 직접 승인하지는 않습니다.
