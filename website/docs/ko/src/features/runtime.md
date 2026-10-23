# Runtime {#runtime}

테스트, 린터, 렌더러처럼 종료 상태로 통과 여부를 판정할 수 있는 일반 프로그램을 실행하려면 Runtime eval을
선언하세요. 모델이나 API 키는 필요 없습니다. `library` 폴더라면 다음과 같습니다.

```toml
name = "library"

[evals.tests]
title = "The library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"], timeout_ms = 240000 }
payload.instruction = "Run the tests of {library}."
```

`artifactize verify library/tests --wait`는 거기서 `cargo test --locked`를 실행하고, 종료 코드가
0이면 `library/tests [run-…]: GREEN`을, 그렇지 않으면 RED를 출력합니다.

`command`와 `args`는 따로 씁니다. `args`의 항목은 하나씩 그대로 프로그램의 인자가 되며, 셸이
따옴표, 파이프, `*`를 해석하지 않습니다. 작업 디렉터리는 Artifact 폴더이고, 파일 Artifact라면
그 파일이 들어 있는 폴더입니다. 프로그램은 작성자가 평소 쓰는 도구 환경에서 실행되지만,
호출한 쪽의 artifactize 변수는 전달되지 않습니다. 출력과 임시 파일을 둘 공간은 저장소 밖에 따로 마련됩니다. 실행 파일 찾기와 어느
OS에서나 동작하는 명령은 [운영체제](others/platforms.md)를 보세요.

시작하지 못했거나, 시간이 초과됐거나, 중단됐거나, 입력이 잘못되면 판정 대신 동작 오류가
됩니다. Runtime 명령은 Agent나
Human의 소유자 필드를 돌려주지 않습니다. 실행이 끝나면 종료 코드와 stdout/stderr를 결과에 저장하며, 저장할 출력의 크기에는 제한이 있습니다. 프로그램이 실행 중일 때는
`artifactize request logs REQUEST_ID`로 지금까지 출력된 로그를 정해진 크기까지 읽을 수 있으며,
수집이 끝나지 않은 로그는 일부만 있다고 표시됩니다. 이 명령은 그 시점의 로그를 읽을 뿐이며, 로그를 실시간으로 계속
보내 주지는 않습니다. 로그의 끝부분이나 특정 범위를 읽는 방법, 로그를 검색하는 방법, 수집 상태는
[Runtime 로그 CLI 레퍼런스](../cli/index.md#agent-streaming-and-runtime-logs)에서 설명합니다. 로그
보관 기간과, 현재 정리 기능이 지우지 않고 남겨 두는 원본 로그는 공통
[Runtime 실행 레퍼런스](declarations.md#runtime-execution)에서 설명합니다. CLI,
[TUI](../tui/index.md), [데스크톱](../desktop/index.md)은 저장된 증거를 공통으로 읽습니다.

선언한 인자에 `{spec}/requirements.json` 같은 참조를 쓰면 그 Artifact가 입력이 됩니다. 명령이 실행되는
동안 fingerprint가 포함하는 파일이 바뀌면 판정 대신 ERROR(`INPUT_CHANGED`)가 됩니다.
fingerprint 밖의 파일은 확인하지 않으며, 재사용하지 않는 eval(`fingerprint = false`)은 다시
확인하지 않습니다. 이후 Run은 입력이 바뀌지 않았으면 RED 결과도 그대로 재사용하며, 명령을 다시
실행하지 않습니다. 정확한 한도, 환경, 치환, 프로세스 정리는 [Fingerprint](fingerprint.md)와
[Runtime 실행 레퍼런스](declarations.md#runtime-execution)를 보세요.
