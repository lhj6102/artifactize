# fingerprint와 재사용 {#fingerprints-and-reuse}

`artifactize verify`를 실행할 때, 평가 기준과 입력 파일들이 이전에 저장된 `GREEN` 또는 `RED` 판정 때와 같다면 eval을 다시 실행하지 않고 이전 결과를 재사용합니다.

## fingerprint (`artifactsum`, `script`, `false`) {#fingerprints-artifactsum-script-false}

Artifact를 선언할 때 상태를 식별하는 데 쓸 fingerprint를 지정합니다.

- **기본값 (로컬 대상의 `artifactsum`)**: `folder`, `file`, `aspect` Artifact에서 `fingerprint`를 생략하면(또는 폴더에서 `fingerprint = {}`로 적으면) 자체 파일들을 SHA-256으로 해시합니다.
  - **폴더(Folder)**는 `path` 아래의 모든 파일을 해시하며 기본 제외 목록(`.git`, `target/`, `node_modules/`, `__pycache__/`, `*.pyc`), `.gitignore` 규칙에 맞는 경로, 그리고 선언된 모든 `parts`(폴더, 파일, Aspect 모두 포함)의 대상 경로를 자동으로 제외합니다. `fingerprint = { files = ["src"], ignore = ["*.log"] }`로 대상 경로를 좁히거나 제외 패턴을 추가할 수 있습니다. 기본 제외 목록, `.gitignore`, `ignore`는 디렉터리를 탐색할 때 적용됩니다. `files`에 직접 지정한 파일 경로는 파일·Aspect `parts`에 속하지 않는 한 해시되며, 폴더 `part` 안의 경로를 `files`에 직접 적으면 거부됩니다.
  - **파일(File)** Artifact는 대상 파일 하나만 해시합니다.
  - **Aspect**는 `select`로 고른 경로들만(하위 `parts` 제외) 해시합니다.
  - **정의 파일(`.artf`)** 대상은 `[annotations]`, `#` 주석, 서식 변경을 제외하고 정규화한 선언 내용의 해시를 계산합니다.
- **사용자 지정 스크립트**:
  ```toml
  fingerprint = { script = { command = "python3", args = ["hash.py"], files = ["input.txt"], timeout_ms = 10000 } }
  ```
  명령은 Artifact의 실행 기준 폴더에서 실행되며 표준 입력(`stdin`)으로 `{"version": 1, "artifactId": "<name>"}` JSON을 받습니다. 스크립트는 종료 코드 `0`으로 끝나야 하며 표준 출력(`stdout`)에 ASCII 영문자, 숫자, `.`, `_`, `:`, `-`로 이루어진 한 줄짜리 fingerprint 토큰 하나만 출력해야 합니다. `verify`와 `status` 모두 이 스크립트를 실행할 수 있습니다. `files`는 생략할 수 있으며, 스크립트 실행 전에 있어야 하는 파일의 경로를 실행 기준 폴더에 대한 상대 경로로 적습니다(이 파일들을 자동으로 해시하지는 않음). **Live** Artifact에서 `fingerprint`를 생략하면 스냅샷이 없어 재사용 키를 만들지 않습니다. 관찰한 상태(예: 배포된 커밋 SHA)가 유지되는 동안 Agent나 Human 리뷰를 재사용하려면 스냅샷을 출력하는 스크립트를 선언하세요. `runtime` eval의 대상이나 하위 `parts`, 직접 참조한 `{name}` 입력 가운데 Live Artifact가 있으면 재사용 키를 만들지 않습니다. 따라서 스냅샷 스크립트가 있어도 기존 판정을 재사용할 수 없으며, 실행 대상으로 고른 eval은 선행 조건을 충족하고 `--reuse-only`의 적용 대상이 아닐 때만 새로 실행됩니다.
- **재사용 끄기**: `fingerprint = false`를 선언하면 해당 Artifact와 이를 참조하는 모든 eval에서 캐시 저장 및 재사용을 끕니다. 이 eval들은 재사용 키가 없어 결과를 재사용할 수 없습니다. 실행 대상으로 선택되고 의존성 게이트와 `--reuse-only` 조건을 충족할 때 새로 실행됩니다.

## 재사용 키 (Reuse key) {#the-reuse-key}

각 eval의 **재사용 키**는 다음 네 가지를 조합한 64자리 16진수 해시입니다.

1. **eval 정의 해시**: eval의 `kind`(`runtime`, `agent`, `human`), `payload`(`instruction` 포함), `pass_schema`, `fail_schema`, 그리고 `runtime` eval의 `command`와 `args`.
2. **대상 Artifact**: 이름, 종류(`folder`, `file`, `aspect`, `live`), 현재 fingerprint.
3. **대상 Artifact의 모든 하위 `parts`**(재귀 포함): 각 부분의 이름, 종류, fingerprint. 대상에 첫 하위 Artifact를 추가하거나 마지막 하위 Artifact를 제거하면 재사용 키가 바뀌며, 그 하위 Artifact를 이미 입력으로 직접 참조하고 있었어도 마찬가지입니다. 따라서 하위 부분이 바뀌면 그 부분과 상위 Artifact의 eval을 다시 실행합니다. 상위 Artifact의 자체 파일만 바뀌었을 때는 구성 관계만으로는 하위 부분을 다시 실행하지 않습니다(단, 하위 부분의 eval이 `{whole}`을 직접 참조하면 다시 실행됨).
4. **직접 참조한 `{name}` 의존성**: eval의 지시문이나 Runtime `args`에 적은 각 입력 Artifact의 이름, 종류, fingerprint(1단계만 포함하며, 입력이 참조하는 상위 의존성은 이 키에 들어가지 않고 Run 통과 요건으로만 확인합니다).

### eval 정의 해시를 바꾸지 않는 항목 {#what-does-not-change-the-eval-definition-hash}

실행 옵션과 표시용 메타데이터는 eval 정의 해시에서 제외되므로, 평가 기준을 바꾸지 않고 실행 설정을 조정할 수 있습니다.

- Agent의 `backend`, `model`, `reasoning`, `timeout_ms`, `max_tokens`, `max_tool_calls`, `--profile`로 선택한 대체 실행 프로필(단, `runtime` 대체 실행 프로필에서 `command`나 `args`가 달라지면 별도 키 사용).
- eval의 `title`, `mutex` 태그, Artifact의 `tags`, 실제 디스크 경로, 도구 선언(`views`). (단, `.artifactize/`를 대상으로 삼는 정의 리뷰 노드처럼 자체 `artifactsum`이 `.artf` 파일을 포함하는 Artifact에서는 해당 `.artf` 파일의 필드를 수정하면 파일 fingerprint가 바뀝니다.)

실제 디스크 경로가 키에 포함되지 않으므로, 다른 워크트리나 복제본, 팀원의 개발 환경([팀 리뷰 저장소](../guides/team-and-ci.md) 연동 시), CI에서도 Artifact 이름과 종류, fingerprint, 평가 기준만 같으면 결과를 그대로 재사용합니다.

## 완료된 결과 재사용과 `--force` {#completed-result-reuse}

- **가장 최근에 완료된 기록 선택 및 실행 중인 작업의 결과 공유**: eval을 실행하는 동안 입력이 바뀌지 않았다면 `GREEN`이나 `RED`로 끝난 결과를 해당 재사용 키로 저장합니다. `ERROR`, 취소된 eval, 또는 선행 조건이 나중에 실패해 철회된 실행 결과는 저장하지 않습니다. 전체 Run을 나중에 취소하더라도 이미 저장한 정상 결과는 그대로 남습니다. 로컬 캐시나 원격 저장소에 같은 키의 기록이 여러 개 있으면 완료 시각이 가장 최근인 기록을 재사용합니다. 완료된 결과가 아직 없는 경우도 있습니다. 이때 같은 상태 디렉터리에서 재사용 키와 실행 프로필이 같은 `runtime` 또는 `agent` 작업이 진행 중이면, `--force`를 쓰지 않은 Run은 새로 실행하지 않고 그 결과를 기다립니다.
- **변경 내역 확인**: 재사용 키가 있는 eval에 재사용할 결과가 없으면, `artifactize status`는 같은 전체 eval ID와 평가 기준으로 실행한 최신 로컬 캐시 기록을 찾습니다. 기록이 있으면 현재 fingerprint와 비교해 어떤 `{name}` 의존성이나 대상 파일이 바뀌었는지 알려 줍니다. `status`는 `artifactsum` 기록에 파일 목록이 있으면 바뀐 개별 파일을 나열하고, 스크립트를 썼다면 fingerprint가 바뀌었다고만 표시합니다.
- **강제 재실행 (`--force`)**: `artifactize verify SELECTOR --force`를 실행하면 직접 선택한 실행 가능 eval(및 선택한 Artifact의 하위 `parts`, 의존성 제외)은 캐시 결과나 진행 중인 실행의 결과를 쓰지 않고 새로 실행합니다. 또한 해당 Run 전체에서 원격 저장소 조회를 생략하고, 새로 완료된 `GREEN` 또는 `RED` 결과를 로컬 캐시와 원격 저장소에 최신 기록으로 저장합니다.
