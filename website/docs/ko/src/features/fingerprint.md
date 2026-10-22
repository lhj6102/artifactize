# Fingerprint와 재사용 {#fingerprints-and-reuse}

fingerprint는 Artifact의 리뷰가 무엇에 의존하는지 나타냅니다. eval이 바뀌지 않았고
그 eval이 의존하는 Artifact들의 fingerprint도 바뀌지 않았다면 eval의 이전 결과를
재사용하므로, 리뷰 비용은 바뀐 만큼만 듭니다.

## Fingerprint {#fingerprints}

폴더와 파일 Artifact는 기본적으로 **artifactsum**을 씁니다. 자기 파일을 해시하려면
`fingerprint`를 생략하고, 아니면 스크립트를 고르세요. 루트 graph Artifact는 언제나
[구조적 그래프 fingerprint](#graph-fingerprint)를 쓰며, `fingerprint`를 선언해
그 값을 덮어쓰거나 끌 수 없습니다. 폴더와 파일 Artifact에서 아래의 설정 가능한
형식은 도구 선언이나 다른 Artifact의 값을 추가하지 않습니다. 의존성은
[재사용 키](#the-reuse-key)를 통해 들어갑니다.

- 생략하면 artifactsum을 씁니다. `artifactsum:` 뒤에 Artifact의
  입력 경로와 바이트에 대한 SHA-256이 붙습니다. 폴더에서는 `fingerprint = {}`도 됩니다. 폴더는 자기 파일을 해시하고,
  파일 Artifact는 자기 대상 파일 하나를 해시합니다.
- `fingerprint = { script = { command = "hash", args = [] } }`는 명령의
  출력을 fingerprint로 씁니다. 그 자체로는 아무것도 해시하지 않으므로, 명령은 다시 리뷰해야 할
  모든 입력과 스크립트 변경을 반영해야 합니다.
- `fingerprint = false`는 재사용을 끕니다. `fingerprint = true`는 거부됩니다.

## Artifactsum {#artifactsum}

Artifact는 자기 파일이 바뀌거나, 그 eval이 의존하는 Artifact의
fingerprint가 바뀌면 다시 리뷰됩니다. 폴더의 입력을 좁히려면:

```toml
fingerprint = { files = ["src"], ignore = ["*.log"] }
```

- `files`: 각각 UTF-16 코드 단위 1024개 이하인 소유자 기준 상대 경로 1–64개(고유),
  기본값 `["."]`(소유자의 파일). 각 경로는 존재해야 하고 Artifact 안에 있어야 합니다. 자식 Artifact로 들어가는 경로는
  거부됩니다. 디렉터리 탐색은 자식 Artifact 폴더와 모든
  `*.artf` 선언을 건너뜁니다. 명시적인 `.artf` 파일 입력은 조용히 건너뛰지 않고
  `config check`에서 실패합니다. 이름이 `.artf`로 끝나는 디렉터리는 잘못된
  선언이며 탐색에 실패합니다.
  옆에 있는 파일 Artifact의 대상은 폴더의 해시에 남으며,
  자동으로 자식 관계를 만들지 않습니다.
- `ignore`: 소유자 기준 상대인 `.gitignore` 스타일 glob 최대 64개이며,
  부정은 쓸 수 없습니다. 내장 무시 항목 `.git`,
  `__pycache__/`, `*.pyc`, `target/`, `node_modules/`처럼 언제나 제외합니다. `.gitignore` 파일이 있으면
  Git의 무시 규칙에 따라 파일을 더 제외합니다. 저장소 루트(`--repo`)부터
  Artifact와 탐색하는 디렉터리까지의 모든 `.gitignore`가 적용됩니다. 패턴은
  그 `.gitignore`가 있는 폴더를 기준으로 하고, `!` 부정 패턴도 쓸 수 있으며, 하위 폴더의 파일이 상위 폴더의 파일을
  덮어씁니다. 이름으로 명시한, 선언이 아닌 `files`는 결코 무시되지 않습니다.

파일 Artifact는 대상 파일 이름만 해시합니다. 명시적인 artifactsum 형식은
`fingerprint = { files = ["hero.png"] }`처럼 그 파일만 가리켜야 합니다.
파일 Artifact에서는 `fingerprint = {}`, `files = ["."]`, `ignore`(`[]`라도)가 거부됩니다.
기본값을 쓰려면 이 필드를 생략하고, fingerprint가
그 파일보다 많은 것을 포함해야 한다면 스크립트를 쓰세요.

예전 `dependencies` fingerprint 옵션은 거부됩니다. fingerprint는 자기
Artifact만 포함합니다. 탐색 제한과 매니페스트는 [레퍼런스](declarations.md#artifactsum)에 있고, 스크립트 형식은
[fingerprint 스크립트](declarations.md#fingerprint-scripts)에 있습니다.

## 재사용 키 {#the-reuse-key}

eval은 대상 Artifact와, 대상이 직접 연결하는 것에 의존합니다.

- 대상의 자식 Artifact
- eval이 지시문이나 runtime args에서 이름으로 가리키는 Artifact

artifactize는 그보다 더 멀리 연결을 따라가지 않습니다. 두 단계
떨어진 변경이 중요한지는 fingerprint를 어떻게 정의하느냐에 달려 있습니다. fingerprint
스크립트는 args에서 이름으로 가리킨 어떤 Artifact의 파일이든 읽을 수 있습니다. 키는 다음과 같습니다.

```text
hash(Eval definition hash, sorted (Artifact name, kind, fingerprint) of each covered Artifact)
```

Artifact 이름과 kind(`folder`, `file`, `graph`)는 대상과
포함된 모든 의존성에 대해 키의 일부입니다. 폴더에서 파일로 바뀌면 스크립트가 같은 fingerprint를 내더라도
이전 결과를 결코 재사용하지 않습니다. 물리적 경로는
결코 키 입력이 아니므로, 이름, kind, fingerprint가 일치하는 사본은 재사용됩니다.
포함된 Artifact의 이름을 바꾸면 키가 바뀝니다. 이름이 다른 대상이라도
평가 기준과 포함된 Artifact 맵 전체가 일치하면 결과를 공유할 수 있습니다.
키는 포함된 Artifact 중 어느 것이 대상인지 따로 식별하지 않기 때문입니다.
예를 들어 동일한 eval이 서로를 참조하는 두 Artifact는 결과를 공유할 수 있습니다.
**eval 정의 해시**는 **평가 기준**에 대해 키를 재귀적으로 정렬한
정규 JSON의 소문자 SHA-256입니다.

- eval kind(runtime, agent, human)
- 지시문을 포함한 `payload`
- `pass_schema`와 `fail_schema`
- runtime eval의 경우 command와 args

**평가 실행 옵션**은 키에 들어가지 않습니다. Agent의 백엔드, 모델, reasoning,
`timeout_ms`, `max_tool_calls`, `max_tokens`, runtime `timeout_ms`, 선택된
프로필 변형이 그렇습니다. eval ID와 제목, 태그, 저장소 경로, 쓰지 않는
프로필 변형, 도구 선언(`views`)도 들어가지 않습니다. 도구는
Artifact를 보는 방법이므로, 도구의 설명이나 스키마만 바꿔서는 다시 리뷰하지 않습니다. 도구가
바뀔 때 다시 리뷰하게 하려면 관련 파일을 fingerprint에 포함하세요.

실행형 eval은 그 대상과 그것이 의존하는 모든 Artifact에
fingerprint가 있을 때만 키를 가집니다. 생략하면 폴더와 파일 Artifact에는
그런 kind의 basis Artifact를 포함해 artifactsum이 주어지고, graph Artifact는 언제나
구조적 그래프 fingerprint를 줍니다. 폴더나 파일 Artifact가
`fingerprint = false`를 선언하면 그 eval은 재사용되지 않으며, 모든 `verify`가 그것을
다시 리뷰합니다. 강제하지 않은 READY Runtime/Agent eval이라면 `status`가 실행 이유에서
재사용이 꺼진 대상이나 의존성을 알려 줍니다. READY Human eval은 대신
맡기와 제출을 기다리는 요청이라는 이유를 주며, 강제된 eval, 막힌 eval, 기다리는 eval은
각각 실행, 관문, 대기 이유를 씁니다.

## 완료된 결과 재사용 {#completed-result-reuse}

리뷰 종료 시 재확인에 성공하면 GREEN 또는 RED 결과가 그 키의
이력에 추가됩니다. 자체적으로 완결된 `executions` 행에 완료 시각, 크기, 마지막
사용 시각이 기록됩니다([상태](others/state.md#state)). 오류와 취소는
결코 그곳에 기록되지 않습니다. 키에 기록이 둘 이상 있으면
완료 시각 기준으로 가장 최근 것이 GREEN이든 RED이든 재사용됩니다. 그래서 서로 다른
프로필로 만든 결과도 서로 재사용할 수 있습니다. `--profile fast`로 만든 리뷰는
선언된 프로필을 충족하고, 그 반대도 마찬가지입니다.

기록에는 그것을 만든 프로필과 출처가 남습니다. 감사 필드와 출처 종류 목록은 [Run 결과
레퍼런스](run.md#verify-and-runs)에서 다룹니다.

이전 결과를 재사용하기 전에 artifactize는 리뷰 범위에 들어간 파일 대상을 다시 검증합니다.
심볼릭 링크를 거치지 않는 일반 파일로 존재해야 합니다. `fingerprint = false`는
재사용을 끌 뿐, 이 파일 대상 안전 검사를 끄지는 않습니다. 대상은
fingerprint 준비/재확인 때, eval 시작/종료 때, 그리고 모든 도구 호출 때도 확인됩니다.

캐시 적중은 eval을 실행하거나 요청된 프로필이나 스키마로 다시 검증하지 않고
원래 결과를 돌려줍니다. 결과를 만든 프로필은 요청된 프로필과 구별되어 남으며,
결과를 재사용한 요청에는 사용량이 새로 들지 않습니다. 재사용된 RED는
관문과 최종 의무에서 여전히 RED입니다. 실행 선택 밖의 의존성은
실행하지 않고도 캐시된 증거를 제공할 수 있습니다. 결과는 원본 저장소가 삭제된 뒤에도
읽을 수 있지만, 결과 텍스트에 들어 있는 외부 경로가 이식 가능하게 바뀌지는 않습니다.

[팀 리뷰 저장소](team/reference.md#remote-review-store-client)가 있으면
`verify`는 모든 키를 저장소에도 묻고, 로컬의 최신 기록과 저장소의 최신 기록 중
마지막으로 완료된 것을 재사용합니다. 저장소에 닿을 수 없으면
로컬의 최신 기록을 씁니다.

키가 없으면 캐시 조회도 게시도 없습니다. `--force`는 명시적으로
선택된 실행형 eval을 로컬이든 저장소든 이전 결과를 읽거나 그에 합류하지 않고 다시 실행하며,
완료된 GREEN 또는 RED는 더 새 기록으로 추가되어(설정된 저장소에도
게시되어) 이후 실행이 이를 재사용합니다. 강제된 결과와 키가 없는
결과도 자기 Run은 충족하며 그 실행은 남습니다.

## 0.9로 업그레이드 {#upgrading-to-09}

선언, 상태 디렉터리, 터미널 변경은
[설치: 0.9로 업그레이드](../getting-started/install.md#upgrading-to-09)를 보세요.
재사용 키 v2는 스크립트 fingerprint와 팀
리뷰 저장소 기록을 포함해 0.9 이전의 어떤 결과와도 일치하지 않습니다. 그래서 첫 전체 `verify`는
Human 승인을 포함해 모든 실행형 eval을 한 번씩 리뷰하며, 이후 Run은 평소대로 재사용합니다.

## 그래프 fingerprint {#graph-fingerprint}

루트 [`.artf` graph Artifact](declarations.md#graph-artifact)는
프로젝트 전체의 현재 선언을 이름 기준으로 한 번 정규 추출해 계산한
`graph:<sha256>`을 씁니다. 객체와 관계는 정렬되며, 자식
폴더 위치와 저장소 경로는 식별 기준이 아닙니다. 추출에는 다음이 포함됩니다.

- Artifact 이름과 kind, `basis`, fingerprint 범위: artifactsum `files`와
  `ignore`, 스크립트 `command`, `args`, `files`, 또는 `fingerprint = false`.
  graph Artifact에는 재귀적으로 들어간 자기 다이제스트가 아니라
  그래프 범위 표시가 있습니다.
- 입력 → 소비자 관계: 자식 소유(자식 폴더 경로가 아닌 이름),
  지시문 참조, runtime argv 참조
  (eval ID, argv 인덱스, 논리적 접미사 포함), 그리고 그 입력 대상.
- 정규화된 eval ID, 소유 Artifact, 제목, 선택적인 `gate`, 그리고 평가 기준: 기본
  프로필 kind, 지시문과 소유자 맥락을 포함한 전체 `payload`,
  `pass_schema`, `fail_schema`, runtime `command`/`args`.

제외되는 것은 평가 실행 옵션(백엔드, 모델, reasoning, fingerprint 스크립트 시간 제한을 포함한
모든 `timeout_ms` 값, `max_tool_calls`,
`max_tokens`, `profile_variants`, eval `mutex` 태그), 모든 도구 뷰와 세션, Artifact 태그, 저장소
경로, 소스 파일 내용입니다. 선언된 범위 경로와 범위 지정 인자
접미사는 의미가 있습니다. 이런 논리적 선언과
소유를 유지하는 위치 이동은 그래프 fingerprint를 바꾸지 않습니다. 실행 옵션은 여전히
일반 상태 기록에 저장됩니다.

이것은 fingerprint 알고리즘일 뿐입니다. 재사용할지 다시 리뷰할지는 기존 재사용 키, Run, status, 리뷰
흐름이 결정합니다. 그래프는 저장된 diff나 구조 이력이 아니라
현재 상태에 대해 평가됩니다. 그래프 eval의 제목을 바꾸면,
일반 eval 정의 해시에서는 제목이 제외되더라도 그래프 fingerprint는 무효가 됩니다. 두 내장 `graph` 도구 모두 이 같은 추출을 씁니다.
