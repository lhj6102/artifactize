# 코딩 에이전트와 함께 쓰기 {#working-with-coding-agents}

`artifactize` 플러그인을 설치하면 **Claude Code**와 **Codex**에 세 가지 구성 요소가 한 번에 추가됩니다.

- **[Hey Arti](#hey-arti-shape-and-audit-your-project)**: 저장소를 분석하고 팀에 맞는 Artifact와 eval 구성을 제안하며 결과를 진단하는 프로젝트 안내 스킬(Claude Code에서는 `/artifactize:hey-arti`, Codex에서는 `$artifactize:hey-arti`).
- **[Focus then sweep](#focus-then-sweep-iterate-on-red-evals)**: `RED`가 나온 eval을 고칠 때 해당 eval만 먼저 반복 실행하고 마지막에 Artifact Graph 전체를 한 번 검증하는 빌더 스킬(Claude Code에서는 `/artifactize:focus-then-sweep`, Codex에서는 `$` 스킬 선택기에서 `$artifactize:focus-then-sweep` 선택).
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

**기타 호스트 (수동 설정)**: 표준 입출력(stdio)으로 `artifactize mcp`를 등록하고([MCP 서버 등록](../reference/cli-and-mcp.md#register-the-mcp-server)), [`hey-arti/SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md)와 [`focus-then-sweep/SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/focus-then-sweep/SKILL.md) 원본 파일을 대화에 첨부하세요. 한 호스트에 플러그인과 수동 MCP 등록을 중복으로 설정하지 마세요.

## Hey Arti: 프로젝트 구조 설계와 점검 {#hey-arti-shape-and-audit-your-project}

Claude Code에서는 `/artifactize:hey-arti`를 입력하고, Codex에서는 `$` 스킬 선택기에서 `$artifactize:hey-arti`를 고릅니다.

```text
/artifactize:hey-arti Artifactize를 설명하고 이 프로젝트에 어떻게 적용할 수 있을지 알려 줘
/artifactize:hey-arti 기존 테스트와 리뷰 절차를 Artifactize로 옮기는 것을 도와줘
```

Arti와 함께 작업하는 순서:

1. **저장소 파악과 인터뷰**: Arti가 코드, 문서, 명세, 테스트 명령을 읽은 뒤 코드만으로는 알 수 없는 목표, 함께 바뀌는 범위, 외부 명세나 공용 정책, 사람이 직접 판단해야 할 기준을 묻습니다.
2. **근거와 함께 구성 제안**: 저장소 전체를 하나로 묶는 대신 독립적으로 수정·리뷰하는 단위로 Artifact와 eval을 제안하고, 사용자가 동의할 때까지 `.artifactize/<name>.artf` 파일을 바꾸지 않습니다.
3. **선언 작성 및 검증**: 동의한 내용을 바탕으로 선언을 작성하고 `config_check`로 문법을 확인한 뒤, `status`로 어떤 eval을 실행하고 재사용할지 미리 보여 주고 `verify_start`로 검증을 시작합니다.
4. **사람의 권한 존중**: Arti는 팀이 리뷰하지 않기로 합의한 입력에만 `basis = true`를 쓰고, 사람 대신 Human 승인 요청을 맡거나 판정을 제출하지 않으며, 대기 중인 승인 요청은 [YELLOW를 목표로 작업하기](#working-toward-yellow)에 따라 사람에게 넘깁니다.

그 밖에도 큰 저장소에 **단계적으로 도입**하거나, 기존 선언을 **점검(`config_check`, `config_graph`)**하거나, `status`로 **검사가 다시 실행된 이유를 진단**할 때 Arti에게 요청할 수 있습니다.

## Focus then sweep: RED eval 반복 수정 {#focus-then-sweep-iterate-on-red-evals}

Claude Code에서는 `/artifactize:focus-then-sweep`을 호출하고, Codex에서는 실패한 Run 수정을 요청하기 전에 `$` 스킬 선택기에서 `$artifactize:focus-then-sweep`을 선택합니다.

수정 때문에 다른 eval이 의존하는 fingerprint가 바뀌면 그 eval들의 이전 결과도 무효가 됩니다. 수정할 때마다 `verify --all`을 실행하면 시간과 토큰이 낭비되므로, `focus-then-sweep` 스킬은 상황에 맞춰 세 가지 계획 중 하나를 고릅니다.

- **Plan A: 개별 집중 후 마지막에 전체 검증 (기본값)**
  1. **집중(Focus)**: `RED`가 나온 eval 하나를 골라 먼저 실패 원인을 확인합니다(`verify_result`, `request_show`, 또는 `request_logs`; CLI는 `artifactize run show RUN_ID --json`). 평가 대상 Artifact를 고친 뒤 해당 eval만 [선행 조건 충족 전 미리 실행](../concepts/runs.md#speculation)으로 다시 실행하세요. CLI에서는 `artifactize verify --eval app/review --speculate --wait`를 실행합니다. MCP에서는 `verify_start`에 `scope: {"kind": "eval", "evalId": "app/review"}`와 `speculate: true`를 넘긴 뒤, 자동 검사가 남아 있는 동안 같은 Run ID로 `verify_wait`를 반복 호출합니다. 설치된 Codex 플러그인처럼 호스트가 프로젝트 루트를 알려 주지 않을 때는 모든 MCP 호출에 `"project": "/프로젝트/절대/경로"`를 함께 넘기세요. Run 전체 결과와 관계없이 수정 중인 eval이 통과할 때까지 반복하세요([검증 대상 선택하기](../concepts/runs.md#verify-and-runs)와 [Run 시작, 진행 상황 확인, 취소](../concepts/runs.md#submit-observe-and-cancel) 참고). 판정은 `run_show`나 `request_show`에서 확인하세요. 아직 제출되지 않은 선행 승인 요청이 검증 범위에 있으면 `PROVISIONAL`도 통과로 봅니다. 개별 eval을 수정하는 중에 다른 필수 eval에서 `RED`나 `ERROR`가 나오면 그 eval을 다음에 고치고, `STALE`이거나 아직 검증하지 않은 항목은 전체 검증 때 확인합니다. 선행 eval 때문에 `BLOCKED` 상태라면 그 선행 `RED`·`ERROR` eval부터 먼저 해결합니다. 사람이 `RED`로 판정하면 검토 대상을 요청 내용에 맞게 고칩니다. 그다음 해당 Artifact를 선택해(`artifactize verify app`, 선행 eval도 함께 실행하려면 `artifactize verify app --recursive`) 선행 조건을 충족한 뒤 새 승인 요청을 기록하고, 승인 결과를 기다리지 않고 다음 작업으로 넘어갑니다.
  2. **전체 검증(Sweep)**: 수정한 `RED` eval이 모두 통과했거나 새 승인 요청만 남은 상태가 되면 프로젝트 전체를 한 번 검증하고(`artifactize verify --all --wait`, 또는 MCP `verify_start`에 `scope: {"kind": "all"}`), [YELLOW를 목표로 작업하기](#working-toward-yellow)에 따라 결과를 확인합니다. 입력과 평가 기준이 그대로인 판정은 재사용하고, 선행 조건을 충족하면 수정의 영향을 받는 하위 eval을 다시 실행합니다. 전체 검증에서 `RED`나 `ERROR`가 나오면 다시 집중 단계로 돌아가고, 승인 외의 미충족 요건(`INCOMPLETE`)이 있으면 먼저 해결하거나 알린 뒤 다시 전체를 검증합니다.
- **Plan B: 예상 영향 범위를 묶어 집중 후 전체 검증**: 공용 명세, 규칙 문서, 코어 라이브러리처럼 다른 eval에 영향을 주는 부분을 고칠 때는 `artifactize config graph app --impact`(MCP `config_graph`에 `scope: {"kind": "artifact", "artifactId": "app"}`과 `impact: true`)로 하위 eval을 찾고 `status`로 재검증이 필요한 대상을 확인합니다. 원래 `RED`였던 eval과 영향받는 하위 eval을 한 Run에서 함께 실행합니다(`--evals app/review,docs/matches-app --speculate --wait`, 또는 MCP `verify_start`에 `scope: {"kind": "evals", "evalIds": ["app/review", "docs/matches-app"]}`와 `speculate: true`). 집중 대상이 순환 참조에 속해 있으면 처음 고른 eval과 수정의 영향을 받는 하위 eval을 그대로 두고, 순환 참조에 속한 Artifact의 모든 eval(이미 집중 중인 Artifact의 나머지 eval 포함)을 같은 Run에 추가합니다. 그다음 마지막에 Plan A와 같이 전체를 검증합니다.
- **Plan C: 매번 전체 검증**: eval 수가 적거나 실행이 빠를 때, 또는 한 번의 수정이 대부분의 eval에 영향을 줄 때는 Plan C를 씁니다. 수정할 때마다 전체 검증(`artifactize verify --all --wait`, 또는 MCP `verify_start`에 `scope: {"kind": "all"}`)을 실행하고 [YELLOW를 목표로 작업하기](#working-toward-yellow)를 따르세요. 단, `RED`, `ERROR`, 또는 승인 외의 `INCOMPLETE` 요건이 나오면 승인 요청을 기다리거나 개별 집중으로 전환하지 않고 바로 문제를 해결한 뒤 전체 검증을 다시 실행합니다.

### YELLOW를 목표로 작업하기 {#working-toward-yellow}

MCP에서는 Run이 `"preparing"`, `"waiting"`, `"running"` 상태인 동안 같은 Run ID로 `verify_wait`를 반복 호출하고, Run이 끝나면 `verify_result`(`ready: true`)와 `run_show`의 `validation.obligations`를 확인합니다. `verify_wait`가 `"waiting_human"`을 반환하거나 CLI에서 실행 가능한 자동 검사가 더 이상 없을 때는 결과를 바로 점검합니다(`Ctrl+C`로 `--wait` 표시만 중단하는 방법은 [Run 시작, 진행 상황 확인, 취소](../concepts/runs.md#submit-observe-and-cancel) 참고). 먼저 `run_show`나 `artifactize run show RUN_ID`에서 `requests`와 `evidence`를 확인하세요. Run이 아직 진행 중일 때는 eval과 `parts`가 없고 `basis = true`도 아닌 Artifact가 검증 요건으로 남아 있는지 `config_graph`나 `status`로 확인합니다. 어떤 자동 검사나 Human 요청이 `RED` 또는 `ERROR`이면 남은 작업을 기다리지 않고 바로 처리합니다(Plan A·B는 집중 단계로 복귀, Plan C는 수정 후 전체 재검증). 승인 외의 요건이 미충족(`INCOMPLETE`)이면 해결하거나 알린 뒤 다시 전체를 검증합니다. 아직 실행 중이거나 실행 가능한 자동 검사가 남아 있으면 `verify_wait`를 계속 호출합니다. 모든 자동 검사가 통과(`GREEN` 또는 `PROVISIONAL`)하고 다른 미충족 요건 없이 미제출 승인만 남으면([승인과 Run의 종료](../concepts/runs.md#sign-offs-and-the-end-of-a-run) 참고), 에이전트는 세션을 열어 둔 채 기다리지 않고 다른 작업을 계속하며 `request_list`와 `request_show`로 대기 중인 요청을 확인해 사람 리뷰어에게 전달합니다(`artifactize review REQUEST_ID`). 에이전트는 사람 대신 승인 요청을 맡거나 판정을 제출하지 않습니다.
