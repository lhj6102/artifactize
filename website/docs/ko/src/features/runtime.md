# Runtime {#runtime}

Runtime eval은 일반 프로그램을 실행합니다. 테스트, 린터, 렌더러처럼 같은 입력에 늘 같은 답을 내는
명령으로 판정할 수 있는 검사에 쓰세요. 모델이나 API 키는 필요 없습니다.

```toml
name = "library"

[evals.tests]
title = "The library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"], timeout_ms = 240000 }
payload.instruction = "Run the tests of {library}."
```

`command`와 `args`는 따로 씁니다. `args`의 항목은 하나씩 그대로 프로그램의 인자가 되며, 셸이
따옴표, 파이프, `*`를 해석하지 않습니다. 작업 디렉터리는 Artifact 폴더이고, 파일 Artifact라면
그 파일이 들어 있는 폴더입니다. 프로그램은 작성자의 평소 도구 환경을 쓰되, 호출한 쪽의
artifactize 변수는 빠지고 저장소 밖에 별도의 출력·임시 공간을 받습니다. 실행 파일 찾기와 어느
OS에서나 동작하는 명령은 [운영체제](others/platforms.md)를 보세요.

종료 코드 0이면 **GREEN**, 그 밖의 종료 코드면 **RED**입니다. 시작하지 못했거나, 시간이
초과됐거나, 중단됐거나, 입력이 잘못되면 판정 대신 동작 오류가 됩니다. Runtime 명령은 Agent나
Human의 소유자 필드를 돌려주지 않습니다. 저장된 결과에는 완료 뒤의 종료 코드와 크기가 제한된
stdout/stderr가 들어 있습니다. 프로그램이 실행 중일 때는
`artifactize request logs REQUEST_ID`로 지금까지 출력된 로그를 정해진 크기까지 읽을 수 있으며,
수집이 끝나지 않은 로그는 일부만 있다고 표시됩니다. 이는 그때그때 살펴보는 것이지 자동으로 로그를 계속 보내
주는 기능은 아닙니다. tail/범위/검색과 캡처 증거는
[runtime 로그 CLI 레퍼런스](../cli/index.md#agent-streaming-and-runtime-logs)에서, 지금의 정리
기능이 남겨 두는 원본 캡처를 포함한 보존 방식은 공통
[runtime 실행 레퍼런스](declarations.md#runtime-execution)에서 설명합니다. CLI,
[TUI](../tui/index.md), [데스크톱](../desktop/index.md)은 같은 저장된 증거를 읽습니다.

선언한 인자에 `{spec}/requirements.json` 같은 참조를 쓰면 그 입력이 들어옵니다. 명령이 실행되는
동안 fingerprint가 포함하는 파일이 바뀌면 판정 대신 ERROR(`INPUT_CHANGED`)가 됩니다.
fingerprint 밖의 파일은 확인하지 않으며, 재사용하지 않는 eval(`fingerprint = false`)은 다시
확인하지 않습니다. 나중의 Run은 바뀌지 않은 결과를 RED여도 그대로 재사용하며, 명령을 다시
실행하지 않습니다. 정확한 한도, 환경, 치환, 프로세스 정리는 [Fingerprint](fingerprint.md)와
[runtime 실행 레퍼런스](declarations.md#runtime-execution)를 보세요.
