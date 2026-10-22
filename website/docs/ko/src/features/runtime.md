# Runtime {#runtime}

Runtime eval은 평범한 프로그램을 실행합니다. 테스트, 린터, 렌더러 같은
결정적인 명령이 이미 답을 정해 주는 검사에 쓰세요. 모델이나 API
키는 필요 없습니다.

```toml
name = "library"

[evals.tests]
title = "The library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"], timeout_ms = 240000 }
payload.instruction = "Run the tests of {library}."
```

`command`와 `args`는 따로 씁니다. artifactize는 셸 명령이 아니라 인자 벡터를
시작합니다. 작업 디렉터리는 Artifact 폴더이고, 파일 Artifact라면 그 파일이 들어 있는
폴더입니다. 프로그램은 작성자의 평소 툴체인 환경을 쓰되,
artifactize 호출자의 변수는 빠지고, 저장소 밖에 전용 출력 공간과 임시
공간을 받습니다. 실행 파일 찾기와 이식 가능한 명령은
[운영체제](others/platforms.md)를 보세요.

종료 코드 0이면 **GREEN**, 다른 종료 코드면 **RED**입니다. 시작 실패, 시간 초과,
중단, 잘못된 입력은 대신 운영 오류가 됩니다. Runtime 명령은 Agent/Human 소유자 필드를 돌려주지 않습니다. 저장된 결과에는
완료 후의 종료 코드와 크기가 제한된 stdout/stderr가 들어 있습니다. 실행이 아직
출력을 덧붙이는 동안에는 `artifactize request logs REQUEST_ID`로 크기가 제한된 스냅샷을 읽을 수 있습니다.
확인되지 않은 캡처에는 일부라는 표시가 명시적으로 붙습니다. 이것은 스냅샷 확인이지
자동 스트리밍 로그 피드가 아닙니다. [runtime 로그 CLI 레퍼런스](../cli/index.md#agent-streaming-and-runtime-logs)에서
끝부분/범위/검색과 캡처 증거를 설명하고, 공용
[runtime 실행 레퍼런스](declarations.md#runtime-execution-library)가 현재 정리 기능이 그대로 남겨 두는 원본 캡처를 포함한
보존 세부 사항을 다룹니다. CLI, [TUI](../tui/index.md), [데스크톱](../desktop/index.md)은
같은 저장된 증거를 읽습니다.

선언한 피연산자 안의 `{spec}/requirements.json` 같은 참조는 그 입력을
받아들입니다. 실행 중에 파일이 바뀌면 리뷰는 무효가 됩니다. 이후 Run은 명령을 다시 실행하지 않고
바뀌지 않은 결과를 RED 결과까지 포함해 재사용합니다.
정확한 제한, 환경, 치환, 프로세스 정리는 [Fingerprint](fingerprint.md)와 [runtime 실행 레퍼런스](declarations.md#runtime-execution-library)를
보세요.
