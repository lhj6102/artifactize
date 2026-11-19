# Artifact와 Artifact Graph {#artifacts-and-the-graph}

**Artifact**는 프로젝트에서 리뷰하는 모든 작업 단위입니다. 폴더, 단일 파일, 폴더 안에서 고른 일부 경로(**Aspect**), 또는 외부 서비스 URI(**Live**)를 대상으로 삼을 수 있습니다. `.artifactize/<name>.artf` 파일을 만들면 파일 이름(확장자 제외)이 그 Artifact의 고유 이름이 됩니다. `.artifactize/` 안의 하위 폴더는 사람이 파일을 정리하기 위한 용도일 뿐 네임스페이스나 상하 관계를 만들지 않습니다.

```toml
# .artifactize/guide.artf
path = "guide/"
parts = ["intro", "usage"]
tags = ["type:docs"]

[evals.terms]
title = "Every bold term is defined in the glossary"
profile = { kind = "runtime", command = "python3", args = ["check_terms.py", "{glossary}/terms.txt", "{intro}/page.md", "{usage}/page.md"] }
payload.instruction = "Check that every bold term in {intro} and {usage} is defined in {glossary}."
```

`artifactize config check`를 실행하면 프로젝트 명령을 실행하지 않고 선언을 검증합니다. 문법 오류가 있으면 파일, 줄, 열을 출력하고 종료 코드 `2`로 끝납니다. 로컬 대상 파일이나 폴더가 아직 없으면 안내만 출력하고 종료 코드 `0`으로 끝납니다. 이후 `verify`를 실행할 때는 누락된 대상을 쓰는 eval만 `ERROR/PREPARATION_FAILED`가 됩니다.

## 대상 종류 (`path`와 `select`) {#target-kinds-path-and-select}

모든 로컬 `path`는 프로젝트 루트를 기준으로 적고, **Live** 대상은 URI로 적습니다.

| 선언 | 종류 | 대상 및 명령 실행 기준 폴더 |
|---|---|---|
| `path = "src/"` (또는 `"./"`) | **폴더(Folder)** | 프로젝트 루트에서 지정한 폴더를 찾고 그 폴더에서 명령을 실행합니다. |
| `path = "hero.png"` | **파일(File)** | 프로젝트 루트 기준 해당 일반 파일 하나. 명령은 파일이 들어 있는 폴더에서 실행되며 `{name}`은 그 파일 경로로 치환됩니다(`/하위경로` 불가). |
| `path = "src/"` + `select = ["api", "model.rs"]` | **Aspect** | 해당 폴더 아래에서 선택한 경로 모음. 폴더 일부만 따로 리뷰하고 재사용할 때 씁니다. |
| `path = "https://example.com"` | **Live** | URI로 식별하는 외부 상태. Artifactize가 직접 접속하지 않고, 프로젝트 루트에서 실행하는 읽기 전용 명령에 `{name}`(또는 `{name}/하위경로`)을 그대로 넘깁니다. 내장 파일 도구와 `artifactsum`은 쓸 수 없으며, `fingerprint`를 생략하면 스냅샷이 없어 재사용 키도 생성되지 않습니다. |

## Artifact Graph와 eval 의존성 {#eval-dependencies}

**Artifact Graph**는 입력 Artifact와 그 입력을 사용하는 eval을 연결합니다. 같은 저장소나 폴더에 있다는 이유만으로는 의존성이 생기지 않습니다.

- **입력 참조 (`{name}`)**: eval의 `payload.instruction`이나 Runtime `args`에 `{spec}`을 적으면 `spec`이 그 eval의 입력이 됩니다. 그러면 해당 리뷰의 [도구 범위](../guides/tools.md)에 `spec`이 추가되고, 그 eval의 [재사용 키](reuse.md#the-reuse-key)에도 `spec`이 포함됩니다.
- **eval 단위 게이트**: 기본 `verify`는 선행 eval부터 순서대로 실행합니다. 예를 들어, `app/e2e`가 `app`과 같은 순환 참조에 속하지 않는 `{e2e-tests}`를 참조한다고 가정합니다. `e2e-tests`(및 `e2e-tests`가 속한 순환 참조)의 모든 eval에 유효한 통과 결과가 생길 때까지 `app/e2e`는 대기(`WAIT_DEPENDENCY`)합니다. 아직 결과가 없는 선행 eval은 `--recursive`나 `--all` 등으로 선택 범위에 포함되었을 때만 실행됩니다. 그중 하나라도 `RED`나 `BLOCKED`가 되면 `app/e2e`도 `BLOCKED` 상태가 되며, 실행을 막은 선행 eval을 `blockedBy`에 기록합니다. 게이트는 eval을 새로 실행할 수 있는지, 해당 eval이 Run 통과 요건을 충족하는지 결정합니다. 선행 eval이 `RED`면 `app/e2e`를 새로 실행할 수 없습니다. 다만 `app/e2e`에 이미 일치하는 캐시 결과가 있으면 `verify`는 그 캐시 결과를 그대로 연결하며, `status`에는 상태 `BLOCKED`, 동작 `reuse`로 표시됩니다. `{e2e-tests}`를 참조하지 않는 `app`의 다른 eval은 기다리지 않고 바로 실행됩니다.
- **순환 참조(Cycle)**: 여러 Artifact의 eval이 직접 또는 `{name}` 참조 사슬을 거쳐 서로를 참조하면 **순환 참조**가 생깁니다. 같은 순환 참조에 속한 eval은 서로의 결과를 확인하기 전에 실행할 수 있으며, 순환 밖의 선행 조건은 공통으로 적용됩니다. Human eval은 같은 Artifact의 자동 eval이 통과한 뒤에 시작합니다. `config graph`의 묶음(components)에는 `parts` 구성 관계도 함께 표시되는데, `parts` 관계 때문에 순환이 생겨도 `{name}`으로 참조한 입력의 선행 조건은 그대로 적용됩니다.
- **Human 승인 선행 조건**: 한 Artifact에 선언된 Human eval을 시작하려면 같은 Artifact의 모든 자동(`runtime`, `agent`) eval이 통과(`GREEN` 또는 `PROVISIONAL`)해야 합니다. Human eval이 `{name}`으로 참조한 입력에 적용되는 선행 조건도 모두 충족해야 합니다. 자세한 동작은 [승인과 Run의 종료](runs.md#sign-offs-and-the-end-of-a-run)를 보세요.

## 구성 관계 (`parts`) {#composition}

상위 Artifact의 `parts = ["combat", "physics"]`에 다른 Artifact 이름을 나열하면 부분(part)과 전체(whole)의 구성 관계가 만들어집니다. 폴더 경로가 겹치거나 안에 들어 있다는 이유만으로는 구성 관계가 생기지 않습니다.

- **판정**: 하위 `parts`(그 하위의 `parts` 포함) 중 하나라도 실패하면 상위 Artifact는 통과할 수 없습니다. 자체 eval 없이 `parts`만 가진 Artifact는 하위 부분들의 상태를 요약해 보여 주며 모든 부분이 통과하면 통과합니다.
- **선택**: `artifactize verify game`을 실행하면 `game`과 모든 하위 `parts`가 함께 선택됩니다.
- **재사용**: 각 Artifact의 자체 `artifactsum`에서는 선언된 `parts`(폴더, 파일, Aspect 모두 포함)의 파일이 제외되고, 상위 Artifact eval의 재사용 키에는 모든 하위 `parts`의 fingerprint가 합쳐집니다. 자세한 규칙은 [fingerprint와 재사용](reuse.md#the-reuse-key)을 보세요.
- **내장 파일 도구의 접근 범위**: 상위 Artifact의 내장 파일 도구는 eval에서 `{part}`를 직접 참조하지 않는 한 폴더 `parts`를 건너뜁니다. 동적 도구와 고정 경로 도구가 폴더·파일·Aspect `parts`의 범위를 어떻게 다루는지는 [내장 도구](../guides/tools.md#builtin-tools)를 보세요.

## Basis Artifact, 정의 리뷰, 태그 {#basis-artifacts-definitions-and-tags}

- **`basis = true`**: 자체 eval 없이 그대로 수용하기로 합의한 입력 Artifact(외부 요구사항 문서, 잠금 파일 등)를 표시합니다. `basis`를 참조하는 eval의 재사용 키에는 여전히 그 fingerprint가 포함됩니다.
- **`.artifactize/` 정의 리뷰**: 정의 파일도 일반 파일과 똑같이 리뷰할 수 있습니다. `path = ".artifactize/"`(또는 하위 폴더나 개별 `.artf` 파일)로 Artifact를 선언해 프로젝트 구조를 검증하세요. 선언 내용을 정규화할 때 어떤 변경을 제외하는지는 [fingerprint와 재사용](reuse.md#fingerprints-artifactsum-script-false)을 참고하세요.
- **`tags`**: `tags = ["type:code"]`는 평가 기준에 영향을 주지 않고 `status`, `config graph`, `monitor`, `review`(`tag:` 검색)에 표시할 메타데이터를 덧붙입니다(단, `.artf` 파일을 직접 포함하는 Artifact에서는 `.artf` 파일의 의미 있는 필드를 수정하면 파일 fingerprint가 바뀝니다).
