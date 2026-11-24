# fingerprint와 재사용 {#fingerprints-and-reuse}

`artifactize verify`는 eval의 재사용 키가 저장된 `GREEN` 또는 `RED` 판정의 키와 일치하면 eval을 다시 실행하지 않고 그 판정을 재사용할 수 있습니다. 입력이 그대로인 것만으로는 부족합니다. 예를 들어 Live Artifact를 확인하는 runtime eval이나 fingerprint가 꺼진 eval에는 재사용 키가 없습니다.

## fingerprint (`artifactsum`, `script`, `false`) {#fingerprints-artifactsum-script-false}

Artifact를 선언할 때 상태를 식별하는 데 쓸 fingerprint를 지정합니다.

- **기본값 (로컬 대상의 `artifactsum`)**: `folder`, `file`, `aspect` Artifact에서 `fingerprint`를 생략하면(또는 폴더에서 `fingerprint = {}`로 적으면) 해당 Artifact에 포함된 파일의 SHA-256 해시를 계산합니다.
  - **폴더(Folder)**를 대상으로 지정하면 Artifactize는 `path` 아래에서 제외 규칙에 해당하지 않는 파일을 해시합니다. 기본 제외 목록(`.git`, `target/`, `node_modules/`, `__pycache__/`, `*.pyc`), `.gitignore` 규칙에 맞는 경로, 그리고 선언된 모든 `parts`(폴더, 파일, Aspect 모두 포함)의 대상 경로는 자동으로 제외합니다. `fingerprint = { files = ["src"], ignore = ["*.log"] }`로 대상 경로를 좁히거나 제외 패턴을 추가할 수 있습니다. 기본 제외 목록, `.gitignore`, `ignore`는 디렉터리를 탐색할 때 적용됩니다. `files`에 직접 지정한 파일은 기본적으로 해시 대상에 포함합니다. 다만 파일이나 Aspect로 선언한 하위 Artifact의 파일은 해시 대상에서 뺍니다. 폴더로 선언한 하위 Artifact 안의 경로는 `files`에 직접 지정할 수 없습니다.
  - **파일(File)** Artifact에서는 대상 파일 하나의 해시만 계산합니다.
  - **Aspect**는 `select`로 고른 경로들만(하위 `parts` 제외) 해시합니다.
  - 대상이 `.artf` **선언 파일**이면 `[annotations]`, `#` 주석, 서식 변경을 제외하고 정규화한 선언 내용의 해시를 계산합니다.
- **사용자 지정 스크립트**:
  ```toml
  fingerprint = { script = { command = "python3", args = ["hash.py"], files = ["input.txt"], timeout_ms = 10000 } }
  ```
  명령은 Artifact의 실행 기준 폴더에서 실행되며 표준 입력(`stdin`)으로 `{"version": 1, "artifactId": "<name>"}` JSON을 받습니다. 스크립트는 종료 코드 `0`으로 끝나야 하며 표준 출력(`stdout`)에 일관된 한 줄짜리 fingerprint 토큰 하나만 출력해야 합니다. `verify`와 `status` 모두 이 스크립트를 실행할 수 있습니다. `files`에는 스크립트를 실행하기 전에 필요한 파일을 적으며, 경로는 명령을 실행하는 폴더를 기준으로 지정합니다. 이 필드는 생략할 수 있으며, 여기에 적은 파일은 자동으로 해시하지 않습니다. **Live** Artifact에서 `fingerprint`를 생략하면 스냅샷이 없어 재사용 키를 만들지 않습니다. Live 대상의 현재 상태(예: 배포된 커밋 SHA)를 스냅샷으로 출력하는 스크립트를 선언할 수 있습니다. 스냅샷이 바뀌지 않으면 Agent나 Human 리뷰의 이전 결과를 재사용합니다. `runtime` eval의 대상이나 하위 `parts`, 직접 참조한 `{name}` 입력 가운데 Live Artifact가 있으면 재사용 키를 만들지 않습니다. 따라서 스냅샷 스크립트가 있어도 기존 판정을 재사용할 수 없습니다. 실행 대상으로 고른 eval은 선행 조건을 충족하고 `--reuse-only`의 적용 대상이 아닐 때만 새로 실행됩니다.
- **재사용 끄기**: `fingerprint = false`를 선언하면 해당 Artifact와 이를 참조하는 모든 eval에서 캐시 저장 및 재사용을 끕니다. 이 eval들은 재사용 키가 없어 결과를 재사용할 수 없습니다. 선택한 eval은 선행 조건을 충족하고 `--reuse-only` 적용 대상이 아닐 때만 새로 실행됩니다.

## 재사용 키 (Reuse key) {#the-reuse-key}

각 eval의 **재사용 키**는 다음 네 가지를 조합한 64자리 16진수 해시입니다.

1. **eval 정의 해시**: eval의 `kind`(`runtime`, `agent`, `human`), `payload`(`instruction` 포함), `red_at`(`agent` eval), `pass_schema`, `fail_schema`, 그리고 `runtime` eval의 `command`와 `args`.
2. **대상 Artifact**: 이름, 종류(`folder`, `file`, `aspect`, `live`), 현재 fingerprint.
3. **대상 Artifact의 모든 하위 `parts`**(재귀 포함): 각 하위 Artifact의 이름, 종류, fingerprint. 하위 Artifact가 없던 대상에 하나를 추가하거나 하나만 남은 하위 Artifact를 제거하면 재사용 키가 바뀝니다. 그 하위 Artifact를 이미 입력으로 직접 참조하고 있었어도 마찬가지입니다. 따라서 하위 Artifact가 바뀌면 해당 하위 Artifact와 상위 Artifact의 eval을 다시 실행합니다. 상위 Artifact에 속한 파일만 바뀌었다면, 구성 관계만으로 하위 Artifact의 eval까지 다시 실행하지는 않습니다. 다만 하위 Artifact의 eval이 `{whole}`을 직접 참조하면 다시 실행합니다.
4. **직접 참조한 `{name}` 의존성**: 재사용 키에는 eval의 지시문이나 Runtime `args`에서 직접 참조한 입력 Artifact의 이름, 종류, fingerprint만 넣습니다. 직접 참조한 입력 Artifact를 거쳐 간접적으로 연결된 Artifact는 재사용 키에 넣지 않고 Run 통과 요건을 확인할 때만 살핍니다.

### eval 정의 해시를 바꾸지 않는 항목 {#what-does-not-change-the-eval-definition-hash}

실행 옵션과 표시용 메타데이터는 eval 정의 해시에서 제외되므로, 평가 기준을 바꾸지 않고 실행 설정을 조정할 수 있습니다.

- Agent의 `backend`, `model`, `reasoning`, `timeout_ms`, `max_tokens`, `max_tool_calls`, `--profile`로 선택한 대체 실행 프로필(단, `runtime` 대체 실행 프로필에서 `command`나 `args`가 달라지면 별도 키 사용).
- eval의 `title`, `mutex` 태그, Artifact의 `tags`, 실제 디스크 경로, 도구 선언(`views`). (단, `.artifactize/`를 검증 대상으로 삼는 등 `.artf` 파일을 해시하는 Artifact에서는 해당 파일의 필드를 수정하면 fingerprint가 바뀝니다.)

재사용 키에는 실제 디스크 경로를 넣지 않습니다. 따라서 Artifact 이름과 종류, fingerprint, 평가 기준만 같으면 다른 워크트리나 복제본, 팀원의 개발 환경([팀 리뷰 저장소](../guides/team-and-ci.md) 연동 시), CI에서도 결과를 그대로 재사용합니다.

## 완료된 결과 재사용과 `--force` {#completed-result-reuse}

- **가장 최근에 완료된 기록 선택 및 실행 중인 작업의 결과 공유**: eval을 실행하는 동안 입력이 바뀌지 않았다면 `GREEN` 또는 `RED` 판정을 해당 재사용 키로 저장합니다. 오류가 나거나 취소된 eval의 결과는 저장하지 않습니다. 미리 실행한 eval의 결과는 선행 조건을 확인하기 전에 로컬 캐시에 먼저 저장될 수 있습니다. 나중에 선행 eval이 실패하면 미리 실행한 eval의 결과를 무효로 처리해 다시 쓰지 않습니다. 전체 Run을 나중에 취소하더라도 이미 저장한 정상 결과는 그대로 남습니다. 로컬 캐시나 팀 리뷰 저장소에 같은 키의 기록이 여러 개 있으면 완료 시각이 가장 최근인 기록을 재사용합니다. 완료된 결과가 아직 없는 경우도 있습니다. 이때 같은 상태 디렉터리에서 재사용 키와 실행 프로필이 같은 `runtime` 또는 `agent` 작업이 진행 중일 수 있습니다. `--force`를 쓰지 않은 Run에서는 Artifactize가 새 작업을 시작하는 대신 이미 진행 중인 작업의 결과를 해당 Run에 반영합니다.
- **변경 내역 확인**: 재사용 키가 있는 eval에 재사용할 결과가 없으면 `artifactize status`는 로컬 캐시를 확인합니다. 이때 전체 eval ID와 평가 기준이 현재 eval과 일치하는 기록 중 가장 최근 것을 고릅니다. 이전 기록이 있으면 저장된 fingerprint를 현재 값과 비교해 어떤 입력 Artifact가 바뀌었는지, 대상 파일에 변경이 있는지 알려 줍니다. `status`는 `artifactsum` 기록에 파일 목록이 있으면 바뀐 개별 파일을 나열하고, 스크립트를 썼다면 fingerprint가 바뀌었다고만 표시합니다.
- **강제 재실행 (`--force`)**: `artifactize verify SELECTOR --force`를 실행하면 직접 선택한 항목 가운데 실행 가능한 eval은 캐시 결과나 이미 실행 중인 작업의 결과를 쓰지 않고 다시 실행합니다. 이 규칙은 선택한 Artifact의 하위 `parts`에도 적용되지만 선행 의존성에는 적용되지 않습니다. 또한 해당 Run 전체에서 팀 리뷰 저장소 조회를 생략하고, 새로 완료된 `GREEN` 또는 `RED` 결과를 로컬 캐시와 팀 리뷰 저장소에 최신 기록으로 저장합니다.
