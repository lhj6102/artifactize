# 예제 {#examples}

[설치](install.md)에 나온 대로 바이너리를 설치한 뒤 예제 프로젝트를
실행해 보세요. 각 README에 정확한 명령이 적혀 있습니다.

- [Runtime relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations): 부모와 자식 폴더에서 명령을
  실행하는 검사, 지시문과 명령 인자 안의 `{artifact}` 참조, 입력만 제공하는 Artifact, 일부러
  실패시켜 볼 수 있는 검사, 파일이 바뀔 때까지 결과를 재사용하는 모습을 보여 주며, `status`가
  무엇이 바뀌었는지 설명합니다.
- [Agent tools](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools): 내장
  `read`, `grep`, `view_image`, `section` 도구와 선언한 `json` 도구를 쓰는 Agent eval,
  통과/실패 스키마, 백엔드와 모델 선택, 그리고 내장 `open`, `read` 도구를 쓰는
  Human 승인을 보여 줍니다.
- [Posts](https://github.com/lhj6102/artifactize/tree/main/examples/posts): Markdown 파일
  Artifact 세 개, 글마다 하나씩 둔 사이드카 선언, 참조된 basis Artifact 안의 공용 스타일 검사기,
  글별 단어 수 제한, artifactsum으로 글마다 결과를 따로 재사용하는 방식을 보여 줍니다.
  글 하나를 바꾸면 해당 글의 eval만 다시 실행되고, 공용 검사기나 규칙을 바꾸면 세 글의 eval이 모두 다시 실행됩니다.
- [eval 의존성](../features/artifact.md#eval-dependencies):
  eval마다 입력을 지정하는 방법과, 의존하는 eval을 먼저 실행하는 순서를 설명합니다.
- [팀 워크스루](../features/team/walkthrough.md): 두 머신과 CI가 하나의
  `artifactize server`를 통해 서로의 판정을 재사용합니다.
