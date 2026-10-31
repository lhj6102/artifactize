# Focus then sweep: verify를 반복하는 방법 {#focus-then-sweep-iterate-with-verify}

코딩 어시스턴트가 RED eval을 고치면 고친 Artifact의 내용이 달라지므로, 그 Artifact를 읽는 eval은
GREEN이었던 것까지 모두 다시 리뷰해야 합니다. 수정할 때마다 `verify --all`을 실행하면 반복할
때마다 시간과 비용이 들지만, eval 하나를 겨냥한 수정이 다른 eval을 깨뜨리는 경우는 드뭅니다.
**focus-then-sweep** 스킬은 어시스턴트가 고치고 있는 eval만 다시 실행하고, 나머지는 마지막에
한 번에 리뷰하도록 안내합니다.

스킬 파일은
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/focus-then-sweep/SKILL.md)이며,
[플러그인](plugin.md)에 함께 들어 있습니다. Claude Code에서는 `/artifactize:focus-then-sweep`로
호출하고, Codex에서는 `$artifactize:focus-then-sweep`를 입력하세요. Run 결과가 RED로 나오면
어시스턴트가 알아서 이 스킬을 쓰기도 합니다. 다른 호스트에서는 [Hey Arti](hey-arti.md#get-the-skill)와
같은 방법으로 원본 파일을 그대로 대화에 첨부하세요.

## 세 가지 방식 {#the-three-plans}

어시스턴트는 작업마다 하나를 고르며, 기본은 방식 A입니다. 어시스턴트는
[MCP 도구](mcp.md)로 답할 수 있으면 MCP 도구를 쓰고, 그렇지 않으면 CLI를 씁니다.

- **방식 A: 먼저 focus, 마지막에 sweep**
  - **Focus**: RED eval 하나를 고친 뒤 GREEN이 될 때까지 그 eval만 다시 실행하고
    (`artifactize verify --eval app/review --speculate --wait`, 또는 scope
    `{"kind": "eval", "evalId": "app/review"}`와 `speculate: true`로 `verify_start`),
    그다음 RED eval로 넘어갑니다. 서로 겹치지 않는 수정이라면 여러 RED eval을 Run 하나에서
    함께 focus할 수 있습니다(`--evals app/review,docs/matches-app`, 또는 scope
    `{"kind": "evals", "evalIds": [...]}`). 수정 때문에 다시 리뷰해야 하는 eval은 반복할
    때마다 실행하지 않습니다. 고칠 대상은 그 eval이 평가하는 Artifact이며, GREEN을 받으려고
    eval을 느슨하게 바꾸지 않습니다.
    focus Run의 결과는 `verify_start` 다음에 `verify_wait`와 `verify_result`로 확인합니다.
    `verify_result`는 실패한 eval과 그 요청 ID를 알려 주고, 리뷰어가 보고한 결과는
    `request_show`로, Runtime eval의 출력은 `request_logs`로 봅니다(CLI에서는
    `artifactize run show RUN_ID --json`). 다시 리뷰해야 하는 eval의 결과가 아직 없으므로 이런
    Run은 보통 `Unmet obligation:` 줄과 함께 INCOMPLETE(종료 코드 4)로 끝납니다. 예상한
    결과이므로 어시스턴트는 focus한 eval의 판정만 봅니다. BLOCKED eval에는
    `Dependency verdict RED: core/tests`처럼 막고 있는 RED eval이나 ERROR로 끝난 eval이
    나옵니다. 어시스턴트는 그 eval을 먼저 고치고 focus합니다. ERROR는 검사를 실행하지 못했다는
    뜻이므로, 어시스턴트는 판정이 아니라 그 원인을 고칩니다.
    `--speculate`를 붙이면 eval이 참조하는 입력이 수정 때문에 다시 리뷰 대상이 되었어도 그
    eval을 실행합니다. RED 승인을 다시 받을 때는 eval 대신 그 Artifact를 선택합니다. 같은
    Artifact의 자동 eval이 모두 GREEN이 된 뒤에야 사람에게 승인을 요청하기 때문입니다.
  - **Sweep**: RED eval이 모두 GREEN이 되면 전체를 한 번 실행합니다.
    `artifactize verify --all --wait`를 쓰거나, scope `{"kind": "all"}`로 `verify_start`를
    호출한 뒤 `verify_wait`와 `verify_result`로 결과를 봅니다. 바뀌지 않은 결과는 focus Run의
    결과까지 포함해 재사용하고, 다시 리뷰해야 하는 eval만 실행합니다. 새로 RED가 나오면 다시
    focus로 돌아가고, sweep이 GREEN이나 YELLOW로 끝나면 반복을 마칩니다.
- **방식 B: 영향받을 eval까지 focus, 마지막에 sweep**: 공유 명세나 규칙 문서를 바꾼 경우처럼
  수정이 다른 eval에 영향을 줄 수 있으면, focus할 때 그 eval도 함께 실행합니다. 어시스턴트는
  `config graph ARTIFACT --impact`(MCP에서는 `impact`를 켠 `config_graph`)와 `status`로 그
  eval을 찾습니다. 순환에 속한 eval은 함께 실행합니다. Artifact를 선택하면 같은 순환에 속한
  Artifact의 eval도 함께 실행됩니다.
- **방식 C: 매번 sweep**: 수정할 때마다 `verify --all`을 실행합니다. focus로 절약할 수 있는 것이 적을 때
  씁니다. eval이 적거나 비용이 적을 때, 대부분의 eval이 참조하는 입력을 바꿨을 때, 여러 RED eval의
  원인이 하나일 때가 그렇습니다.

선택자와 Run에 필요한 결과는 [Run](../features/run.md#verify-and-runs)에, `--speculate`가
바꾸는 점은 [추측 실행](../features/run.md#speculation)에 있습니다.

## YELLOW를 목표로 {#toward-yellow}

반복의 목표는 나머지가 모두 통과하고 승인만 남은 YELLOW입니다. 승인은 다른 eval을 막지
않습니다. 아무도 승인 요청을 맡지 않은 채 [유휴 유예 시간](../features/run.md#sign-offs-and-the-end-of-a-run)
(따로 바꾸지 않았다면 2분)이 지나면 Run은 YELLOW로 끝납니다. 어시스턴트는 승인할 요청이 남아
있어도 작업을 계속하고, 남은 요청을 알려 주며 `artifactize review REQUEST_ID` 같은 방법으로
사용자에게 넘깁니다. 사용자 대신 승인 요청을 맡거나 제출하지 않습니다. 승인 결과를 기다리지 않고
작업을 마치며, 상태를 반복해서 확인하거나 대화를 이어 가지 않습니다. 사용자가 나중에
승인하면 다음 verify가 그 승인을 재사용합니다. 사용자가 승인하기 전까지 YELLOW는 승인된 상태가
아닙니다.
