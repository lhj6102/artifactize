# CLI와 MCP 레퍼런스 {#cli-and-mcp-reference}

대부분의 `artifactize` 하위 명령은 명령 앞이나 뒤에 `--repo PATH`, `--state-dir PATH`, `--json`을 받습니다.
- `monitor`와 `review`는 대화형 TUI 화면이므로 `--json`을 받지 않습니다.
- `mcp`는 `--repo`와 `--state-dir`를 사용하며, 항상 표준 출력(`stdout`)으로 JSON-RPC 메시지를 보내고 진단 로그는 표준 오류(`stderr`)에 출력합니다.
- 내장 도구(`artifactize tools <read|list|glob|grep|section|help|open>` 및 단독 `artifactize-tools`)는 현재 작업 디렉터리에서 동작하고, 결과는 도구별 텍스트 또는 JSON 형식으로 출력하며 `--repo`, `--state-dir`, `--json`을 쓰지 않습니다.

`SELECTOR` 자리에는 위치 인자 `ARTIFACT` 또는 `ARTIFACT/EVAL`, `--eval ID`, `--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`, `--all` 중 하나를 지정합니다(`--evals-file`과 `--artifacts-file`에 넘기는 파일에는 각각 전체 eval ID 또는 Artifact 이름을 JSON 문자열 배열로 넣거나 한 줄에 하나씩 적습니다).

## CLI 명령 레퍼런스 {#command-reference}

| 명령 | 플래그 및 인자 | 출력 및 동작 | 종료 코드 |
|---|---|---|---|
| `verify SELECTOR` | `[--profile NAME]` (선택 사항; 지정하면 대체 실행 프로필 적용 전 선택 범위의 모든 실행 가능 eval에 해당 대체 실행 프로필이 있어야 하며, Runtime 대체 실행 프로필의 `args`로만 추가된 의존성은 기본 프로필 사용), `--recursive`, `--force`, `--jobs N`, `--fingerprint-jobs N` (기본값: 사용 가능한 CPU 수), `--max-executions N`, `--wait` \| `--watch`, `--request-key KEY`, `--comment TEXT`, `--reuse-only KINDS`, `--speculate` | Run을 제출하거나 진행 상황을 확인합니다([Run과 실행 흐름](../concepts/runs.md) 참고). | `0` 접수 완료, 또는 `--wait`/`--watch` 시 [Run 결과 코드](../concepts/runs.md#sign-offs-and-the-end-of-a-run) |
| `status [SELECTOR]` (기본 `--all`) | `[--profile NAME]`, `--recursive`, `--force`, `--fingerprint-jobs N` (기본값: 사용 가능한 CPU 수) | eval 상태(`PASS`, `RED`, `ERROR`, `BLOCKED`, `WAIT_DEPENDENCY`, `WAITING_HUMAN`, `DEFERRED`, `PROVISIONAL`, `STALE`, `UNREVIEWED`)와 Artifact 상태(위 상태에 `INCOMPLETE`, `BASIS` 포함), 그리고 예상 동작(`execute`, `reuse`, `wait`, `blocked`)을 보여 줍니다. 선택 범위 밖의 요건은 `not included`로 표시하며 동작별 합계에서 제외합니다. 재사용 키는 있지만 현재 쓸 수 있는 결과가 없을 때, 같은 전체 eval ID와 평가 기준으로 저장한 이전 로컬 기록이 있으면 그 기록과 비교해 바뀐 입력을 알려 줍니다. | `0` 충족됨, `1` 아직 충족하지 못한 검증 요건이 있음(개별 eval `ERROR` 예측 포함), `2` 명령/상태 오류 |
| `config check` | — | 프로젝트 코드를 실행하지 않고 `.artifactize/` 선언을 정적으로 검증합니다. | `0` 유효함, `2` 오류 |
| `config graph [ARTIFACT]` | `--impact` | Artifact, eval, 순환 참조로 묶인 항목, 각 입력을 사용하는 eval을 출력합니다(`--impact`를 지정하면 해당 Artifact에 의존하는 하위 항목도 표시). | `0` |
| `config migrate` | `--repo PATH` | 기존 트리 내 `.artf` 파일들을 `.artifactize/<name>.artf`로 옮깁니다. | `0` |
| `config rename OLD NEW` | — | 정의 파일, `parts`, `{OLD}` 참조에서 Artifact 이름을 `OLD`에서 `NEW`로 바꿉니다. | `0` |
| `run list` | `--repo-only` \| `--all`, `--limit N` (50), `--offset N` (0) | 기본값(`--repo-only`)은 현재 저장소 또는 `--repo`로 지정한 저장소의 Run을 보여 주고, `--all`은 상태 디렉터리의 모든 저장소에 저장된 Run을 보여 줍니다. 목록은 최신순으로 정렬하며 상태, 개수, 메모를 함께 표시합니다. | `0` |
| `run show RUN_ID` | `--wait [--timeout-ms MS]` | 저장된 Run 전체 JSON(`evidence`, `requests`, `usage`, `validation`)을 출력합니다. `--wait`는 Run이 끝날 때까지 기다립니다(기본 대기 제한 시간 10분, `--timeout-ms`는 `--wait`와 함께 사용). | `0` (`--wait` 시 Run 결과 코드, `--wait` 제한 시간 초과 시 Run은 실행 중인 채로 `3`) |
| `run watch RUN_ID` | — | Run이 끝날 때까지 진행 상태(`update`, `eval_result`, `outcome`)를 JSON 줄로 출력합니다. | Run 결과 코드 |
| `run cancel RUN_ID` | — | 아직 끝나지 않은 Run(`RUNNING` 또는 `WAITING_HUMAN`)에 대한 취소 요청을 기록·확인하고 이미 저장된 결과는 유지합니다. | `0` |
| `request list` | `--run RUN_ID` | 저장된 요청 목록과 상태, 현재 맡은 담당자를 보여 줍니다(텍스트 출력 기준이며, JSON에는 `claim`과 `summary` 포함). | `0` |
| `request show ID` | — | 저장된 요청 JSON(`evalId`, `status`, `result` 등 최상위 요청 필드와 `claim`, `execution`, `definition`, `provenance`, `summary`)을 출력합니다. | `0` |
| `request logs ID` | `--stream stdout\|stderr\|both` (기본 `both`), `[--tail N \| --start N [--end N] \| --end N]` (기본 `--tail 50`), `--search TEXT` | 캡처된 Runtime `stdout`/`stderr` 로그를 조회합니다(`complete`, `partial`, `saved_audit`, `missing`, 또는 Agent·Human 요청의 `not_runtime`; `--tail`과 `--start`/`--end` 범위 조회는 함께 쓸 수 없음). | `0` |
| `request claim ID` | `--reviewer NAME` | 대기 중인 Human 요청의 담당자로 지정합니다(기본 리뷰어: `$USER`, 없으면 Unix는 `$LOGNAME`, Windows는 `%USERNAME%`). | `0` |
| `request unclaim ID` | `--reviewer NAME` | 판정을 제출하지 않고 Human 담당 지정을 해제합니다. | `0` |
| `request tool ID TOOL` | `--reviewer NAME` | 맡은 요청에 선언된 Human 도구를 실행합니다. | `0` (`2` 도구 오류) |
| `request submit ID` | `--verdict GREEN\|RED`, `[--fields JSON \| --fields-file PATH]`, `--reviewer NAME` | 결과 필드(`--fields`와 `--fields-file`은 상호 배타적이며 둘 다 생략하면 `{}` 제출)를 판정 스키마로 검증하고 Human 요청 판정을 제출합니다. | `0` (`RED` 제출도 `0`) |
| `session show REF` | `--summary` | 저장된 Agent 리뷰 대화 전문(또는 턴·토큰·도구 호출 요약)을 출력합니다. | `0` |
| `session send REF MSG` | — | 기존 판정을 바꾸지 않고 저장된 Agent 대화에 후속 질문을 보냅니다. | `0` |
| `tools check [EVAL]` | 정적 검증: `[EVAL \| --eval ID] [--tool NAME]` 또는 `[--artifact ID] [--audience agent\|human] [--tool NAME]`; 시험 실행: `--execute --tool NAME (EVAL \| --eval ID \| --artifact ID --audience agent\|human) [--args JSON]` (`--args`는 Agent 도구 전용) | 도구 선언과 실행 파일을 정적으로 검증합니다(`--execute`로 단일 도구 시험 실행 가능). | `0` 준비됨/성공, `1` 검증/실행 실패 |
| `tools <subcommand>` | `read`, `list`, `glob`, `grep`, `section`, `help`, `open` | 현재 디렉터리에서 내장 도구를 실행합니다(`artifactize-tools`와 동일). | `0` 성공, `1` 도구 오류, `2` 사용법 오류 |
| `cache list` | `--history` | 각 재사용 키의 최신(또는 전체) 캐시 기록을 나열합니다. | `0` |
| `cache show KEY` | `--history` | 64자리 16진수 `KEY`의 캐시 기록을 JSON으로 출력합니다. | `0` (`4` 기록 없음) |
| `cache rm KEY` | — | 해당 `KEY`의 로컬 캐시 기록을 삭제합니다. | `0` |
| `login` / `logout` | `openai\|anthropic\|gemini\|codex\|claude-agent` | 표준 입력으로 API 키를 저장/삭제하거나 Codex·Claude Code에 로그인/로그아웃합니다. | `0` |
| `provider` | `list`, `add NAME (--alias P \| --api openai-responses\|openai-chat\|anthropic\|gemini --url URL [--key])`, `remove NAME` | `$STATE/auth/providers.json`의 사용자 지정 Agent 프로바이더를 관리합니다. | `0` |
| `models PROVIDER` | 내장 또는 사용자 지정 프로바이더 이름 | 해당 프로바이더에서 사용할 수 있는 모델 ID와 표시 이름을 나열합니다. | `0` |
| `remote` | `login URL [--share summary\|full]`, `status`, `push [--dry-run]`, `logout` | 팀 리뷰 저장소 연결을 설정하고 결과를 동기화합니다(`summary`는 최상위 `executionPaths`를 공유하고 `execution.provenance.repoPath` 및 `execution.provenance.executionPaths`가 포함된 전체 `execution` 객체는 제외하며, `full`은 `execution`을 포함). | `0` (`status` 미로그인 시 `1`) |
| `server` | `run [--listen IP:PORT]`, `token add NAME --scopes SCOPES`, `token list`, `token revoke NAME [--purge]`, `rm KEY` | 팀 리뷰 저장소 서버를 실행하고 토큰과 기록을 관리합니다. | `0` |
| `doctor` | `--repo PATH` | 상태 디렉터리, 스키마, 제한 설정, 자격 증명, 원격 저장소, 데스크톱 앱, 새 릴리스를 점검합니다. | `0` 준비됨, `1` 심각한 오류 |
| `limits` | `--signoff-idle-grace SECONDS` | `$STATE/limits.json`(해당 상태 디렉터리를 쓰는 Run들이 공유)의 제한을 조회하거나 설정합니다. | `0` |
| `prune` | `[--older-than DURATION]`, `[--dry-run]` | 미결 요청이 없는 완료된 `GREEN`/`RED`/`ERROR`/`INCOMPLETE` Run의 임시 출력 폴더를 삭제하고 Agent 세션을 정리합니다. `--older-than`을 생략하면 정리 대상인 Run은 완료된 시점과 관계없이 모두 정리하며, `YELLOW` 및 실행 중 Run은 건너뜁니다. | `0` |
| `monitor [PATH \| --repo PATH \| --all]` | 시작 범위는 하나만 지정할 수 있습니다 | 대화형 터미널 Monitor UI를 실행합니다. | `0` |
| `review [REQUEST_ID]` | `--repo PATH` \| `--all`, `--reviewer NAME` | 대화형 터미널 Review UI를 실행합니다. | `0` |

## 상태 디렉터리, 캐시, `limits.json` {#state-cache-and-limits}

모든 로컬 Run 기록, 캐시, 저장된 Agent 대화, 그리고 Artifactize가 저장한 자격 증명(`codex`, API 키, 사용자 지정 프로바이더, 원격 저장소 토큰; `claude-agent` 로그인은 Claude Code 자체가 보관)은 저장소 바깥의 상태 디렉터리 하나에 저장됩니다. 경로 우선순위는 `--state-dir PATH` → `$ARTIFACTIZE_STATE_HOME` → `$XDG_STATE_HOME/artifactize` → Windows의 `%LOCALAPPDATA%\artifactize` 또는 Linux·macOS의 `~/.local/state/artifactize`입니다(Microsoft Store 패키지는 분리된 전용 상태를 사용). 상태 디렉터리가 Git 워크트리나 Artifactize 프로젝트 안에 있으면 `$STATE/auth/` 아래 자격 증명 저장이 거부됩니다.

같은 상태 디렉터리를 사용하는 모든 Run에 공통으로 적용할 한도가 필요하면 `$STATE/limits.json`에서 설정하세요.

```json
{
  "backends": { "codex": 4, "openai": 16 },
  "signoffIdleGraceSeconds": 120,
  "agentSessions": { "enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368 }
}
```

- `backends`: 같은 상태 디렉터리를 사용하는 Run들 전체에서 백엔드별로 동시에 실행할 수 있는 최대 Agent 리뷰 수.
- `signoffIdleGraceSeconds`: 맡은 사람이 없는 Human 승인만 남았을 때 Run이 `YELLOW`로 끝나기 전까지 기다리는 유예 시간(초 단위, 자세한 동작은 [승인과 Run의 종료](../concepts/runs.md#sign-offs-and-the-end-of-a-run) 참고).
- `agentSessions`: `$STATE/agent-sessions/`의 대화 저장 여부와 용량 정리 기준을 설정합니다.

## MCP stdio 서버 (`artifactize mcp`) {#mcp-stdio-server}

`artifactize mcp`는 표준 입출력(stdio)을 통해 검증 및 비대화형 조회 도구를 제공합니다. 각 도구 호출은 다음 순서로 대상 프로젝트를 찾습니다.

1. 호출할 때 선택적 `project` 인자에 넘긴 프로젝트 폴더의 절대 경로를 먼저 사용합니다.
2. 그다음에는 서버를 시작할 때 `--repo PATH`로 지정한 경로를 사용합니다. 프로젝트 폴더에서 시작하지 않는 호스트에 서버를 수동으로 등록할 때는 이 옵션을 쓰세요.
3. 호스트가 알려 주는 첫 번째 로컬 MCP 루트(root)를 사용합니다.
4. MCP 루트 기능을 지원하지 않는 호스트에서는 서버가 시작된 작업 디렉터리를 사용합니다(단, 서버를 실행한 플러그인 폴더 자체는 제외).

서버에 `--repo`, 호스트 MCP 루트, 또는 허용된 시작 디렉터리가 없어 프로젝트를 알 수 없는 경우(예: Codex 플러그인이 `--repo` 없이 플러그인 폴더에서 서버를 시작한 경우)에는 호출할 때마다 `project`에 절대 경로를 넘기세요.

### MCP 서버 등록 {#register-the-mcp-server}

Claude Code와 Codex에서는 [플러그인](../agent-plugin/plugin.md)을 설치하면 서버가 자동으로 등록됩니다. 프로젝트 폴더에서 수동으로 등록하려면 다음 명령을 실행하세요.

```sh
claude mcp add --transport stdio --scope project artifactize -- artifactize mcp   # Claude Code
codex mcp add artifactize -- artifactize mcp                                      # Codex CLI
```

### 검증 및 조회 도구 {#verification-and-inspection-tools}

모든 MCP 도구에서 `project` 인자에 프로젝트 폴더의 절대 경로를 지정할 수 있으며, 이 인자는 생략할 수 있습니다. 조회 결과를 페이지별로 나누어 받을 때는 선택적 `cursor` 인자도 함께 지정할 수 있습니다.

| 도구 | 인자 | 용도 |
|---|---|---|
| `verify_start` | **`scope`** (`{"kind":"all"}`, `{"kind":"artifact","artifactId":"..."}`, `{"kind":"eval","evalId":"..."}`, `{"kind":"artifacts","artifactIds":[...]}`, `{"kind":"evals","evalIds":[...]}`), 선택적 `request_key`, `comment`, `jobs`, `speculate` | Run을 제출하고 Run ID와 초기 상태를 돌려줍니다. |
| `verify_status` | **`run`** | 저장된 Run 상태, 개수, 경과 시간(초), `next_check`를 돌려줍니다. |
| `verify_wait` | **`run`**, 선택적 `seconds` (0–40, 기본 30) | Run이 끝나거나 제한 시간이 지나면 대기를 멈춥니다. |
| `verify_watch` | **`run`** | 최대 30초 동안 Run의 진행 상황을 실시간으로 확인합니다. 호스트가 요청 메타데이터에 `_meta.progressToken`을 넣어 호출하면 `notifications/progress`(`update`, `eval_result`, `outcome`) 알림을 보내며, 진행 알림을 표시하지 않는 호스트에서는 `verify_wait`를 쓰세요. |
| `verify_result` | **`run`**, 선택적 `cursor` | 최종 판정과 페이지 단위의 `findings`를 돌려줍니다(실행 중에는 `ready: false`). |
| `verify_cancel` | **`run`** | 아직 끝나지 않은 Run의 취소를 요청합니다. |
| `status` | 선택적 `scope` (Selection 객체, 기본 `{"kind":"all"}`) | 현재 상태를 보여 주고, `verify`를 실행하면 각 eval을 실행(`execute`)할지, 재사용(`reuse`)할지, 대기(`wait`)시킬지, 차단(`blocked`)할지 알려 줍니다. |
| `config_check` | 없음 | `.artifactize/` 선언을 정적으로 검증합니다. |
| `config_graph` | 선택적 `scope` (Selection 객체), `impact` (불리언, 기본 `false`) | Artifact, eval, 순환 참조로 묶인 항목, 각 입력을 사용하는 eval을 조회합니다(`impact: true`를 지정하면 해당 Artifact에 의존하는 하위 항목도 표시). |
| `request_logs` | **`id`**, 선택적 `stream` (`"stdout"\|"stderr"\|"both"`), `tail`, `start`, `end`, `search` | 요청 `id`의 Runtime 출력 줄을 조회합니다. |
| `run_list` | 선택적 `limit` (기본 20), `offset` (기본 0) | 이 프로젝트에서 실행한 Run의 기록을 최신순으로 보여 줍니다. |
| `run_show` | **`run`** | 저장된 Run의 전체 JSON을 조회합니다. |
| `request_list` | 선택적 `run` | 이 프로젝트(또는 지정한 `run`)의 저장된 요청 목록을 조회합니다. |
| `request_show` | **`id`** | 저장된 요청의 전체 JSON을 조회합니다. |
| `session_show` | **`reference`**, 선택적 `summary` (불리언, 기본 `false`) | 저장된 Agent 리뷰 대화(또는 요약)를 조회합니다. |
| `tools_check` | 선택적 `eval`, 또는 `artifact`, `audience` (`"agent"\|"human"`), `tool` | 선언된 리뷰 도구를 정적으로 검증합니다. |
| `doctor` | 없음 | 로컬 상태, 자격 증명, 원격 저장소, 데스크톱 앱, 최신 릴리스를 점검합니다. |
| `models` | **`provider`** | 내장 또는 사용자 지정 프로바이더의 모델 ID 목록을 조회합니다. |
| `remote_status` | 없음 | 원격 리뷰 저장소 설정과 연결 가능 여부를 확인합니다. |
| `cache_show` | **`key`**, 선택적 `history` (불리언, 기본 `false`) | 64자리 16진수 재사용 키의 캐시 기록을 조회합니다. |

조회 도구는 CLI의 JSON 출력을 페이지별로 나눠 `entries`와 `next_cursor`가 들어 있는 객체(`{"encoding":"json","entries":[{"offset":0,"text":"..."}],"next_cursor":...}`)로 반환합니다. `next_cursor`가 `null`이 될 때까지 같은 인자에 `cursor`만 추가해 호출한 뒤 `entries[].text`를 순서대로 이어 붙여 JSON으로 파싱하세요. `config_check`로 선언을 검증하는 방법은 [Hey Arti](../agent-plugin/plugin.md#hey-arti-shape-and-audit-your-project)를, 에이전트가 검증 루프에서 `verify_start`, `verify_wait`, `verify_result`, `request_show`, `request_logs`를 조합해 호출하는 방법은 [Focus then sweep](../agent-plugin/plugin.md#focus-then-sweep-iterate-on-red-evals)을 참고하세요.

백엔드 로그인이나 로그아웃, Human 요청의 담당 지정·담당 해제·판정 제출, 그리고 `prune`, `cache rm`, `remote push`, `session send`는 MCP에서 지원하지 않으므로 사람이 직접 CLI나 UI에서 수행하세요.
