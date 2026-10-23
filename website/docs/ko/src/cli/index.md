# CLI {#cli}

프로젝트에서 `artifactize verify --all --wait`를 실행하면 리뷰가 시작됩니다. 명령은 Run ID를
알려 주고, Run이 끝나면 eval마다 결과를 출력하며, 모두 GREEN이면 0으로
종료합니다. 이 페이지는 모든 명령의 플래그, 출력, 종료 코드를 정리하고, 이어서 `verify`의 정책, 예산, 선택 파일, 프로필 변형을 자세히 설명합니다.

## 명령 레퍼런스 {#command-reference}

모든 명령은 `--repo PATH`, `--state-dir PATH`, `--json`을 하위 명령 앞이나 뒤에 각각 한
번까지 받습니다. 저장소를 읽는 명령은 `--repo`를 생략하면 대개 현재 디렉터리를 쓰지만,
기본값은 명령마다 다릅니다. **doctor는 `--repo`를 직접 줄 때만 프로젝트 선언을
확인합니다**. Monitor와 Review는 아래 설명처럼 ALL에서 시작합니다. 상태는 기본으로 공유
[상태 디렉터리](../features/others/state.md#state)에 저장됩니다. 저장소나 상태를 쓰지 않는 명령은 이
옵션들을 무시합니다. Monitor와 Review는 `--json`을 거부합니다. `mcp`는 항상 stdout에
JSON-RPC를, stderr에 시작할 때의 진단 메시지를 씁니다. 현재 디렉터리 도구의 출력은 이미 아래 표처럼 텍스트나
JSON이므로 `--json`을 줘도 바뀌지 않습니다. `tools help`에서 프로그램 인자 뒤에 오는 플래그는
artifactize가 아니라 그 프로그램에 전달됩니다.
`SELECTOR` 자리에는 위치 인자 `ARTIFACT`나 `ARTIFACT/EVAL`, `--eval ID`, `--evals CSV`,
`--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`, `--all` 중 하나만
씁니다. `CSV`는 `verify --evals app/lint,app/tests`나 `verify --artifacts app,docs`처럼
이름이나 eval ID를 쉼표로 이은 목록입니다. `--evals`와 `--artifacts`는 여러 번 줄 수 있으며,
값은 순서대로 합쳐지고 중복은 빠집니다.
`help [COMMAND]`와 `-h`/`--help`는 도움말을 출력합니다. 명령 없이 `artifactize`만
실행해도 도움말이 나옵니다(종료 코드 0, `--json`을 줘도 텍스트). `-V`/`--version`은 루트
명령과 `artifactize tools`에서 버전을 출력합니다.

| 명령 | 플래그 | 출력 | 종료 코드 |
|---|---|---|---|
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--jobs N`(필요할 때만), `--fingerprint-jobs N`(CPU 수), `--max-executions N`, `--wait` 또는 `--watch`, `--request-key KEY`, `--reuse-only KINDS`, `--speculate`, `--defer-human` | Run ID 또는 `{run,state}`. `--wait`면 최종 텍스트/JSON, `--watch`면 JSON 줄 이벤트 | 접수되면 0. `--wait`나 `--watch`면 Run 결과 |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--fingerprint-jobs N`(CPU 수). 기본은 `--all` | 텍스트 또는 JSON | 충족하면 0, 아니면 1 |
| `mcp` | | [MCP stdio 프로토콜](../agent-plugin/mcp.md). 진단은 stderr | 정상적으로 연결이 끊기면 0 |
| `config check` | | 텍스트 또는 JSON | 0 |
| `config graph [ARTIFACT]` | `--impact` | 텍스트 또는 JSON | 0 |
| `run list` | `--repo-only` \| `--all`, `--limit N`(50), `--offset N`(0) | 텍스트 또는 JSON | 0 |
| `run watch RUN_ID` | | JSON 줄로 상태 변화, eval 결과, 최종 검증 | Run 결과 |
| `run cancel RUN_ID` | | 취소 요청을 받았다는 JSON | 0 |
| `run show RUN_ID` | `--wait`, `--timeout-ms MS`(600000, `--wait` 필요) | JSON | 0. `--wait`면 Run 결과 |
| `request list` | `--run RUN_ID` | 텍스트 또는 JSON | 0 |
| `request show ID` | | JSON | 0 |
| `request logs ID` | `--stream stdout\|stderr\|both`, `--tail N`, `--start N`, `--end N`, `--search TEXT` | 크기가 제한된 증거 JSON | 0 |
| `request claim ID` | `--reviewer NAME`(`$USER`) | JSON | 0 |
| `request unclaim ID` | `--reviewer NAME`(`$USER`) | JSON | 0 |
| `request tool ID TOOL` | `--reviewer NAME` | 텍스트 또는 JSON | 0. 도구 오류면 2 |
| `request submit ID` | `--verdict GREEN\|RED`, `--fields JSON` \| `--fields-file PATH`, `--reviewer NAME` | JSON | 0. RED여도 0 |
| `session show REF` | `--summary` | 텍스트, 또는 저장된 이벤트(또는 요약)를 담은 JSON | 0 |
| `session send REF MESSAGE` | | 텍스트 또는 JSON | 0 |
| `cache list` | `--history` | 텍스트 또는 JSON | 0 |
| `cache show KEY` | `--history` | JSON | 0. 없으면 4 |
| `cache rm KEY` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 준비됐으면 0, 아니면 1 |
| `tools read PATH` | — | UTF-8 텍스트 | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `tools list [PATH]` | — | JSON | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `tools glob PATTERN [PATH]` | — | JSON | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `tools grep PATTERN [PATH]` | `--glob GLOB`, `--case-insensitive` | JSON | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `tools section PATH HEADING` | — | Markdown 텍스트 | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `tools help PROGRAM [SUBCOMMAND...]` | 뒤따르는 인자는 `--help` 앞에 붙여 PROGRAM에 전달 | 텍스트 | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `tools open TARGET` | — | 기본 애플리케이션으로 엶 | 성공 0, 도구 오류 1, 사용법 오류 2 |
| `login codex` | 로그인 URL은 stderr에 출력. 터미널 stdin에서 붙여 넣은 리디렉션 URL을 읽을 수 있음 | `Signed in with Codex.`. JSON은 `{"provider":"codex","signed_in":true}` | 0 |
| `logout codex` | 로컬 토큰을 지우고 폐기를 시도. 읽기 전용 인증 파일은 건드리지 않음 | `Signed out of Codex.`. JSON은 `{"provider":"codex","signed_in":false,"revoked":BOOL}` | 로컬 로그아웃 뒤 0 |
| `remote login URL` | `--share summary\|full`(summary). 토큰은 stdin으로 | `Signed in to URL as NAME (scopes: …).`. JSON은 `{"url":…,"share":…,"principal":…,"scopes":[…]}` | 0 |
| `remote logout` | | `Signed out of the remote review store.`와 함께, 서버에서는 `server token revoke` 전까지 토큰이 유효하다는 안내. JSON은 `{"signedIn":false}` | 0 |
| `remote status` | | URL, 공유 수준, 토큰 출처, 접속 가능 여부, 이어서 주체와 범위 또는 오류. JSON은 아래 참고 | 로그인 상태면 0, 아니면 1 |
| `remote push` | `--dry-run` | 텍스트 또는 JSON | 0 |
| `models openai\|anthropic\|codex` | | 텍스트 또는 JSON | 0 |
| `doctor` | | 텍스트 또는 JSON | 준비됐으면 0, 심각한 오류면 1 |
| `prune` | `--older-than DURATION`, `--dry-run` | 텍스트 또는 JSON | 0 |
| `monitor [PATH]` | ALL에서 시작. PATH나 `--repo PATH`는 처음 고를 저장소만 정함. `--all`은 경로와 함께 쓸 수 없음 | 터미널 UI | 0 |
| `review [REQUEST_ID]` | `--repo PATH`(`repo:` 검색어), `--all`(기본값, `--repo`와 함께 쓸 수 없음), `--reviewer NAME`(`$USER`) | 터미널 UI | 0 |
| `server run` | `--listen ADDR`(`127.0.0.1:8417`): 숫자로 쓴 `IP:PORT`(IPv6은 `[::1]:8417`), 호스트 이름은 안 됨. 포트 0이면 빈 포트를 골라 출력에 알려 줌 | `Listening on http://ADDR/`. JSON은 `{"listen":ADDR}` | 0 |
| `server token add NAME --scopes SCOPES` | `--scopes`(필수, 기본값 없음. `read`, `publish`, `human`을 쉼표로 이은 목록) | 토큰을 한 번 출력하고 저장해 두라고 안내. JSON은 `{"name":NAME,"scopes":[...],"token":TOKEN}` | 0 |
| `server token list` | | 텍스트 또는 JSON | 0 |
| `server token revoke NAME` | `--purge` | JSON(`--json`이 없어도) | 0 |
| `server rm KEY` | | JSON | 0 |

Run 결과 종료 코드(`verify --wait`, `run show --wait`)는 0 GREEN, 1 RED, 2 ERROR 또는
취소, 4 INCOMPLETE, 5 SIGNOFFS_PENDING(승인만 남았다는 뜻이지 승인되었다는 뜻이 아님)입니다.
옵션 없는 `verify`는 검증 결과와 관계없이 Run이 접수되면 종료 코드 0으로 끝납니다. Ctrl-C로 진행 상황을 지켜보는
명령을 멈추면 종료 코드 130으로 끝나며, 이 명령을 끝내도 Run은 취소되지 않습니다. `run show --wait`는 RUNNING
상태의 Run이 끝날 때까지 상태를 계속 확인합니다. Run이 끝나기 전에 이 명령의 제한 시간이 지나면 현재 Run을 출력하고 3으로
종료하며, Run은 계속 실행됩니다. 모든 명령은 사용법 오류(알 수 없는 옵션, 반복할 수 없는 옵션의 반복, 충돌하거나 빠진 옵션과 값)나 동작 오류가 나면 종료 코드 2로 끝납니다. 오류는 stderr에 텍스트로, 또는
`--json`이면 stdout에 `{"error":"..."}`로 나옵니다(stdout을 프로토콜에만 쓰는 `mcp`는
예외). 도구 실패에는 두 가지 예외가 있습니다. 현재 디렉터리 도구(`tools read`, `list`, `glob`,
`grep`, `section`, `help`, `open`)는 `--json`을 줘도 공유 도구 실행기의 종료 코드 1과 진단
메시지를 그대로 씁니다. `request tool`로 실행한 Human 도구가 실패하면 평소와 같은 결과(출력된
내용, `--json`이면 `{"content":[…],"isError":true}`)를 stdout에 출력하고 2로 종료합니다.
`tools check`는 선언, 사전 점검, 실행, 정리 실패를 진단 보고서에 담고(`--execute` 호출이
실패한 경우 포함) 1로 종료합니다. 세 경우 모두 사용법 오류는 일반 규칙을 따릅니다. `plan`,
`history`, `--full`은 없습니다.

목록 옵션인 `--reuse-only`와 `server token add --scopes`는 여러 번 줄 수 있으며, 쉼표로
구분한 값이 모두 합쳐집니다. 공통 옵션인 `--repo`, `--state-dir`, `--json`은 한 번만
받습니다. `run show --timeout-ms`는 1부터 2147483647까지의 정수(밀리초)여야 하며, 범위를
벗어나면 사용법 오류(종료 코드 2)입니다.

`logout codex`가 0을 돌려주는 것은 로컬에서 로그아웃했다는 뜻이지, 서버에서 토큰이
폐기됐다는 뜻은 아닙니다. 서버에 토큰 폐기를 요청하지만, 폐기가 성공한다고 보장하지는 않습니다. 폐기를 확인하지 못하면 stderr에
경고하고, JSON 결과에 `"revoked":false`가 들어갑니다(확인되면 true). 로컬 토큰은 그래도
지워지며, 읽기 전용 외부 인증 파일은 절대 바꾸지 않습니다.


### 식별자 인자 {#identity-arguments}

Run ID와 요청 ID 인자(`request list --run`과 `review REQUEST_ID` 포함)는 ASCII 글자,
숫자, `-`, `_`, `.`로 이뤄진 1–200자여야 하며 점만으로 이뤄질 수는 없습니다. `cache show`, `cache rm`, `server rm`의 재사용 키 인자는 정확히 64자의 소문자
16진수여야 합니다. 형식이 잘못된 식별자를 지정하면 사용법 오류로 처리하고 종료 코드 2로 끝납니다. 형식은 맞지만 없는
캐시 키라면 `cache show`는 `null`(`--history`면 `[]`)을 출력하고 4로 종료하며, `cache rm`은
`removed: false`를 알립니다.

`server token add NAME`과 `server token revoke NAME`은 글자, 숫자, `-`, `_`, `.`로 이뤄진
1–64자의 ASCII를 받습니다. 빈 이름, 공백, 슬래시, ASCII가 아닌 글자, 너무 긴 이름은
거부되며 2로 종료합니다. 이 이름은 표시용이며, 인증에 쓰는
비밀 토큰이 아닙니다.

`session show`와 `session send`는 Agent 리뷰의
[저장된 대화](../features/agent.md#saved-conversations)를 읽고 이어 갑니다. 인자, 출력, 오류는
[Agent 세션](#agent-sessions)에 있습니다.

이전 버전의 상태를 여는 명령은 [상태](../features/others/state.md#state)에서 설명한 대로 그
상태를 업그레이드합니다. 열 수 없는 상태면 2로 종료하고, `doctor`는 이를 심각한 오류로 알리고
1로 종료합니다.

## 제출과 지켜보기 {#submission-and-watching}

옵션 없는 `verify`는 Run을 제출하고 Run ID를 출력한 뒤, 접수되면 0으로 종료합니다. 스크립트에서
결과에 따른 종료 코드가 필요하면 `--wait`를 붙이세요. 같은 `--request-key`로 다시 제출하면 처음
만든 Run을 돌려줍니다. 키는 ASCII 글자, 숫자, `.`, `_`, `:`, `-`로 이뤄진 1–128자이며, 조건에 맞지 않는 키를 쓰면 명령이 종료 코드 2로 끝납니다. 자세한 내용은
[제출, 지켜보기, 취소](../features/run.md#submit-observe-and-cancel)에 있습니다.

```sh
artifactize verify --all --request-key build-42
artifactize run show RUN_ID --wait
artifactize run cancel RUN_ID
```

`verify --wait`는 stderr에 Run ID를 알리고, Run이 끝날 때까지 대기한 다음 eval마다 결과를
출력하고 Run의 [종료 코드](#command-reference)로 종료합니다. Run 자체의 진단(최대 64 KiB)과
Run 오류도 stderr에 나오며, `--json`일 때도 마찬가지입니다.
[runtime-relations 예제](../getting-started/examples.md)에서 GREEN, RED, 막힌 eval을 나란히
볼 수 있습니다.


## Verify 모드 {#verify-modes}

`--speculate`와 `--defer-human`은 요청마다 따로 켜는 스위치이며, 둘 다 기본으로 꺼져
있습니다. 접수된 Run은 `modes: {speculate, defer_human}`을 기록하며, 같은
`--request-key`로 다시 시도하면 처음 요청의 모드를 그대로 씁니다.

| 모드 | 실행과 확정 방식 |
|---|---|
| 둘 다 없음 | 의존성의 GREEN 결과가 나올 때까지 대기. Run은 사람의 승인이 제출될 때까지 대기 |
| `--defer-human` | 자동 검사 게이트가 정해질 때까지 대기. 미룰 수 있는 승인은 건너뛰고 잠정 결과로 진행 |
| `--speculate` | 게이트가 정해지기 전에 요청한 종류를 모두 시작. 사람의 승인은 제출될 때까지 대기 |
| 둘 다 | 자동 리뷰는 미리 시작하고, 미룰 수 있는 승인은 그 승인의 선행 조건을 충족하면 요청 |

어느 모드에서든 Human eval은 같은 Artifact의 Human이 아닌 eval이 모두 GREEN이 된 뒤에만
요청되며, 그중 하나라도 RED나 ERROR면 BLOCKED가 됩니다. `--speculate`에서는 새로 준비된 작업을
실행할 `--jobs`나 백엔드 자리가 모자라면 artifactize가 실행 중인 추측 실행 리뷰를 중단할 수 있습니다. 그 리뷰는 나중에
다시 시작하며, 다시 시작할 때도 `--max-executions`로 정한 실행 횟수에 한 번 더 포함됩니다. 이 모드들은 실행 순서를 바꿀 뿐 승인 여부를
바꾸지 않습니다. `verify --wait`는 평소와 같은 결과 종료 코드를 씁니다. 0 GREEN,
1 RED, 2 ERROR, 4 INCOMPLETE, 5 SIGNOFFS_PENDING입니다.
[실행 모드](../features/run.md#speculation-and-deferred-sign-offs)에서 스케줄링, 취소, 잠정
결과, 엄격한 Human 게이트, 나중에 제출한 승인 결과를 재사용하는 방법을 설명합니다.

## Runtime CLI {#runtime-cli}


선택자, 게이트, 결과, 출력은 [Run, 상태, 검증](../features/run.md#verify-and-runs)에서
다룹니다.

없어진 예전 게이트 우회 설정은 [업그레이드 안내](../getting-started/update.md#upgrading-from-011)를
보세요. `--force`는 직접 선택한 실행 가능한 eval만 새로 리뷰하도록 표시하며, 재귀로 포함된
의존성에는 적용되지 않습니다. 실행 범위를 넓히거나 게이트를 건너뛰지도 않습니다. Run에는 결정된
정책과 각 요청의 force 여부가 기록됩니다. 강제로 실행하는 eval은 캐시 결과를 읽지 않고,
실행 중인 작업에도 합류하지 않습니다. `--force`로 새로 실행한 eval의 결과가 GREEN이나 RED면 그 키의 가장 최근 기록으로 추가되고, 리뷰
저장소가 설정되어 있으면 게시되어 이후 Run이 재사용합니다. 의존성은 로컬 기록을 계속
재사용할 수 있지만, `--force`로 실행한 Run은 저장소에서 아무것도 읽지 않습니다. 추측 실행과 승인 미루기도
최종 검증 요건을 면제하지 않습니다.

동시에 실행되는 Run이 언제 같은 리뷰에 합류하는지, `--force`가 이를 어떻게 바꾸는지는
[Run끼리 작업 나누기](../features/others/state.md#state)에서 설명합니다.

eval 동시 실행 수에는 기본 상한이 없어서, READY인 eval은 준비되는 즉시 시작합니다. 동시
실행이 문제를 일으킬 때만 `--jobs N`(1–4294967295)을 붙이세요. 그러면 이 Run의 eval 수가
제한됩니다. Agent 리뷰는 이와 별개로 `$STATE/limits.json`에 따라 머신 전체에서 백엔드마다
제한됩니다([백엔드 수용량](../features/others/state.md#backend-capacity)). 백엔드 자리가
빌 때까지 대기 중인 요청은 `--jobs`의 동시 실행 수에도, 실행 시작 횟수에도 포함되지 않습니다.

`--fingerprint-jobs N`(`verify`와 `status`, 1–4294967295)은 fingerprint를 동시에 몇 개까지
계산할지 정합니다. 기본값은 프로세스가 쓸 수 있는 CPU 수입니다. `verify`에서는 준비 단계와
Runtime/Agent 리뷰가 끝날 때 하는 재확인이 한도 하나를 함께 쓰며, eval 수를 제한하는
`--jobs`와는 별개입니다. Human 도구 실행과 제출은 CPU 수만큼 따로 fingerprint를 계산하며 이
설정을 따르지 않습니다. 결과와 출력은 이 한도나 완료 순서와 상관없이 같습니다.

`verify --max-executions N`은 Run 하나에서 실행기를 시작할 수 있는 횟수를 0–18446744073709551615
사이로 정합니다(생략하면 무제한). Artifact 선언 필드가 아닙니다. Run은 직접 지정한 `jobs`
상한(무제한이면 필드가 없음), `fingerprintJobs`, `maxExecutions`, `executionsStarted`를
기록합니다. 준비를 마친 Runtime이나 Agent 실행은 시작에 실패하거나 나중에 ERROR가 되더라도 시작 횟수에 1회로 집계됩니다. Human 대기, 담당 지정, 도구 실행은 시작 횟수에 들어가지 않습니다. fingerprint 준비와
재확인, 캐시 적중, 합류해서 대기 중인 요청도 마찬가지입니다. 실행기 준비 초기에 실패한 경우는 시작
횟수에 넣지 않습니다. 시작 횟수를 센 다음 Agent의 `execution_paths`를 고정하므로, 고정에
실패한 실행도 시작 횟수에 포함하며 모델을 호출하지 않은 채 ERROR/PREPARATION_FAILED를 기록합니다.
0으로 두면 재사용, 합류, Human 리뷰만 할 수 있습니다. 합류한 Run이 실패해서 리뷰를 다시
실행해야 하면, 다시 실행하는 리뷰는 그 리뷰가 속한 Run의 남은 횟수를 씁니다. 횟수를 다 써도 실행 중인 eval은 멈추지 않지만,
남은 READY 요청은 `BUDGET_EXHAUSTED`가 되고 Run은 이유와 함께 INCOMPLETE(종료 코드 4)로
끝납니다. 모든 요청을 시작했다면 실행 횟수가 한도에 정확히 도달해도 오류가 아닙니다.

Human 대기와 취소는 [제출, 지켜보기, 취소](../features/run.md#submit-observe-and-cancel)를
따릅니다. 담당 지정, 도구, 승인은 [Human 가이드](../features/human.md)에서 다룹니다.

선택 파일은 4 MiB 이하의 일반 파일이어야 합니다. ID를 JSON 문자열 배열로 담거나, 앞뒤 공백을 뺀 ID를 한 줄에 하나씩 담으세요. UTF-8 BOM과 CRLF도 쓸 수 있습니다. 중복을 빼기 전 기준으로 ID가 1–100000개여야
합니다. 빈 줄은 무시하며, ID에는 ASCII 공백, 제어 문자, 쉼표를 쓸 수 없습니다. 형식이 잘못된
JSON 배열을 줄 단위 형식으로 다시 읽지는 않습니다. 상대 경로는 `--repo`가 아니라 CLI의 현재
디렉터리를 기준으로 합니다.

`verify ... --profile NAME`은 선택과 재귀 정책으로 포함된 모든 실행 가능한 eval에 대해, 선언된
`profile_variants` 항목 하나를 통째로 고릅니다. 변형은 그다음에 적용됩니다. 실행 가능한
eval마다 안전한 이름의 변형을 64개까지 선언할 수 있고, 모든 변형은 원래 리뷰어 종류를
유지합니다. 없는 변형 이름을 주면 Run을 만들기 전에 실패하며, 원본 선언 파일은 수정하지 않습니다.
`--recursive`를 주면 이미 선택된 의존성에도 변형이 적용됩니다. 예를 들어 `app/review
--profile fast`가 실행하는 Runtime 변형의 인자에 `{fixtures}`가 있으면, `fixtures`의 eval은
`fast`가 아니라 기본 프로필로 Run에 들어옵니다. 고른 변형은 실행 옵션일 뿐 재사용 키에 들어가지
않습니다. 그래서 Runtime 변형이 명령이나 인자를 바꾸지 않는 한, 변형(그리고 선언된
프로필)이 달라도 결과를 서로 재사용할 수 있습니다. 저장된 요청의 `profile`과 `options`는 결과를 만든 실행을
나타내고, 캐시가 다른 프로필의 결과를 돌려주면 `requestedProfile`에 요청한 프로필이 남습니다.
텍스트 출력은 결과를 만든 프로필의 설정이 다를 때만 재사용 표시에 `profile NAME`을 붙입니다.
설정이 같은 변형의 결과에는 표시가 붙지 않으며, 저장된 `options.variant`로 구분합니다.

## 재사용만 하기 {#reuse-only}

`verify --reuse-only KINDS`는 `runtime`, `agent`, `human`을 쉼표로 이은 목록을 받습니다.
목록에 있는 종류의 eval은 결과를 재사용만 할 수 있습니다. 재사용할 수 있는 것은 로컬 캐시에
있는 그 재사용 키의 완료된 기록이나, 설정되어 있다면 [리뷰 저장소](review-store.md)에 있는
기록입니다. 그런 기록이 없으면 eval을 실행하지 않습니다. 실행기를 시작하거나 provider를 부르거나 Human 요청을 만들지 않으며, 같은 재사용 키로 진행 중인 리뷰가 있어도 그 리뷰가 끝날 때까지 대기하지 않습니다.
강제로 실행하는 eval은 캐시 결과를 읽지 않으므로, 목록에 있는 종류에 `--force`를 주면 절대 실행되지
않습니다. 목록에 없는 종류는 평소처럼 실행합니다. CI에서
`artifactize verify --all --wait --reuse-only agent,human`을 쓰면 테스트는 실행하고 리뷰는
모두 캐시나 저장소에서 가져옵니다([CI](../features/team/ci.md)).

실행하지 않은 eval은 `STALE`로 표시되고, 이유는 `blockedReason`에 나옵니다.

```text
  docs/review [run-Hq2b9X-4]: STALE — Not reused (--reuse-only agent): no cached or stored result, so it was not executed.
```

이 eval에 의존하는 eval도 다른 증거가 없을 때와 마찬가지로 대기 상태가 되거나 실행이 차단됩니다. 예산을 다 썼을
때처럼, 다른 eval이 RED나 ERROR여도 Run은 이유와 함께 INCOMPLETE(종료 코드 4)로 끝나며, 그
종류들을 `reuseOnly`로 기록합니다. `status`는 이 플래그를 받지 않으며, 옵션 없는 `verify`가
무엇을 할지 예측합니다.

## 의존성 선택과 출력 {#dependency-selection-and-output}

`artifactize verify app/e2e`는 `--eval app/e2e`처럼 그 eval을 선택합니다. 필요한 의존성
범위의 eval까지 실행하려면 `--recursive`를 붙이세요. 붙이지 않으면 입력 증거가 이미 있어야
합니다. 파일 Artifact는 선언된 이름으로 선택하며, 사이드카 선언은 그 파일이 들어 있는 폴더와 자동으로
관계를 맺지 않습니다.

의존성 순환을 포함해 어떤 eval이 대기 상태가 되거나 실행이 차단되는지는
[eval 의존성](../features/artifact.md#eval-dependencies)을 보세요.

status 출력의 동작 값은 `execute`, `reuse`, `wait`, `blocked`입니다. 요청, 검증, status에는
`blockedBy`로 eval ID가 들어갑니다. 태그, 파일 종류, 경로는 graph와 status 출력에 나오고,
[Monitor](../tui/index.md#monitor)는 이를 화면에 보여 줍니다.

### status 상태 {#status-states}

`status`는 Artifact와 eval마다 상태를 출력합니다. 상태는 저장된 판정이 아니라 지금의 파일과
선언을 기준으로 합니다. 저장된 GREEN 결과의 재사용 키가 현재 키와 일치하면 `PASS`로 표시되며, 동작 항목에는 다음에 `verify`를 실행하면 무엇을 할지 나옵니다.

| 상태 | eval | Artifact |
|---|---|---|
| `PASS` | 현재 재사용 키에 맞는 GREEN 결과가 있고 게이트도 충족됨 | eval이 모두 `PASS`이고 의존하는 Artifact도 모두 검증 요건을 충족함 |
| `RED` | 현재 재사용 키에 맞는 RED 결과가 있음 | eval 중 하나가 `RED` |
| `BLOCKED` | 게이트가 RED | eval 중 하나가 `BLOCKED` |
| `WAIT_DEPENDENCY` | 게이트에 아직 현재 결과가 없음 | eval 중 하나가 대기 중 |
| `WAITING_HUMAN` | 사람의 처리가 필요한 Human 요청이 있음 | — |
| `DEFERRED` | 사람의 처리가 필요한 미룬 승인 요청이 있음 | eval 중 하나가 `DEFERRED` |
| `PROVISIONAL` | GREEN이지만 게이트가 아직 `DEFERRED`나 `PROVISIONAL` | eval 중 하나가 `PROVISIONAL` |
| `STALE` | 전에 리뷰했지만 현재 재사용 키에 맞는 결과가 없음(이전 오류, 키 없는 결과, `--force` 포함) | eval 중 하나가 `STALE` |
| `UNREVIEWED` | 리뷰한 적이 없음 | 리뷰한 eval이 없거나 eval이 없음 |
| `INCOMPLETE` | — | 그 Artifact의 eval은 `PASS`이지만 의존하는 Artifact의 검증 요건이 충족되지 않음 |
| `BASIS` | — | 의존하는 Artifact가 모두 검증 요건을 충족한 basis Artifact |

Artifact는 그 eval들의 상태 중 `RED`, `BLOCKED`, `WAIT_DEPENDENCY`, `DEFERRED`, `PROVISIONAL`,
`STALE`, `UNREVIEWED` 순서로 처음 해당하는 것을 보여 줍니다. `ERROR`는 Run 출력에만 나오며,
`status`는 마지막 시도가 실패한 eval을 `STALE`로 보여 줍니다.

## 저장된 Run 증거와 터미널 화면 {#saved-run-evidence-and-terminal-views}

저장된 증거가 있으면 `run show RUN_ID --json`과 `verify --wait --json`의 출력 객체 최상위에
`evidence` 필드가 들어갑니다. Run 필드는 `run`으로 감싸지 않고 출력 객체에 바로 들어갑니다. `evidence`는 이
Run에서 요청 없이 사용한 저장된 판정을 eval ID별로 담습니다. 일부 eval만 실행한 Run에서 상위 eval의 결과를 캐시에서 가져와 쓴 경우가 그 예입니다. 전체 기록에서 가장 최근 판정을 담는 것은 아닙니다. [Run](../features/run.md#verify-and-runs)을
보세요. 터미널에서 쓰는 키와 화면 배치는 [Monitor](../tui/index.md#monitor)와
[Human 리뷰](../tui/index.md#review)에 있습니다.

## 현재 디렉터리 도구 {#current-directory-tools}

`artifactize tools read|list|glob|grep|section|help|open ...`은 내장 도구를 현재
디렉터리에서 실행합니다. Store가 아닌 설치에는 `artifactize-tools`라는 이름으로도 들어
있습니다. stdout에 텍스트나 JSON을 출력하고, 도구가 실패하면 stderr에 오류를 남기고 1로,
사용법이 잘못되면 2로 종료합니다. `artifactize tools --help`가 도구 목록을 보여 주며,
`artifactize tools check`는 이와 달리 선언된 리뷰 도구를 점검합니다. 하위 명령별 범위
규칙과 제한은 [`artifactize-tools`](../features/tools/builtin.md#artifactize-tools-cli)에
있습니다.

## Agent 스트리밍과 Runtime 로그 {#agent-streaming-and-runtime-logs}

`verify SELECTOR --watch`는 Run을 제출한 직후부터 진행 상황을 보여 주고, `run watch RUN_ID`로는 나중에 진행 상황 보기를 시작할 수 있습니다. 둘 다 `--json`과 상관없이 JSON 줄을 바로바로 내보냅니다. `kind: update`는 실시간
상태와 개수, `kind: eval_result`는 저장된 요청의 간단한 결과, `kind: outcome`은 Run의 최종
검증입니다. `--watch`와 `--wait`는 함께 쓸 수 없습니다. 지켜보는 명령은 Run 결과에 맞는 종료 코드로 끝나며, 중단하면
130으로 끝나고 지켜보기만 멈춥니다. 다시 연결하면 완료된 eval을 다시 보여 줄 뿐, 지난 이벤트를
그대로 재생하지는 않습니다. 자세한 내용이 필요하면 결과의 `next_cursor`를 MCP
`verify_result`에 넘겨 다음 내용을 읽거나 저장된 Run을 살펴보세요.

Runtime eval이 실패했을 때:

```sh
artifactize request logs REQUEST_ID --stream stderr --tail 30
artifactize request logs REQUEST_ID --stream stdout --start 100 --end 150
artifactize request logs REQUEST_ID --search 'failure' --tail 10
artifactize config graph INPUT_ARTIFACT --impact --json
```

로그 조회는 요청 감사 기록 전체 대신, 로그가 보존되었는지 여부와 원래 줄 번호를 담은 조회 결과를 돌려줍니다. tail은 기본으로 스트림마다 조건에 맞는 마지막 50줄을 돌려줍니다. 범위 조회는
스트림마다 최대 200줄과 제한된 바이트만 돌려주고, 이어서 볼 위치를 `next_start`로 알려
줍니다. tail과 범위 조회는 함께 쓸 수 없습니다. `search`는 1–2048바이트의 문자열을 대소문자를
구분해 그대로 찾습니다. 원래 2048바이트가 넘는 줄은 `clipped`로 표시하고 앞뒤 일부만
보여 줍니다. Runtime 원본 스트림 전체는 저장된 감사 기록의 128 KiB 한도를 넘는 뒷부분까지 Run 출력에 남습니다. `prune`은 이 원본 로그를 지우지 않으며, 정해진 주기로 지우는 기능도 없습니다. 원본 로그가 없거나 이전 버전 또는 원격 실행의 감사 기록만 남아 있으면 그 상태를 분명히 표시합니다. 공통
[Runtime 보존 레퍼런스](../features/declarations.md#runtime-execution)를 보세요. 공통 증거와
이벤트 스키마, 클라이언트 전달 제한은 [MCP 집중 진단](../agent-plugin/mcp.md#streaming-and-focused-diagnosis)에
있습니다.

## 도구 진단 {#tool-diagnostics}

```sh
artifactize tools check                         # every Agent/Human eval's scope
artifactize tools check app/review              # same as --eval app/review
artifactize tools check --artifact app --audience agent
artifactize tools check --execute --artifact app --audience agent --tool read --args '{"path":"README.md"}'
artifactize tools check --execute --artifact app --audience human --tool inspect
artifactize tools check --execute --eval app/review --tool coverage_app --args '{}'
```

`tools check`는 선언의 유효성, 실행 파일이 있는지, 범위가 정해진 argv 인자와 선언된 실행 경로를 확인한 뒤 범위, 스키마, 도구별 준비 상태를 JSON으로 출력합니다. 기본으로는
정적으로만 확인하므로 프로젝트 도구나 fingerprint 스크립트를 실행하지 않고 상태나 출력 폴더도
만들지 않습니다. 위치 인자 선택자나 `--eval`은 Agent/Human eval 하나를 고르며, `--artifact`나
`--audience`와 함께 쓸 수 없습니다. 실제로 실행하려면 `--tool`을 지정하고, eval 선택자를 쓰거나 `--artifact`와 `--audience`를 함께 지정해야 합니다. `--artifact`를 쓰면 짧은 동작 이름과 공개된
이름을 모두 받으며, 범위는 그 Artifact와 자식 Artifact입니다. eval을 쓰면 그 eval이 공개하는
이름(`<tool>_<artifact>`)으로 도구를 지정합니다. 도구는 그 eval의 범위에서 실행되므로, 인자가
다른 Artifact를 가리키는 도구도 그 eval의 리뷰에서처럼 실행됩니다.
`--args`는 Agent 실행에서만 쓸 수 있습니다. Human 스키마는 빈 객체만 받고, Human 명령에는 임의의
인자를 넘길 수 없습니다. 점검은 Run, 판정, 캐시 항목을 만들지 않습니다. Agent 호출은 작성자의
환경에서 별도의 임시 출력 폴더(실행 뒤 삭제)를 쓰고, Human 호출은 리뷰어의 실제 환경을 씁니다.
종료 코드는 준비 완료나 성공이면 0, 선언·사전 점검·도구·정리 실패면 1, 호출이 잘못되면
2입니다. `--repo`, `--state-dir`, `--json`을 받으며, `--json`이 없어도 보고서는 JSON입니다.

## Run 목록 {#run-list}

`run list [--repo-only | --all]`은 저장된 Run을 최신순으로 텍스트 표로, `--json`이면 JSON
배열로 보여 줍니다. 기본 대상은 정규화한 `--repo` 경로(생략하면 현재 디렉터리)입니다.
`--repo-only`는 이 기본값을 명시하고, `--all`은 공유 상태의 모든 저장소를 포함합니다.
`--limit N`(기본 50)과 `--offset N`(기본 0)은 각각 0–4294967295 범위로, 걸러 낸 결과를
페이지로 나눕니다. 범위를 벗어난 값을 지정하면 종료 코드 2로 끝납니다. 각 행에는 Run ID, 저장소, 생성·완료
시각, 상태, 판정과 상태별 요청 수(해당 상태가 없으면 0)가 나옵니다. 읽을 수 없는 요청은 세지
않으며, `unreadable`에 손상된 기록과 그 이유가 나오고 유효한 증거는 그대로 볼 수 있습니다.
[상태 문제 해결](../features/others/state.md#unreadable-records)을 보세요. 읽기만 하므로
프로젝트 코드를 실행하지 않고, 없는 상태를 만들지 않으며, 저장소가 지금 남아 있지 않아도
됩니다. 별도의 `history` 명령은 없습니다.

## Config graph {#config-graph}

`config graph [ARTIFACT]`는 기본으로 프로젝트 전체를 보여 주고, ARTIFACT를 주면 순환으로
묶인 Artifact까지 포함해 그 Artifact에 필요한 범위를 보여 줍니다. `--impact`는 그 결과를 쓰는 하위 Artifact와 각 Artifact가 쓰는 입력도 포함합니다. 이는 구조상 영향이 미칠 수 있는 범위를 넉넉하게
잡은 것이지, 재사용 키를 정확히 예측한 것은 아닙니다. MCP `config_graph`에서도 Artifact나 eval을 하나 또는 여러 개 골라 같은 정의를 볼 수 있습니다. 실제로 무엇을 실행하고 재사용할지는
`status`로 확인하세요. 텍스트 출력에는 태그와 파일 종류 표시가 붙은 Artifact, eval, 컴포넌트,
입력에서 사용처로 가는 관계가 나옵니다. 전체 JSON에는 정해진 정적 Artifact/eval 정의(프로필,
payload, 스키마, 도구 뷰), 태그, 자식·지시문·argv 관계 정보, 순환 표시, 그리고 서로 의존하는
Artifact 묶음인 강한 연결 요소(SCC)가 의존성 순서대로 들어 있습니다. Artifact 항목에는
`kind`(`file`, `folder`, `graph`)와 작업 공간 기준 대상 `path`가 있습니다. status도 파일 종류와
경로를 보여 주며, [Monitor](../tui/index.md#monitor)는 이를 화면에 보여 줍니다. 컴포넌트
ID는 전체 그래프 기준이라 일부만 보여 줄 때는 번호가 띄엄띄엄일 수 있습니다. `config check`는 프로젝트 코드를 실행하지 않고 선언이 유효한지 확인하며, JSON 출력에는 Artifact와 eval 개수를 담습니다. 출력 수준은
텍스트 하나와 전체 JSON 하나뿐이며, `plan`, `--compact`, `--full`은 없습니다. 예전의 최상위
`graph` 명령은 도움말에 나오지 않으며, 실행하면 `graph moved to "artifactize config graph"`를
출력하고 2로 종료합니다.

## Human 요청 {#human-requests}

`claim`, `unclaim`, `tool`, `submit`은 기본 리뷰어 이름으로 `$USER` 값을 사용하며, 값이 없으면 Linux와
macOS에서는 `$LOGNAME`, Windows에서는 `%USERNAME%` 값을 사용합니다. 모두 없으면 `--reviewer NAME`을
주세요. `--reviewer NAME`으로 동작마다 같은 리뷰어를 직접 지정할 수도 있습니다. 리뷰어 이름은
계정 인증이 아니라 담당자를 구분하는 데 쓰이며, 같은 요청을 여러 리뷰어가 겹쳐 처리하지 않도록 로컬 잠금을 겁니다. 요청을 맡은 사람만 도구를 실행하고,
제출하고, 담당을 해제할 수 있습니다. `request claim`은 담당 정보를, `request unclaim`은 해제한
담당 정보를 출력합니다. 다른 리뷰어가 맡았거나 더 이상 대기 중이 아닌 요청의 담당을 해제하려
하면 종료 코드 2로 끝납니다. 도구 이름은 `<operation>_<artifactId>` 형식이며, 호출할 때 임의의 인자를 넘길 수 없습니다.
텍스트 출력은 캡처한 텍스트나 실행 알림을 보여 주고, `--json`은 도구 결과를 출력합니다. 도구가
실패하면 2로 종료하며 판정을 만들어 내지 않습니다.

제출하려면 `--verdict GREEN|RED`가 필요합니다. `--fields`와 `--fields-file`은 함께 쓸 수
없습니다. 둘 중 하나로 소유자가 정한 필드만 담은 JSON 객체를 넘기며, 판정은 이 객체에 넣지 않습니다. 생략하면 `{}`입니다. 파일은
일반 파일이어야 하고, 필드와 전체 결과는 256000바이트까지입니다. JSON, 스키마, 리뷰어 이름,
판정이 잘못되면 2로 종료하며, 요청은 수정 후 다시 제출할 수 있는 상태로 남습니다. 제출에 성공하면 RED여도
0으로 종료하며, 별도의 검증 과정을 시작하지 않습니다.

도구를 실행하거나 제출을 저장하기 전에 리뷰한 파일과 선언을 다시 확인합니다. fingerprint가
바뀌었거나 계산할 수 없으면 요청은 판정 없이 ERROR(`INPUT_CHANGED`나
`FINGERPRINT_RECHECK_FAILED`)로 끝나고 명령은 종료 코드 2로 끝납니다. 새 요청을 받으려면
`verify`를 다시 실행하세요. eval의 선언이나 범위가 바뀌었으면 제출을 종료 코드 2로 거부하고
요청은 대기 상태로 남습니다. 선언을 되돌린 뒤 제출하거나 다시 verify하세요.

이미 맡은 요청에 같은 리뷰어로 `request claim`을 다시 실행해도 기존 담당 정보를 돌려주고 0으로
종료하므로, 담당 지정은 반복해도 안전합니다. 제출은 한 번만 받으며, 다시 제출하면 종료 코드 2로 끝납니다. 명령이 중단되는 등 제출
결과가 불확실하면 다시 제출하기 전에 `request show`로 요청을 확인하세요.

## Agent 세션 {#agent-sessions}

**읽기.** `REF`는 `ref`, 요청 ID(`run-FiVNCV-1`), `sessionId` 중 하나입니다.

```sh
artifactize session show run-FiVNCV-1
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · codex gpt-6-luna (reasoning max)
Reference: alice@laptop/34bb3c3a-…/run-FiVNCV/run-FiVNCV-1/e83bf374-…
Request run-FiVNCV-1 (Run run-FiVNCV), eval notes/review: GREEN

── System
You are an artifactize evaluator. …

── User (turn 1)
Eval: Review notes (notes/review)
Target Artifact: notes
Instruction: Check Artifact notes (tools: read_notes). …

── Assistant (turn 1)
[reasoning] Reading the notes
[tool call read_notes] {"path":"notes.md"}

── User (turn 2)
[tool result read_notes]
{"artifactId":"notes","lines":[{"number":1,"text":"notes: R1 holds.\n"}], …}

── Assistant (turn 2)
{"result":{"verdict":"GREEN","verdictFields":{}}}

── Repair prompt (turn 3)
Your final response did not match the required schema: …

── Assistant (turn 3)
{"result":{"verdict":"GREEN","verdictFields":{"covered":["R1"]}}}

── Result: {"covered":["R1"],"verdict":"GREEN"}
```

텍스트 출력은 도구 결과를 2000자에서 자르고 reasoning은 요약만 보여 줍니다. `--json`은
`{"reference", "file", "events"}`를 출력합니다. 이 JSON에는 참조, 세션 파일, 저장된
이벤트 전체가 들어 있어 대화를 그대로 재생할 수 있습니다.

### 세션 요약 {#session-summary}

`--summary`는 세션 파일만 읽어 대화의 토큰 사용량과 도구 호출 수 등을 집계합니다. 턴(provider
요청)별 토큰 수와 합계, 도구별 호출 수와 실패 수, follow-up, 백엔드, 모델, 시간 범위가
나옵니다. 오류를 반환했거나, 예산 한도·시간 제한·리뷰 종료 때문에 실행하지 못한 호출을 실패한 호출로 셉니다.

```sh
artifactize session show run-FiVNCV-1 --summary
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · codex gpt-6-luna (reasoning max)
Reference: alice@laptop/34bb3c3a-…/run-FiVNCV/run-FiVNCV-1/e83bf374-…
Request run-FiVNCV-1 (Run run-FiVNCV), eval notes/review: GREEN
Time: 2026-10-06T09:14:02.118604115Z to 2026-10-06T09:16:41.502330019Z (159.4 s)
Turns: 5 · follow-ups: 1
Tokens: input 21410 · output 1318 · cache read 15872 · reasoning 704
  turn 1: input 3920 · output 212 · cache read 0 · reasoning 128
  turn 2: input 4310 · output 260 · cache read 3840 · reasoning 160
  turn 3: input 4402 · output 96 · cache read 4224 · reasoning 64
  follow-up 1, turn 1: input 4366 · output 410 · cache read 3840 · reasoning 224
  follow-up 1, turn 2: input 4412 · output 340 · cache read 3968 · reasoning 128
Tool calls: 3 (1 failed)
  read_notes: 2
  search_notes: 1 (1 failed)
```

여러 번 시도한 턴에는 `(2 attempts)`처럼 시도 횟수가 붙습니다. 토큰 수는 그 턴의 각 시도에서
provider가 보고한 값을 합산한 것이며, 한 번도 보고되지 않은 항목은 생략합니다. `--json`은 같은
내용을 객체 하나로 출력합니다. `backend`, `model`, `reasoning`, `startedAt`, `endedAt`,
`durationMs`, `followUps`, `turns`(각 턴마다 follow-up에 대한 답이면 `followUp`, 그리고
`turn`, `attempts`, `tokens`), `tokens`, `toolCalls`
(`{"read_notes": {"calls": 2, "failed": 0}, …}`), `sessionId`, `requestId`,
`reference`가 들어 있습니다.

**이어서 묻기.**

```sh
artifactize session send run-FiVNCV-1 "Which line shows R1, and does it still hold?"
```

```text
Session e83bf374-5136-484c-8d2f-2ce38ee4ca57 · follow-up 1 · codex gpt-6-luna · files changed since this review

notes.md line 1 says R1 holds, but it now reads "R1 fails", so the verdict would change.
```

`--json`을 주면 `send`는 `{"reference", "sessionId", "requestId", "send", "filesChanged",
"answer", "usage"}`를 출력합니다. `send`는 follow-up 번호, `filesChanged`는 리뷰한 입력이
바뀌었는지(알 수 없으면 null), `answer`는 답변 텍스트, `usage`는 시도별 토큰 수입니다.

follow-up은 저장된 대화를 같은 모델과 현재 프로젝트 도구로 이어 갑니다. 답변은 일반
텍스트이며, 기록된 판정, 결과, 재사용에는 영향을 주지 않습니다. 리뷰한 입력이 바뀌었으면 헤더에
경고가 나옵니다. 질문마다 프로필의 예산이 새로 적용되며, 한 세션에 보낸 질문은 차례대로 처리합니다.
캐시, 저장되는 메시지, 호환성은 [follow-up 재생 레퍼런스](../features/others/state.md#follow-up-replay)를
보세요.

다른 머신이나 다른 상태의 참조를 주면 그 대화가 저장된 위치를 알려 주고 실패합니다. 예를 들면
`Session e83bf374-… lives in alice@laptop's state 34bb3c3a-…; this is bob@desk's state
5d0e…`입니다. 오래된 대화를 자동으로 정리하는 과정에서 세션이 지워졌으면 `was removed by the session GC`라는 안내가 나오고, 처음부터 저장하지 않은 세션에는 `was not saved`라는 안내가 나옵니다. 이런 경우, 아직 실행 중인 요청에
`session send`를 쓴 경우, follow-up이 실패한 경우에 `session show`와 `session send`는 종료 코드 2로 끝납니다.

**보관.** 저장소가 크기 한도를 넘으면 오래된 대화부터 지웁니다. 한도와 이를 바꾸는 방법은
[Agent 세션](../features/others/state.md#agent-sessions)에 있습니다.

**개인정보.** 대화에는 리뷰어가 도구로 읽은 모든 내용(파일 내용 포함)과 리뷰어가 쓴 모든 내용이
들어 있습니다. 이 머신에서 소유자만 읽을 수 있는 파일로 남고, 팀 리뷰 저장소를 포함해 어디에도
올리지 않습니다. follow-up은 리뷰의 매 턴과 마찬가지로 대화를 그 리뷰의 백엔드로 보냅니다.
저장하지 않으려면 `"agentSessions": {"enabled": false}`로 끄고, 이미 저장된 대화는
`$STATE/agent-sessions`를 지워서 없애세요.
