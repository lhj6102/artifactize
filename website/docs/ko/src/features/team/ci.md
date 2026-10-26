# CI {#ci}

**지속적 통합(CI)** 은 팀이 코드를 푸시하거나 풀 리퀘스트를 열 때 자동으로 검사를 실행합니다.
CI 작업은 러너 머신에서 여러 명령을 차례로 실행하며,
개발자의 Agent/Human 승인 결과를 재사용할 뿐 그 결정을 직접 내려서는 안 됩니다.

CI 단계로 `artifactize verify --all --wait --reuse-only agent,human`을 실행하세요. 테스트는 실행하되 리뷰 결과는 캐시나 팀 저장소에서 재사용하므로, CI에서는 모델을 호출하지 않고 사람에게 승인을 요청하지도 않습니다.
선택한 모든 eval이 GREEN이면 종료 코드는 0이며, RED(1), ERROR(2), INCOMPLETE(4)이면
CI 작업은 실패로 처리됩니다. 모든 종료 코드는 [명령 레퍼런스](../../cli/index.md#command-reference)에 있습니다. Agent 리뷰나
Human 승인에 재사용할 결과가 없으면 그 eval은 [실행되지 않고](../../cli/index.md#reuse-only) Run이 INCOMPLETE로 남으므로,
개발자가 `verify`로 현재 재사용 키에 맞는 결과를 만든 뒤에야 CI가 통과합니다. 기본 artifactsum을 쓰면 결과를 재사용하며, 대상이나 의존성에 `fingerprint = false`를 지정하면 재사용하지 않습니다.

## 대기 중인 승인을 병합 게이트에서 빼기 {#keep-pending-sign-offs-out-of-the-merge-gate}

승인은 자동 검사를 막지 않으므로, 개발자의 `verify`는 사람이 자리를 비운 동안에도 자동 검사를 끝냅니다.
YELLOW(종료 코드 5), DEFERRED 승인, PROVISIONAL 자동 검사 결과는 모두 승인이 끝났다는 뜻이 아닙니다. CI의
`--reuse-only agent,human` 게이트를 유지하세요. 필요한 승인이 없으면 승인을 요청하지 않고 Run이
INCOMPLETE로 남으며, 통과로 간주되지 않습니다.
사람이 GREEN을 제출하면 CI는 바뀌지 않은 자동 검사 결과와 승인을 재사용합니다. RED는 의존하는 eval을 막습니다. verify가 접수됐다는 것만으로 CI가 성공했다고 보지 마세요.

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
표현식이 실행 시점에 시크릿에 저장한 토큰 값을 전달하므로, 토큰을 이 파일에 직접 붙여 넣지 마세요.
다른 CI 서비스에서도 시크릿 저장 기능이나 환경 변수 설정을 쓰세요.

```yaml
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

- 읽기 토큰으로는 결과를 재사용할 수 있지만 게시할 수는 없습니다.
- 리뷰 저장소에 연결할 수 없어도 검증을 멈추지 않습니다. `verify`는 한 번 경고하고 로컬 캐시를 사용해 검증을 계속합니다.
  그러면 저장소에만 있는 리뷰는 재사용되지 않으므로 Run은 INCOMPLETE가 됩니다.
- 토큰이 거부되거나, TLS 연결에 실패하거나, 설정이 잘못되면 작업이 실패하므로(종료 코드 2),
  잘못된 설정이 오류 없이 무시되는 일은 없습니다.
- `ARTIFACTIZE_REMOTE=off`는 명령 하나에 대해 저장소를 끕니다. 그래도 읽을 수 없는
  `remote.json`이 있으면 명령이 실패하니, 먼저 고치거나 지우세요.

[팀 워크스루](walkthrough.md#5-ci-reuses-both)에서 이를 처음부터 끝까지 실행해 봅니다.

## 모델 없이 Agent eval 테스트하기 {#test-agent-evals-without-a-model}

CI 작업에서는 임시 상태 디렉터리를 쓰고 리뷰 저장소 연동을 끈 채, 루프백 주소로 띄운 가짜 provider에 연결해 Agent eval을 실행할 수 있습니다.
[가짜 provider로 테스트하기](../agent.md#test-against-a-fake-provider)를 보세요.
