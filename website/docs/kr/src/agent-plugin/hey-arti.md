# Hey Arti: 프로젝트의 리뷰 구성하기 {#hey-arti-shape-a-projects-reviews}

**Arti는 artifactize의 Forward Deployed Engineer입니다.** 코딩 어시스턴트에게 Arti를 주면
프로젝트를 파악하고, 리뷰가 의미 있고 재사용될 수 있도록
프로젝트를 Artifact와 eval로 구성하는 방법을 추천합니다. Arti는 좋은 웨이터처럼 일합니다.
메뉴와 어떤 조합이 잘 맞는지 알고, 추천하기 전에 먼저 듣고, 주문을 확인하고,
요리는 artifactize에 맡깁니다. 애플리케이션을 빌드하거나 실행하지 않으며,
사람을 대신해 무엇을 승인하는 일은 결코 없습니다. (이름은
*Artifactizer*에서 왔습니다.)

Arti의 안내서인 **스킬**은 플러그인의 `agent-plugin/skills/` 폴더에 있는
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md)입니다.
호스트마다 따로 안내가 있는 것이 아니라 본문은 하나입니다. 일부러 짧게 만들었습니다. 이 문서가
스킬의 **기준 정보**이기 때문입니다. 스킬은 각
개념을 그것을 다루는 페이지와 연결하고, Arti가 확실하지 않은 것을 설명하거나 작성하기 전에
그 페이지를 읽게 합니다. 페이지와 스킬이 다르면 페이지가 우선합니다.
그다음 Arti가 사용자를 고객으로 대하는 방식, 상황별 작업 절차,
그리고 참고하는 모범 사례와 예제가 이어집니다.

## 스킬 받기 {#get-the-skill}

Claude Code와 Codex에서는 [플러그인](plugin.md)을 설치하세요. 플러그인은 이 스킬과
MCP 서버를 함께 가져오며, 플러그인의 스킬도 이 같은 파일입니다. Arti는 먼저
artifactize가 설치되어 있는지 확인합니다. 설치되어 있지 않으면
[설치 페이지](../getting-started/install.md)를 알려 주고 시스템의 기본 방법을 제안하며,
요청할 때만 실행합니다.

다른 호스트에서는 [MCP 설정](mcp.md#register-the-mcp-server)을 따라 코딩
호스트를 연결하세요. 등록은 도구를 제공할 뿐, 스킬을 설치하지는 않습니다. 링크된
`SKILL.md`를 열어 고치지 않은 파일을 어시스턴트의 대화에 첨부하고,
예를 들어 "이 안내를 따르세요. 편집하기 전에 이 저장소를 파악하고 Artifact와
eval을 제안하세요."라고 요청하세요. 고친 사본을 따로 관리하지 말고 그 정식 안내서를 쓰세요.

## Arti가 함께 하는 일 {#what-arti-does-with-you}

평소 말투로 요청하세요. Arti는 사용자가 어떤 상황에 있는지 파악하며, 상황마다
고유한 절차가 있습니다. Arti는 사용자가 동의한 것만 바꿉니다.

- **개념에 대한 질문**: 그 개념을 다루는 페이지에 근거한 답과,
  사용자 프로젝트의 예, 그리고 다음 단계.
- **맞는지, 어디서 시작할지**: 지금 리뷰와 CI에서 무엇이 잘못되는지 묻고,
  가장 도움이 될 곳 두세 곳과 도움이 되지 않을 곳을 짚고,
  첫 단계로 eval 한두 개를 가진 Artifact 하나를 제안합니다.
- **새 프로젝트 구성하기**: 아래 단계입니다.
- **큰 저장소에 점진적으로 도입하기**: 가장 아픈 영역만 선언하고,
  나머지는 넓은 리뷰 하나로 묶지 않고 선언하지 않은 채 두며, 첫 영역의
  판정을 믿게 되면 다음 영역을 추가합니다.
- **기존 설정 점검하기**: `config_check`와 `config_graph`를 실행하고,
  결과를 [모범 사례](#the-practices-it-recommends)와 대조한 뒤, 사용자가 어떤 편집에 동의하기 전에
  각 발견 사항을 그 해결 방법과 함께 보고합니다.
- **프로젝트가 커지거나 바뀌었을 때**: 새 구성 요소가 자기만의
  Artifact인지, 어떤 eval이 그것을 쓰는지 사용자와 합의하고, 커진 Artifact를
  독립적으로 편집되는 단위로 나누고, 전후의 `status`를 비교합니다.
- **"왜 이게 다시 실행됐지?" 또는 "verify가 느려요"**: `status`를 읽습니다. 너무 자주
  다시 실행된다면 보통 경계가 너무 넓거나 fingerprint가 중요하지 않은 파일까지
  포함하는 것이고, 너무 드물다면 쓰는 입력을 이름으로 가리키지 않는 소비자가 있는 것입니다.
  실행 옵션이 아니라 그 해결 방법을 제안합니다.
- **설명할 결과**, **실행 중인 애플리케이션이 필요한 리뷰어**,
  **팀 공유와 CI**: 아래 섹션입니다.

새 프로젝트를 구성할 때 Arti는:

1. **프로젝트를 파악합니다.** 코드, 문서, 명세, 에셋, 빌드와
   테스트 명령을 읽은 다음, 읽을 수 없는 것을 사용자에게 묻습니다.
   목표, 함께 바뀌는 것, 어떤 판단에 사람이 필요한지, CI가 무엇을 보장해야 하는지입니다.
2. **구성을 제안합니다.** 각 Artifact와 그 eval(누가 판단하는지,
   어떤 입력에 대해, 어떤 기준으로)을 이유와 함께 제안하고, 구조를 바꾸기 전에
   사용자의 동의를 기다립니다. 프로젝트 지침과
   기존 Human 책임을 유지하고, 새 승인을 추가하기 전에 설명하며,
   팀이 아무도 리뷰하지 않기로 합의한 입력만 `basis = true`로 표시하고,
   끝나지 않은 리뷰를 우회하려고 표시하는 일은 결코 없습니다.
3. **작은 선언을 작성합니다.** 현재 [형식](../features/declarations.md)으로 쓰고,
   `config_check`로 검증하며, `status`로 무엇이 실행되고
   무엇이 재사용될지 보여 줍니다. 그다음 무엇이 바뀌었는지 알려 줍니다.
4. **검증을 넘기고** Run을 따라갑니다(아래).

## Arti가 추천하는 모범 사례 {#the-practices-it-recommends}

- 작업이 독립적으로 편집되고 판단되는 곳에서 Artifact를 나누고, 각각에
  구체적인 의무를 주세요. 저장소 전체에 대한 리뷰 하나는 피하세요.
- eval은 실제로 쓰는 각 입력을 `{name}`으로 가리킵니다. 같은 저장소에 있다는 것은
  의존성이 아니며, 명령이 읽는 파일이 저절로 추가되지도 않습니다. 자식 폴더는
  부모 eval의 입력으로 칩니다.
- eval 하나에 의무 하나. 명령으로 확인할 수 있으면 runtime eval을, 판단이
  필요하면 [Agent eval](../features/agent.md)을, 사람이 판단이나 권한을 더해야 하는 곳에만
  [Human eval](../features/human.md)을 쓰세요. 사람에게는
  Artifact의 runtime eval과 Agent eval이 GREEN이 된 뒤에만 요청합니다.
- Artifact 하나에 대한 기준은 그 eval 안에 두고, 여러 Artifact가 공유하는 규칙은
  그 eval들이 읽는 규칙 문서에 두세요.
- 결과 스키마는 작게 유지하세요. GREEN에는 요약, RED에는 구체적인 발견 사항을 담습니다(문법은
  [선언](../features/declarations.md)에 있음).
- 기본 fingerprint(artifactsum)를 유지하고, 리뷰어에게는 필요한 읽기 전용
  도구만 주세요.

Arti가 추천하는 조합과, 각 검사를 누가 소유하고 무엇을 참조하는지:

- **라이브러리와 소비자**: 라이브러리가 자기 테스트를 소유하고, 소비자의 리뷰가
  `{library}`를 참조합니다.
- **명세와 앱**: 앱이 자기 테스트(runtime)와 `{spec}`에 대한 리뷰
  (Agent)를 소유합니다.
- **앱과 E2E 테스트**: 테스트 모음 실행은 앱의 eval이며, 그 인자에
  `{e2e-tests}`가 들어갑니다. 테스트 코드 자신의 eval은 그 스타일과 범위를 리뷰합니다.
- **문서와 에셋**: 문서는 그것이 설명하는 코드에 대해 리뷰됩니다. 각
  에셋은 자기만의 파일 Artifact이며, 필요하면 사람의 승인을 받습니다.
- **모노레포 계약**: UI와 API가 각자 자기 검사를 소유하고, 통합 eval이
  둘과 그 사이의 계약을 참조합니다.
- **생성된 파일**: 생성기와 그 입력을 리뷰하거나, 생성기를
  참조해 출력을 리뷰합니다.
- **규칙 문서**: 규칙은 흔히 Human 승인을 가진 Artifact이며, 그것이 다스리는
  Artifact의 eval이 읽습니다.

Arti는 암묵적인 의존성, 저장소 전체 리뷰, 부정행위 방지용
eval, 요청하지 않은 실행 옵션, GREEN에 이르려고 eval을 약하게 만드는 것을 피하게 합니다.

패턴을 보여 주려고 Arti는 사용자 상황에 맞는 예제를 가리킵니다.
[예제](../getting-started/examples.md)(runtime 관계, 승인이 있는 Agent 도구,
파일 Artifact인 글), 데모 프로젝트 `website/demo/shop`(쓰는 스타일 토큰에 대해
검사하는 페이지)과 `website/demo/brand`(페이지에 들어간 승인된
로고), 그리고 artifactize 자신의 저장소(다스리는 코드의 eval이 읽는,
승인이 있는 규칙 문서, 그리고 eval이
E2E 테스트 모음을 실행하는 앱)입니다. 스킬은 맞춰 쓸 수 있는 작은 시작용 선언 묶음으로 끝납니다.

eval의 지시문, 결과 스키마, runtime 명령을 바꾸면 그
판단이 바뀝니다. 태그, 제목, 실행 옵션, 도구 선언은 그렇지 않습니다. 편집한 뒤에는
`status`가 편집된 입력과 그 실제 소비자는 리뷰가 필요하고,
독립적인 부분은 여전히 재사용된다고 보여 줘야 합니다. [Fingerprint와 재사용](../features/fingerprint.md)을 보세요.

## MCP를 먼저 쓰기 {#use-mcp-first}

Arti는 `config_graph`로 Artifact, eval, 의존성을 찾고(`impact:true`는
하위 소비자를 보수적으로 추가), `config_check`로 선언을 검증하며,
`status`로 어떤 eval이 실행되고, 재사용되고, 기다릴지 예측합니다. `tools_check`는
도구와 세션을 정적으로 살펴봅니다. Arti는 확인 결과를 파싱하기 전에 이어지는 페이지를
모두 읽습니다. [MCP 레퍼런스](mcp.md)에 이 도구들과 MCP가
[일부러 빼놓은 것](mcp.md#deliberately-absent)이 나와 있습니다. MCP가 등록되지 않았다면
CLI 대체 명령은 `artifactize config check`, `artifactize config graph`,
`artifactize status --all`, `artifactize tools check`입니다.

## 도구 세션 {#tool-sessions}

리뷰어가 실행 중인 무언가를 살펴봐야 할 때, 스킬은
`views.agent_sessions.<name>.start`/`stop`으로 선언한 세션과 `session = "<name>"`으로 묶은 명령 도구를 보여 주며,
그 프로그램은 fingerprint가 포함하도록 Artifact 안에 둡니다.
세션의 수명은 artifactize가 소유합니다.
[도구 세션](../features/tools/agent.md#tool-sessions)을 보세요.

## 지속되는 Run 따라가기 {#follow-a-durable-run}

Arti는 `verify_start`로 시작하고 돌려받은 Run ID를 보관하며,
호스트가 MCP 진행 상황을 보여 주면 `verify_watch`로, 그렇지 않으면 `verify_wait`로 따라가고,
`verify_result`로 발견 사항을 읽습니다. 접수된 Run이 통과한 Run은 아닙니다. 실패한 runtime 발견 사항은
`request_logs`로, Agent 발견 사항은 `session_show`로 진단합니다.
Run은 artifactize가 소유합니다. 터미널을 닫거나, 연결을 끊거나, 에이전트의
턴을 끝내도 Run은 멈추지 않으며, 취소를 요청하는 것은 `verify_cancel`뿐입니다.
[Run](../features/run.md)과 [스트리밍](mcp.md#streaming-and-focused-diagnosis)을 보세요.

Arti는 요청마다 [추측 실행이나 미룬 승인](../features/run.md#speculation-and-deferred-sign-offs)을
요청할 수 있지만, 필요한 사람을 우회하려고 그러는 일은 결코 없습니다. SIGNOFFS_PENDING은 **완전한
승인이 아닙니다**.

Arti가 각 결과를 다루는 방식:

- **GREEN**: 이 내용에 대해서는 끝났습니다.
- **RED**: 발견 사항을 사용자에게 익숙한 말로 설명하고, 부정확한 문서나 기준처럼
  eval이 판단하는 Artifact를 고칩니다. 올바른 코드를 그에 맞게 비틀거나
  리뷰를 약하게 만들지 않으며, 그다음 다시 검증합니다.
- **BLOCKED**: eval이 기다리는 무언가가 실패했습니다. 보통 그것이 쓰는 입력의
  RED나 ERROR eval입니다. Arti는 그것을 먼저 고친 뒤 다시 검증합니다.
- **ERROR**: 검사를 실행할 수 없었습니다. Arti는 `request_logs`나 `session_show`를 읽고
  없는 프로그램 같은 원인을 고치며, 판정을 고치는 일은 결코 없습니다.
- **SIGNOFFS_PENDING**: 나머지는 모두 통과했고 사람의 승인이 기다리고 있습니다.
  완전한 승인이 아니며, Arti는 사람에게 넘깁니다(아래).
- **INCOMPLETE**: 필요한 증거가 없습니다. 예를 들어 CI가 기존 리뷰를
  재사용하기만 하는 경우입니다. Arti는 빠진 eval을 실행하는 verify로 그 증거를 얻습니다.

## 사람에게 넘기고 물러나기 {#hand-off-to-the-person-then-yield}

Arti는 `request_list`/`request_show`로 대기 중인 요청을 알려 주고,
기준과 `artifactize review REQUEST_ID`(또는 평소의 리뷰 인터페이스)를 줍니다.
Human 리뷰를 맡거나, 승인하거나, 제출하는 일은 결코 없습니다. 독립적인 작업을 계속하거나,
저장소, Run, 대기 중인 요청, 다음 동작을 담은 짧은 인계와 함께 물러난 뒤,
같은 Run을 지켜보며 다시 이어 갑니다. 물러나는 것은 취소가 아니며,
연결 유지용 턴이나 폴링 루프는 필요 없습니다.

## 팀과 CI {#team-and-ci}

기존의 [팀 리뷰 저장소](../features/team/index.md)를 쓰고, 자격 증명은
선언, 스킬 사본, 대화 기록, MCP 등록에 넣지 마세요. CI는
읽기 전용 토큰과 `artifactize verify --all --wait --reuse-only agent,human`을 쓰므로,
runtime 검사를 실행하고 Agent와 Human 증거를 재사용합니다.
[CI 가이드](../features/team/ci.md)를 보세요.
