# 팀 리뷰 저장소와 CI {#team-review-store-and-ci}

팀원들의 개발 환경과 CI는 자체 호스팅 리뷰 저장소(`artifactize server`) 하나를 통해 검증 판정을 공유하고 재사용합니다. 각 컴퓨터에서 로컬 상태와 캐시를 따로 관리하며, 저장소는 완료된 기록을 [재사용 키](../concepts/reuse.md#the-reuse-key)별로 보관합니다.

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
- **HTTPS 또는 SSH 터널**: `server run`은 기본으로 로컬 루프백(`127.0.0.1:8417`)에서 일반 HTTP로 동작하며, 클라이언트는 루프백이 아닌 주소에 접속할 때 `https://`를 요구합니다. 서버 앞에 TLS 리버스 프록시를 두거나 SSH 포트 포워딩(`ssh -N -L 127.0.0.1:18417:127.0.0.1:8417 user@reviews-host` 실행 후 `http://127.0.0.1:18417/`로 로그인)으로 연결한 뒤 `artifactize remote status`로 연결 상태를 확인하세요.

## 2. 개발 머신에서 로그인 및 공유 (`artifactize remote`) {#remote-review-store-client}

연결 정보는 `.artf` 선언 파일에 넣지 않고 각 컴퓨터에서 따로 설정하며, 데스크톱 앱에서는 **Settings → Remote**를 사용합니다.

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/
artifactize remote status
artifactize remote push --dry-run
artifactize remote push
artifactize remote logout
```

기본 `summary` 수준 대신 전체 실행 기록까지 공유하려면 `remote login`에 `--share full`을 넘기세요.

- **자동 조회와 업로드**: 해당 권한 범위가 있는 토큰으로 로그인해 두면 `verify`는 재사용 키가 있는 eval에 대해 저장소를 조회하고 새 판정을 업로드합니다. 로컬과 원격 기록 중 어느 쪽을 쓸지, `--force`를 지정하면 원격 조회가 어떻게 달라지는지는 [완료된 결과 재사용](../concepts/reuse.md#completed-result-reuse) 절을 참고하세요. 토큰에 `human`과 `publish`가 있으면 `request submit`이 Human 승인을 업로드합니다. `--force` 없이 시작한 Run이 Human 승인을 기다리는 동안 다른 머신에서 일치하는 승인이 올라오면 대기 중인 요청이 자동으로 완료됩니다. CLI를 계속 켜 두고 진행 상황을 보고 있지 않아도 됩니다.
- **공유 수준 (`summary`와 `full`)**: 기본값인 `summary`에서는 팀원들이 판정, 결과 필드, 재사용 키, fingerprint, 선언된 프로필(Runtime `command`와 `args` 포함), 선언된 도구·세션 `execution_paths` 해시, 토큰 사용량, 작성자·리뷰어 이름, Agent 세션 참조를 볼 수 있습니다. 반면 로컬 저장소 루트 경로, 캡처된 `stdout`/`stderr`, Agent 대화 전문은 공유하지 않으며, `full`(`--share full`)을 선택하면 로컬 저장소 루트 경로와 캡처된 Runtime 출력도 함께 공유합니다(정확한 JSON 기록 필드는 [CLI 레퍼런스의 `remote`](../reference/cli-and-mcp.md#command-reference) 참고). `summary`에서도 선언된 `execution_paths`와 명령, `args`, 결과 필드에 직접 적은 문자열은 함께 공유되므로 선언 인자나 결과 필드에는 비밀 정보를 넣지 마세요.
- **오프라인 동작과 `remote push`**: 저장소에 연결할 수 없거나 HTTP 5xx 오류가 나면 `verify`는 경고를 한 줄 출력한 뒤 원격 저장소 없이 로컬 상태만 사용해 검증을 계속합니다. 인증 오류(401/403), TLS 오류, 프로토콜 오류가 나면 연결을 고치거나 `artifactize remote login`을 다시 실행할 수 있도록 Run이 실패로 끝납니다(`verify --wait` 또는 `run show RUN_ID --wait`에서 종료 코드 `2`인 `ERROR` 확인; 평가 기준이나 fingerprint가 로컬 프로젝트와 맞지 않는 원격 기록은 무시하고 유효한 로컬 캐시 기록을 쓰거나 `--reuse-only`가 허용할 때 새로 실행합니다). `artifactize remote push --dry-run`으로 업로드 대상을 미리 확인하고, `artifactize remote push`로 상태 디렉터리 전체에서 재사용 키별로 가장 최근에 생성된 로컬 `GREEN` 또는 `RED` 판정을 업로드하세요.
- **환경 변수 재정의**: `ARTIFACTIZE_REMOTE`(URL 또는 `off`), `ARTIFACTIZE_REMOTE_TOKEN`, `ARTIFACTIZE_REMOTE_SHARE`(`summary` 또는 `full`)는 로컬 설정보다 우선합니다.

## 3. CI 설정 (`--reuse-only agent,human`) {#ci}

CI는 테스트나 린터 같은 결정론적 Runtime 검사만 직접 실행하고, 비용이 들거나 사람의 판단이 필요한 Agent 리뷰와 Human 승인에는 개발자가 올린 판정을 재사용해야 합니다.

CI에는 **`read`** 권한만 있는 토큰을 발급하고 `artifactize verify --all --wait --reuse-only agent,human`을 실행하세요.

```yaml
# .github/workflows/ci.yml의 jobs.<job>.steps 항목 예시:
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

CI 병합 게이트 동작 방식:

- **Runtime eval**: 선행 조건이 충족되면 Runtime eval을 실행하되, 현재 입력과 평가 기준에 맞는 캐시 결과가 있으면 그 결과를 재사용합니다.
- **Agent 및 Human eval**은 직접 실행하지 않습니다(`--reuse-only agent,human`). 현재 커밋의 fingerprint 및 평가 기준과 일치하는 최신 기록이 `GREEN`이면 그대로 재사용하고, 일치하는 최신 기록이 `RED`이면 그 `RED` 판정을 재사용해 하위 의존 검사를 막습니다.
- **재사용할 리뷰 결과가 없을 때**: `--reuse-only` 대상인 Agent나 Human eval에 재사용할 수 있는 캐시 기록이 없으면 eval을 실행하지 않고 `STALE`로 표시합니다. 예를 들어 사람의 승인이 남아 개발자의 Run이 `YELLOW`로 끝난 경우나 리뷰 후 파일이 바뀐 경우가 여기에 해당합니다. 그 eval에 의존하는 하위 검사는 선행 조건이 충족될 때까지 실행하지 않습니다. 이 경우 다른 eval이 `RED`를 반환했더라도 Run은 **`INCOMPLETE`**(종료 코드 `4`)로 실패합니다. 반대로 선행 eval에서 `RED`가 나와 이미 실행이 막힌 `--reuse-only` eval은 `BLOCKED`로 남으며 Run은 종료 코드 `1`로 끝날 수 있습니다.
