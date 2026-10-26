# Hey Arti: 프로젝트의 리뷰 구성하기 {#hey-arti-shape-a-projects-reviews}

코딩 어시스턴트에게 "Hey Arti, 이 프로젝트는 어떻게 리뷰해야 할까?"라고 물어보세요. Arti는
프로젝트를 파악하고, 무엇이 중요한지 묻고, 리뷰가 의미 있고 재사용될 수 있도록 Artifact와 eval을
제안합니다. 선언은 동의를 받은 뒤에만 바꿉니다. 애플리케이션을 빌드하거나 실행하지 않으며, 사람을
대신해 승인하지도 않습니다.

Arti의 안내서인 **스킬**은
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md)이며,
[플러그인](plugin.md)에 함께 들어 있습니다.

## 스킬 받기 {#get-the-skill}

Claude Code와 Codex에서는 [플러그인](plugin.md)을 설치하세요. 플러그인은 이 스킬 파일과 MCP 서버를 함께 제공합니다. Arti는 먼저
artifactize가 설치되어 있는지 확인합니다. 설치되어 있지 않으면 [설치 페이지](../getting-started/install.md)를 안내하고, 사용 중인 시스템에 맞는 기본 설치 방법을 제안합니다. 설치는 요청을 받은 경우에만 진행합니다.

다른 호스트에서는 [MCP 설정](mcp.md#register-the-mcp-server)을 따라 코딩
호스트를 연결하세요. 등록은 도구를 제공할 뿐, 스킬을 설치하지는 않습니다. 링크된
`SKILL.md` 파일을 고치지 말고 원본 그대로 어시스턴트의 대화에 첨부하고,
예를 들어 "이 안내를 따르세요. 편집하기 전에 이 저장소를 파악하고 Artifact와
eval을 제안하세요."라고 요청하세요. 고친 사본을 따로 관리하지 말고 그 정식 안내서를 쓰세요.

## Arti가 함께 하는 일 {#what-arti-does-with-you}

평소 말투로 요청하세요. Arti는 사용자가 어떤 상황에 있는지 파악하고 상황에 맞는 절차로 진행합니다. Arti는 사용자가 동의한 것만 바꿉니다.

- **개념에 대한 질문**: 그 개념을 설명하는 페이지를 바탕으로 답하고, 사용자 프로젝트에 적용한 예와 다음 단계를 제시합니다.
- **도입할 만한지, 어디서 시작할지**: 지금 리뷰와 CI에서 무엇이 문제인지 묻고,
  가장 도움이 될 곳 두세 곳과 도움이 되지 않을 곳을 짚고,
  첫 단계로 eval이 한두 개 있는 Artifact 하나를 제안합니다.
- **새 프로젝트 구성하기**: 아래에 설명한 순서대로 진행합니다.
- **큰 저장소에 점진적으로 도입하기**: 문제가 가장 큰 영역부터 선언하고, 나머지는 선언하지 않은
  채 둡니다. 저장소 전체를 리뷰 하나로 묶지는 않습니다. 첫 영역의 판정을 신뢰할 수 있게 된 뒤
  다음 영역을 추가합니다.
- **기존 설정 점검하기**: `config_check`와 `config_graph`를 실행하고,
  결과를 [모범 사례](#the-practices-it-recommends)와 대조하고, 문제점과 해결 방안을 먼저 설명한 뒤 수정해도 되는지 사용자에게 묻습니다.
- **프로젝트가 성장하거나 바뀌었을 때**: 새 구성 요소를 별도 Artifact로 둘지, 어떤 eval이 그것을 쓰는지 사용자와 합의하고, 너무 커진 Artifact는
  따로 편집할 수 있는 단위로 나누고, 변경 전후의 `status`를 비교합니다.
- **"왜 이게 다시 실행됐지?" 또는 "verify가 느려요"**: `status`를 읽습니다. 너무 자주
  다시 실행된다면 Artifact의 범위를 너무 넓게 잡았거나 fingerprint에 불필요한 파일까지
  포함했기 때문일 수 있고, 너무 드물다면 입력을 쓰는 eval이 그 Artifact를 이름으로 참조하지 않았을 수 있습니다.
  실행 옵션을 조정하는 대신, 찾은 원인을 해결할 방법을 제안합니다.
- **결과 설명**, **리뷰어가 실행 중인 애플리케이션을 살펴봐야 하는 경우**, **팀 공유와 CI 설정**은 아래 섹션에서 다룹니다.

새 프로젝트를 구성할 때 Arti는 다음 순서로 진행합니다.

1. **프로젝트를 파악합니다.** 코드, 문서, 명세, 에셋, 빌드와
   테스트 명령을 읽은 다음, 파일만으로는 알 수 없는 목표, 함께 바뀌는 것, 사람의 판단이
   필요한 부분, CI가 보장해야 하는 것을 사용자에게 묻습니다.
2. **구성을 제안합니다.** 누가 어떤 입력을 어떤 기준으로 판단할지를
   담아 각 Artifact와 그 eval을 이유와 함께 제안하고, 구조를 바꾸기 전에
   사용자의 동의를 받습니다. 프로젝트 지침을
   따르고 기존 Human 리뷰 담당자와 승인 책임은 그대로 유지하며, 새 승인 절차를 추가하기 전에 사용자에게 설명합니다.
   팀이 아무도 리뷰하지 않기로 합의한 입력만 `basis = true`로 표시하며,
   끝나지 않은 리뷰를 우회하려는 목적으로는 절대 표시하지 않습니다.
3. **필요한 내용만 담아 간단한 선언을 작성합니다.** 현재 [형식](../features/declarations.md)으로 쓰고,
   `config_check`로 검증하며, `status`로 무엇이 실행되고
   무엇이 재사용될지 보여 줍니다. 그다음 무엇이 바뀌었는지 알려 줍니다.
4. **검증 실행을 artifactize에 맡기고** Run의 진행 상황을 확인합니다(아래).

## Arti가 추천하는 모범 사례 {#the-practices-it-recommends}

- 독립적으로 고치고 리뷰할 수 있는 단위로 Artifact를 나누고, 각각의
  검증 요건을 구체적으로 정하세요. 저장소 전체에 대한 리뷰 하나는 피하세요.
- eval은 실제로 쓰는 각 입력을 `{name}`으로 가리킵니다. 같은 저장소에 있다고 해서
  의존 관계가 생기지는 않으며, 명령이 읽는 파일도 eval의 입력에 자동으로 추가되지는 않습니다. 자식 폴더는
  부모 eval의 입력으로 간주합니다.
- eval 하나에는 확인할 것을 하나만 두세요. 명령으로 확인할 수 있으면 Runtime eval을, 판단이
  필요하면 [Agent eval](../features/agent.md)을, 사람이 직접 판단하거나 승인해야 하는 경우에만
  [Human eval](../features/human.md)을 쓰세요. 사람에게는
  Artifact의 Runtime eval과 Agent eval이 GREEN이 된 뒤에만 요청합니다.
- Artifact 하나에 대한 기준은 그 eval 안에 두고, 여러 Artifact가 공유하는 규칙은
  그 eval들이 읽는 규칙 문서에 두세요.
- 결과 스키마는 작게 유지하세요. GREEN에는 요약을, RED에는 구체적인 지적 사항을 담습니다(문법은
  [선언](../features/declarations.md)에 있습니다).
- 기본 fingerprint(artifactsum)를 유지하고, 리뷰어에게는 필요한 읽기 전용
  도구만 주세요.

아래 표는 Arti가 추천하는 조합별로 어떤 Artifact에 eval을 선언하고, 각 eval이 무엇을 참조해야 하는지 안내합니다.

- **라이브러리와 그 사용처**: 라이브러리에 테스트를 두고, 라이브러리를 쓰는 코드의 리뷰에서는
  `{library}`를 참조합니다.
- **명세와 앱**: 앱에는 앱의 테스트를 실행하는 Runtime eval과 `{spec}`을 기준으로 리뷰하는 Agent eval을 둡니다.
- **앱과 E2E 테스트**: 테스트 모음을 실행하는 eval은 앱에 선언하며, 그 인자에 `{e2e-tests}`가 들어갑니다. 테스트 코드의 eval에서는 코드 스타일을 리뷰하고, 테스트가 각 기능을 충분히 검증하는지 확인합니다.
- **문서와 에셋**: 문서가 설명하는 코드와 문서 내용이 맞는지 리뷰합니다. 각
  에셋은 각각 별도의 파일 Artifact이며, 필요하면 사람의 승인을 받습니다.
- **모노레포의 연동 규약**: UI와 API에 각각 eval을 두고, 통합 eval에서
  두 구성 요소와 그 사이의 인터페이스 규약을 참조합니다.
- **생성된 파일**: 생성기와 그 입력을 리뷰하거나, 생성기를
  참조해 출력을 리뷰합니다.
- **규칙 문서**: 규칙 문서는 대개 Human 승인을 받는 Artifact로 선언하며,
  그 규칙이 적용되는 Artifact의 eval이 읽습니다.

Arti는 암묵적인 의존성, 저장소 전체를 한 번에 리뷰하는 eval, Builder가 규칙을 어기는지 감시하는 데만 쓰는 eval(예: 아무도 테스트를 고치지 않았는지 해시로 확인하는 eval), 요청하지 않은 실행 옵션, GREEN을 얻으려고 판단 기준을 완화하는 일을 피하라고 안내합니다.

Arti는 사용자 상황에 맞는 예제로 리뷰 구성 방법을 보여 줍니다.
[예제](../getting-started/examples.md)에서는 Runtime 관계, 승인이 있는 Agent 도구,
파일 Artifact로 선언한 글을 볼 수 있습니다. 데모 프로젝트 `website/demo/shop`에서는 페이지가
사용하는 스타일 토큰을 기준으로 검사하고, `website/demo/brand`에서는 페이지에 들어간
로고의 승인을 받습니다. artifactize 자체 저장소에서는 승인받은 규칙 문서를
그 규칙이 적용되는 코드의 eval이 읽고, 앱의 eval이
E2E 테스트 모음을 실행합니다. 스킬 안내서 끝에는 프로젝트에 맞게 고쳐 쓸 수 있는 간단한 시작용 선언 예제가 있습니다.

eval의 지시문, 결과 스키마, Runtime 명령을 바꾸면 그
판단이 바뀝니다. 태그, 제목, 실행 옵션, 도구 선언은 그렇지 않습니다. 수정한 뒤 `status`를 확인하면, 바뀐 입력과 그 입력에 의존하는 대상만 다시 리뷰하고 나머지 결과는 재사용한다고 표시되어야 합니다. [Fingerprint와 재사용](../features/fingerprint.md)을 보세요.

## MCP를 먼저 쓰기 {#use-mcp-first}

프로젝트에 어떤 Artifact, eval, 의존성이 선언되어 있고 변경이 어디에 영향을 주는지, 선언이
올바른지, 어떤 eval을 실행하거나 재사용하거나 대기시킬지, 도구와 세션이 제대로 설정되어
있는지 Arti에게 물어보세요. Arti는 artifactize의 [MCP 도구](mcp.md)가 등록되어 있으면 그것으로,
없으면 `artifactize config check`, `artifactize config graph`, `artifactize status --all`,
`artifactize tools check`로 확인해 답합니다.

## 도구 세션 {#tool-sessions}

리뷰어가 실행 중인 프로그램을 살펴봐야 할 때, Arti는
`views.agent_sessions.<name>.start`/`stop`으로 선언한 세션과 `session = "<name>"`으로 묶은 명령 도구를 보여 주며,
프로그램 파일도 Artifact의 fingerprint에 반영되도록 해당 Artifact 안에 둡니다.
세션의 시작부터 종료까지는 artifactize가 관리합니다.
[도구 세션](../features/tools/agent.md#tool-sessions)을 보세요.

## 계속 실행되는 Run 지켜보기 {#follow-a-durable-run}

Arti는 `verify_start`로 시작하고 돌려받은 Run ID를 보관하며,
호스트가 MCP 진행 상황을 보여 주면 `verify_watch`로, 그렇지 않으면 `verify_wait`로 진행 상황을 확인하고,
`verify_result`로 지적 사항을 읽습니다. Run이 접수되었다고 해서 검증을 통과한 것은 아닙니다. Runtime eval의 실패 원인은 `request_logs`로, Agent eval의 지적 사항은 `session_show`로 살펴봅니다.
Run은 artifactize가 관리합니다. 터미널을 닫거나, 연결을 끊거나, 에이전트의
턴을 끝내도 Run은 멈추지 않으며, Run을 취소하는 방법은 `verify_cancel`뿐입니다.
[Run](../features/run.md)과 [스트리밍](mcp.md#streaming-and-focused-diagnosis)을 보세요.

Arti는 verify 요청마다 [추측 실행](../features/run.md#speculation)을 켤 수 있습니다. 이번 Run에서 실행 대상으로 선택한 승인 요청은
다른 eval의 실행을 막지 않습니다. 선택 범위 밖의 승인은 다른 eval의 실행을 막으므로, scope에 해당 Artifact를 추가하거나
선택 범위를 `{"kind": "all"}`로 넓히지 않으면 Run은 INCOMPLETE로 끝납니다. YELLOW는 **완전한 승인이 아니라** 사람의 승인이 아직 남아 있다는 뜻이며, 그 승인은 사람이 직접 제출해야 합니다.

Arti는 각 결과를 다음과 같이 다룹니다.

- **GREEN**: 현재 내용에 대한 검증이 끝났습니다.
- **RED**: 지적 사항을 사용자에게 익숙한 말로 설명하고, 부정확한 문서나 기준처럼
  eval이 판단하는 Artifact를 고칩니다. 올바른 코드를 잘못된 문서나 기준에 억지로 맞추거나
  리뷰 기준을 완화하지 않으며, 그다음 다시 검증합니다.
- **BLOCKED**: 그 eval을 실행하기 전에 통과해야 할 상위 eval이 실패했습니다. 보통 해당 eval이 입력으로 쓰는 Artifact에서 RED나 ERROR가 난 경우입니다. Arti는 그 입력을 먼저 고친 뒤 다시 검증합니다.
- **ERROR**: eval을 실행할 수 없었습니다. Arti는 `request_logs`나 `session_show`를 읽고
  필요한 프로그램이 없는 등 실행을 막은 원인을 해결하며, 판정 자체를 고치지는 않습니다.
- **YELLOW**: 나머지는 모두 통과했고 사람의 승인만 남았습니다.
  완전한 승인이 아니며, Arti는 사람에게 넘깁니다(아래).
- **INCOMPLETE**: 필요한 증거가 없습니다. 예를 들어 CI가 기존 리뷰를
  재사용하기만 하는 경우입니다. Arti는 `verify`로 빠진 eval을 실행해 부족한 증거를 확보합니다.

## 사람에게 넘기고 물러나기 {#hand-off-to-the-person-then-yield}

Arti는 `request_list`/`request_show`로 대기 중인 요청과 그 리뷰 기준을 알려 주고,
`artifactize review REQUEST_ID`(또는 평소 쓰는 리뷰 인터페이스)로 요청을 여는 방법을 안내합니다.
Human 리뷰를 절대 맡거나, 승인하거나, 제출하지 않습니다. 독립적인 작업을 계속하거나, 저장소, Run, 대기 중인 요청, 다음 할 일을 짧게 정리해 사용자에게 넘깁니다. Run이 아직 끝나지 않았다면 나중에 진행 상황을 확인하고 작업을 이어 갑니다. Run이 YELLOW로 끝났다면 그 결과는 바뀌지 않고, 나중에 제출한 승인은 요청에만 반영되므로, Arti는 verify를 다시 실행해 새 Run이 그 승인을 재사용하게 합니다. 사용자에게 넘기고 응답을 마쳐도
Run은 취소되지 않으므로, 연결을 유지하려고 대화를 이어 가거나 상태를 반복해서 조회할 필요가 없습니다.

## 팀과 CI {#team-and-ci}

기존의 [팀 리뷰 저장소](../features/team/index.md)를 쓰고, 자격 증명은
선언, 스킬 사본, 대화 기록, MCP 등록에 넣지 마세요. CI는
읽기 전용 토큰과 `artifactize verify --all --wait --reuse-only agent,human`을 쓰므로,
Runtime eval을 실행하고 Agent와 Human 증거를 재사용합니다.
[CI 가이드](../features/team/ci.md)를 보세요.
