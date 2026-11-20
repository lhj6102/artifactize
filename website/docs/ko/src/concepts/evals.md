# eval: Runtime, Agent, Human {#evals-runtime-agent-and-human}

**eval**(`[evals.<id>]`, 전체 eval ID는 `<artifact>/<id>`)은 Artifact 하나를 대상으로 하는 검사입니다. 모든 eval에는 통과 조건을 요약하는 `title`, Runtime·Agent·Human 중 하나를 지정하는 `profile`, 그리고 구체적인 평가 기준을 적는 `payload.instruction`이 필요합니다. 대체 실행 프로필은 [`profile_variants`](../reference/declarations.md#eval-fields-evalsid)에 선언합니다.

| 종류 | 평가자 | 판정 및 결과 |
|---|---|---|
| `runtime` | 프로그램 실행 (`command` + `args`) | 종료 코드가 `0`이면 `GREEN`으로 판정하고, 그 밖의 경우에는 `RED`로 판정합니다. 실행 파일 누락, 시간 초과, 출력 저장 실패, 실행 중 파일 변경(`INPUT_CHANGED`) 같은 실행 오류는 `ERROR`로 기록됩니다. |
| `agent` | 선언된 읽기 전용 도구를 쓰는 단일 LLM (`backend` + `model`) | 엔진이 정한 `{priority, reason}` 형식의 `findings` 목록(`P0`~`P3`)을 반환합니다. Artifactize는 `red_at` 기준으로 `GREEN` 또는 `RED`를 판정합니다. |
| `human` | 대기 중인 요청을 맡아 양식을 제출하는 사람 | 리뷰어가 `GREEN` 또는 `RED` 판정 양식을 제출하면 Artifactize가 양식의 입력값을 스키마로 검증합니다. |

## Runtime eval {#runtime-evals}

```toml
[evals.tests]
title = "The library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"], timeout_ms = 240000 }
payload.instruction = "Run the tests of {library}."
```

- 지정한 실행 기준 폴더([대상 종류](artifacts.md#target-kinds-path-and-select))에서 `command`를 직접 실행합니다. 셸 명령 문자열을 해석하는 대신 `args`의 각 항목을 개별 인자로 넘깁니다(Windows에서 `.cmd`/`.bat` 배치 스크립트는 `cmd.exe`로 실행). 표준 입력(`stdin`)은 비워 둡니다. 호출한 프로세스의 환경 변수를 명령에 넘기되 기존 `ARTIFACTIZE_*` 변수는 제외하고, `ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, `ARTIFACTIZE_TMP_DIR`을 새로 넣어 전달합니다. `TMPDIR`/`TMP`/`TEMP`는 전용 임시 디렉터리를 가리킵니다.
- `args`에 적은 `{name}`이나 `{name}/하위경로`는 해당 Artifact의 절대 경로(또는 Live URI)로 바뀝니다. 다른 Artifact를 참조하면 해당 Artifact가 eval의 입력으로 추가됩니다. eval에서 검사 대상 자체를 참조하면 `{name}`을 해당 경로나 URI로 바꾸기만 하고 의존성은 추가하지 않습니다.
- Artifactize는 실행 중에 수집한 `stdout`과 `stderr`를 Run에 저장합니다. 저장된 출력은 `artifactize request logs REQUEST_ID`나 데스크톱 앱 Detail 패널에서 볼 수 있습니다. 실행 전후의 fingerprint나 Live 스냅샷을 비교해 값이 달라졌으면 판정 대신 `ERROR`(`INPUT_CHANGED`)를 기록합니다.

## Agent eval {#agent-evals}

```toml
[evals.review]
title = "The implementation satisfies the spec"
profile = { kind = "agent", backend = "codex", model = "gpt-6.2-sol", reasoning = "high", timeout_ms = 240000, max_tokens = 100000, max_tool_calls = 100 }
red_at = "P1"
payload.instruction = "Review {app} against {spec}. Report each unmet requirement as a finding."
```

- **백엔드와 모델**: `backend`에는 내장 백엔드인 `openai`, `anthropic`, `gemini`, `codex`, `claude-agent` 중 하나를 적습니다. [`artifactize provider`](../reference/cli-and-mcp.md#command-reference)나 데스크톱 앱 **Settings → Providers**에서 등록한 사용자 지정 프로바이더 이름도 쓸 수 있습니다. 내장 모델 목록이나 자동 대체(fallback)는 없으며 `model`에는 프로바이더가 제공하는 정확한 모델 ID(`artifactize models BACKEND`)를 적어야 합니다.
- **추론 강도와 사용량 한도**: `reasoning`은 프로바이더에 그대로 전달됩니다(`openai`와 `codex`: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`; `claude-agent`: `low`, `medium`, `high`, `xhigh`, `max`; `anthropic`: `low`, `medium`, `high`, `max`; `gemini`: `minimal`, `low`, `medium`, `high`). 선택 필드인 `max_tokens`는 프로바이더가 보고한 토큰 수를 기준으로 리뷰 전체의 사용량을 제한하고, `max_tool_calls`는 도구 호출 시도 횟수를 제한합니다. 한도를 넘기면 `ERROR/PROVIDER_BUDGET_EXCEEDED`로 종료됩니다.
- **읽기 전용 리뷰**: Agent는 대상 Artifact와 `{name}`으로 참조한 Artifact에 선언된 [도구](../guides/tools.md)만 사용해 리뷰합니다. `claude-agent`를 쓸 때도 로컬 Claude Code의 훅, 스킬, `CLAUDE.md`, 메모리, MCP 서버는 사용하지 않습니다.
- **지적 사항과 판정**: Agent는 `{priority, reason}` 항목으로 이루어진 `findings` 목록을 JSON 객체로 반환합니다. 우선순위는 `P0`이 가장 높고 `P3`이 가장 낮으며, 지적 사항이 없으면 빈 목록을 반환합니다. 이 형식에 맞지 않으면 `ERROR/INVALID_RESULT`로 끝나며, 저장된 대화는 `artifactize session show REF`로 확인할 수 있습니다. `red_at` 이상의 지적 사항이 하나라도 있으면 `RED`로 판정합니다(`red_at`을 생략하면 지적 사항이 하나만 있어도 `RED`). `red_at`보다 낮은 우선순위의 지적 사항은 `GREEN` 판정에서도 결과에 함께 남습니다.
- **제한 시간과 백엔드 중단**: `timeout_ms`는 Agent 리뷰 전체 시간을 제한합니다. 인증 실패(`AUTHENTICATION`)나 할당량 초과(`QUOTA`)가 발생하면 해당 Run에서는 그 백엔드의 새 리뷰를 더 이상 시작하지 않습니다(`BACKEND_STOPPED`). `artifactize login`으로 다시 로그인하거나 할당량을 확보한 뒤 `verify`를 다시 실행하세요.
- **저장된 대화**: 기본 설정에서는 각 리뷰의 전체 대화와 도구 호출 내역을 로컬의 `$STATE/agent-sessions/`에 저장합니다. 이 동작은 [`limits.json`의 `agentSessions`](../reference/cli-and-mcp.md#state-cache-and-limits)에서 설정할 수 있습니다. `artifactize session show REF [--summary]`로 내용을 확인하고, `artifactize session send REF "질문"`으로 기존 판정을 바꾸지 않고 후속 질문을 보낼 수 있습니다.

## Human eval {#human-reviews}

```toml
[evals.approved]
title = "The release notes are approved"
profile = { kind = "human" }
payload.instruction = "Open {notes} and verify the highlights before signing off."
```

Human eval에서는 리뷰어가 대기 중인 요청을 맡고 선언된 [Human 도구](../guides/tools.md#human-tools)로 대상을 확인한 뒤 `GREEN` 또는 `RED` 판정 양식을 제출합니다(`artifactize request` 명령, `artifactize review`, 또는 데스크톱 앱 사용). 승인을 요청하는 시점, 여러 Run이 기존 승인 요청의 결과를 공유하는 방식, 승인 결과를 재사용하는 조건은 [승인과 Run의 종료](runs.md#sign-offs-and-the-end-of-a-run)를 참고하세요.
