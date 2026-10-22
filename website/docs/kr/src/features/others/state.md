# 상태, 캐시, 제한 {#state-cache-and-limits}

artifactize가 상태를 어디에 보관하는지, 재사용 캐시를 어떻게 확인하고 제한하는지,
머신 전체에서 Agent 백엔드를 어떻게 제한하는지, 저장된 Agent 대화를 어떻게 보관하고
제한하는지, 그리고 로컬 진단과 유지 보수 명령을 다룹니다.

## 상태 {#state}

`state.sqlite` 하나에 모든 저장소의 Run이 들어 있으며(내장 SQLite, WAL, 스키마 9),
각 Run에는 정규화된 저장소 경로가 기록됩니다. 상태 홈은
`$ARTIFACTIZE_STATE_HOME`, 없으면 `$XDG_STATE_HOME/artifactize`, 없으면(Windows에서)
`%LOCALAPPDATA%\artifactize`, 없으면 `$HOME/.local/state/artifactize`입니다.

**Microsoft Store 패키지**는 Flexible Virtualization 제외 없이
Windows의 기본 AppData 가상화를 씁니다. AppData 아래의 새 상태는 패키지 전용입니다.
Store 버전은 zip이나 Cargo 설치와 공유하지 않고 자기만의 Run, 로그인, 캐시를
유지합니다. 제거하면 Windows가 그 전용 상태를 지웁니다.
위의 경로는 앱의 논리적 보기일 뿐, 반드시 실제 디스크 위치는 아닙니다.
[Windows 패키징 스모크 테스트](https://github.com/lhj6102/artifactize/actions/runs/39138230810)는
`state.sqlite`와 `review-store.sqlite`가 모두
`%LOCALAPPDATA%\Packages\lhj6102.Artifactize_ayfzfzbsv48sg\LocalCache\Local\artifactize` 아래에 있음을 측정했으며,
새로 준비한 Windows
Server 러너(빌드 26100)에서 실제 `%LOCALAPPDATA%\artifactize` 디렉터리는 만들어지지 않았습니다.

같은 테스트는 Runtime eval을 통해 PowerShell 자식 프로세스를 실행했습니다. 그 자식이 새로 만든
`%LOCALAPPDATA%` 파일은 **리디렉션되지 않았습니다**. 패키지되지 않은 셸이 그 파일을 패키지 전용 위치가 아니라
실제 AppData에서 찾았습니다. 이는 그 러너와 명령을 측정한 것이지,
모든 자식 프로그램이나 Windows 버전을 측정한 것이 아닙니다. 사용자가 설정한 명령은
사용자의 권한으로 실행됩니다. 그 AppData 쓰기가 패키지 전용이라거나,
Store 앱을 제거할 때 지워진다고 가정하지 마세요.

이것은 샌드박스가 아니며, 패키지되지 않은 기존 상태에 접근할 수 없다는 보장도 아닙니다.
[Microsoft 문서](https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization#default-msix-behavior)에 따르면
Windows 10 1903 이상에서 읽기는 가상화되지 않은 기존
AppData 파일로 대체될 수 있고, 그 기존 파일을 수정하면 실제 파일에 영향을 줄 수 있습니다.
Store와 패키지되지 않은 설치를 나란히 두어 기존 데이터를 격리할 수 있다고 기대하지 마세요.
AppData 밖에 명시적으로 둔 상태는 AppData 가상화나
Store의 전용 상태 제거 정리에 포함되지 않습니다.

`--state-dir PATH`는 `PATH/runs` 아래의 전용 출력 디렉터리를 포함해
상태 전체를 옮깁니다. 저장된 Run은 원래 저장소가 제거된 뒤에도 읽을 수 있습니다.
리뷰 대상 저장소나 다른 artifactize 워크스페이스 안의 상태/출력은
심볼릭 링크 상위 경로를 거친 경우를 포함해 거부됩니다.
이 안전 검사에서는 `.artf` 선언, 예전
`artifactize.json`, 예전 `.artifactizeignore`가 들어 있는 상위 경로가 있으면 워크스페이스로 봅니다.
예전 선언은 읽지 않더라도 마찬가지입니다. 사이드카만 있는 워크스페이스도 보호됩니다.
데이터베이스 파일과 그 WAL 사이드카는
일반 파일이어야 합니다.

알려진 제한: 검색 권한만 있는 상위 경로는 지나갈 수는 있지만 나열할 수는 없습니다. 나열이
권한 거부로 실패하면, 보호 장치는 `.git`, `index.artf`, 예전 `artifactize.json`, 예전
`.artifactizeignore`에 대한 고정 표시 확인으로 대체합니다. 그 상위 경로에서
사이드카만 있는 워크스페이스는 발견할 수 없습니다. 그런 워크스페이스 밖에 상태와 자격 증명을
명시적으로 두세요. 디렉터리 나열이 안 되는 곳에서는 사이드카 감지에 의지하지 마세요.

상태는 무엇이 리뷰되었는지, 어떤 판정이었는지, 어떻게 실행되었는지를
다섯 테이블에 보관합니다.

| 테이블 | 한 행이 나타내는 것 | `data`의 저장된 JSON 외의 열 |
|---|---|---|
| `runs` | Run | `id`, `repo`, `status`, null 가능한 `request_key`, `revision` |
| `run_definitions` | 정의가 있는 Run | `run_id` |
| `requests` | Run의 eval | `id`, `run_id`, `eval_id`, `ordinal`(Run의 선택 순서에서의 위치), `execution_id`, `status`, `claimed_by`와 `claimed_at`(대기 중인 요청의 Human 맡기), `revision` |
| `executions` | 실행 | `id`, `key`, `eval_def_hash`, `status`, `owner_pid`와 `owner_start_time`(소유 프로세스), `backend`(Agent 리뷰의 백엔드. [백엔드 용량](#backend-capacity) 참조), `profile_identity`(키가 있는 점유의 실제 프로필), [키 이력](#cache-inspection) 열 `completed_at`, `bytes`, `last_used`, 그리고 `revision` |
| `state_meta` | 상태의 이름 붙은 값 | `name`, `value` |

Run의 `data`는 바뀌는 상태입니다. 상태, 제어, 검증, 증거입니다. 그
정의(시작할 때의 Artifact, eval, 관계)는 Run의 수명 동안 고정되며
`run_definitions`에 Run과 함께 한 번 쓰입니다. Run을 다시 저장해도
정의는 결코 다시 쓰지 않습니다. 트리거가 `runs`,
`requests`, `executions` 행이 갱신될 때마다 `revision`을 올리므로, 읽는 쪽은 기록을 읽지 않고
숫자를 비교해 무엇이 바뀌었는지 알 수 있습니다. Run에는 eval마다, ordinal마다 요청이 하나씩 있습니다.
(재사용 키, 실제 평가 프로필) 조합마다 실행 하나만
동시에 활성(RUNNING 또는 WAITING_HUMAN)일 수 있습니다. 강제(`--force`) 실행은 이 점유를 우회하며
키의 활성 슬롯을 결코 차지하지 않습니다. `state_meta`에는 상태의 고정
ID, 즉 데이터베이스를 만들 때 생성한 무작위 UUID(행 `id`)가 있으며,
[Agent 세션 참조](#agent-sessions)가 이를 가리킵니다. 리뷰가 어떻게 진행되었는지, 그
대화와 도구 호출은 상태에 없습니다. 그것은 리뷰의
[저장된 세션](#agent-sessions)에 있습니다.

지원되는 업그레이드는 순서가 정해진 추가 단계이며, 여는 쓰기 트랜잭션 하나에서
`user_version`과 함께 적용됩니다. 버전은 그 잠금 아래에서 다시 읽습니다. 스키마 6은
null 가능한 `runs.request_key`와, 키가 NULL이 아닌 경우의 `(repo, request_key)`에 대한 부분 고유 인덱스를
추가해 7로 업그레이드됩니다. 기존 Run의 키는 NULL로 남습니다. 스키마 7은
`executions.profile_identity`를 추가하고, 저장된 프로필로 채우고,
활성 키 고유 인덱스를 `(key, profile_identity)`로 바꿔 8로 업그레이드됩니다. 스키마 8은
각 Run의 정의를 기록에서 `run_definitions`로 옮기고(읽히지 않는
기록은 그 텍스트를 유지), `revision` 열과 그 트리거, 그것을 읽는 인덱스를 추가해
9로 업그레이드됩니다. Run/요청/실행 ID, 캐시,
상태 정체성, Human 승인은 남으며, 다시 열어도 다시 마이그레이션하지 않습니다. 기존 상태를
읽는 쪽과 `doctor`는 없는 데이터베이스를 만들지 않고 같은 사전 확인을 씁니다. 마이그레이션에는
쓰기 권한이 필요하며, 실패한 트랜잭션은 버전 확인을 우회하지 않고 되돌려집니다.

같은 저장소/키는 같은 접수된 Run을 돌려주며, NULL 키는 결코 제출을 합치지 않습니다.
작업자의 PID/시작 정체성과 입장, 취소 의도, 활동, 실패 코드는
선택적이거나 기본값이 있는 Run JSON 제어 필드에 있습니다. 이 필드가 없는 예전 Run도 읽을 수 있으며
죽은 작업자로 취급되지 않습니다. 저장된 Run 종료 시 검증은 과거 기록으로 남습니다. 자세한 결과는
또 다른 결과 스냅숏으로 저장하지 않고 저장된 Run, 요청, 실행에서 도출합니다.
죽은 작업자 복구는 확정된 증거를 유지하고, 정체성을 확인한 소유 자식만 정리하며,
죽은 슬롯을 놓습니다. 작업을 다시 실행하지 않고 OWNER_DIED를 기록하며, 확인되지 않은
정리는 그 저널 기록을 유지합니다.

**다운그레이드 경계:** 스키마 6~8만 지원하는 바이너리는 업그레이드 후의 스키마 9
상태를 열 수 없으며, 버전 보호 장치가 이를 거부합니다. 알 수 없거나 더 새 버전과 스키마 1–5는 여전히 지원되지 않습니다.
명령은 2로 종료하고 `doctor`는 치명적 오류(종료 코드 1)를 보고합니다. 이전 스키마에 대한 메시지는 다음과 같습니다.

```text
This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). Only schema 6 and later supported versions can be upgraded.
```

새 0.9 상태는 Human 승인을 포함해 모든 실행형 eval을 한 번씩 리뷰한 뒤,
이전처럼 재사용합니다. 재사용 키 v2는 스크립트 fingerprint와 팀 저장소 기록을 포함해
이전의 어떤 기록과도 일치하지 않습니다.
[0.9로 업그레이드](../fingerprint.md#upgrading-to-09)를 보세요.

토큰은 결코 저장소 안에 저장되지 않습니다. Codex
로그인과 리뷰 저장소 토큰을 담는 `$STATE/auth`가 git 작업 트리나
artifactize 워크스페이스(위의 현재 또는 예전 표시가 있는 상위 경로) 안에 있거나,
`--repo` 안에 있으면 gitignore된 폴더라도 거부됩니다. `.git` 확인은 Artifact 선언 표시와 별도로 Git 작업
트리를 보호합니다. 그러면 `login codex`와
`remote login`은 로그인하기 전에 두 폴더를 모두 알려 주는 메시지와 함께 실패합니다.

```text
Codex sign-in storage /work/project/.state/auth is inside the git work tree /work/project; artifactize keeps tokens outside repositories. Use a state directory outside it, or set ARTIFACTIZE_CODEX_AUTH_FILE.
```

저장소 밖의 상태 디렉터리를 쓰거나, 저장소가 필요 없는 `ARTIFACTIZE_CODEX_AUTH_FILE`과
`ARTIFACTIZE_REMOTE_TOKEN`을 쓰세요. `doctor`는 그런 상태를 그곳에 아무것도 저장되지 않은 동안에는
경고로 보고하고, 로그인이 저장되면(예를 들어
기존 상태 위에서 `git init`을 한 뒤) 치명적 오류로 보고합니다. 어떤 쓰기 트랜잭션도
하위 프로세스나 비동기 중단에 걸쳐 있지 않습니다. 재사용 키가 있는 완료된 결과는
이 데이터베이스 안에서 공유됩니다. 다른 워크트리와 프로세스를 포함한 동시 Run은
재사용 키와 실제 평가 프로필 전체가 모두 일치할 때만 진행 중인 실행에
자동으로 합류합니다. 대기열의 요청은 기다리는 실행을 가리키며,
실행 시작이나 백엔드 슬롯을 차지하지 않습니다. 완료되면 그
실행의 GREEN, RED, ERROR가 기다리던 쪽에 합류한 재사용으로 전달됩니다. ERROR는 이후 Run을 위한
새 캐시 항목이 아닙니다. Runtime이나 Agent 실행에서 취소나
죽은 소유자(PID와 프로세스 시작 시각으로 확인)는 점유를 놓습니다. 기다리던 쪽 하나가
대체 실행을 시작하고, 나머지는 그것에 합류합니다. Human 승인은 대체되지 않습니다.
그것을 소유한 Run이 취소되거나 작업자가 멈추면, 그 승인과 그것을 기다리는
모든 요청이 어느 Run에서든 ERROR로 끝나며, 새 `verify`가 다시 요청합니다.
기다리는 쪽을 취소해도 소유자는 취소되지 않습니다. `--force`는 언제나
자기 실행을 하며 결코 합류하지 않습니다. 공유 설정은 없으며, 진행 중인 실행의
공유는 결코 원격 저장소를 넘지 않습니다. 완료된 GREEN/RED 재사용은 만든 프로필이 달라도
재사용 키만을 기준으로 합니다.

## 읽을 수 없는 기록 {#unreadable-records}

데이터베이스와 다른 기록이 정상이어도 저장된 기록 하나를 읽을 수 없는 경우가 있습니다.
Artifactize는 판정을 지어내지 않고 그 기록을 리뷰 증거에서 건너뜁니다. 잘못된 요청은
Run 목록의 요청 수에서 제외됩니다. 요청이 손상된 Run도 여전히 열 수 있고,
읽을 수 없는 Run 기록은 평범한 저장된 Run으로 취급하지 않고 읽을 수 없음으로 표시합니다.

status와 Run 목록은 기록의 `kind`,
`id`, `reason`과 함께 `unreadable` 진단을 보여 주며, 증거를 읽는 쪽은 stderr에도 `Unreadable ...` 진단을
출력합니다. 올바른 기록은 계속 쓸 수 있습니다. 진단을 위해 상태를 보존하고,
새 증거가 필요하면 새 검증을 시작하세요. 개수가 빠졌다고 해서
손상된 리뷰가 통과했다는 뜻은 아닙니다. 이는 데이터베이스 열기나
지원되지 않는 스키마 실패와 다르며, 그런 실패는 명령이 상태를 아예 읽지 못하게 할 수 있습니다.
`doctor`는 그 데이터베이스/스키마 준비 상태를 확인할 뿐, 기록 복구 도구가 아닙니다.

## 캐시 확인 {#cache-inspection}

```sh
artifactize cache list [--history] [--json]
artifactize cache show KEY [--history]
artifactize cache rm KEY
```

이 명령들은 저장소를 불러오지 않고 공유 상태 홈이나 `--state-dir PATH`를 씁니다.
각 재사용 키는 완료된 모든 GREEN/RED 기록을 보관하며, 완료 시각 기준으로 가장 최근 기록을
`verify`가 재사용합니다. `list`는 각
키의 가장 최근 기록을 보여 줍니다. 키, eval, 판정, 완료 시각, 만든 사람, 저장소(또는 그 기록이 온 원격 저장소),
키가 가진 기록 수, 보존된 JSON 바이트, 마지막 사용 시각입니다(텍스트
표, 또는 Eval 정의 해시, 대상의
fingerprint, 프로필 변형도 담은 JSON 배열). `--history`는 모든 기록을 나열하며, 각 키 안에서
최신순입니다. `show`는 키의 가장 최근 저장된 실행을 JSON으로 출력합니다.
결과, 실제 `profile`, `options`(백엔드, 모델, reasoning, 제한,
변형), `fingerprints`(키가 포함하는 각 Artifact), 추가 필드 `artifactKinds`
(포함된 이름을 `folder`/`file`/`graph`로 대응), 출처, 사용량,
`producer`(`user@host`와 artifactize 버전. 제출된 Human 결과는
`reviewer`도 기록)입니다. 키가 없으면 `null`을 출력하고 4로 종료합니다. `show
--history`는 키의 모든 기록을 최신순 JSON 배열로 출력합니다.
[공유 원격 리뷰 저장소](../team/reference.md)에서 미러링한 항목에는 `origin`
(저장소, 게시자, 게시 시각)이 붙으며, `list`는 저장소 대신 이를
보여 줍니다. 읽기는 없는 상태를 만들거나 접근 시각을 갱신하지 않습니다. 키의
기록은 이력 열
(`completed_at`, `bytes`, `last_used`)이 설정된 완료된 GREEN/RED 실행이며, 가장 최근 기록은 가장 늦게 완료된 것,
그다음 가장 최근에 저장된 것입니다. `rm`은 키의 모든 기록을 이력에서 빼고
`{"removed":true}`(없으면 false)를 출력하며, 저장된 Run과
실행 자체는 보존합니다. `rm`은 활성 실행이나 기다리는 쪽이 있는 키를 거부합니다. 키는
`cache list`나, `verify --wait --json`과 `run show`에 나오는 요청의 `key`에서 얻습니다.

재사용 키를 어떻게 만들고 적중하면 무엇을 돌려주는지는
[완료된 결과 재사용](../fingerprint.md#completed-result-reuse)에 있고, 항목과 바이트
제한은 [캐시 제한](#cache-limits)에 있습니다.

## 캐시 제한 {#cache-limits}

새로 재사용할 수 있는 결과를 저장하면, **기록 10,000개**와 저장된 결과 데이터 **1 GiB** 이내로 유지하도록
가장 오래 재사용되지 않은 제거 가능한 캐시 기록을 자동으로 제거합니다.
기록 하나는 최대 **16 MiB**를 차지할 수 있습니다. 그보다 큰 결과도
자기 Run과 현재 기다리는 쪽에는 전달되지만, 이후 재사용을 위해 보관되지는 않습니다.

기록을 재사용하면 마지막 사용 시각이 갱신되지만, 확인만 해서는 갱신되지 않습니다. 정리는
가장 오래 쓰지 않은 기록부터 제거하며, 동점은 키와 완료
시각으로 가립니다. 그 키의 리뷰가 실행 중인 동안에는 키의 기록을 유지하고,
다른 Run이 아직 필요로 하는 기록도 유지하므로 이 제한을 일시적으로 넘을 수 있습니다. 정리 실패는
이미 완료된 판정을 바꾸지 않고 stderr에 출력되며, 다음에 결과를 저장할 때 정리를 다시 시도합니다.

캐시 정리와 `cache rm`은 기록이 재사용되지 않게 할 뿐입니다.
저장된 실행 감사 기록, Run 영수증, Run 출력 파일은 제거하지 않습니다. 그래서 이 제한은
데이터베이스 전체나 활성 Run의 임시 파일을 제한하지 않습니다. 일치하는 증거는
나이에 따른 만료가 없으며, 캐시 정리는 Run 파일을 자동으로 삭제하지 않습니다.
제거할 수 있는 끝난 Run 출력을 지우려면 `artifactize prune`을 쓰세요.

## 백엔드 용량 {#backend-capacity}

READY eval에는 기본 Run 동시 실행 상한이 없습니다. 명시적인 `--jobs N`은
그 Run만 제한하므로, 제한 없는 동시 실행이 문제를 일으킬 때 추가하세요. 그래도 한 머신의 여러
Run(워크트리, CI 작업, 팀원의 통합 실행)이 한 백엔드에
속도 제한이나 구독이 허용하는 것보다 많은 Agent 리뷰를 한꺼번에 보낼 수 있습니다.
`$STATE/limits.json`은 그 상태 디렉터리를 쓰는 모든 `verify`에 걸쳐
백엔드별로 진행 중인 Agent 리뷰 수를 제한합니다.

```json
{"backends": {"codex": 4, "openai": 16}}
```

키는 프로필이 선언하는 백엔드 이름(`openai`, `anthropic`, `codex`)이며,
각 제한은 1–100000입니다. 항목이 없는 백엔드는 무제한이고, 파일이 없으면
아무것도 제한되지 않습니다. 파일은 `verify`가 시작할 때 읽습니다. 일반 파일이 아니거나,
올바른 JSON이 아니거나, 다른 필드가 있거나, 알 수 없거나 제거된
백엔드를 지정하거나, 제한이 범위를 벗어나면 `verify`는 Run을 만들기 전에 실패합니다(종료 코드 2).
`doctor`는 제한을 보고하거나, 문제를 치명적 오류로 보고합니다.

기록은 `state.sqlite`의 `executions` 테이블에 남습니다. Agent 리뷰의
실행은 자기 `backend`를 기록하며, 상태가 RUNNING인 동안 그 백엔드의 슬롯 하나를
차지합니다. 슬롯은 리뷰가 시작되기 직전에 잡습니다. 쓰기
트랜잭션 하나(`BEGIN IMMEDIATE`)가 그 백엔드의 RUNNING 실행을 세고,
제한 아래라면 이 실행을 삽입합니다. 결과를 재사용하거나 그 키의 살아 있는
실행에 합류할 요청은 슬롯을 잡지 않습니다. 슬롯은 리뷰의 실행이
완료되면 풀립니다. 소유 프로세스가 종료한 실행(거두어지지 않은
좀비 포함. 프로세스 상태, PID, 시작 시각으로 확인)은 다음에 어떤 프로세스가 슬롯을 요청할 때 ERROR(`OWNER_DIED`)로 끝나며,
그와 함께 슬롯도 풀립니다. 별도의 정리는 없습니다. 리뷰의 실패가
그 Run에서 백엔드를 멈추면([`BACKEND_STOPPED`](../agent.md#stopping-a-backend)),
슬롯이 풀리기 전에 멈춤이 기록되므로, 그 슬롯을 기다리던 요청은
시작되지 않습니다.

백엔드의 모든 슬롯이 쓰이고 있으면 요청은 QUEUED로 기다리며,
`Waiting for a free codex slot: all 4 are in use on this machine
(limits.json).` 같은 이유가 붙습니다. 슬롯이 풀리면 재시도는 Run의 스케줄링 우선순위를 따릅니다.
READY 작업이 먼저, 그다음 남은 의존성 경로가 가장 긴 작업이며, 선택 순서는
동점을 가릴 때만 씁니다. 그래서 나중에 선택된 요청이 다음 슬롯을
먼저 받을 수 있습니다. 기다리는 동안 요청은 명시적인 `--jobs` 상한에 포함되지 않고
`--max-executions` 시작도 쓰지 않으므로, runtime eval, Human 요청, 다른 백엔드는
계속 실행됩니다. 입장하면 `--jobs`가 주어진 경우 그에 포함되며, 그
상한이 없으면 백엔드 용량은 다른 동시 실행 제한을 걸지 않습니다. 명시적으로 선언한
[eval mutex 태그](../declarations.md#eval-mutex-tags)는 같은 공유 상태 저장소에 걸쳐
충돌하는 runtime eval과 Agent eval을 따로 늦추며, 기다리는 동안 백엔드 슬롯을
차지하지 않습니다. 명시적인 `run cancel RUN_ID`는
기다리는 요청을 상태 ERROR, `errorCode = "CANCELLED"`로 끝내고, 차지한 슬롯을 놓습니다.
`verify --wait`를 중단하면 지켜보기만 멈출 뿐 Run은 취소되지 않습니다.

도구 세션 상태는 저장된 Agent 대화와 별도로 `$STATE/tool-sessions/` 아래에 있습니다.
Artifactize는 정리가 확인되면 이를 제거하고, 확인되지 않은
상태는 복구를 위해 보관합니다. 시작, 중지, 비정상 종료 복구에 대한 하나의 자세한 계약은
[도구 세션 수명 주기](../tools/agent.md#tool-sessions)를 보세요.

## Agent 세션 {#agent-sessions}

각 Agent 리뷰의 대화는
[`session show`와 `session send`](../agent.md#saved-conversations)를 위해 상태 아래에 저장됩니다.
`limits.json`은 대화를 저장할지와 그 저장소가 얼마나 커질 수 있는지를 정합니다.

```json
{"agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368}}
```

| 필드 | 기본값 | 의미 |
|---|---|---|
| `enabled` | `true` | `false`면 새 대화를 저장하지 않음. 저장된 대화는 계속 읽을 수 있음 |
| `maxBytes` | 1 GiB | 정리를 시작하는 저장소 크기. 최소 1 |
| `targetBytes` | `maxBytes`가 768 MiB보다 크면 768 MiB, 그렇지 않으면 `(maxBytes / 4) * 3`(정수 나눗셈을 먼저 함) | 정리가 저장소를 줄여 놓는 크기. 명시적인 값이 기본값을 덮어쓰며 `maxBytes`보다 작아야 함 |

정리는 `verify` Run이 eval을 스케줄하고 확정한 뒤 한 번, 그리고
`prune`에서 실행됩니다. 준비, 스케줄링, 저장 중에 그보다 먼저 실패한 Run은 건너뛰며,
`prune`은 요청받을 때 정리합니다.
대화 파일이 `maxBytes`보다 많이 차지하면, 가장 오래전에 쓴 세션부터
하나씩 `targetBytes` 이하가 될 때까지 삭제하므로, 정리할 때마다 세션 하나가 아니라
일정한 양을 비웁니다. 아직 RUNNING인 요청의 세션이나
`session send`가 잡고 있는 세션은 결코 삭제하지 않습니다. 그런 세션 때문에
이후 정리 전까지 저장소가 목표보다 클 수 있습니다. `verify` 중에 실패한 정리는
stderr에 보고되고 Run은 그대로 둡니다. 다른
`limits.json` 규칙도 적용됩니다. 알 수 없는 필드, 타입이 틀린 값, `maxBytes` 0,
`maxBytes`보다 작지 않은 `targetBytes`는 `verify`와 `prune`을 실패시키며(종료 코드 2),
`doctor`는 이를 치명적 오류로 보고합니다.

저장소는 `$STATE/agent-sessions/`이며 소유자 전용이고(0700, Windows에서는 보호된 소유자 전용 DACL),
그렇지 않으면 거부됩니다. 각 세션은 `sessionId`로 이름 붙은,
덧붙이기만 하는 소유자 전용(0600) JSON Lines 파일 하나입니다.

```text
$STATE/agent-sessions/
  e83bf374-5136-484c-8d2f-2ce38ee4ca57.jsonl   the conversation, one event per line
  e83bf374-5136-484c-8d2f-2ce38ee4ca57.lock    held while a session send appends to it
```

이벤트는 자기 `kind`를 밝히고 보통 `at`에 시각을 기록합니다. 예전
기록에는 선택적인 타임스탬프나 머리말 메타데이터가 없을 수 있습니다. 새 기록은 다음을 씁니다.

| `kind` | 담는 것 |
|---|---|
| `review` | 첫 줄: `version`(1), `sessionId`, `runId`, `requestId`, `evalId`, `target`, `producer`, `state`, `backend`, `model`, `reasoning`, `parameters`(보낸 그대로, `prompt_cache_key` 포함), `budgets`(`timeoutMs`, `maxToolCalls`, `maxTokens`), `tools`(제공한 정의) |
| `message` | 제공자가 받거나 보낸 그대로의 메시지 하나(`message`: rig 메시지. `role`은 `system`, `user`, `assistant`이며 텍스트, 도구 호출, 도구 결과, reasoning, 암호화된 내용과 서명을 포함), 그리고 그것을 실은 `turn`(제공자 요청). 도구 결과 메시지에는 각 결과가 실패했는지를 순서대로 담은 `isError`가 있음. 복구 프롬프트에는 `"repair": true`가 있고, 후속 질문의 틀을 씌운 질문에는 사람이 직접 쓴 말이 `question`에 있음 |
| `delivery` | 표시 전용 스트리밍 데이터: `turn`, `attempt`, 비어 있지 않은 `block` 식별자, `contentKind`(`text` 또는 `summary`), `text`, `state`(`delta`, `complete`, `interrupted`) |
| `attempt` | `turn`의 제공자 시도 하나: `attempt` 번호, 제공자가 보고한 `usage` 카운터(보고된 경우 `inputTokens`, `outputTokens`, `totalTokens`, `cacheReadTokens`, `cacheWriteTokens`, `reasoningTokens`), 실패했으면 `error`와 `errorCode` |
| `end` | 리뷰의 `result`, 또는 `errorCode`와 `error` |
| `send` | 후속 질문 시작: `send` 번호, 사람의 `text`, 그 앞에 보낸 `framing`, `filesChanged`, 제공한 `tools` |
| `answer` | 후속 질문의 답 `text`, 또는 `errorCode`와 `error` |

delivery의 `delta` 기록은 증분 텍스트를 담고, `complete`는 완료된
블록을 표시하며, `interrupted`는 완료 전에 끝난 전달(중단된 시도 포함)을
표시합니다. 이 기록은 실시간 표시에만 쓰입니다. 대화 재생 이력이나 사용량 합계에는
결코 더해지지 않으며, 그것은 의미 있는 메시지와 시도 기록에서 나옵니다.

후속 질문의 이벤트에는 그 `send` 번호도 붙습니다. 예산, 마감 시간,
반복된 호출 ID가 턴의 도구 호출을 멈추면, 실행된 호출의 결과는
저장되고 나머지에는 결과가 없습니다. 요청의 판정, 결과, 사용량은
`state.sqlite`에 남습니다. 세션은 대화와 그 도구 호출에 대한 유일한 기록이며,
[`session show --summary`](../agent.md#session-summary)가 이를 셉니다.
비정상 종료로 잘린 마지막 줄은 파일을 읽을 때 무시됩니다.

### 후속 질문 재생 {#follow-up-replay}

`session send`는 저장된 리뷰의 백엔드, 모델, reasoning, 세션 ID를 씁니다.
OpenAI와 Codex는 같은 `prompt_cache_key`를 유지하고, Codex는
`session-id` 헤더도 유지합니다. 저장된 이력과 시스템 프롬프트는 바뀌지 않은 채 재생되므로
캐시된 앞부분이 일치합니다. 현재 프로젝트 도구가 다시 제공되며, 포함된
fingerprint가 바뀌었으면 머리말과 모델의 맥락에 경고가 더해집니다.

질문은 새 리뷰가 아니라 후속 질문이라는 틀을 씌운 새 사용자 턴입니다. 그
틀은 일반 텍스트를 요청하고, 도구 사용을 허용하며, 기록된 판정은
바뀌지 않는다고 밝힙니다. 최신 리뷰 시스템 프롬프트도 같은 구분을 밝히며,
예전 저장 대화는 새 턴의 틀에 의지합니다. `session show`는
사람의 원래 질문을 출력하고, JSON 내보내기는 그 틀도 보존합니다.

질문, 답, 도구 호출은 같은 저장된 스레드에 덧붙여집니다. 한 세션으로 보내는 것은
직렬화되어, 각각 앞선 답 뒤에 이어집니다. 질문마다
새 프로필 예산을 받습니다. RUNNING 요청에는 후속 질문을 할 수 없으며, 기록된 판정,
결과, 사용량, 재사용 증거는 바뀌지 않습니다.

### 제공자 전송 {#provider-transport}

rig-core 0.43.0 제공자 클라이언트는 `store:false`, 암호화된 reasoning 재생,
병렬 도구 호출 끄기를 적용한 스트리밍 OpenAI Responses를 쓰거나,
Anthropic Messages를 씁니다. Anthropic에는 리뷰 예산과 별개로 턴당 출력 상한
16384 토큰이 필요합니다. 리뷰 마감 시간 하나가 모든 턴, 재시도,
도구를 포함합니다. 불완전한 응답은 부분 답에 판정이 들어 있더라도 오류입니다.
중첩된 HTTP 재시도와 리디렉션은 꺼져 있으며, artifactize는
[재시도와 오류 코드](../agent.md#retries-and-error-codes)에 설명된 대로 턴 재시도를 처리합니다.

### 대화 기록 리더 {#transcript-reader}

Monitor Detail은 원본 메시지 봉투나 비공개 reasoning 페이로드가 아니라,
읽을 수 있는 메시지 텍스트, 공개 reasoning 요약, 접힌 도구 활동을 보여 줍니다.
스트리밍 갱신은 임시 텍스트를 최종 내용으로 바꾸며, 중단된
부분 출력에는 라벨이 붙습니다. 공개 OpenAI/Codex 요약은 제공자 클라이언트 라이브러리인 rig에
전달되는 바이트를 바꾸지 않고 스트리밍 프레임에서 읽습니다.
표시 전용 이벤트는 재생되는 모델 이력에 결코 들어가지 않습니다.

리더는 스크롤을 옮기기 전까지 새 메시지를 따라갑니다. 크기를 바꾸면
일시 정지 중에는 읽던 위치를 유지하고, 따라가는 중에는 맨 아래에 머뭅니다. 도구
활동을 펼치면 전체 도구 인자나 성공한 출력이 아니라 이름, 대상, 기록된 상태가
나옵니다. `session show --json`은 저장된 이벤트 전체를 내보냅니다.
없는 로컬 세션, 저장된 적이 없는 대화, 원격 참조는
따로 보고하며, 원격 참조로 임의의 로컬 파일을 열 수는 없습니다.

이력은 전용 임시 텍스트 파일과 행 파일에 증분 방식으로 색인되며,
보이는 페이지만 불러옵니다. 처음 2 MiB로 이력을 자르는 고정 한도는 없습니다.
완전한 이벤트 하나를 디코딩하는 동안에는 그 크기에 비례하는 메모리가 필요하며,
다른 이력과 레이아웃 작업은 크기가 제한된 조각 사이마다 양보합니다. 형식이 잘못된 완전한
이벤트는 오류를 보고하고, 일부만 있는 줄과 불완전한 UTF-8은 다음
덧붙이기를 기다립니다. 교체, 잘림, 삭제는 리더를 초기화하며, 리더를 닫거나 초기화하면
그 임시 파일이 제거됩니다. 이후의 `session send` 이벤트는 원래 리뷰가 끝난 뒤에도
읽을 수 있습니다.

### 로컬 리더 {#local-readers}

Monitor와 Review는 상태를 읽기만 하며, 새로 고치려고 선언이나
fingerprint를 다시 탐색하지 않습니다. 유일한 예외는 대기 중인 Human 리뷰의
Detail을 여는 것입니다. 그때는 리뷰를 맡거나 무엇을 실행하지 않고, 그 리뷰의 도구 명령을 보여 주려고
현재 선언을 읽습니다. 리더는 변경을 일어나는 대로 따라갑니다. 처음 연 리더가
임시 소유자 전용 로컬 채널(데몬 없음)을 열고, 쓰는 쪽은 그곳에 어떤
Run을 바꿨는지 알리며, 리더는 열어 둔 Run을 포함해 그 Run만 다시 읽습니다.
채널이 없거나 무언가를 놓치면 5초마다 하는 확인이 따라잡고,
교체된 데이터베이스는 처음부터 다시 읽습니다. 리더를 열 때는
Run 목록과 연 Run만 읽으므로, 아주 큰 Run도 빠르게 따라갈 수 있습니다.
이는 로컬 조정이지, 팀 저장소에서 밀어 주는 전달이 아닙니다.

Run 작업자는 이와 별도로 Human 대기 마감 시간 없이 죽은 실행 소유자와 백엔드 슬롯,
선택적인 원격 결과를 조정합니다. 명시적인 Human 동작은
공유된 원자적 맡기, fingerprint 재확인, 스키마 검증,
`submit_and_publish` API를 씁니다. 팔로워는 원래 요청으로 해석됩니다. 로컬 확정 뒤
게시에 실패하면 그 확정은 보존되고 표시됩니다.

## 로컬 진단과 유지 보수 {#local-diagnostics-and-maintenance}

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|codex [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune`은 현재 저장소가 아니라 선택된 상태의 모든 저장소에 걸친 Run을 대상으로 합니다.
제거할 수 있는 Run 상태는 정확히 **GREEN, RED, ERROR, INCOMPLETE**이며,
완료 타임스탬프가 있고, 요청이 모두 종료 상태이고, 살아 있는 실행 소유자
(PID와 프로세스 시작 시각)가 없어야 합니다. SIGNOFFS_PENDING Run은 Human 요청이 확정된 뒤에도
**언제나 건너뜁니다**. 이후의 제출이 과거 Run 상태를 다시 쓰지 않기 때문입니다.
끝나지 않은 다른 Run도 건너뜁니다. `--older-than`은 Run 완료 시각을 비교하며,
정수 `s`, `m`, `h`, `d`, `w` 기간을 쓰세요. 나이 필터가 없으면 제거할 수 있는 모든
Run을 고려합니다. `--dry-run`은 삭제하지 않고 `wouldRemove`를 돌려주며,
일반 JSON은 `removed`를 돌려주고, 둘 다 `skippedRuns`를 포함합니다. `prune`은 또한
`--older-than`과 상관없이 크기 한도에 따라 [Agent 세션 저장소](#agent-sessions)를 정리합니다.
`removedSessions`는 삭제된 세션을 나열하고, `--dry-run`에서는
`wouldRemoveSessions`가 크기 한도를 넘는 세션을 나열합니다. 시험 실행은 실제 prune이 건드리지 않는
실행 중인 `session send`를 확인하지 않으므로, 둘의 결과가 다를 수 있습니다.

`doctor`가 확인하는 것, `models` 출력, `prune`이 정확히 어떤 디렉터리를
제거하는지는 [레퍼런스](#doctor-models-and-prune)에 있습니다.

### Doctor, models, prune {#doctor-models-and-prune}

`doctor`는 제공자를 호출하지 않으며 Run, 판정, 캐시 항목, 인증
파일을 만들지 않습니다. 해석된 상태 디렉터리를 보고하고, 즉시 제거하는 임시
디렉터리로 쓰기 가능 여부를 시험합니다(상태가 아직 없으면 가장 가까운 기존 상위 경로에서).
쓰기 시험 전에 `--repo`가 없어도 같은 현재/예전
워크스페이스 표시 안전 경계를 적용하며, 안전하지 않은 상태는 그
워크스페이스 안에 시험 디렉터리를 쓰지 않고 실패합니다. 상태 데이터베이스의
스키마를 읽으며, 필요하면 지원되는 추가 업그레이드를 적용합니다.
[이전 artifactize가 쓴](#state) 데이터베이스(스키마 1–5)나 더 새 버전이 쓴
데이터베이스는 치명적 오류(종료 코드 1)이며, 다른 명령이 거부할 때와 같은 메시지와
세부 정보의 `schema`, `supported`가 함께 나옵니다. `--repo`는
추가로 `config check`와 같은 정적 검증을 실행합니다. `limits` 확인은
[`limits.json`](#backend-capacity)을 읽고 백엔드 용량을 보고합니다(잘못된
파일은 치명적 오류). `sessions` 확인은
[Agent 세션 저장소](#agent-sessions)를 보고합니다. 담고 있는 세션 수와 그 바이트,
`maxBytes`, `targetBytes`, 저장이 `enabled`인지, 저장소의 `directory`, 상태의
`stateId`입니다. API 키는
있는지 없는지만 보고하며, 결코 검증하거나 출력하지 않습니다. 키가 없으면
경고입니다. 선택적인 백엔드가 모두 설정되어 있을 필요는 없기 때문입니다.
[테스트 엔드포인트](../agent.md#test-against-a-fake-provider)가 있는 백엔드는
`testEndpoint`로 보고하는 경고이며, 잘못된 테스트 엔드포인트는 치명적 오류입니다.
`codex` 확인은 잠금, 갱신, 네트워크 호출 없이 로그인을 읽습니다. 로그인이 없으면
경고이고, 저장된 로그인은 액세스 토큰이 만료되었더라도 통과하며(다음
사용 때 갱신), 만료된 `ARTIFACTIZE_CODEX_AUTH_FILE` 토큰은
경고입니다. [저장소 안의](#state) `$STATE/auth`는 그곳에 로그인이 저장되지 않은 동안에는
세부 정보의 `refused`에 메시지를 담은 경고이고, 저장되면
치명적 오류입니다. 세부 정보에는 `source`(`stored`, `file`, `none`), `expiresAt`,
`expired`, 그리고 설정된 경우 `testEndpoint`/`testAuthEndpoint`가 나옵니다. 원격 리뷰
저장소 확인도 오프라인입니다. 해석된 URL, 공유 수준, 토큰 출처를 보고하며,
토큰이 없으면 경고이고, 잘못된 설정(루프백이 아닌 호스트로의 일반 HTTP 포함)이나
안전하지 않은 토큰 파일은 치명적 오류입니다. 잘못된 설정,
안전하지 않거나 쓸 수 없는 상태, 잘못된 인증 저장소는 치명적 오류입니다. 종료 코드는
치명적 오류가 없으면 0, 있으면 1, 호출 오류면 2입니다.

`models openai`와 `models anthropic`은 rig를 통해 해당 API 키로
제공자의 모델 엔드포인트를 호출하며, Anthropic 페이지 나누기를 따라갑니다. `models codex`는
로그인한 계정의 [Codex](../agent.md#codex) 모델을 나열합니다.
테스트 엔드포인트는 여기서도 제공자의 API 루트를 대신합니다. 계정이나 백엔드로의
대체는 없습니다. 텍스트는 탭으로 구분한 slug/name 쌍을 나열하고, JSON은
항상 `{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`입니다.
목록은 제공자의 순서를 유지합니다.

`state/runs/<run-id>` 아래의 알려진 스크래치 디렉터리만 제거합니다. runtime
`output`(그리고 이전 버전이 남긴 `tmp`, `home`, `cache`)과 남은 도구 출력입니다.
Run 루트, 알 수 없는 파일/디렉터리, 데이터베이스 행, 결과, 재사용 기록은
남습니다. 심볼릭 링크(중첩된 링크 포함), 디렉터리가 아닌 대상,
저장소 내용은 삭제하기 전에 거부됩니다. `.git`과 위의 현재/예전
Artifact 표시는 워크스페이스 사본을 보호하며, Run 출력 안에 복사된
Git 없는 프로젝트도 포함됩니다. `--dry-run`과 실제 prune 모두 이 경계를 적용합니다.
데이터베이스 읽기는 5초의 busy 시간 제한을 가지며 삭제 전에 끝나고, prune은
쓰기 잠금을 잡지 않습니다. 이것은 격리 보관, 비정상 종료 복구 장치,
악의적인 파일 시스템 경쟁 보호가 없는 평범한 prune입니다.
저장된 `run show`와 `request show`는 prune 뒤에도 읽을 수 있습니다.
