# MCP stdio 서버 {#mcp-stdio-server}

**MCP(Model Context Protocol)**는 코딩 어시스턴트를 artifactize의 도구에 연결해, 어시스턴트가
프로젝트를 살펴보고 리뷰를 시작하거나 지켜볼 수 있게 합니다. `artifactize mcp`는 CLI와 **같은
바이너리**로 검증 도구와 비대화형 확인 도구를 **stdio**로 제공합니다. 호스트는 네트워크 포트에
연결하는 대신 `artifactize mcp`를 실행하고, 그 프로그램의 표준 입력과 출력으로 메시지를
주고받습니다. 프로젝트 디렉터리에서 시작해 코딩 호스트에 등록하세요.
[아래의 등록 방법](#register-the-mcp-server)을 보세요.
`--state-dir PATH`는 상태 디렉터리를 지정합니다. Run, 요청, 세션을 조회할 때는 호출에서 지정한
프로젝트의 기록만 읽으며, 캐시 기록은 같은 상태를 쓰는 여러 저장소에서 재사용할 수 있습니다.

## 어느 프로젝트인가 {#which-project}

각 도구 호출은 프로젝트 하나를 다룹니다. 프로젝트를 정할 때는 다음 항목을 차례로 확인하고, 가장 먼저 찾은 값을 씁니다.

1. 도구 호출의 `project` 인자로 준 프로젝트 폴더의 절대 경로(모든 도구가 이 인자를 받습니다)
2. 서버를 시작할 때 준 `--repo PATH`
3. 호스트의 첫 로컬 [MCP root](https://modelcontextprotocol.io/specification/2025-06-18/client/roots).
   첫 호출 때 물으며(답은 최대 10초 대기), 호스트의 root가 바뀐 뒤 다시 묻습니다.
4. 호스트가 root를 주지 않으면 서버를 시작한 디렉터리. 단, 그 디렉터리가 서버를 실행한
   플러그인의 폴더라면 제외합니다.

이 방법으로도 프로젝트를 정할 수 없으면, 서버는 `project` 인자가 필요하다는 오류를 돌려줍니다. 이 오류는 root를 지원하는 호스트가 제때 답하지 않거나 로컬 폴더를 지정하지 않았을 때, 또는 서버가 플러그인 폴더에서 시작됐을 때 납니다.

최신 버전과 이전 버전의 MCP 호스트에서 모두 동작합니다. 프로토콜 메시지는 stdout으로,
진단 정보는 stderr로 출력합니다. 제한 시간 안에서 실행되는 `verify_watch` 호출은 표준 MCP 진행 알림도 보낼 수 있습니다. 자세한 내용은
[프로토콜 버전](#protocol-versions)을 보세요.

## MCP 서버 등록하기 {#register-the-mcp-server}

[설치 설정](../getting-started/install.md)을 마친 다음, **프로젝트 디렉터리**에서
코딩 호스트를 실행하세요.
이 서버는 HTTP 리뷰 저장소(`artifactize server`)가 아니므로, 토큰을 인자로 넘길 필요도, 호스트의 시간 제한을 바꿀 필요도 없습니다.

Claude Code와 Codex에서는 [플러그인](plugin.md)이 Hey Arti 스킬과 함께
이 서버를 등록합니다. 아래 명령은 수동 등록입니다. 이 명령으로
호스트에 서버를 직접 등록해야 하며, artifactize가 호스트 설정을 자동으로
추가하거나 바꾸지는 않습니다. 호스트에서 제공하는 신뢰 확인과 승인 절차를 따르세요.

### Claude Code {#claude-code}

프로젝트 디렉터리에서 다음을 실행하세요.

```sh
claude mcp add --transport stdio --scope project artifactize -- artifactize mcp
claude mcp get artifactize
```

프로젝트 범위로 등록하면 호스트가 프로젝트 폴더에 `.mcp.json` 파일을 만듭니다. 이를 공유한다면
자격 증명과 머신별 경로를 넣지 마세요. 이 프로젝트에서 Claude Code를 시작/재시작하고,
MCP 보기에서 서버가 연결되었는지 확인한 뒤, 아래의 검증 도구와 확인 도구가
나열되는지 확인하세요.

### Codex CLI {#codex-cli}

stdio 명령을 등록한 다음, 프로젝트 디렉터리에서 Codex를 시작하세요.

```sh
codex mcp add artifactize -- artifactize mcp
codex mcp get artifactize
```

등록한 명령은 Codex의 MCP 설정에 기록됩니다. 명령을 전역으로 등록했더라도 Codex는 해당 프로젝트 디렉터리에서 시작해야 합니다. 호스트가 프로젝트에서
실행될 수 없다면, 스킬 디렉터리를 가리키지 말고
`artifactize --repo PATH mcp`로 프로젝트별 명령을 등록하세요.
호스트에서 평소 쓰는 권한·인증 절차가 그대로 적용되므로 우회 플래그는 필요 없습니다.

이렇게 등록하면 도구만 제공되며, [Hey Arti 스킬](hey-arti.md)은 제공되지 않습니다.

## 검증 도구 {#verification-tools}

| 도구 | 인자 | 답 |
| --- | --- | --- |
| `verify_start` | `scope`, 선택적으로 `request_key`, `comment`, `jobs`, `speculate` | Run을 제출합니다. Run 상태 요약이며 승인 결과는 아닙니다 |
| `verify_status` | `run` | 저장된 상태, 개수, 경과 초, `next_check` |
| `verify_wait` | `run`, 선택적으로 `seconds` | Run이 끝나거나 한도에 도달할 때까지 대기합니다. 기본값은 30이며 0–40의 정수입니다. 첫 호출에서 root를 물어야 한다면 그 답을 받은 뒤 대기하며, 한 번의 확인은 45초 안에 끝납니다 |
| `verify_watch` | `run`, 그리고 도구 호출에 선택적으로 표준 `_meta.progressToken` | 최대 30초 동안 변경을 지켜봅니다. 진행 메시지에는 JSON Run 이벤트가 들어 있습니다. 호출이 끝나면 간단한 상태 요약을 반환합니다 |
| `verify_result` | `run`, 선택적으로 `cursor` | 판정과 간결한 지적 사항 최대 20개, `next_cursor`를 돌려줍니다. 실행 중이면 `ready:false`입니다 |
| `verify_cancel` | `run` | 명시적 취소 확인입니다. 취소 뒤 정리가 끝났는지는 Run이 최종 상태가 될 때까지 지켜보세요 |

`comment`에는 에이전트가 지금 하는 일을 짧게 적습니다. 터미널의 Monitor, 데스크톱 앱과 `run_show`에서
Run과 함께 사람에게 보이며 Run의 동작은 바꾸지 않습니다. 제어 문자와 앞뒤 공백 없는 한 줄,
1–200자여야 하고, 그 밖의 값은 Run을 접수하기 전에 거부됩니다.

`scope`에는 선택 종류를 `kind` 필드로 나타내는 공통 선택(Selection) JSON 객체를 전달합니다.

```json
{"kind":"all"}
```

다른 형식으로는 `{"kind":"artifact","artifactId":"library"}`,
`{"kind":"eval","evalId":"library/tests"}`,
`{"kind":"artifacts","artifactIds":["library","consumer"]}`,
`{"kind":"evals","evalIds":["library/tests","consumer/review"]}`가 있습니다.
배열로 선택할 때는 ID를 1–64개 넣을 수 있으며, 처음 나온 순서를 유지합니다. 의존성 처리와 결과 재사용에는 CLI와 같은 기본 규칙이 적용되며,
전송 방식에 따른 별도의 스케줄링이나 수명 주기 설정은 없습니다. 이 객체는
CLI 선택 파일이 아닙니다. `--evals-file`과 `--artifacts-file`은 `kind` 태그 없이
ID의 JSON 배열이나 한 줄에 ID 하나를 받습니다.

`request_key`는 ASCII 문자/숫자/`._:-` 1–128개입니다. 프로젝트를 고쳤거나 Run이 끝났더라도,
같은 저장소에서 같은 키로 다시 요청하면 처음 만든 Run을 돌려줍니다. 새 시도에는
새 키를 쓰거나 키를 쓰지 마세요. 키를 생략하면 언제나 새 Run을 만들며, 기존
eval 결과 재사용은 유지됩니다.

동시 실행은 기본으로 무제한입니다. READY eval은 모두 시작할 수 있으며,
`limits.json`의 백엔드 제한만 따릅니다. 선택 인자 `jobs`(1–4294967295)는 CLI `--jobs`와 같은
Run별 상한이며, 제한 없는 동시 실행이 문제를 일으킬 때만 추가하세요. 동시 실행 상한이 없는
Run에는 `jobs` 필드가 없습니다.

`speculate`는 기본값이 false이며 CLI의 `--speculate`와 뜻이 같습니다. 요청된 Runtime과 Agent 작업을 상위 eval의 판단이 끝나기 전에 시작하되, 일반 스케줄링 한도는 그대로 적용합니다. 상위 eval이 RED나 ERROR가 되면 그 결과로 무효가 된 하위 작업만 취소합니다.
자세한 내용은 [추측 실행](../features/run.md#speculation)에 있습니다. 승인 대기 중인 Run이 언제
`waiting_human`이 되고 언제 `yellow`로 끝나는지는 [승인과 Run 종료](../features/run.md#sign-offs-and-the-end-of-a-run)에 있습니다.
`yellow` Run에서 `verify_result`는 판정 `YELLOW`를 보고하며, 이는 완전한 승인이 아닙니다.

Run이 실행 중일 때 돌려주는 응답은 2 KiB보다 작습니다. 대기 상태는 오류가 아니라 정상입니다.
`verify_wait`는 Runtime, Agent, Human eval을 똑같이 다룹니다. 사람이 필요하다는 이유만으로
일찍 끝나지 **않습니다**. `next_check`는 다음에 상태를 확인할 시점을 알려 주는 값일 뿐,
빠르게 폴링하거나 모델 컨텍스트를 유지하라는 뜻이 아닙니다.

Run은 artifactize가 관리합니다. MCP 요청 처리를 중단하거나, MCP 연결을 닫거나,
지켜보기를 멈추거나, 에이전트 턴을 끝내도 Run은 취소되지 않습니다. Run을 취소하는 방법은 `verify_cancel`뿐입니다.
RED/INCOMPLETE/ERROR/취소됨은 잘못된 도구 호출이 아니라 저장된 Run 결과입니다.
Human 요청의 담당 지정·해제와 판정 제출은 사람이 직접 처리하며, MCP에서는 이 기능을 제공하지 않습니다.

## 확인 도구 {#inspection-tools}

모든 확인 도구는 아래 나열한 인자 외에, 세부 결과의 다음 페이지를 읽는 `cursor`도 선택적으로 받습니다.
CLI 대응 명령과 같은 JSON을 세부 페이지로 나눠 돌려줍니다.

| 도구 | 인자(필수라고 표시한 것 외에는 선택) | CLI 대응 명령 |
| --- | --- | --- |
| `status` | `scope`(기본 all) | `status --all` 또는 선택한 status |
| `config_check` | 없음 | `config check` |
| `config_graph` | `scope`(기본 all), `impact`(false) | `config graph [ARTIFACT] [--impact]`. MCP는 eval/여러 개 선택도 받습니다 |
| `request_logs` | **`id`**, `stream`(`stdout`/`stderr`/`both`), `tail`, `start`, `end`, `search` | 해당 플래그를 쓴 `request logs ID` |
| `run_list` | `limit`(0–20, 기본 20), `offset`(0–4294967295, 기본 0) | 이 저장소에 대한 `run list --limit 20 --offset N`, 최신 순 |
| `run_show` | **`run`** | `run show RUN` |
| `request_list` | `run` | 이 저장소로 제한된 `request list [--run RUN]` |
| `request_show` | **`id`** | `request show ID` |
| `session_show` | **`reference`**, `summary`(기본 false) | `session show REF [--summary]` |
| `tools_check` | `eval`, 또는 `artifact`/`audience`/`tool` | 정적 `tools check`입니다. `audience`는 `agent` 또는 `human`입니다 |
| `doctor` | 없음 | `doctor` |
| `models` | **`provider`**(기본 provider `openai`, `anthropic`, `gemini`, `codex` 또는 커스텀 provider 이름입니다. 이름은 ASCII 영문자, 숫자, `_`, `-`로 된 1–64자이며 영문자나 숫자로 시작합니다) | `models PROVIDER` |
| `remote_status` | 없음 | `remote status` |
| `cache_show` | **`key`**, `history`(기본 false) | `cache show KEY [--history]` |

`session_show.reference`는 저장된 세션 참조, 요청 ID, 세션
ID를 받으며, CLI와 마찬가지로 `producer`(리뷰를 실행한 `user@host`)와 상태를 확인합니다. 캐시 기록이 없으면
CLI의 JSON 출력과 마찬가지로 null을 반환하고, history를 지정하면 빈 배열을 반환합니다. 정적
`tools_check`는 프로젝트 도구를 호출하거나 세션을 시작하지 않습니다. `status`는
신뢰하는 fingerprint 명령을 실행하고 설정된 리뷰 저장소를 조회할 수 있습니다. 선언된
명령은 OS 샌드박스 안에서 실행되지 않습니다. MCP는 로그인/로그아웃 동작을 제공하지 않으며
자격 증명을 돌려주는 일도 없습니다. `models`, `doctor`, `remote_status`는 백엔드
준비 상태를 확인할 수 있고, `doctor`는 CLI와 마찬가지로 [새 릴리스](../getting-started/update.md#newer-releases)도 확인합니다. Codex 모델 목록을 조회할 때는 CLI와 마찬가지로 artifactize에 저장된 토큰을 갱신할 수 있으며, 갱신이 영구히 거부되면 저장된 로그인 정보를 지울 수 있습니다.

### 세부 페이지 {#detail-pages}

세부 페이지에는 텍스트 항목이 최대 하나 들어 있으며, 구조화된 답과
이전 형식의 텍스트 사본을 포함한 전체 크기가 64 KiB 미만입니다. 각 항목은 잘라 낸 미리 보기나 별도 형식의 결과가 아니라
**CLI가 출력하는 JSON 원문**을 UTF-8 텍스트로 나눈 조각입니다.

```json
{"encoding":"json","entries":[{"offset":0,"text":"{\"ok\":true,\"artifacts\":3,\"evals\":4}"}],"next_cursor":null}
```

`next_cursor`가 null이 될 때까지 `entries[].text`를 순서대로 이어 붙인 다음,
이어 붙인 텍스트를 JSON으로 파싱하세요. JSON은 공백 없이 직렬화되므로, CLI가 보기 좋게 출력할 때 넣는 공백은
보존되지 않습니다. 이전 페이지의 `cursor`를 더해 **같은 도구를 같은 인자로** 다시 호출하세요.
offset은 UTF-8 바이트 오프셋입니다. `run_list.offset`은
다음 Run 기록 묶음을 지정하고, cursor는 같은 묶음의 JSON에서 다음 조각을 읽을 때 씁니다.
`verify_result`는 대신 구조화된 `findings` 배열을 페이지 단위로 넘깁니다.

cursor는 같은 도구와 같은 인자에서만 쓸 수 있습니다. 다음 페이지를 요청할 때마다 확인을 다시 실행하며, 그 결과가 바뀌지 않았을 때만 cursor를 쓸 수 있습니다. 새 Run이나 결과가 생겼을 때뿐 아니라
파일, 모델 목록, 백엔드 상태가 바뀌어도 거부됩니다. 시간이 흐른다고 만료되지는 않습니다. cursor가 거부되면 같은 도구를 첫 페이지부터 다시 호출하세요. 실패한
도구 호출은 `{code,message}`와 함께 MCP `isError:true`를 돌려줍니다. `operation_failed`에는 CLI의
오류 메시지가 처음 2,048자까지 담깁니다. `response_too_large`는 응답이 세부 페이지 하나에 담기지
않는다는 뜻이므로, `request_logs`의 조회 범위나 검색 조건을 조정해 조회할 내용을 줄이세요. 내용 일부를 생략한
페이지를 전체 결과인 것처럼 반환하지 않습니다.

## 일부러 빼놓은 것 {#deliberately-absent}

자격 증명 로그인/로그아웃, Human 담당 지정/제출/담당 해제, prune, 캐시 제거,
원격 푸시, 대화형 Monitor/review, 저장소 서버, 프로젝트 도구 실행,
Agent 세션 follow-up은 없습니다. 이런 동작은 CLI와 일반 Human 리뷰 인터페이스에서 하세요.


## 스트리밍과 집중 진단 {#streaming-and-focused-diagnosis}

`verify_start`로 제출하고 Run ID를 보관한 다음, `_meta`에 표준
도구 호출 메타데이터 `{"progressToken":"your-observer"}`를 넣어 `verify_watch`를 호출하세요.
이 호출이 대기하는 동안 artifactize는 Run이 바뀔 때마다
`notifications/progress`를 보냅니다. 알림의 `message`는 JSON 객체입니다.

- `{"kind":"update","data":...}`: 간결한 실시간 상태/개수입니다. **승인이 아닙니다**.
- `{"kind":"eval_result","data":...}`: 저장된 요청/eval 상태 하나와 간결한
  요약입니다. 재사용된 결과와 잠정 결과를 포함하며, **Run 판정이 아닙니다**.
- `{"kind":"outcome","data":...}`: `verify_result`의 스키마를 쓰는 최종 Run 검증입니다.
  남은 지적 사항은 그 `next_cursor`를 `verify_result`에 넘겨 읽으세요.

연결할 때마다 기준 상태와 이미 완료된 eval을 다시 보내므로, 필요하면
요청 ID와 상태를 기준으로 이전 연결에서 받은 항목과 겹치는 것을 걸러 내세요. 호출이 제한 시간에 도달해 반환될 때
Run이 이미 끝난 상태여도, 클라이언트가 outcome 알림을 받았다고 보장할 수는 없습니다.
최종 검증 결과는 `verify_result`로 확인하세요. 진행 토큰이 없으면
이 호출은 알림을 보내지 않은 채 대기합니다. 진행 알림을 쓰지 않는 클라이언트에서는
`verify_wait`를 쓰세요. 빠른 폴링은 필요 없습니다.
구독은 호출이 끝나거나(30초 한도) 클라이언트가 취소하거나 연결이
끊기면 끝나며, 이후의 호출은 저장된 Run에 다시 연결합니다(Run을 누가 관리하는지는
[검증 도구](#verification-tools)를 보세요). 함께 도착한 변경은 알림 하나로 합쳐질 수 있으며,
각 알림은 전체 이벤트 이력이 아니라 저장된 상태를 반영합니다.

진행 알림은 **클라이언트나 에이전트 호스트**가 화면에 보여 주거나 작업
대기열 또는 대화로 전달해야 합니다. MCP 알림만으로는 모델 턴이 생기거나 추론 캐시가 유지되지 않으며, 호출이나 연결이 끝난 뒤에는 알림도 전달되지 않습니다. artifactize는
임의의 호스트에 메시지를 끼워 넣을 수 없습니다. 셸을 쓸 수 있는 호스트에서는
`artifactize verify --all --watch`나 `artifactize run watch RUN`이 종료될 때까지
같은 이벤트를 한 줄에 JSON 객체 하나씩 출력하며, 호스트는 그 프로세스의 실행을 관리하고 출력을
전달할 수 있습니다. artifactize에서 이런 전송 방식을 제공한다고 해서 모든 코딩 호스트에서 진행 상황을 실시간으로 대화에 전달할 수 있는 것은 아닙니다.

실패 원인을 조사할 때는 `run_show`로 감사 기록 전체를 읽는 대신 그 `request` ID로 `request_logs`를 조회하세요. `stream`의 기본값은 both입니다. 범위가 없으면 일치하는 마지막 50
줄을 돌려주며, `tail`은 일치하는 줄 1–200개를 고릅니다. `start`/`end`는 1부터 세는
원래 줄 번호이며 양 끝을 포함하고, `tail`과 함께 쓸 수 없습니다. `search`는 대소문자를 구분하는
리터럴이며(UTF-8 1–2048바이트), tail 선택 전에 적용됩니다. 시작 줄부터 순서대로 읽는 범위 조회는
최대 200줄과 스트림별 바이트 한도까지만 돌려주며, 이어서 읽으려면 stream/search/end를 그대로 두고
`next_start`를 다음 시작 위치로 쓰세요. 원본 줄이 2048바이트를 넘으면 앞부분과 끝부분만 정해진 길이만큼 보여 주고 `clipped` 표시를 붙이며, 원본 전체는 로컬 디스크에 남습니다. 줄 중간이
잘려 보이지 않더라도 그 부분에 검색어가 있으면 검색 결과에 포함될 수 있습니다. 결과는 원본 바이트를 그대로 내보낸 것이 아니라, 읽기 좋게 정리한 UTF-8 진단 텍스트입니다. 이 응답에도 앞서 설명한 세부 페이지 나누기와 크기 제한이 그대로 적용됩니다.

각 스트림의 `evidence`는 `complete`(끝부분을 포함한 전체 로컬 캡처),
`partial`(실행 중이거나 중단된 캡처처럼 확인된 EOF가 없음), `saved_audit`
(로컬 캡처가 오래되었거나 정리된 경우, 또는 원격 실행의 기록만 있는 경우. 처음 128 KiB만 있을 수 있음), `missing`,
또는 `not_runtime`(대신 요청/세션 증거를 쓰세요) 중 하나입니다. 내용이 비어 있는 로그와 로그가 아예 없는 경우는 구분합니다.
재사용된 요청의 로그는 결과를 만든 원래 로컬 실행에서 찾으며, 원격 기록만으로는
로그가 로컬에 보존되어 있다고 보장할 수 없습니다. 원본 캡처를 얼마나 오래 보관하는지와
`artifactize prune`이 무엇을 남기는지는
[Runtime 실행 레퍼런스](../features/declarations.md#runtime-execution)에 있습니다.

`config_graph`는 훅을 호출하지 않고 정적 정의, eval, 같은 순환에 속한 Artifact, 입력 → 사용처
관계를 찾습니다. Artifact나 eval을 고르면 그 대상의 의존성 범위가 나오며,
`impact:true`를 더하면 영향을 받을 수 있는 하위 Artifact와 각각에 필요한 입력도
포함됩니다. 이는 구조상 영향을 받을 가능성이 있는 대상을 넉넉히 포함할 뿐이며, 포함된 모든 eval의 재사용 키가 실제로 바뀌었다는 뜻은 아닙니다. 실제로 어떤 결과를 재사용할지는 `status`로 확인하세요.

## 프로토콜 버전 {#protocol-versions}

서버는 MCP 버전 2026-07-28과 그 이전 버전을 쓰는 클라이언트와 호환됩니다. MCP Tasks 확장은
제공하지 않으며, 진행 알림은 그 알림을 요청한 호출이 끝날 때까지만 옵니다.
