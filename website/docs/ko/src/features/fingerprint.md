# Fingerprint와 재사용 {#fingerprints-and-reuse}

`artifactize verify --all --wait`를 두 번 실행해 보세요. 두 번째에는 모든 eval이
`GREEN (reused …)`으로 나오고 아무것도 다시 실행되지 않습니다. 파일 하나를 바꾸고 다시
verify하면 그 파일에 의존하는 eval만 실행됩니다. fingerprint는 Artifact의 리뷰가 무엇에 달려
있는지를 나타냅니다. eval이 바뀌지 않았고 그 eval이 의존하는 Artifact의 fingerprint도 그대로면
이전 결과를 재사용하므로, 리뷰 비용은 바뀐 만큼만 듭니다.

## Fingerprint {#fingerprints}

폴더와 파일 Artifact는 기본으로 **artifactsum**을 씁니다. `fingerprint`를 생략하면 그 Artifact의 파일을
해시하고, 스크립트를 고를 수도 있습니다. 루트 그래프 Artifact는 항상
[구조 기반 그래프 fingerprint](#graph-fingerprint)를 쓰며, `fingerprint`를 선언해 그 값을
바꾸거나 끌 수 없습니다. 폴더와 파일 Artifact에서 아래 형식들은 도구 선언이나 다른 Artifact의
값을 더하지 않습니다. 의존성은 [재사용 키](#the-reuse-key)를 통해 들어갑니다.

- 생략하면 artifactsum을 씁니다. 그 Artifact의 입력 경로와 내용을 SHA-256으로 해시한 값에
  `artifactsum:`을 붙입니다. 폴더라면 `fingerprint = {}`도 됩니다. 폴더는 그 안의 파일을,
  파일 Artifact는 대상 파일 하나를 해시합니다.
- `fingerprint = { script = { command = "hash", args = [] } }`는 명령의 출력을
  fingerprint로 씁니다. artifactize가 따로 해시를 계산하지 않으므로, 다시 리뷰해야 할 입력과 스크립트의
  변경을 모두 명령의 출력에 반영해야 합니다.
- `fingerprint = false`는 재사용을 끕니다. `fingerprint = true`는 거부합니다.

## Artifactsum {#artifactsum}

Artifact는 그 파일이 바뀌거나, eval이 의존하는 Artifact의 fingerprint가 바뀌면 다시
리뷰합니다. 폴더의 입력을 좁히려면 다음처럼 씁니다.

```toml
fingerprint = { files = ["src"], ignore = ["*.log"] }
```

- `files`: 그 Artifact의 폴더를 기준으로 한 중복 없는 경로 1–64개이며, 각 경로는 UTF-16 코드 단위 1024개까지입니다.
  기본값은 `["."]`이며, 그 Artifact에 속한 파일을 가리킵니다. 경로는 존재해야 하고 Artifact 안에 있어야 하며,
  자식 Artifact 안을 가리키는 경로는 거부합니다. 디렉터리를 훑을 때 자식 Artifact 폴더와
  `*.artf` 선언은 모두 건너뜁니다. `.artf` 파일을 직접 입력으로 적으면 조용히 건너뛰지 않고
  `config check`가 실패합니다. 이름이 `.artf`로 끝나는 디렉터리는 잘못된 선언이며 선언 찾기에서
  실패합니다. 옆에 있는 파일 Artifact의 대상은 폴더 해시에 그대로 들어가며, 자동으로 자식
  관계가 생기지는 않습니다.
- `ignore`: 소유자 기준의 `.gitignore` 형식 glob 최대 64개이며, 고유해야 하고 부정 패턴은 쓸 수
  없습니다. 비어 있거나, `#`로 시작하거나, 제어 문자가 든 패턴은 안 됩니다. 내장 ignore인
  `.git`, `__pycache__/`, `*.pyc`, `target/`, `node_modules/`처럼 항상 제외합니다. `.gitignore`
  파일은 git 방식대로 더 제외합니다. 저장소 루트(`--repo`)부터 Artifact와 훑는 디렉터리까지의
  `.gitignore`가 모두 적용됩니다. 패턴은 그 파일이 있는 폴더를 기준으로 하고, 부정 패턴도 쓸 수
  있으며, 하위 디렉터리의 `.gitignore` 규칙이 상위 디렉터리의 규칙보다 우선합니다. `files`에 직접 적은, 선언이 아닌 파일은
  ignore되지 않습니다.

파일 Artifact는 대상 파일만 해시합니다. artifactsum 형식을 직접 쓸 때는
`fingerprint = { files = ["hero.png"] }`처럼 그 파일만 적어야 합니다. 파일 Artifact에서는
`fingerprint = {}`, `files = ["."]`, `ignore`(`[]`라도)를 거부합니다. 기본값을 쓰려면 필드를
생략하고, 파일 말고 더 많은 것을 포함해야 하면 스크립트를 쓰세요.

예전 `dependencies` fingerprint 옵션은 거부합니다. fingerprint는 그 Artifact만 포함합니다.
탐색 제한은 [레퍼런스](declarations.md#artifactsum)에, 스크립트 형식은
[fingerprint 스크립트](declarations.md#fingerprint-scripts)에 있습니다.

## 재사용 키 {#the-reuse-key}

eval은 대상 Artifact와, 대상과 직접 이어진 것에 의존합니다.

- 대상의 자식 Artifact
- eval이 지시문이나 runtime 인자에서 이름으로 가리킨 Artifact

artifactize는 그보다 더 멀리 연결을 따라가지 않습니다. 두 단계 떨어진 변경이 중요한지는
fingerprint를 어떻게 정하느냐에 달렸습니다. fingerprint 스크립트는 인자에서 이름으로 가리킨
Artifact의 파일이면 무엇이든 읽을 수 있습니다. 그래서 eval의 기준이 바뀌거나, 대상이나 포함한
Artifact의 이름, 종류, fingerprint가 바뀌면 그 eval을 다시 실행합니다.

키에는 대상 Artifact와 포함된 모든 의존성의 이름과 종류(`folder`, `file`, `graph`)가
들어갑니다. 폴더가 파일로 바뀌면 스크립트가 같은 fingerprint를 내더라도 이전 결과를 재사용하지
않습니다. 실제 경로는 키에 들어가지 않으므로, 복사본의 이름, 종류, fingerprint가 원본과 같으면
기존 리뷰 결과를 재사용합니다. 포함한 Artifact의 이름을 바꾸면 키가 바뀝니다. 대상 이름이 달라도 평가 기준과
포함한 Artifact 전체가 같으면 결과를 함께 쓸 수 있습니다. 키는 포함한 Artifact 중 어느 것이
대상인지 따로 구분하지 않기 때문입니다. 예를 들어 똑같은 eval이 서로를 참조하는 두 Artifact는
결과를 함께 쓸 수 있습니다. **eval 정의 해시**는 eval의 **평가 기준** 중 하나가 바뀔 때마다
바뀝니다.

- eval 종류(runtime, agent, human)
- 지시문을 포함한 `payload`
- `pass_schema`와 `fail_schema`
- runtime eval이라면 명령과 인자

**평가 실행 옵션**은 키에 들어가지 않습니다. Agent의 백엔드, 모델, reasoning, `timeout_ms`,
`max_tool_calls`, `max_tokens`, runtime의 `timeout_ms`, 고른 프로필 변형이 그렇습니다. eval
ID와 제목, 태그, 저장소 경로, 쓰지 않은 프로필 변형, 도구 선언(`views`)도 들어가지 않습니다.
도구는 Artifact를 보는 방법일 뿐이므로, 도구의 설명이나 스키마만 바꿔서는 다시 리뷰하지
않습니다. 도구 변경도 반영하려면 관련 파일을 fingerprint에 포함하세요.

실행 가능한 eval은 대상과 의존하는 모든 Artifact에 fingerprint가 있을 때만 키를 가집니다.
생략하면 폴더와 파일 Artifact(그 종류의 basis Artifact 포함)는 artifactsum을 쓰고, 그래프
Artifact는 항상 구조 기반 그래프 fingerprint를 씁니다. 폴더나 파일 Artifact가
`fingerprint = false`를 선언하면 그 eval은 재사용하지 않아서, `verify`할 때마다 다시
리뷰합니다. `status`는 eval마다 상태와 동작의 이유를 알려 줍니다.
[status 상태](../cli/index.md#status-states)를 보세요.

## 완료된 결과 재사용 {#completed-result-reuse}

리뷰가 끝날 때 입력이 바뀌지 않은 GREEN이나 RED 결과는 그 키로 보관합니다
([캐시 살펴보기](../cli/maintenance.md#cache-inspection)). 오류와 취소는 보관하지 않습니다. 한
키에 기록이 여러 개 있으면 GREEN이든 RED이든 완료 시각이 가장 늦은 것을 재사용합니다. 그래서
평가 기준과 입력이 같으면 프로필이 달라도 기존 결과를 재사용할 수 있습니다. 선언된 프로필로 요청한
리뷰에 `--profile fast` 같은 Agent 변형으로 만든 결과를 재사용할 수 있고, 그 반대도 됩니다.
명령이나 인자가 다른 runtime 변형은 별도의 키를 가지며, 다른 입력을 가리킬 수도 있습니다.

기록에는 결과를 만든 프로필과 출처가 남습니다. 감사 필드와 출처 종류 목록은
[Run 결과 레퍼런스](run.md#verify-and-runs)에 있습니다.

재사용하기 전에 artifactize는 포함한 파일 대상을 다시 확인합니다. 대상은 심볼릭 링크를 거치지
않는 일반 파일로 존재해야 합니다. `fingerprint = false`는 재사용을 끌 뿐, 이 파일 대상 안전
확인을 끄지는 않습니다. 대상은 fingerprint 준비와 재확인, eval 시작과 끝, 도구 호출 때마다
확인합니다.

캐시가 적중하면 eval을 실행하지 않고, 요청한 프로필이나 스키마로 다시 검사하지도 않고 원래
결과를 돌려줍니다. 결과를 만든 프로필과 요청한 프로필은 구분해 기록하며, 결과를 재사용한 요청에는
새 사용량이 생기지 않습니다. 재사용한 RED는 게이트와 최종 검증 요건에서 여전히 RED입니다. 이번에 실행하도록
선택하지 않은 의존성의 결과도 캐시에서 가져와 검증에 쓸 수 있습니다. 원래 저장소를 지운 뒤에도 결과는 읽을 수
있지만, 결과 텍스트에 들어 있는 외부 경로까지 옮겨 쓸 수 있게 바꿔 주지는 않습니다.

[팀 리뷰 저장소](../cli/review-store.md#remote-review-store-client)가 있으면 `verify`는 모든 키를 저장소에서도 조회하고, 로컬 기록과 저장소 기록 중 더 나중에 완료된 결과를 재사용합니다. 리뷰 저장소에 접속할 수 없으면 로컬에 저장된 최신 기록을 씁니다.

키가 없으면 캐시를 찾지도 게시하지도 않습니다. `--force`를 쓰면 직접 선택한 실행 가능한 eval은 로컬
캐시나 리뷰 저장소의 결과를 재사용하거나 실행 중인 리뷰에 합류하지 않고 새로 실행합니다. 완료된 GREEN이나 RED는 더 새로운
기록으로 덧붙여지고(설정된 저장소에도 게시되고), 이후 Run이 그 기록을 재사용합니다. 강제로
실행했거나 재사용 키 없이 실행한 결과도 그 Run의 검증에 반영되고 실행 기록으로 남습니다.

## 0.9로 업그레이드 {#upgrading-to-09}

선언, 상태 디렉터리, 터미널 변경은
[설치: 0.9로 업그레이드](../getting-started/install.md#upgrading-to-09)를 보세요. 재사용 키
v2는 스크립트 fingerprint와 팀 리뷰 저장소 기록을 포함해 0.9 이전 결과와 하나도 맞지 않습니다.
그래서 처음 전체 `verify`를 할 때 Human 승인을 포함해 실행 가능한 eval을 모두 한 번 리뷰하고,
그 뒤 Run부터는 평소처럼 재사용합니다.

## 그래프 fingerprint {#graph-fingerprint}

루트 [`.artf` 그래프 Artifact](declarations.md#graph-artifact)는 `graph:<sha256>`을 씁니다. 이
값은 파일이 옮겨지거나 내용이 바뀔 때가 아니라, 프로젝트의 구조나 기준이 바뀔 때 바뀝니다. 다음을
포함합니다.

- Artifact 이름과 종류, `basis`, fingerprint 범위: artifactsum의 `files`와 `ignore`, 스크립트의
  `command`, `args`, `files`, 또는 `fingerprint = false`.
  
- 입력에서 소비자로 가는 관계: 자식 소유(자식 폴더 경로가 아니라 이름), 지시문 참조, runtime argv
  참조(eval ID, argv 위치, 논리적 접미 경로 포함)와 그 입력 대상.
- eval ID, 소유 Artifact, 제목, 선택 항목 `gate`, 평가 기준: 기본 프로필 종류, 지시문과 소유자
  맥락을 포함한 `payload` 전체, `pass_schema`, `fail_schema`, runtime `command`/`args`.

평가 실행 옵션(백엔드, 모델, reasoning, fingerprint 스크립트 시간 제한을 포함한 모든
`timeout_ms`, `max_tool_calls`, `max_tokens`, `profile_variants`, eval `mutex` 태그), 모든 도구
뷰와 세션, Artifact 태그, 저장소 경로, 소스 파일 내용은 빠집니다. 선언한 범위 경로와 범위가 정해진
인자의 접미 경로는 의미가 있습니다. 이런 논리적 선언과 소유 관계를 유지한 채 위치만 옮기면 그래프
fingerprint는 바뀌지 않습니다. 실행 옵션은 여전히 일반 상태 기록에 저장됩니다.

그래프 리뷰는 항상 이전 구조와의 차이가 아니라 현재 구조를 봅니다. 그래프 eval의 제목을 바꾸면
그래프를 다시 리뷰합니다. 다른 eval은 제목을 바꿔도 다시 리뷰하지 않습니다.
