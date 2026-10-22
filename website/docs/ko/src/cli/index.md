# CLI {#cli}

모든 명령과 그 플래그, 출력, 종료 코드, 그리고 `verify`의
정책, 예산, 선택 파일, 프로필 변형에 대한 세부 사항입니다.

## 명령 레퍼런스 {#command-reference}

모든 명령은 `--repo PATH`, `--state-dir PATH`, `--json`을 하위 명령 앞이나
뒤에서 각각 최대 한 번 받습니다. 저장소를 읽는 명령은 `--repo`를 생략하면 보통
현재 디렉터리를 쓰지만, 기본값은 명령마다 다릅니다.
**doctor는 명시적인 `--repo`가 있을 때만 프로젝트 선언을 확인합니다**. Monitor와
Review는 아래 설명대로 ALL에서 시작합니다. 상태의 기본값은 공유
[상태 홈](../features/others/state.md#state)입니다. 저장소나 상태를 쓰지 않는
명령은 그 옵션을 무시합니다. Monitor와 Review는 `--json`을 거부합니다.
`mcp`는 언제나 stdout에 JSON-RPC를, stderr에 시작 진단을 씁니다.
현재 디렉터리 도구의 출력은 `--json`으로 바뀌지 않으며, 아래 나열한 대로 이미 텍스트이거나 JSON입니다.
`tools help`의 프로그램 피연산자 뒤에 오는 플래그는 artifactize가 아니라
그 프로그램에 전달됩니다.
`SELECTOR`는 위치 인자 `ARTIFACT` 또는 정규화된 `ARTIFACT/EVAL`,
`--eval ID`, `--evals CSV`,
`--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`, `--all` 중 정확히 하나입니다.
`CSV`는 `verify --evals app/lint,app/tests`나 `verify --artifacts app,docs`처럼
이름이나 정규화된 eval ID를 쉼표로 구분한 목록입니다.
`--evals`와 `--artifacts`는 각각 반복할 수 있으며, 그 값은 순서대로 합쳐지고
중복은 빠집니다.
`help [COMMAND]`와 `-h`/`--help`는 도움말을 출력합니다. `-V`/`--version`은 루트 명령과
`artifactize tools`에서 버전을 출력합니다.

| 명령 | 플래그 | 출력 | 종료 코드 |
|---|---|---|---|
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--jobs N`(선택), `--fingerprint-jobs N`(CPU 수), `--max-executions N`, `--wait` 또는 `--watch`, `--request-key KEY`, `--reuse-only KINDS`, `--speculate`, `--defer-human` | Run ID 또는 `{run,state}`. `--wait`면 최종 텍스트/JSON, `--watch`면 JSON 줄 이벤트 | 접수되면 0. `--wait`나 `--watch`면 결과 |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--fingerprint-jobs N`(CPU 수). 기본 `--all` | 텍스트 또는 JSON | 충족이면 0, 아니면 1 |
| `mcp` | | [MCP stdio 프로토콜](../agent-plugin/mcp.md). 진단은 stderr | 정상적으로 연결이 끊기면 0 |
| `config check` | | 텍스트 또는 JSON | 0 |
| `config graph [ARTIFACT]` | `--impact` | 텍스트 또는 JSON | 0 |
| `run list` | `--repo-only` \| `--all`, `--limit N`(50), `--offset N`(0) | 텍스트 또는 JSON | 0 |
| `run watch RUN_ID` | | JSON 줄 갱신, eval 결과, 최종 검증 | Run 결과 |
| `run cancel RUN_ID` | | 취소 확인 JSON | 0 |
| `run show RUN_ID` | `--wait`, `--timeout-ms MS`(600000, `--wait` 필요) | JSON | 0. `--wait`면 결과 |
| `request list` | `--run RUN_ID` | 텍스트 또는 JSON | 0 |
| `request show ID` | | JSON | 0 |
| `request logs ID` | `--stream stdout\|stderr\|both`, `--tail N`, `--start N`, `--end N`, `--search TEXT` | 크기가 제한된 증거 JSON | 0 |
| `request claim ID` | `--reviewer NAME`(`$USER`) | JSON | 0 |
| `request unclaim ID` | `--reviewer NAME`(`$USER`) | JSON | 0 |
| `request tool ID TOOL` | `--reviewer NAME` | 텍스트 또는 JSON | 0. 도구 오류면 2 |
| `request submit ID` | `--verdict GREEN\|RED`, `--fields JSON` \| `--fields-file PATH`, `--reviewer NAME` | JSON | 0, RED여도 0 |
| `session show REF` | `--summary` | 텍스트, 또는 저장된 이벤트(또는 요약)의 JSON | 0 |
| `session send REF MESSAGE` | | 텍스트 또는 JSON | 0 |
| `cache list` | `--history` | 텍스트 또는 JSON | 0 |
| `cache show KEY` | `--history` | JSON | 0. 없으면 4 |
| `cache rm KEY` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 준비되면 0, 아니면 1 |
| `tools read PATH` | — | UTF-8 텍스트 | 성공 0, 도구 오류 1, 사용법 2 |
| `tools list [PATH]` | — | JSON | 성공 0, 도구 오류 1, 사용법 2 |
| `tools glob PATTERN [PATH]` | — | JSON | 성공 0, 도구 오류 1, 사용법 2 |
| `tools grep PATTERN [PATH]` | `--glob GLOB`, `--case-insensitive` | JSON | 성공 0, 도구 오류 1, 사용법 2 |
| `tools section PATH HEADING` | — | Markdown 텍스트 | 성공 0, 도구 오류 1, 사용법 2 |
| `tools help PROGRAM [SUBCOMMAND...]` | 뒤따르는 인자는 `--help` 앞에 PROGRAM으로 전달 | 텍스트 | 성공 0, 도구 오류 1, 사용법 2 |
| `tools open TARGET` | — | 기본 애플리케이션을 엶 | 성공 0, 도구 오류 1, 사용법 2 |
| `login codex` | 로그인 URL은 stderr. 터미널 stdin에서 붙여 넣은 리디렉션 URL을 읽을 수 있음 | `Signed in with Codex.`. JSON `{"provider":"codex","signed_in":true}` | 0 |
| `logout codex` | 로컬 토큰을 제거하고 폐기를 시도. 읽기 전용 인증 파일은 건드리지 않음 | `Signed out of Codex.`. JSON `{"provider":"codex","signed_in":false,"revoked":BOOL}` | 로컬 로그아웃 뒤 0 |
| `remote login URL` | `--share summary\|full`(summary). 토큰은 stdin | 텍스트 또는 JSON | 0 |
| `remote logout` | | 텍스트 또는 JSON | 0 |
| `remote status` | | 텍스트 또는 JSON | 로그인 상태면 0, 아니면 1 |
| `remote push` | `--dry-run` | 텍스트 또는 JSON | 0 |
| `models openai\|anthropic\|codex` | | 텍스트 또는 JSON | 0 |
| `doctor` | | 텍스트 또는 JSON | 준비되면 0, 치명적 오류면 1 |
| `prune` | `--older-than DURATION`, `--dry-run` | 텍스트 또는 JSON | 0 |
| `monitor [PATH]` | ALL에서 시작. PATH나 `--repo PATH`는 처음 선택할 저장소만 정함. `--all`은 경로와 함께 쓸 수 없음 | 터미널 UI | 0 |
| `review [REQUEST_ID]` | `--repo PATH`(`repo:` 검색어), `--all`(기본값. `--repo`와 함께 쓸 수 없음), `--reviewer NAME`(`$USER`) | 터미널 UI | 0 |
| `server run` | `--listen ADDR`(`127.0.0.1:8417`) | `Listening on http://ADDR/`. JSON `{"listen":ADDR}` | 0 |
| `server token add NAME` | `--scopes read,publish,human` | 토큰(한 번만)과 보관하라는 안내. JSON `{"name":NAME,"scopes":[...],"token":TOKEN}` | 0 |
| `server token list` | | 텍스트 또는 JSON | 0 |
| `server token revoke NAME` | `--purge` | JSON(`--json`이 없어도) | 0 |
| `server rm KEY` | | JSON | 0 |

Run 결과 코드(`verify --wait`, `run show --wait`): 0 GREEN, 1 RED, 2 ERROR 또는
취소, 4 INCOMPLETE, 5 SIGNOFFS_PENDING(승인이 아니라 승인만 남았다는 뜻). 그냥 `verify`는 검증이 아니라 접수되면 0으로 종료합니다.
Ctrl-C는 지켜보기를 종료 코드 130으로 멈추며, 닫아도 Run은 결코 취소되지 않습니다. `run show --wait`는 RUNNING인
Run을 끝날 때까지 따라가며, 자기 시간 제한이 먼저 끝나면 현재
Run을 출력하고 3으로 종료하며 Run은 계속 실행되게 둡니다. 모든 명령은 사용법 오류
(알 수 없거나, 반복할 수 없는데 반복했거나, 충돌하거나, 빠진 옵션과 값)와 운영
오류에 2로 종료합니다. 텍스트는 stderr로 가고, `--json`이면 stdout에 `{"error":"..."}`가 갑니다(단
`mcp`는 stdout을 프로토콜 전용으로 유지). 도구 실패 두 가지는 예외입니다.
현재 디렉터리 도구(`tools read`, `list`, `glob`, `grep`, `section`, `help`,
`open`)는 `--json`이어도 공용 도구 실행기의 종료 코드 1과 바뀌지 않은 진단을 유지합니다.
`request tool`이 실행한 Human 도구가 실패하면 평소의
결과를 stdout에 출력하고(렌더링된 내용, 또는 `--json`이면 `{"content":[…],"isError":true}`)
2로 종료합니다. `tools check`는 실패한 `--execute` 호출을 포함한 선언, 사전 확인, 실행,
정리 실패를 진단 보고서에 보고하고
1로 종료합니다. 셋 모두 사용법 오류는 여전히 일반 규칙을 따릅니다.
`plan`, `history`, `--full`은 없습니다.

목록 옵션 `--reuse-only`와 `server token add --scopes`는 반복할 수 있으며,
쉼표로 구분한 값이 반복할 때마다 쌓입니다. 이는 한 번만 받는 공통 `--repo`, `--state-dir`, `--json` 옵션에는
적용되지 않습니다.
`run show --timeout-ms`에는 1부터 2147483647 밀리초까지의 정수가 필요하며,
범위를 벗어난 값은 사용법 오류입니다(종료 코드 2).

`logout codex`가 0을 돌려주면 로컬 로그아웃을 확인한 것이지, 반드시 서버 쪽
폐기를 확인한 것은 아닙니다. 서버 쪽 폐기를 시도하지만 성공을 보장하지는 않습니다. 확인할 수 없으면 명령은
stderr에 경고하고 그 JSON 결과에 `"revoked":false`가 들어갑니다(확인되면 true).
로컬 토큰은 그래도 제거되며, 읽기 전용 외부 인증 파일은 결코 바뀌지 않습니다.


### 식별자 인자 {#identity-arguments}

Run/요청 ID 인자(`request list --run`과 `review REQUEST_ID` 포함)는
상태를 읽기 전에 검증됩니다. ASCII 문자, 숫자, `-`, `_`, `.` 1–200개이며,
점으로만 이루어질 수는 없습니다. `cache show`, `cache rm`, `server rm`의 재사용 키 인자에는
정확히 64개의 소문자 16진수 문자가 필요합니다. 형식이 잘못된 식별자는 사용법 오류로
2로 종료합니다. 올바르지만 없는 캐시 키라면 `cache show`는 여전히 `null`
(`--history`면 `[]`)을 출력하고 4로 종료하며, `cache rm`은 `removed: false`를 보고합니다.

`server token add NAME`과 `server token revoke NAME`은 ASCII
문자 1–64개를 받습니다. 문자, 숫자, `-`, `_`, `.`입니다. 빈 이름, 공백, 슬래시,
ASCII가 아닌 텍스트, 더 긴 이름은 **토큰 저장소를 열기 전에** 종료 코드 2로 거부됩니다.
이 이름은 인증에 쓰는 비밀 토큰이 아니라 화면에 보여 줄 이름입니다.

`session show`와 `session send`는 Agent 리뷰의
[저장된 대화](../features/agent.md#saved-conversations)를 읽고 이어 갑니다. `REF`는
요청의 `session.ref`(`user@host/STATE_ID/RUN_ID/REQUEST_ID/SESSION_ID`), `run-Hq2b9X-1` 같은
요청 ID, 또는 `sessionId`입니다. 결과를 재사용한 요청은 그것을 만든 리뷰를
가리킵니다. `show`는 대화를 텍스트로 출력하고, `--json`이면
저장된 이벤트를 그대로 `{"reference","file","events"}`로 출력합니다. `show --summary`는
세션 파일만으로 계산한 [세션 요약](../features/agent.md#session-summary)을 출력합니다.
백엔드, 모델, 시간 범위, 턴과 그 토큰과
합계, 도구별 도구 호출과 실패한 수, 후속 질문입니다. `--json`은
`{"reference","sessionId","requestId","backend","model","reasoning","startedAt","endedAt","durationMs","followUps","turns","tokens","toolCalls"}`를 출력합니다.
`send`는 세션, 후속 질문 번호, 모델을 담은 머리말 아래에 답을 출력하며,
리뷰가 포함한 Artifact가 바뀌었으면 `files changed since this review`를 붙입니다.
`--json`은 `{"reference","sessionId","requestId","send","filesChanged","answer","usage"}`를 출력합니다.
둘 다 다른 머신이나 상태에 대한 참조, GC가 제거한 세션,
저장되지 않은 세션, 아직 실행 중인 요청(`send`), 실패한 후속 질문에 2로 종료합니다.

기존 상태를 여는 명령은 쓰기 권한이 있으면 지원되는 추가 스키마 업그레이드를
적용합니다. 스키마 6~8은 Run,
캐시, Human 승인을 보존하며 9로 업그레이드됩니다(6 → 7 → 8 → 9, 각 단계는 추가 방식). 스키마 1–5와 9보다 높은 버전은 거부됩니다([상태](../features/others/state.md#state)).
`doctor`는 호환되지 않는 스키마를 치명적 오류로 보고하고 1로 종료합니다.

## 제출과 지켜보기 {#submission-and-watching}

그냥 `verify`는 제출하고, Run ID를 출력한 뒤 접수되면 0으로 종료합니다. 스크립트에
결과의 종료 코드가 필요한 곳에는 `--wait`를 추가하세요. 반복된 `--request-key`는 원래의
Run을 돌려줍니다. 세부 사항은 [제출, 지켜보기, 취소](../features/run.md#submit-observe-and-cancel)에서
다룹니다.

```sh
artifactize verify --all --request-key build-42
artifactize run show RUN_ID --wait
artifactize run cancel RUN_ID
```

## Verify 모드 {#verify-modes}

`--speculate`와 `--defer-human`은 서로 독립된 요청별 스위치이며, 둘 다 기본으로
꺼져 있습니다. 접수된 Run은 `modes: {speculate, defer_human}`을 기록하며, 같은
`--request-key`로 재시도하면 첫 요청의 모드를 유지합니다.

| 모드 | 배정과 확정 |
|---|---|
| 둘 다 없음 | 의존성의 GREEN 증거를 기다림. 소유한 Run은 사람을 기다림 |
| `--defer-human` | 기계 관문은 기다림. 미룰 수 있는 승인은 잠정적으로 지나쳐 실행 |
| `--speculate` | 해결되지 않은 관문 전에 요청된 모든 kind를 시작. 사람은 기다림 |
| 둘 다 | 기계 리뷰를 추측 실행으로 시작. 미룰 수 있는 승인은 마지막에 요청 |

모든 모드에서 Human eval은 그 Artifact의 Human이 아닌 다른 eval이 모두
GREEN이 된 뒤에만 요청되며, 그중 하나가 RED나 ERROR면 BLOCKED가 됩니다.
이 모드들은 승인이 아니라 실행 순서를 바꿉니다. `verify --wait`는
평소의 결과 코드를 씁니다. 0 GREEN, 1 RED, 2 ERROR, 4 INCOMPLETE, 5
SIGNOFFS_PENDING입니다. [실행 모드](../features/run.md#speculation-and-deferred-sign-offs)에서
스케줄링, 취소, 잠정 결과, 엄격한 Human 관문,
이후의 승인 재사용을 설명합니다.

## Runtime CLI {#runtime-cli}

이 개발용 예제에는 소스 체크아웃, Rust, 그리고
[설치](../getting-started/install.md#build-from-source-all-platforms)에 나온 플랫폼 빌드 사전 요구 사항이 필요합니다.
저장소 루트에서 실행하세요. `cargo run`은 그 체크아웃에서 CLI를 빌드합니다.
설치된 CLI라면 자기 프로젝트 경로와 함께 `artifactize`를 직접 쓰세요.

```sh
cargo run -q -p artifactize -- --repo crates/artifactize-core/tests/fixtures/runtime verify --all --wait
cargo run -q -p artifactize -- run show RUN_ID
```

이 픽스처는 의도적으로 GREEN, RED, 시간 초과 ERROR, RED로 막힌 의존 eval과
ERROR를 기다리는 의존 eval, Artifact 두 개짜리 순환을 포함합니다. 전체 종료 코드는 2입니다.

선택자, 관문, 결과, 출력은 [Run, status, 검증](../features/run.md#verify-and-runs)에서 다룹니다.

제거된 예전 관문 우회 설정은 [업그레이드 레퍼런스](../getting-started/install.md#removed-gate-bypass-settings)를
보세요. `--force`는 재귀적 의존성이 아니라 명시적으로
선택된 실행형 eval만 새로 리뷰하도록 표시합니다.
실행 범위를 넓히지도, 관문을 우회하지도 않습니다. Run은 해석된 정책과 각
요청의 force 플래그를 기록합니다. 강제 실행한 eval은 캐시된 결과를 재사용하지 않고,
진행 중인 실행에 합류하지도 않습니다. 강제된 GREEN이나 RED는 그 키의 가장 새 기록으로 추가되고,
설정된 리뷰 저장소에 게시되며, 이후 실행이 이를 재사용합니다. 의존성은
여전히 자기 로컬 기록을 재사용할 수 있지만, 강제된 Run은 저장소에서 아무것도 읽지 않습니다.
추측 실행과 미루기는 결코 최종 요구 사항을 면제하지 않습니다.

[진행 중인 실행 공유](../features/others/state.md#state)에서 동시 Run이
언제 같은 실행에 합류하는지, 그리고 프로필 식별 정보, 인계, `--force`가
그 자동 공유에 어떤 영향을 주는지 설명합니다.

기본 eval 동시 실행 상한은 없습니다. READY eval은 모두 준비되는 즉시
시작합니다. 그 동시 실행이 문제를 일으킬 때만 `--jobs N`(최소 1)을 추가하세요. 그러면
이 Run의 eval을 제한합니다. Agent 리뷰는 여전히
`$STATE/limits.json`으로 머신 전체에서 백엔드별로 제한됩니다
([백엔드 용량](../features/others/state.md#backend-capacity)). 백엔드 슬롯을 기다리는 요청은
선택적인 `--jobs` 슬롯도, 실행기 시작도 차지하지 않습니다.

`--fingerprint-jobs N`(`verify`와 `status`에서, 최소 1)은 fingerprint를
한 번에 몇 개 계산할지 제한하며, 기본값은 프로세스가 쓸 수 있는 CPU 수입니다.
`verify`에서는 공유 한도 하나가 준비와 Runtime/Agent
리뷰 종료 시 재확인을 포함하며, eval을 제한하는 `--jobs`와는 별개입니다. Human 도구
동작과 제출은 따로 CPU 수만큼의 fingerprint 병렬성을 쓰며
이 설정을 물려받지 않습니다. 결과와 출력은 이 한도나
완료 순서에 의존하지 않습니다.

`verify --max-executions N`은 Run 하나가 실행기를 시작할 수 있는 횟수의 예산을
0 이상으로 정합니다(생략하면 무제한). 이 예산은 Run 안의 모든 eval이 함께 씁니다. Artifact 선언 필드는 아닙니다.
Run은 명시적인 `jobs` 상한(무제한이면 필드 없음),
`fingerprintJobs`, `maxExecutions`, `executionsStarted`를 기록합니다. 준비된
Runtime 또는 Agent 호출은 프로세스를 띄우지 못하거나
나중에 ERROR를 돌려주더라도 시작 횟수를 1 차감합니다. Human 대기, 맡기, 도구 동작은 차감하지 않습니다. fingerprint 준비/재확인, 캐시 적중,
다른 실행에 합류해 기다리는 요청도 차감하지 않습니다. 처음 실행기를 준비하다 실패하면 차감하지 않습니다. Agent
`execution_paths` 고정은 시작 횟수를 차감한 뒤에 이루어집니다. 고정에 실패해도
차감은 그대로이며, 모델을 호출하지 않고 ERROR/PREPARATION_FAILED를 기록합니다.
예산이 0이어도 재사용, 합류, Human 리뷰는 할 수 있습니다. 기다리던 요청이 실패하거나 비정상 종료된 소유자 대신 실행을 넘겨받을 때
예산의 적용을 받으며, 자기 Run의 남은 예산을 씁니다. 예산이 바닥나도 실행 중인 eval은 결코 중단되지 않지만,
남은 READY 요청은 `BUDGET_EXHAUSTED`가 되고 Run은 이유와 함께 INCOMPLETE로
끝납니다(종료 코드 4). 시작하지 못한 요청 없이 예산을 정확히 다 쓰는 것은
오류가 아닙니다. 예산을 미리 잡아 두거나 되돌려 주는 기능, 실행 허가 풀, 재시작 기록은 따로 없습니다.

Human 대기와 취소는 [제출, 지켜보기, 취소](../features/run.md#submit-observe-and-cancel)를 따르며,
맡기, 도구, 승인은 [Human 가이드](../features/human.md)에서 다룹니다.

선택 파일은 4 MiB 이하의 일반 파일이어야 하며, JSON
문자열 배열이나 한 줄에 앞뒤 공백을 제거한 ID 하나를 담습니다(UTF-8 BOM과 CRLF 허용). 중복 제거 전
ID가 1–100000개여야 합니다. 빈 줄은 무시되며, ID에는
ASCII 공백, 제어 문자, 쉼표가 들어갈 수 없습니다. 형식이 잘못된 JSON 배열은
결코 줄 단위 파싱으로 대체되지 않습니다. 상대 파일 경로는 `--repo`가 아니라
CLI의 cwd 기준으로 해석됩니다.

`verify ... --profile NAME`은 변형을 적용하기 전에, 선택과 재귀 정책이 포함한
모든 실행형 eval에 대해 선언된 완전한 `profile_variants` 항목을 고릅니다.
각 실행형 eval은 안전한 이름을 가진 변형을 최대 64개까지 선언할 수 있으며,
모두 기본 리뷰어 kind를 유지합니다. 알 수 없는 변형은
Run을 만들기 전에 실패하며, 소스 선언은 결코 다시 쓰지 않습니다. 라이브러리는
`project::selection::ProfileSelection::Named` 또는 `ProfileSelection::Evals`(
정규화된 eval에서 이름으로 가는 맵)를 받으며, 변형 적용 전 포함 범위 밖의 매핑은 실패합니다.
`--recursive`에서는 그 범위에 이미 있는 실행형 의존성에 변형이 적용됩니다. 그다음
Runtime 변형 인자가 범위 지정 참조와 의존성
관문을 다시 만들고, 포함 범위를 다시 계산합니다. 그 변형 인자로
새로 포함된 의존성은 기본 프로필을 유지하며, 확장된 범위에 이름 붙은 변형을
다시 적용하지는 않습니다. 선택된
변형은 실행 옵션입니다. 재사용 키에 들어가지 않으므로,
runtime 변형이 명령이나 args를 바꾸지 않는 한 서로 다른 변형(그리고 선언된 프로필)의 결과는 서로 재사용할 수 있습니다.
저장된 요청의 `profile`과 `options`는
결과를 만든 실행을 기술하며, 적중이 다른 프로필의 결과를 돌려주면 `requestedProfile`이
요청한 프로필을 유지합니다. 텍스트 출력은 결과를 만든 프로필의 설정이 다를 때만
재사용 표시에 `profile NAME`을 붙입니다. 설정이 동일한 변형의 결과에는
표시가 없으며, 저장된 `options.variant`로 식별합니다.

## 재사용만 하기 {#reuse-only}

`verify --reuse-only KINDS`는 `runtime`, `agent`,
`human`을 쉼표로 구분한 목록을 받습니다. 나열된 kind의 eval은 결과를 재사용하기만 할 수 있습니다. 로컬 캐시에 있는,
또는 [리뷰 저장소](../features/team/reference.md)가 설정되어 있다면 그곳에 있는
재사용 키의 완료된 기록입니다. 그런 기록이 없으면 그 eval은 실행되지 않습니다. 실행기 시작도,
제공자 호출도, Human 요청도 없으며, 그 키로 진행 중인 실행도 기다리지 않습니다.
강제된 eval은 캐시된 결과를 읽지 않으므로, 나열된 kind에 `--force`를 쓰면 결코
실행되지 않습니다. 나열되지 않은 kind는 평소대로 실행됩니다. CI에서
`artifactize verify --all --wait --reuse-only agent,human`은 테스트를 실행하고 모든
리뷰를 캐시나 저장소에서 가져옵니다([CI](../features/team/ci.md)).

실행되지 않은 eval은 `STALE`로 보고되며, 그 이유는
`blockedReason`에 있습니다.

```text
  docs/review [run-Hq2b9X-4]: STALE — Not reused (--reuse-only agent): no cached or stored result, so it was not executed.
```

이 eval에 의존하는 eval은 다른 증거가 빠졌을 때와 마찬가지로 대기하거나 막힙니다.
Run은 다른 eval이
RED나 ERROR여도, 예산이 바닥난 경우처럼 이유와 함께 INCOMPLETE로 끝나며(종료 코드 4), 그 kind를 `reuseOnly`로 기록합니다.
`status`는 이 플래그를 받지 않습니다. 그냥 `verify`가 무엇을 할지 예측합니다.

## 의존성 선택과 출력 {#dependency-selection-and-output}

`artifactize verify app/e2e`는 `--eval app/e2e`와 똑같이 정규화된 eval을 고릅니다.
필요한 의존성 범위의 eval을 실행하려면 `--recursive`를 추가하세요. 없으면
입력 증거가 이미 있어야 합니다. 파일 Artifact는 선언된 이름으로
선택하며, 사이드카는 주변 폴더와의 자동 관계를 만들지 않습니다.

의존성 순환을 포함해 어떤 eval이 기다리거나 막히는지는
[eval 의존성](../features/artifact.md#eval-dependencies)을 보세요.

status는 `execute`, `reuse`, `wait`, `blocked` 동작을 씁니다. 요청, 검증,
status에는 정규화된 eval ID인 `blockedBy`가 붙습니다. monitor는 관문 eval을
Artifact별로 `Waits for` 아래에 묶습니다. 태그, 파일 kind, 경로는 graph,
status, monitor 출력에 계속 보입니다.

## 저장된 Run 증거와 터미널 보기 {#saved-run-evidence-and-terminal-views}

`run show RUN_ID --json`과 `verify --wait --json`은 `evidence`가 비어 있지 않으면
최상위에 포함합니다. Run의 필드는 `run` 객체로 감싸지 않고 출력 객체의 최상위 필드로 들어갑니다.
`evidence`는 부분 Run의 캐시된 상위 증거처럼, 이 Run에서 요청 없이 쓴 저장된 판정에
정규화된 eval ID를 대응시킵니다. 이는 전역 최신
판정이 아닙니다. [Run](../features/run.md#verify-and-runs)을 보세요.
터미널 키와 레이아웃은 [Monitor](../tui/index.md#monitor)와
[Human 리뷰](../tui/index.md#review)를 보세요.

## 현재 디렉터리 도구 {#current-directory-tools}

`artifactize tools`는 `artifactize-tools`와 같은 CLI 정의와 실행기를 써서
[독립 실행형 범위 지정 도구](../features/tools/builtin.md#artifactize-tools-cli)도 제공합니다. 이 도구들은
`--repo`가 아니라 현재 디렉터리를 쓰며, 링크한 레퍼런스에서 경로,
URL, 실행 파일 찾기 규칙을 설명합니다. `artifactize tools --help`는 `check`와
이 도구들을 함께 나열하고, `artifactize tools SUBCOMMAND --help`는 각각을 설명합니다.


### Agent 스트리밍과 runtime 로그 {#agent-streaming-and-runtime-logs}

`verify SELECTOR --watch`는 제출하고 저장된 Run을 바로 따라가며,
`run watch RUN_ID`는 나중에 연결합니다. 둘 다 `--json`과 상관없이 바로바로 내보낸 JSON 줄을 출력합니다.
실시간 상태/개수는 `kind: update`, 간결한
저장된 요청 결과는 `kind: eval_result`, 최종 Run 검증은 `kind: outcome`입니다. `--watch`와
`--wait`는 함께 쓸 수 없습니다. 지켜보기는 Run 결과로 종료하며, 중단하면
130으로 종료하고 지켜보기만 멈춥니다. 다시 연결하면 정확한 과거 이벤트 기록이 아니라
완료된 eval을 다시 보냅니다. outcome의 `next_cursor`는 MCP
`verify_result`로 따라가거나, 전체 세부 내용이 필요하면 저장된 Run을 확인하세요.

실패한 runtime 발견 사항에는:

```sh
artifactize request logs REQUEST_ID --stream stderr --tail 30
artifactize request logs REQUEST_ID --stream stdout --start 100 --end 150
artifactize request logs REQUEST_ID --search 'failure' --tail 10
artifactize config graph INPUT_ARTIFACT --impact --json
```

로그 조회는 요청 감사 기록 전체가 아니라 로그 내용과 원래 줄 번호, 그리고 로그가
보존되어 있는지 없는지를 명시적으로 돌려줍니다. tail은 기본적으로 스트림당 일치하는 줄 50개를 돌려주고, 범위
조회 결과는 스트림당 최대 200줄과 정해진 바이트 수로 제한되며, 이어 읽으려면
`next_start`를 씁니다. tail과 범위는 함께 쓸 수 없습니다. `search`는 대소문자를 구분하는
1–2048바이트 리터럴 문자열입니다. 원본이 2048바이트보다 긴 줄은 크기가 제한된 앞/끝 텍스트와 함께
`clipped`로 표시됩니다. 128 KiB 저장 감사 기록을 넘는 끝부분을 포함한 전체 원본 runtime 스트림은
Run 출력에 남습니다. 현재 `prune`은 원본 스트림 파일과 완료
표시를 호출 루트에 남겨 두며, 원본 로그 보존 정책은 제공하지 않습니다. 없는 증거나
오래되었거나 원격에 있는 감사 기록뿐인 증거는 명시적으로 라벨이 붙습니다. 공용
[runtime 보존 레퍼런스](../features/declarations.md#runtime-execution-library)를 보세요. 공용 증거/이벤트 스키마와 클라이언트 전달 한계는
[MCP 집중 진단](../agent-plugin/mcp.md#streaming-and-focused-diagnosis)을 보세요.
