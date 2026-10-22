# CI {#ci}

**지속적 통합(CI)** 은 팀이 코드를 푸시하거나 풀 리퀘스트를 열 때 자동으로 검사를 실행합니다.
CI 작업은 러너 머신에서 실행되는 명령의 연속이며,
개발자의 Agent/Human 승인을 재사용해야지 그 결정을 직접 내려서는 안 됩니다.

CI 단계로 `artifactize verify --all --wait --reuse-only agent,human`을 실행하세요. 테스트는 실행하되 리뷰 결과는 캐시나 팀 저장소에서 재사용하므로, CI에서는 모델을 호출하거나 사람의 승인을 요청한 채 대기하지 않습니다.
선택된 모든 eval이 GREEN일 때만 0으로 종료하므로, RED(1), ERROR(2), INCOMPLETE(4)는
작업을 실패시킵니다. 모든 종료 코드는 [명령 레퍼런스](../../cli/index.md#command-reference)에 있습니다. 재사용할 것이 없는
Agent 리뷰나 Human 승인은 [실행되지 않고](../../cli/index.md#reuse-only) Run을 INCOMPLETE로 남기므로, CI는
개발자의 `verify`가 현재 키에 대해 그것을 만든 뒤에야 통과합니다. Artifactsum이
기본으로 재사용을 켜 두며, 대상이나 의존성의 `fingerprint = false`는 재사용을 끕니다.

## 미룬 작업을 병합 게이트에서 빼기 {#keep-deferred-work-out-of-the-merge-gate}

개발자는 사람이 자리를 비운 동안 기계 작업을 마치려고 `--defer-human`을 쓸 수 있으며,
`--speculate`를 함께 쓸 수도 있습니다. SIGNOFFS_PENDING(종료 코드 5), DEFERRED 승인,
PROVISIONAL 기계 결과는 승인이 아닙니다. CI의
`--reuse-only agent,human` 게이트를 유지하세요. 빠진 승인은 실행되지 않고
INCOMPLETE로 남으며, `--defer-human`을 넘기더라도 통과로 셀 수 없습니다.
사람이 GREEN을 제출하면 CI는 바뀌지 않은 자동 검사 결과와 승인을 재사용합니다. RED는 의존하는 eval을 막습니다. 그냥 verify가 접수된 것을 CI 성공으로 취급하지 마세요.

## 팀 저장소의 판정 재사용하기 {#reuse-verdicts-from-the-team-store}

아래 YAML은 완전한 워크플로가 아니라 **GitHub Actions 단계 하나**입니다. 리뷰된 커밋을
체크아웃하고 러너에서 `artifactize`를 쓸 수 있게 한 다음
([설치](../../getting-started/install.md) 참조), 기존 `.github/workflows/NAME.yml` 파일의
`jobs.YOUR_JOB.steps` 아래에 넣으세요. 이번이 처음이라면 GitHub의
[워크플로 가이드](https://docs.github.com/en/actions/writing-workflows/quickstart)에서 이 단계를 담을 워크플로와 작업을 만드는 방법을 확인하세요.

저장소 관리자에게 **read** 범위만 있는 토큰을 요청하세요. GitHub
저장소에서 **Settings → Secrets and variables → Actions**를 열고,
`ARTIFACTIZE_READ_TOKEN`이라는 저장소 시크릿(secret)을 추가해 그 토큰을 값으로 넣으세요. 아래의
예시 URL은 팀이 접근할 수 있는 저장소 URL로 바꾸세요. `${{ secrets... }}`
표현식이 실행 시점에 비밀을 제공하므로, 토큰을 이 파일에 붙여 넣지 마세요.
다른 CI 서비스에서도 시크릿 저장 기능이나 환경 변수 설정을 쓰세요.

```yaml
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

- 읽기 토큰으로는 결과를 재사용할 수 있지만 게시할 수는 없습니다.
- 장애는 오류가 아닙니다. `verify`는 한 번 경고하고 로컬 캐시로 계속합니다.
  그러면 저장소에만 있는 리뷰는 재사용되지 않으므로 Run은 INCOMPLETE가 됩니다.
- 거부된 토큰, TLS 실패, 잘못된 설정은 작업을 실패시키므로(종료 코드 2),
  잘못된 설정이 몰래 건너뛰어지는 일은 없습니다.
- `ARTIFACTIZE_REMOTE=off`는 명령 하나에 대해 저장소를 끕니다. 그래도 읽을 수 없는
  `remote.json`이 있으면 명령이 실패하니, 먼저 고치거나 지우세요.

[팀 워크스루](walkthrough.md#5-ci-reuses-both)에서 이를 처음부터 끝까지 실행해 봅니다.

## 모델 없이 Agent eval 테스트하기 {#test-agent-evals-without-a-model}

CI 작업은 루프백에서 시작한 가짜 provider를 상대로, 버려도 되는 상태 디렉터리를 쓰고 리뷰 저장소 연동을 끈 채로 Agent eval을 실행할 수 있습니다.
[가짜 provider로 테스트하기](../agent.md#test-against-a-fake-provider)를 보세요.
