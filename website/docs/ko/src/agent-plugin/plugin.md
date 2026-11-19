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
2. **근거와 함께 구성 제안**: 저장소 전체를 하나로 묶지 않고 따로 수정하고 리뷰할 수 있는 단위를 찾습니다. 각 단위에 맞는 Artifact와 eval을 제안하고, 사용자가 동의할 때까지 `.artifactize/<name>.artf` 파일은 바꾸지 않습니다.
3. **선언 작성 및 검증**: 동의한 내용을 바탕으로 선언을 작성하고 `config_check`로 문법을 확인합니다. 이어서 `status`로 실행할 eval과 재사용할 결과를 확인한 뒤 `verify_start`로 검증을 시작합니다.
4. **사람의 권한 존중**: Arti는 팀이 리뷰하지 않기로 합의한 입력에만 `basis = true`를 씁니다. 사람 대신 Human 승인 요청을 맡거나 판정을 제출하지는 않습니다. 대기 중인 승인 요청은 [자동 검증을 마치고 승인 요청 넘기기(`YELLOW`)](#working-toward-yellow)에 따라 사람에게 넘깁니다.

그 밖에도 큰 저장소에 **단계적으로 도입**하거나, 기존 선언을 **점검(`config_check`, `config_graph`)**하거나, `status`로 **검사가 왜 다시 실행됐는지 알아볼 때** Arti에게 요청할 수 있습니다.

## Focus then sweep: RED eval 반복 수정 {#focus-then-sweep-iterate-on-red-evals}

Claude Code에서는 `/artifactize:focus-then-sweep`을 호출하고, Codex에서는 실패 원인을 찾아 고쳐 달라고 요청하기 전에 `$` 스킬 선택기에서 `$artifactize:focus-then-sweep`을 선택합니다.

입력 Artifact의 fingerprint가 바뀌면 그 입력을 쓰는 eval의 이전 결과를 재사용할 수 없습니다. 수정할 때마다 `verify --all`을 실행하는 데 시간과 토큰이 많이 듭니다. 그래서 `focus-then-sweep` 스킬은 상황에 맞춰 세 가지 계획 중 하나를 고릅니다.

- **Plan A: 실패한 eval을 하나씩 고친 뒤 전체 검증 (기본값)**
  1. **집중(Focus)**: `RED`가 나온 eval 하나를 골라 먼저 실패 원인을 확인합니다(`verify_result`, `request_show`, 또는 `request_logs`; CLI는 `artifactize run show RUN_ID --json`). 평가 대상 Artifact를 고친 뒤에는 선행 조건이 충족될 때까지 기다리지 말고 해당 eval만 다시 실행하세요([선행 조건이 충족되기 전에 미리 실행하기](../concepts/runs.md#speculation)). CLI에서는 `artifactize verify --eval app/review --speculate --wait`를 실행합니다. MCP에서는 `verify_start`에 `scope: {"kind": "eval", "evalId": "app/review"}`와 `speculate: true`를 넘긴 뒤, 자동 검사가 남아 있는 동안 같은 Run ID로 `verify_wait`를 반복 호출합니다. 호스트가 프로젝트 루트를 알려 주지 않으면 모든 MCP 호출에 `"project": "/프로젝트/절대/경로"`를 함께 넘기세요. Codex 플러그인도 프로젝트 루트를 전달하지 않으므로 MCP를 호출할 때마다 `project`를 지정해야 합니다. Run 전체 결과와 관계없이 수정 중인 eval이 통과할 때까지 반복하세요([검증 대상 선택하기](../concepts/runs.md#verify-and-runs)와 [Run 시작, 진행 상황 확인, 취소](../concepts/runs.md#submit-observe-and-cancel) 참고). 판정은 `run_show`나 `request_show`에서 확인하세요. 검증 범위에 포함된 선행 승인 요청에 아직 판정이 제출되지 않았다면 `PROVISIONAL`도 통과로 봅니다. 개별 eval을 수정하는 중에 다른 필수 eval도 실패할 수 있습니다. `RED`가 나오거나 `ERROR/SUPERSEDED` 이외의 오류가 나면 해당 eval의 실패 원인을 찾아 문제를 고치세요. `STALE`이거나 아직 검증하지 않은 항목은 전체 검증 때 확인합니다. 선행 eval이 `RED`나 `ERROR`여서 실행이 막혔다면 그 eval의 실패 원인부터 해결하세요. 사람이 `RED`로 판정하면 검토 대상을 요청 내용에 맞게 고칩니다. 그다음 해당 Artifact를 다시 검증해 선행 조건을 충족하고 새 승인 요청을 기록합니다(`artifactize verify app`). 선행 eval도 함께 실행해야 한다면 `artifactize verify app --recursive`를 씁니다. 새 요청이 기록되면 승인 결과를 기다리지 않고 다음 작업으로 넘어갑니다.
  2. **전체 검증(Sweep)**: 수정한 `RED` eval이 모두 통과했거나 새 승인 요청만 남으면 프로젝트 전체를 한 번 검증합니다(`artifactize verify --all --wait`, 또는 MCP `verify_start`에 `scope: {"kind": "all"}`). 실행 결과는 [자동 검증을 마치고 승인 요청 넘기기(`YELLOW`)](#working-toward-yellow)에 따라 확인합니다. 입력과 평가 기준이 바뀌지 않은 eval은 이전 판정을 재사용합니다. 수정의 영향을 받은 하위 eval은 선행 조건을 충족한 뒤 다시 실행합니다. 전체 검증에서 `RED`가 나오거나 `ERROR/SUPERSEDED` 이외의 `ERROR`가 발생하면 집중 단계로 돌아갑니다. 승인 외에 충족하지 못한 요건이 남아 `INCOMPLETE`가 나오면, 그 원인을 해결하거나 어떤 요건이 남았는지 알린 뒤 프로젝트 전체를 다시 검증합니다.
- **Plan B: 수정의 영향을 받는 eval부터 실행한 뒤 전체 검증**: 공용 명세, 규칙 문서, 코어 라이브러리처럼 다른 eval에 영향을 주는 부분을 고칠 때 씁니다. 먼저 `artifactize config graph app --impact`(MCP `config_graph`에 `scope: {"kind": "artifact", "artifactId": "app"}`과 `impact: true`)로 하위 eval을 찾습니다. 이어서 `status`로 재검증이 필요한 대상을 확인합니다. 원래 `RED`였던 eval과 영향받는 하위 eval을 한 Run에서 함께 실행합니다(`--evals app/review,docs/matches-app --speculate --wait`, 또는 MCP `verify_start`에 `scope: {"kind": "evals", "evalIds": ["app/review", "docs/matches-app"]}`와 `speculate: true`). 처음 고른 eval과 수정의 영향을 받는 하위 eval은 검증 범위에 그대로 둡니다. 수정 중인 eval이 순환 참조에 속한다면 같은 순환 참조에 속한 Artifact의 모든 eval을 같은 Run에 넣습니다. 이미 검증 범위에 넣은 Artifact에 대해서도 나머지 eval을 모두 추가합니다. 마지막으로 Plan A와 마찬가지로 프로젝트 전체를 검증합니다.
- **Plan C: 매번 전체 검증**: eval 수가 적거나 실행이 빠를 때, 또는 한 번의 수정이 대부분의 eval에 영향을 줄 때는 Plan C를 씁니다. 수정할 때마다 전체 검증(`artifactize verify --all --wait`, 또는 MCP `verify_start`에 `scope: {"kind": "all"}`)을 실행하고 [YELLOW를 목표로 작업하기](#working-toward-yellow)를 따르세요. 다만 `RED`가 나오거나 `ERROR/SUPERSEDED`가 아닌 오류가 발생할 수 있고, 승인 외에 충족하지 못한 요건이 남을 수도 있습니다(`INCOMPLETE`). 이때는 승인 판정을 기다리거나 개별 eval로 범위를 좁히지 말고, 바로 문제를 해결한 뒤 전체 검증을 다시 실행합니다.

### 자동 검증을 마치고 승인 요청 넘기기 (`YELLOW`) {#working-toward-yellow}

MCP에서는 Run이 `"preparing"`, `"waiting"`, `"running"` 상태인 동안 같은 Run ID로 `verify_wait`를 반복 호출합니다. Run이 끝나면 `verify_result`(`ready: true`)와 `run_show`의 `validation.obligations`를 확인합니다. `verify_wait`가 `"waiting_human"`을 반환하거나 CLI에서 실행 가능한 자동 검사가 더 이상 없을 때는 결과를 바로 점검합니다(`Ctrl+C`로 `--wait` 표시만 중단하는 방법은 [Run 시작, 진행 상황 확인, 취소](../concepts/runs.md#submit-observe-and-cancel) 참고). 먼저 `run_show`나 `artifactize run show RUN_ID`에서 `requests`와 `evidence`를 확인하세요. Run이 진행 중일 때는 `config_graph`나 `status`로 아직 검증하지 못한 Artifact가 있는지 확인합니다. 이때 eval과 `parts`가 없고 `basis = true`도 설정하지 않은 항목이 남아 있는지 살핍니다. 자동 검사나 Human 요청에서 `RED` 또는 `ERROR`가 나오면 남은 작업이 끝날 때까지 기다리지 말고 그 결과부터 처리합니다. Plan A·B에서는 집중 단계로 돌아가고, Plan C에서는 수정 뒤 전체를 다시 검증합니다. 새 Run의 요청으로 대체되어 `ERROR/SUPERSEDED`가 된 요청은 실패로 보지 않으며, 리뷰어는 새 Run의 요청을 승인합니다. 승인 외에 충족하지 못한 요건이 남아 `INCOMPLETE`가 나오면, 문제를 해결하거나 남은 요건을 알린 뒤 전체를 다시 검증합니다. 아직 실행 중이거나 실행 가능한 자동 검사가 남아 있으면 `verify_wait`를 계속 호출합니다. 자동 검사가 모두 통과하고(`GREEN` 또는 `PROVISIONAL`) 다른 요건도 충족되면, 아직 판정이 나오지 않은 승인 요청만 남을 수 있습니다([승인과 Run의 종료](../concepts/runs.md#sign-offs-and-the-end-of-a-run) 참고). 이때 에이전트는 세션을 열어 둔 채 기다리지 않고 다른 작업을 계속합니다. 작업이 끝나면 `request_list`와 `request_show`로 대기 중인 요청을 확인해 리뷰를 맡을 사람에게 넘깁니다(`artifactize review REQUEST_ID`). 에이전트는 사람 대신 승인 요청을 맡거나 판정을 제출하지 않습니다.
