# 팀 리뷰 저장소와 CI {#team-review-store-and-ci}

팀에서 직접 운영하는 리뷰 저장소(`artifactize server`)를 쓰면 각 팀원의 개발 환경과 CI에서 검증 결과를 공유하고 재사용할 수 있습니다. 각 컴퓨터에서 로컬 상태와 캐시를 따로 관리하며, 저장소는 완료된 기록을 [재사용 키](../concepts/reuse.md#the-reuse-key)별로 보관합니다.

## 1. 리뷰 저장소 서버 실행 (`artifactize server`) {#review-store-server}

서버 호스트에서 전용 상태 디렉터리를 지정해 팀원과 CI용 토큰을 발급하고 서버를 실행합니다.

```sh
artifactize --state-dir /srv/artifactize server token add alice --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add bob   --scopes read,publish
artifactize --state-dir /srv/artifactize server token add ci    --scopes read
artifactize --state-dir /srv/artifactize server run --listen 127.0.0.1:8417
```

- **토큰 권한 범위 (`--scopes`)**: 필수 옵션이며 `read`(기록 조회), `publish`(`runtime` 및 `agent` 판정 업로드), `human`(Human 승인 업로드, `publish`와 함께 지정)을 쉼표로 구분해 적습니다. `token add`는 발급된 비밀 토큰을 한 번만 출력하므로 안전한 곳에 보관하세요.
- **토큰 및 기록 관리**:
  ```sh
  artifactize --state-dir /srv/artifactize server token list
  artifactize --state-dir /srv/artifactize server token revoke alice [--purge]  # --purge는 alice가 올린 기록도 함께 삭제
  artifactize --state-dir /srv/artifactize server rm REUSE_KEY                  # 특정 64자리 16진수 키의 기록 삭제
  ```
- **HTTPS 또는 SSH 터널**: `server run`은 기본으로 로컬 루프백(`127.0.0.1:8417`)에서 일반 HTTP로 동작합니다. 클라이언트에서 루프백이 아닌 주소로 접속하려면 `https://` 주소를 사용하세요. 서버 앞에 TLS 리버스 프록시를 두거나 SSH 포트 포워딩(`ssh -N -L 127.0.0.1:18417:127.0.0.1:8417 user@reviews-host` 실행 후 `http://127.0.0.1:18417/`로 로그인)으로 연결할 수 있습니다. 연결한 뒤에는 `artifactize remote status`로 상태를 확인하세요.

## 2. 개발 머신에서 로그인 및 공유 (`artifactize remote`) {#remote-review-store-client}

연결 정보는 `.artf` 선언 파일에 넣지 않고 각 컴퓨터에서 따로 설정하며, 데스크톱 앱에서는 **Settings → Remote**를 사용합니다.

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/
artifactize remote status
artifactize remote push --dry-run
artifactize remote push
artifactize remote logout
```

공유 범위를 기본값인 `summary`에서 전체 실행 기록으로 넓히려면 `remote login`에 `--share full`을 지정하세요.

- **자동 조회와 업로드**: 조회나 업로드에 필요한 권한이 있는 토큰으로 로그인해 두면 `verify`는 재사용 키가 있는 eval에 대해 저장소를 조회하고 새 판정을 업로드합니다. 로컬과 원격 기록 중 어느 쪽을 쓸지, `--force`를 지정하면 원격 조회가 어떻게 달라지는지는 [완료된 결과 재사용](../concepts/reuse.md#completed-result-reuse) 절을 참고하세요. 토큰에 `human`과 `publish`가 있으면 `request submit`이 Human 승인을 업로드합니다. 다른 컴퓨터에서 같은 재사용 키의 승인 결과를 올리면 `--force` 없이 시작한 Run에서 대기 중인 해당 요청이 자동으로 완료됩니다. 진행 상황을 지켜보려고 CLI를 켜 둘 필요는 없습니다.
- **공유 수준 (`summary`와 `full`)**: 기본값인 `summary`에서는 팀원들이 판정과 그 issue(`GREEN` 리뷰의 issue를 포함한 Agent issue의 우선순위와 사유, Human issue), 재사용 키, fingerprint, 선언된 프로필(Runtime `command`와 `args` 포함)을 볼 수 있습니다. 도구와 세션의 `execution_paths`에 지정한 경로의 해시, 토큰 사용량, 작성자·리뷰어 이름, Agent 세션 참조도 함께 공유합니다. 반면 로컬 저장소 루트 경로, 캡처된 `stdout`/`stderr`, Agent 대화 전문은 공유하지 않습니다. `full`(`--share full`)을 선택하면 로컬 저장소 루트 경로와 실행 중에 수집한 Runtime 출력도 함께 공유합니다(정확한 JSON 기록 필드는 [CLI 레퍼런스의 `remote`](../reference/cli-and-mcp.md#command-reference) 참고). `summary`에서도 선언된 `execution_paths`와 명령, `args`, issue에 담긴 문자열은 함께 공유되므로 선언 인자나 issue에는 비밀 정보를 넣지 마세요.
- **오프라인 동작과 `remote push`**: 저장소에 연결할 수 없거나 HTTP 5xx 오류가 나면 팀 리뷰 저장소 없이 로컬 상태만 사용해 검증을 계속합니다. 경고를 확인하려면 `verify --wait`를 사용하세요. 인증 오류(401/403), TLS 오류, 프로토콜 오류가 나면 Run은 종료 코드 `2`인 `ERROR`로 끝납니다(`verify --wait` 또는 `run show RUN_ID --wait`로 확인). 이 경우에는 연결 설정을 고치거나 `artifactize remote login`으로 다시 로그인하세요. 평가 기준이나 fingerprint가 로컬 프로젝트와 맞지 않는 원격 기록은 무시합니다. 이때 쓸 수 있는 로컬 캐시가 있으면 결과를 재사용합니다. 로컬 캐시가 없으면 `--reuse-only` 적용 대상이 아닌 eval만 새로 실행합니다. `artifactize remote push --dry-run`으로 업로드 대상을 미리 확인하고, `artifactize remote push`로 상태 디렉터리 전체에서 재사용 키별로 가장 최근에 생성된 로컬 `GREEN` 또는 `RED` 판정을 업로드하세요.
- **환경 변수 재정의**: `ARTIFACTIZE_REMOTE`(URL 또는 `off`), `ARTIFACTIZE_REMOTE_TOKEN`, `ARTIFACTIZE_REMOTE_SHARE`(`summary` 또는 `full`)는 로컬 설정보다 우선합니다.

## 3. CI 설정 (`--reuse-only agent,human`) {#ci}

CI는 테스트나 린터 같은 결정론적 Runtime 평가만 직접 실행하고, 비용이 들거나 사람의 판단이 필요한 Agent 리뷰와 Human 승인에는 개발자가 올린 판정을 재사용해야 합니다.

CI에는 **`read`** 권한만 있는 토큰을 발급하고 `artifactize verify --all --wait --reuse-only agent,human`을 실행하세요.

```yaml
# .github/workflows/ci.yml의 jobs.<job>.steps 항목 예시:
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

`--reuse-only agent,human`을 지정하면 CI는 `runtime` 평가를 직접 실행하거나 캐시 결과를 씁니다. `agent`와 `human` 평가는 직접 실행하지 않고 팀 리뷰 저장소나 로컬 캐시에서 일치하는 최신 판정을 재사용합니다([fingerprint와 재사용](../concepts/reuse.md#completed-result-reuse)). `--reuse-only` 대상 eval이 재사용 여부를 확인할 때 일치하는 판정이 없으면 `STALE`로 남습니다. 예를 들어 승인을 기다리다 Run이 `YELLOW`로 끝났거나 리뷰 뒤에 파일이 바뀐 경우입니다. 이 eval과 여기에 의존하는 하위 eval은 실행되지 않습니다. 더 우선하는 결과가 없으면 Run은 **`INCOMPLETE`**로 끝납니다(종료 코드 `4`; Run 결과 우선순위는 [승인과 Run의 종료](../concepts/runs.md#sign-offs-and-the-end-of-a-run) 참고).
