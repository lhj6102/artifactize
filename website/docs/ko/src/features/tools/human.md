# Human 도구 {#human-tools}

Human 도구 선언 예는 [Human 리뷰](../human.md#human-tools)에 있습니다.
Human 도구는 명령이거나 [내장 도구](builtin.md)(`read`, `list`,
`section`, `help`, `open`, `graph`)입니다. `open`과 `graph`는 실행(launch) 도구이고,
나머지는 출력을 보여 줍니다. 내장 도구는 대상에 대해 같은 자리 표시자와 범위 검사를 쓰며
artifactize 안에서 실행됩니다. 이 페이지의 나머지는 명령에 관한 내용입니다.

인자는 고정된 리터럴 argv이며, 범위 자리 표시자만 쓸 수 있습니다. `{artifactPath}`는
선언한 Artifact의 정규화된 대상 경로(폴더 또는 파일)이고, `{name}`은
Artifact를 가리킵니다. 폴더 참조는 `/path`
접미사를 받으며, 파일 참조는(파일 소유자의 `{artifactPath}` 포함) 이를
거부합니다. 둘 다 `--flag=` 형식을 받으며, runtime argv와 같은
논리적 경로 검사와 심볼릭 링크 금지 검사로 해석됩니다. 다른 중괄호 형식은 거부되며,
임의의 문자열 템플릿이나 이스케이프된 중괄호 치환은 없습니다. 알 수 없는
이름과 잘못된 피연산자 문법은 워크스페이스를 불러올 때 거부됩니다. 도구
피연산자는 결코 의존성을 추가하거나 리뷰의 eval 범위를 넓히지 않습니다. 존재하지만
범위 밖인 Artifact, 없는 입력, 심볼릭 링크는 실행 전에 실패합니다. 입력
메타데이터는 호출할 때마다 확인합니다. 파일 대상이 일반 파일인지, 존재하는지, 심볼릭 링크가 아닌지도
탐색 때와 모든 도구 호출 때 확인하며, `fingerprint = false`에서도 마찬가지입니다.
확인에 실패하면 명령을 실행하지 않습니다.

명령 실행 파일은 파일 Artifact 예외를 포함해 [프로그램 찾기](../others/platforms.md#program-lookup)를
따릅니다. 이는 내장 도구나 피연산자의 범위를 넓히지 않습니다. 셸은 추가되지 않으며, cwd는 선언한 Artifact의 작업 디렉터리
(파일 Artifact라면 들어 있는 폴더)입니다.
Human 명령은 HOME, DISPLAY/WAYLAND_DISPLAY, XDG 설정과 config를 포함해
리뷰어의 **실제 환경 전체**를 물려받습니다.
runtime eval이나 Agent 도구와 달리 전용 TMP/출력 디렉터리를 받지 않으며,
`ARTIFACTIZE_*`를 포함해 모든 변수를 유지합니다.
신뢰하는 명령만 선언하세요. 명령은 리뷰어의 평소 권한과
환경을 가지며, 그 환경에 이미 있는 자격 증명도 포함됩니다.

- `launch`는 stdin을 끊은 채 새 세션/프로세스 그룹을 시작하고, 프로세스 시작에 성공하면
  바로 `{"content":[{"type":"launch","launched":true}],"isError":false}`를 돌려줍니다.
  결과에는 준비 완료 대기도, 내용 캡처도 없습니다.
  리뷰(`review`, monitor, 데스크톱)에서는 stdout과 stderr가 파이프이며,
  artifactize는 프로그램이 파이프를 닫고 종료할 때까지 이를 한 줄씩 읽어 리뷰어의 Tool log에 넣습니다.
  끝 줄에는 종료 코드가 나옵니다. 리뷰를 닫은 뒤에도 파이프는 계속 읽히고
  아무도 보지 않는 내용은 버려지므로, 프로그램이 가득 찬 파이프 때문에 멈추는 일은 없습니다. Unix에서는
  SIGPIPE를 무시하므로, artifactize가 종료한 뒤의 쓰기는 프로그램을 끝내지 않고 실패합니다. `request tool`과 `tool check`는
  stdout과 stderr를 끊어 둡니다. 프로세스를 시작하지 못하면 오류이지만, 시작한 뒤에는 0이 아닌 코드로
  종료하더라도 이미 넘겨준 실행을 되돌리지 않습니다. 프로그램은 artifactize가 종료되거나
  이후에 취소되어도 일부러 계속 실행되며, 언제 닫을지는 리뷰어가 정합니다. `timeout_ms`는
  넘겨진 프로그램을 제한하지 않습니다. launch는 판정도 아니고
  관찰했다는 증거도 아닙니다.
- `output`은 완료를 기다리며, 기본 시간 제한은 120000 ms입니다. 리뷰에서는
  출력되는 대로 줄이 Tool log에 나타납니다. stdout과 stderr는 또한
  각각 최대 128 KiB까지 캡처되고 runtime 출력처럼 정리됩니다. 각각은
  최대 64 KiB인 텍스트 블록이 되며, 잘리면 명시적인 잘림 표시가 붙습니다. 비어 있지 않은 stderr에는
  `stderr:` 라벨이 붙습니다. 0이 아닌 종료나 신호 종료는 `isError: true`를 설정하고, 종료
  상태를 포함하며, 크기가 제한된 텍스트를 유지합니다. 시간이 초과되거나 취소되거나 버려진 호출은 프로그램을 넘겨 계속 실행하지 않고, 일반적인 프로세스 그룹 정리를 거칩니다.

## Tool log {#tool-log}

도구의 stdout과 stderr는 리뷰어의 Tool log에 실시간으로 나타나므로, 도구는
사람이 그 안에서 한 일(고른 선택지, 측정한 값, 저장한 파일)을 출력해
리뷰어가 결과에 복사하게 할 수 있습니다. 로그는 실행마다
`▶ HH:MM:SS · operation · Artifact`를 보여 준 뒤 그 줄들(stderr는 빨간색), 그리고
`■ exit CODE`(또는 `launched`, `done`, `cancelled`, `timed out`, `error: …`)를 보여 줍니다.
내장 도구의 텍스트는 끝날 때 나타나며 `■ done`으로 끝납니다. `open`은
파일을 데스크톱의 여는 프로그램에 넘기고 `■ launched`로 끝납니다. `graph`는 뷰어가 종료할 때까지
그 출력을 기록합니다. 실행 한 번에 최대 1 MiB의 출력을 보여 주고, 나머지는
보여 주지 않는다고 알리며, 4096바이트보다 긴 줄은 여러 조각으로 보여 줍니다. 로그는
리뷰가 열려 있는 동안 메모리에만 있으며, 아무것도 저장되지 않습니다.

데스크톱 앱은 리뷰 밖에서도 Artifact의 Human 도구를
[Detail](../../desktop/index.md#detail)에 나열합니다. 그곳에서 클릭하면 어떤 Run을 보여 주고 있든
브랜치의 워크트리의 현재 상태에서 도구 하나를 바로 실행합니다. 요청,
맡기, fingerprint 확인이 전혀 관여하지 않으며, 어떤 것도 리뷰의
증거로 기록되지 않습니다. 그 줄들은 그 Artifact의 Tool log에 나타나며, 이 로그는
앱이 실행되는 동안 유지됩니다. 라이브러리 호출은 Artifact가 선언한 도구와
그것이 실행할 명령줄을 나열하는 `review::artifact_tools(repo, artifact)`와,
`review::run_artifact_tool(repo, artifact, tool, cancellation, run)`입니다.

내부 호출자는 Human eval 범위에 대해서는 `tools::human::Registry::new(&config, "artifact/eval")`를,
그 자식 범위에 대해서는 `for_artifact(&config, "artifact")`를 쓰고,
그다음 `list()`, `command(name)`(실행하지 않고 해석된 프로그램, argv, 디렉터리를 돌려줌),
`call(name, cancellation).await`, 또는 실행을 Tool log에 기록하는
`call_logged(name, cancellation, run).await`를 씁니다
(`review::log`: `channel()`이 `Sink`를 주고 그 `start`가 `Run`을 열며, 화면은
`Events`를 `ToolLog`에 정리합니다). 이름은
`<operation>_<artifactId>`이며 충돌은 거부됩니다. Agent 도구는 결코 나열되지 않으며,
Agent eval은 Human eval 레지스트리를 만들 수 없습니다. 나열은 아무것도 실행하지 않습니다.
Human 결과는 별도의 텍스트/launch 타입을 쓰며, Agent 결과에는 launch
블록이 들어갈 수 없습니다. 이 하위 수준 레지스트리 작업은 요청을 맡은 사람을 인가하지 않습니다. 기록되는 요청에는
아래의 `human` 수명 주기 API, `request tool`, 또는 `review`를 쓰세요.

## Human 리뷰 {#human-reviews}

Human 리뷰 흐름과 [review](../../tui/index.md#review) 터미널 UI는
[Human 리뷰](../human.md) 가이드에서 다룹니다.

내부 라이브러리는 열린 `store::Receipts`로 비동기 작업을 제공합니다.

- `human::claim(receipts, request_id, reviewer)`는 리뷰어 잠금 하나를 얻습니다.
  같은 리뷰어로 반복하면 멱등이고, 다른 리뷰어는 거부됩니다.
  `human::default_reviewer()`는 `$USER`를 읽습니다(Windows에서는 `USERNAME`으로 대체).
  예약, 갱신,
  만료 타이머, 준비 단계, 준비 완료 훅, 알람은 없습니다.
- `human::unclaim(receipts, request_id, reviewer)`는 판정 없이 그 잠금을 해제해,
  다른 리뷰어가 요청을 맡을 수 있게 합니다. 요청을 맡은 사람만 맡기를 해제할 수 있으며, 요청이 아직 대기 상태일 때만 가능합니다.
- `human::run_human_tool(receipts, request_id, reviewer, tool, cancellation, run)`은
  `run`에 기록하며(기록하지 않으려면 `Run::unwatched()`), 맡은 사람을 인가하고, 기록된 Artifact/eval 범위와 선언을 다시 열고,
  요청에 재사용 키가 있을 때만 등록된 Human 도구를 호출하기 전에 fingerprint를 확인합니다.
  재사용 키가 없는 요청은 fingerprint 값 확인을 건너뛰지만,
  도구 호출마다의 파일 대상 검증은 건너뛰지 않습니다. 도구는
  자유 인자를 받지 않으며 리뷰어의 실제 환경을 씁니다. 범위나
  선언(도구 명령 포함)이 바뀌었으면 갱신된 선언을 실행하는 대신
  재연결을 거부합니다. 일반 도구 오류는
  판정으로 처리되지 않으며, 원인을 고친 뒤 그 동작을 다시 시도할 수 있습니다. Human 도구 실행은 기록되지 않습니다.
- `human::tool_commands(receipts, request_id)`는 기록된 범위를 다시 불러와,
  요청을 맡거나 무엇을 실행하지 않고 명령 도구 이름에서 해석된 명령줄(저장소,
  프로그램, argv, 디렉터리) 또는 도구별 오류로 가는 맵을 돌려줍니다.
  내장 도구에는 명령줄이 없어 빠집니다.
- `human::submit(receipts, request_id, reviewer, result, cancellation)`은
  `pass_schema`/`fail_schema`에 맞는 필드와 함께 GREEN/RED를 받으며, 복구 없이 Agent 결과
  검증기를 씁니다. 잘못되었거나 너무 큰 결과(JSON 256000바이트 초과)는
  요청을 고칠 수 있도록 기다리는 상태로 둡니다. 스키마 오류는 실패한
  인스턴스 경로를 최대 다섯 개까지 나열합니다. 올바른 제출은 요청에 재사용 키가 있을 때만
  fingerprint를 다시 계산합니다. 키가 없는 요청은 fingerprint 값 재확인을 건너뛰지만,
  탐색이나 재연결은 건너뛰지 않습니다. 파일 대상이 없거나 잘못되었거나, 기록된
  범위/선언이 바뀌었으면 제출 전에 재연결이 실패하고 요청은
  기다리는 상태로 남습니다. 재연결에 성공한 뒤 fingerprint 값이 바뀌었으면
  판정 대신 ERROR/INPUT_CHANGED로 확정합니다.
  확정은 맡은 사람과 대기 상태를 트랜잭션으로 다시 확인하므로, 두 번째
  제출은 실패합니다. 확정하면 리뷰어 잠금을 놓고, 저장된 팔로워를 완료하며,
  fingerprint가 있는 GREEN/RED 결과만 캐시에 게시합니다.

`artifactize review` 터미널 UI는
`human::claim`, `run_human_tool`, `unclaim`, `submit`을 프로세스 안에서 호출하며,
`request submit`과 똑같이 제출을 원격 리뷰 저장소에 게시합니다.

`request list`는 저장된 모든 요청(대기 중, 맡은 요청, 확정된 요청)을 포함하며,
선택적으로 `--run`으로 거를 수 있습니다. 텍스트에는 요청/Run/eval ID, 상태, 리뷰어가 들어 있고,
`--json`과 `request show`에는 저장된 감사 기록 전체, 현재 맡은 상태(공유 실행
팔로워 포함), 출처 실행, 요약이 들어 있습니다. `definition` 필드는
Run에 저장된 eval과 그 소유 Artifact를 태그를 포함해 합쳐 보여 주며,
정의를 저장하지 않은 예전 Run에서는 null을 돌려줍니다. 이 조회에는
저장소가 필요 없고 소유자 코드도 실행하지 않습니다. `run show`와 JSON verify에는 Run 요약이 들어 있습니다.
상태별 개수, 리뷰어 kind별 실행/재사용 요청, 경과 시간, 실제
실행기 시작 횟수, 시도 횟수, 사용량 보고 완전성입니다. Run 합계는
재사용된 출처 실행을 제외하며, Run 수준의 `usage.saved`가 그 원래
카운터를 더합니다. 재사용된 요청은 출처 표시와 제공자별 원본 시도를
`reusedUsage`에 유지하며, 자기 요약에는 시도가 없다고 보고합니다(`usageState: "none"`).
보고되지 않은 사용량은 결코 알려진 0 토큰으로 표시하지 않습니다. 별도의
요약 명령이나 `--full` 모드는 없습니다.
