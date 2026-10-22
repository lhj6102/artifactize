# 팀으로 일하기 {#working-as-a-team}

여러 머신과 CI는 공유 리뷰 저장소 하나,
`artifactize server`([설계](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md))를 통해 서로의 판정을 재사용할 수 있습니다. 각 머신은
각자 상태를 유지합니다. 저장소는 각 재사용 키의 모든 기록을 보관하되 가장 최근 기록을
돌려주므로, 나중에 한 리뷰(예: 강제 재리뷰)가 있으면 모든 팀원이 이전 판정 대신 그 판정을
재사용합니다. [팀 워크스루](walkthrough.md)에서 두 머신과 CI가 판정을 공유하는 전체 과정을 처음부터 끝까지 실습합니다.

**서버.** 소규모 서버에 별도의 상태 디렉터리를 두고 리뷰 저장소를 실행하세요.

```sh
artifactize --state-dir /srv/artifactize server token add alice-laptop --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add bob-laptop --scopes read,publish
artifactize --state-dir /srv/artifactize server token add ci --scopes read
artifactize --state-dir /srv/artifactize server run     # http://127.0.0.1:8417/
```

`token add`는 각 토큰을 한 번만 출력하니 비공개로 보관하세요. `server run`은
**루프백**(`127.0.0.1`)에서 대기하므로, 같은 컴퓨터의 프로그램만 직접 연결할 수 있습니다.
암호화되지 않은 네트워크로 토큰을 보내지 않고 팀원이 연결하게 하려면
[아래의 안전한 연결 설정](#connect-teammates-securely)을 쓰세요.
저장소는 SQLite 명령줄 도구로 백업하세요. 아래 명령은 위 상태 디렉터리의
데이터베이스를 읽어 별도의 백업 파일에 쓰는 전체 예입니다.

```sh
sqlite3 -readonly /srv/artifactize/review-store.sqlite ".backup '/srv/backups/artifactize-reviews.sqlite'"
```

두 경로를 호스트에 맞게 바꾸세요. 원본은 서버의 실제
`review-store.sqlite`여야 합니다. 프로젝트 체크아웃 밖에 있는, 이미 존재하고 쓸 수 있는 백업 디렉터리를 고르세요
(이 예에서는 `/srv/backups`). `-readonly`를 쓰면 원본 경로를 잘못 입력해도
그 위치에 빈 데이터베이스가 새로 생기지 않습니다.

서버와 클라이언트는 함께 업그레이드하세요. 0.9의 재사용 키 v2는 더 오래된 기록과 일치하지 않으므로, Human 승인을 포함한
모든 실행형 eval에 새 리뷰가 한 번씩 필요합니다. 전체 레퍼런스는 [리뷰 저장소 서버](../../cli/review-store.md#review-store-server)를
보세요.

## 팀원을 안전하게 연결하기 {#connect-teammates-securely}

**TLS 리버스 프록시**는 저장소 앞에서 암호화된
HTTPS 연결을 받아 로컬로 전달하는 서버입니다. **터널**은 대신 연결을 다른
암호화된 연결 안에 실어 나릅니다. 다음은 저장소 호스트에 이미 SSH 계정이 있는 팀원을 위한
완전한 SSH 터널 설정입니다.

1. 위처럼 그 호스트에서 `artifactize server run`을 계속 실행해 두세요.
2. 팀원의 컴퓨터에서 터미널을 열고 `you@reviews-host`를
   그 사람의 SSH 계정과 호스트 이름으로 바꾸세요.

   ```sh
   ssh -N -L 127.0.0.1:18417:127.0.0.1:8417 you@reviews-host
   ```

   이 터미널은 열어 두세요. `-N`은 원격 명령을 시작하지 않고, `-L`은 로컬 포트
   18417을 SSH를 통해 저장소의 포트 8417로 전달합니다. 로컬 쪽도
   이 컴퓨터로 제한되며, 컴퓨터 사이의 연결은 SSH가 암호화합니다.
3. 팀원 컴퓨터의 다른 터미널에서 그 터널을 통해 로그인하세요.

   ```sh
   artifactize remote login http://127.0.0.1:18417/   # enter this machine's token
   artifactize remote status
   ```

주소가 로컬이므로 여기서는 일반 HTTP가 허용됩니다. 네트워크 트래픽은
암호화된 SSH 터널을 통해 전송됩니다. 저장소의 일반 HTTP 포트를 네트워크에 직접 노출하지 마세요.
터널이 없으면 클라이언트는 TLS 리버스 프록시가 제공하는 HTTPS 주소를 요구합니다.
SSH의 표준 호스트 확인과 인증 절차를 따르고, 이를 우회하지 마세요.

## 로그인하고 결과 공유하기 {#sign-in-and-share-results}

각 머신은 한 번 로그인합니다. 토큰은 명령 인자가 아니라
stdin에서 읽습니다. 터미널이 지원하면 대화형 입력은 가려집니다.
Windows에서 입력을 가리는 데 실패하면 CLI가 경고한 뒤 토큰을 보이는 채로 읽고,
Linux와 macOS에서는 그 실패로 로그인이 중단됩니다. 파이프로 들어온 입력은
터미널 모드를 바꾸지 않고 읽습니다.

```sh
artifactize remote login https://reviews.example/     # paste the token
artifactize remote status
```

그때부터:

- `verify`는 저장소의 결과를 재사용하고 새로 만든 GREEN/RED 결과를 게시합니다.
- `status`는 원격 재사용을 예측합니다.
- `request submit`은 Human 승인을 게시합니다.
- `remote push`는 저장소에 연결할 수 없던 동안 만든 결과를 보냅니다.

전체 레퍼런스는 [원격 리뷰 저장소 클라이언트](../../cli/review-store.md#remote-review-store-client)를 보세요.

**CI.** CI는 환경 변수로만 설정합니다. [CI](ci.md)를 보세요.

**공유 수준.** 기본값인 `summary`는 다음을 보냅니다.

- 판정
- 스키마로 검증한 소유자 필드(runtime eval에서는 종료 코드,
  소요 시간, 잘림 표시만)
- 재사용 키, eval 정의 해시, 그리고 키가 포함하는 각 Artifact의
  fingerprint와 그 `folder`/`file`/`graph` kind
  (기록의 `artifactKinds`)
- 프로필, 그 실행 옵션(백엔드, 모델, reasoning, 제한, 변형),
  사용량 카운터, Agent 결과의 작업 공간 기준 상대 `executionPaths` 고정 값
- 만든 사람(`user@host`), Human 리뷰어, 타임스탬프
- Agent 결과라면 저장된 대화에 대한 [참조](../agent.md#saved-conversations)
  (`producer.session`: 만든 사람, 그 상태 ID, Run,
  요청, 세션 ID)

`summary`는 캡처한 stdout/stderr, 별도의 호출 감사 기록, 로컬
저장소 출처, Agent 대화 자체를 빼고 보냅니다. 그래도 명령과 인자를 포함한
선언된 runtime 프로필 전체는 보냅니다. 그곳에 작성한 경로나
민감한 리터럴은 가려지지 않고 공유됩니다.
`executionPaths` 고정 값도 작업 공간 기준 상대 경로를 담습니다.
`full`(`remote login --share full` 또는 `ARTIFACTIZE_REMOTE_SHARE=full`)은 추가로
캡처한 출력을 포함해 저장된 실행을 그대로 보냅니다. 두 수준 모두 소유자 필드는 자유
텍스트이므로, 공유되는 선언과 `pass_schema`, `fail_schema`가 허용하는 필드에는
비밀을 넣지 마세요.

**신뢰.**

- 서버는 인증된 `publisher`, 즉 토큰 이름을 찍습니다. `producer`와
  Human `reviewer`는 게시하는 머신이 보고한 값입니다.
- `publish` 토큰이 있으면 어떤 키에 대해서든 어떤 판정이든 주장할 수 있습니다. 신뢰할 수 없는 CI(포크
  풀 리퀘스트)에는 `read`만 주고, `human`은 승인하는 사람에게만 주세요. 토큰이
  유출되거나 오용되면 `server token revoke NAME --purge`로 그 토큰이 게시한
  항목도 삭제합니다.
- `verify --force`는 저장소에서 아무것도 읽지 않으므로 의심스러운 기록을 우회하며,
  새로 리뷰해 게시한 결과가 저장소의 최신 기록이 됩니다.
  `cache rm`은 로컬 키의 기록을 지우고, `server rm`은 저장소에서 키의 기록을
  제거합니다.
- 저장소는 `.artf` 선언이 아니라 `$STATE/remote.json`과 환경 변수로만 설정되므로,
  복제한 저장소가 토큰을 다른 곳으로 보낼 수 없습니다.
  토큰은 내 계정만 읽고 쓸 수 있는 파일에 저장되고, 발급받은 저장소에서만 쓰이며, 출력되지 않습니다.
- 저장소의 오류 메시지에 토큰의 일부가 들어 있으면 그 메시지는 `[withheld: it contained part of the credential sent]`로
  바뀝니다.
