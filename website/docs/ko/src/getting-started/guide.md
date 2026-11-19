# 5분 만에 첫 프로젝트 만들어 보기 {#first-project-in-5-minutes}

이 가이드에서는 작은 프로젝트를 직접 만들고 `artifactize verify`를 실행한 뒤, 일부 파일만 바꿨을 때 이전 검사 결과를 어떻게 재사용하는지 확인합니다. AI 계정이나 API 키는 필요하지 않습니다.

## 1. 두 Artifact와 eval 선언하기 {#1-create-two-artifacts-and-a-check}

임시 프로젝트 폴더를 만들고 `spec.md`, `guide.md` 파일과 `.artifactize/` 폴더를 준비합니다.

```sh
mkdir -p /tmp/my-first-artifactize/.artifactize
cd /tmp/my-first-artifactize

cat > spec.md <<'EOF'
# Greeting Spec
Every greeting guide must mention `hello`.
EOF

cat > guide.md <<'EOF'
# User Guide
Run `hello` to greet the team.
EOF
```

이제 두 Artifact를 선언합니다. `spec`은 검증하지 않고 입력으로만 쓰며(`basis = true`), `guide`에는 Runtime eval 두 개를 선언합니다. 첫 번째 eval은 제목 형식을 확인하고, 두 번째 eval은 `{spec}`이 요구하는 `hello`가 본문에 들어 있는지 확인합니다.

```sh
cat > .artifactize/spec.artf <<'EOF'
path = "spec.md"
basis = true
EOF

cat > .artifactize/guide.artf <<'EOF'
path = "guide.md"

[evals.heading]
title = "The guide includes a top-level heading"
profile = { kind = "runtime", command = "grep", args = ["-q", "^# ", "{guide}"] }
payload.instruction = "Check that {guide} includes a top-level Markdown heading."

[evals.mentions-hello]
title = "The guide mentions the command required by the spec"
profile = { kind = "runtime", command = "grep", args = ["-q", "hello", "{guide}"] }
payload.instruction = "Check that {guide} mentions the greeting required by {spec}."
EOF
```

선언이 유효한지 점검하고 Artifact Graph와 현재 상태를 확인합니다.

```sh
artifactize config check
artifactize config graph
artifactize status
```

`config check`는 프로젝트 명령을 실행하지 않고 선언 문법을 검증합니다. `status`는 `guide/heading`과 `guide/mentions-hello` 두 eval의 예상 동작을 모두 `execute`로 표시합니다.

## 2. 검증 실행과 결과 재사용 {#2-verify-and-reuse}

모든 검사를 실행하고 결과가 나올 때까지 기다립니다.

```sh
artifactize verify --all --wait
```

두 eval은 모두 `GREEN`으로 통과하고, 명령은 종료 코드 `0`을 반환합니다. 함께 출력된 Run ID(예: `run-Abc123`)로 저장된 실행 기록을 조회할 수 있습니다. 이제 같은 명령을 한 번 더 실행해 보세요.

```sh
artifactize verify --all --wait
```

이번에는 아무 명령도 실행되지 않습니다. 파일 fingerprint(`artifactsum`)와 eval 정의 해시가 바뀌지 않아 재사용 키도 그대로입니다. 따라서 두 eval 모두 첫 번째 Run의 `GREEN` 결과를 즉시 재사용합니다.

## 3. 입력 하나만 바꾸고 다시 검증하기 {#3-change-one-input-and-see-what-re-runs}

`guide.md`는 그대로 두고 `spec.md`에만 한 줄을 추가합니다.

```sh
printf '\nKeep greetings short.\n' >> spec.md
artifactize status
```

`status`는 `guide/heading`에는 **재사용(`reuse`)**, `guide/mentions-hello`에는 **실행(`execute`)**을 표시합니다. Artifactize는 `{guide}`만 읽는 `guide/heading`의 이전 결과를 재사용하고, 바뀐 `{spec}`을 참조하는 `guide/mentions-hello`만 다시 실행합니다.

```sh
artifactize verify --all --wait
```

실제로 `guide/mentions-hello` 하나만 다시 실행되고 `guide/heading`은 이전 결과를 재사용합니다.

## 4. 저장소 예제 둘러보기 {#4-explore-the-repository-examples}

`https://github.com/lhj6102/artifactize` 저장소를 복제하면 `examples/` 폴더에서 주요 기능을 다룬 예제 프로젝트를 바로 실행해 볼 수 있습니다.

| 예제 | 다루는 기능 |
|---|---|
| [`examples/runtime-relations`](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations) | 상위 Artifact와 `parts` 구성(`intro`, `usage`로 이루어진 `guide`), 지시문과 인자의 `{artifact}` 참조, `basis = true`, 그리고 선행 eval이 `RED`일 때 하위 eval이 막히는(`BLOCKED`) 동작. |
| [`examples/posts`](https://github.com/lhj6102/artifactize/tree/main/examples/posts) | 여러 파일 Artifact가 하나의 공용 스타일 검사기(`{house-style}`)를 참조해 파일별로 결과를 재사용하는 구조와 `tags`. |
| [`examples/agent-tools`](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools) | 내장 도구(`read`, `grep`, `view_image`, `section`)와 사용자 지정 `json` 도구를 쓰는 Agent eval, `pass_schema`/`fail_schema`, 그리고 `open`·`read` 도구를 쓰는 Human 승인. |
| [`examples/live`](https://github.com/lhj6102/artifactize/tree/main/examples/live) | 외부 서비스 URI(`path = "https://..."`)를 `verify`할 때마다 읽기 전용 HTTP 명령으로 확인하는 Live Artifact. |

## 다음 단계 {#next-steps}

- **[코딩 에이전트와 함께 쓰기](../agent-plugin/plugin.md)**: **Arti**와 대화하며 프로젝트에 맞춰 `.artifactize/` 선언을 설계하고 **Focus then sweep**으로 빠르게 문제를 수정합니다.
- **[데스크톱 앱](../guides/interfaces.md)**: `artifactize-desktop`이나 `artifactize monitor`에서 실시간으로 갱신되는 Artifact Graph와 Run 기록을 살펴봅니다.
