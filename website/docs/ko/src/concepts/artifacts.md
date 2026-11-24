# Artifact와 Artifact Graph {#artifacts-and-the-graph}

**Artifact**는 프로젝트에서 리뷰할 작업 단위입니다. 폴더, 단일 파일, 폴더 안에서 고른 일부 경로(**Aspect**), 또는 외부 서비스 URI(**Live**)를 대상으로 삼을 수 있습니다. `.artifactize/<name>.artf` 파일을 만들면 파일 이름(확장자 제외)이 그 Artifact의 고유 이름이 됩니다. `.artifactize/` 안의 하위 폴더는 사람이 파일을 정리하기 위한 용도일 뿐 네임스페이스나 상하 관계를 만들지 않습니다.

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

`artifactize config check`를 실행하면 프로젝트 명령을 실행하지 않고 선언을 검증합니다. 문법 오류가 있으면 오류가 난 파일 이름과 줄·열 번호를 출력하고 종료 코드 `2`를 반환합니다. 로컬 대상 파일이나 폴더가 아직 없으면 안내만 출력하고 종료 코드 `0`으로 끝납니다. 이후 `verify`를 실행할 때는 누락된 대상을 쓰는 eval만 `ERROR/PREPARATION_FAILED`가 됩니다.

## 대상 종류 (`path`와 `select`) {#target-kinds-path-and-select}

모든 로컬 `path`는 프로젝트 루트를 기준으로 적고, **Live** 대상은 URI로 적습니다.

| 선언 | 종류 | 대상 및 명령 실행 기준 폴더 |
|---|---|---|
| `path = "src/"` (또는 `"./"`) | **폴더(Folder)** | 프로젝트 루트에서 지정한 폴더를 찾고 그 폴더에서 명령을 실행합니다. |
| `path = "hero.png"` | **파일(File)** | 프로젝트 루트를 기준으로 지정한 일반 파일 하나를 대상으로 삼습니다. 명령은 파일이 들어 있는 폴더에서 실행되며 `{name}`은 그 파일 경로로 치환됩니다(`/하위경로` 불가). |
| `path = "src/"` + `select = ["api", "model.rs"]` | **Aspect** | Aspect는 해당 폴더에서 고른 경로의 모음입니다. 폴더 일부만 따로 리뷰하고 재사용할 때 씁니다. |
| `path = "https://example.com"` | **Live** | 외부 서비스의 상태를 확인할 때 쓰며, 대상은 URI로 지정합니다. Artifactize가 직접 접속하지 않고, 프로젝트 루트에서 실행하는 읽기 전용 명령에 `{name}`(또는 `{name}/하위경로`)을 그대로 넘깁니다. 내장 파일 도구와 `artifactsum`은 쓸 수 없으며, `fingerprint`를 생략하면 스냅샷이 없어 재사용 키도 생성되지 않습니다. |

대상 종류와 [`type`](../guides/viewer-plugins.md)은 서로 다릅니다. `type`은 도구와 뷰어를 제공하는 플러그인을 고르는 선택 필드입니다.

## Artifact Graph와 eval 의존성 {#eval-dependencies}

**Artifact Graph**는 입력 Artifact와 그 입력을 사용하는 eval을 연결합니다. 같은 저장소나 폴더에 있다는 이유만으로는 의존성이 생기지 않습니다.

- **입력 참조 (`{name}`)**: eval의 `payload.instruction`이나 Runtime `args`에 `{spec}`을 적으면 `spec`이 그 eval의 입력이 됩니다. 그러면 해당 리뷰의 [도구 범위](../guides/tools.md)에 `spec`이 추가되고, 그 eval의 [재사용 키](reuse.md#the-reuse-key)에도 `spec`이 포함됩니다.
- **eval 실행 조건**: 기본 `verify`는 선행 eval부터 순서대로 실행합니다. 예를 들어 `app/e2e`가 `{e2e-tests}`를 참조하는 경우를 보겠습니다. 여기서 `e2e-tests`는 `app`과 같은 순환 참조에 속하지 않는다고 가정합니다. `app/e2e`는 `e2e-tests`의 모든 eval에 유효한 통과 결과가 나올 때까지 대기(`WAIT_DEPENDENCY`)합니다. `e2e-tests`가 순환 참조에 속하면 그 순환 참조에 포함된 모든 eval의 통과 결과가 나와야 `app/e2e`를 실행합니다. 아직 결과가 없는 선행 eval은 `--recursive`나 `--all` 등으로 선택 범위에 포함되었을 때만 실행됩니다. 다만 선택된 선행 Human 승인 요청이 남아 있을 때는 승인을 미루고 하위 평가를 `PROVISIONAL` 상태로 먼저 진행할 수 있습니다([승인과 Run의 종료](runs.md#sign-offs-and-the-end-of-a-run) 참고). 그중 하나라도 `RED`나 `BLOCKED`가 되면 `app/e2e`도 `BLOCKED` 상태가 되며, 실행을 막은 선행 eval을 `blockedBy`에 기록합니다. 이 조건은 eval의 새 실행 여부를 판단할 때 적용하며, Run 통과 요건을 확인할 때도 적용합니다. 선행 eval이 `RED`면 `app/e2e`를 새로 실행할 수 없습니다. 다만 `app/e2e`에 현재 재사용 키와 일치하는 캐시 결과가 있으면 `verify`는 기존 결과를 Run에 반영하며, `status`는 eval의 상태를 `BLOCKED`로, 예상 동작을 `reuse`로 표시합니다. `{e2e-tests}`를 참조하지 않는 `app`의 다른 eval은 `e2e-tests`의 결과와 관계없이 바로 실행됩니다.
- **순환 참조(Cycle)**: eval에 적힌 `{name}` 참조를 따라가다 출발점인 Artifact로 돌아오면 **순환 참조**입니다. 같은 순환 참조에 속한 eval은 서로의 결과를 확인하기 전에 실행할 수 있으며, 순환 밖의 선행 조건은 공통으로 적용됩니다. Human eval은 같은 Artifact의 자동 eval이 통과한 뒤에 시작합니다. `config graph`의 묶음(components)에는 `parts` 구성 관계도 함께 표시되는데, `parts` 관계 때문에 순환이 생겨도 `{name}`으로 참조한 입력의 선행 조건은 그대로 적용됩니다.
- **Human 승인 선행 조건**: 한 Artifact에 선언된 Human eval을 시작하려면 같은 Artifact의 모든 자동(`runtime`, `agent`) eval이 통과(`GREEN` 또는 `PROVISIONAL`)해야 합니다. Human eval이 `{name}`으로 참조한 입력에 적용되는 선행 조건도 모두 충족해야 합니다. 자세한 동작은 [승인과 Run의 종료](runs.md#sign-offs-and-the-end-of-a-run)를 보세요.

## 구성 관계 (`parts`) {#composition}

상위 Artifact의 `parts = ["combat", "physics"]`에 다른 Artifact 이름을 나열하면 부분(part)과 전체(whole)의 구성 관계가 만들어집니다. 경로가 겹치거나 한 폴더가 다른 폴더 안에 있다는 이유만으로 구성 관계가 생기지는 않습니다.

- **판정**: 하위 `parts`(그 하위의 `parts` 포함) 중 하나라도 실패하면 상위 Artifact는 통과할 수 없습니다. 자체 eval이 없고 `parts`만 있는 Artifact는 하위 Artifact의 상태를 요약해 보여 주며, 하위 Artifact가 모두 통과하면 상위 Artifact도 통과합니다.
- **선택**: `artifactize verify game`을 실행하면 `game`과 모든 하위 `parts`가 함께 선택됩니다.
- **재사용**: 각 Artifact의 `artifactsum`을 계산할 때는 선언된 `parts`(폴더, 파일, Aspect 모두 포함)에 속한 파일을 뺍니다. 상위 Artifact의 eval 재사용 키에는 모든 하위 `parts`의 fingerprint를 넣습니다. 자세한 규칙은 [fingerprint와 재사용](reuse.md#the-reuse-key)을 보세요.
- **내장 파일 도구의 접근 범위**: 상위 Artifact의 내장 파일 도구는 eval에서 `{part}`를 직접 참조하지 않는 한 폴더 `parts`를 건너뜁니다. 도구 종류에 따라 어떤 하위 경로에 접근할 수 있는지는 [내장 도구](../guides/tools.md#builtin-tools)를 보세요.

## Basis Artifact, 선언 파일 리뷰, 태그 {#basis-artifacts-definitions-and-tags}

- **`basis = true`**: 팀에서 별도 eval 없이 입력으로 쓰기로 정한 Artifact(외부 요구사항 문서, 잠금 파일 등)를 표시합니다. 이 Artifact를 참조하는 eval의 재사용 키에도 해당 Artifact의 fingerprint를 넣습니다.
- **`.artifactize/` 선언 파일 리뷰**: 선언 파일도 일반 파일과 똑같이 리뷰할 수 있습니다. `path = ".artifactize/"`(또는 하위 폴더나 개별 `.artf` 파일)로 Artifact를 선언해 프로젝트 구조를 검증하세요. 선언 내용을 정규화할 때 어떤 변경을 제외하는지는 [fingerprint와 재사용](reuse.md#fingerprints-artifactsum-script-false)을 참고하세요.
- **`tags`**: `tags = ["type:code"]`는 평가 기준에 영향을 주지 않고 `status`와 `config graph`에 표시할 메타데이터를 덧붙입니다. 다만 Artifact에 포함된 `.artf` 파일은 정규화한 선언 내용이 바뀔 때 fingerprint가 달라집니다.
