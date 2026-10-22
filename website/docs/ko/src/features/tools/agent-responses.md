# Agent 응답 {#agent-responses}

Agent eval이 쓰는 전송 형식과 검증입니다. 설정과 일상적인 사용은
[Agent eval과 백엔드](../agent.md)를 보세요.

## 최종 출력과 검증 {#final-output-and-validation}

최종 답은 각 백엔드의 구조화된 출력을 씁니다. OpenAI와
Codex는 Responses의 `text.format`을, Anthropic은 Messages의 `output_config.format`을 씁니다. 전송되는 답은
`verdict` 다음에 소유자 필드인 `verdictFields`가 오는 `result` 필드를 가진 객체입니다.
두 선택지 모두 같은 판별자를 맨 앞에 두므로, 키 순서가 제약되더라도
소유자 필드가 정렬상 앞에 온다는 이유만으로 판정이 선택되지는 않습니다. 저장되는 결과는
`"verdict":"GREEN"` 또는 `"verdict":"RED"`와 허용된 소유자 필드를 담은 평평한 객체로 남습니다.
최종 답 전까지는 도구를 계속 쓸 수 있고, 후속 질문의 답은 일반 텍스트로 남습니다.

모든 구조화된 객체는 닫혀 있고 모든 필드가 필수입니다. 선택적인 소유자 필드는
null 자리 표시자를 쓰며, 원래 필드가 null을 받지 않으면 이 자리 표시자는 제거됩니다.
지원되지 않는 검증 제약은 필드 설명이 되며, 원래 소유자 스키마에 대한
로컬 검증에서 여전히 적용됩니다. 제공자들의 공통
구조 부분 집합 밖의 값(열린 맵이나 재귀 참조 등)은 그 필드 안에서 JSON으로 인코딩한 텍스트로
전달되고, 검증 전에 디코딩됩니다. 네이티브 배열과 닫힌 객체는 구조화된 상태로 남으며,
Responses에서는 항목 수 제한도 유지됩니다. Anthropic이 지원하지 않는 항목 수 제한은
로컬에서 확인합니다. Responses 형식이 중첩, 속성, enum
제한을 넘게 되면, 판정 전체를 대신 `verdictFields` 안의 JSON 인코딩 텍스트로 전달합니다.
디코딩된 객체는 여전히 같은 원래 소유자 스키마로 검증합니다.
잘못된 최종 출력에는 도구를 끈 복구가 원래 마감 시간 안에서 한 번 허용됩니다.
복구 턴은 Human 제출의 오류처럼, 오류를 알려 주고 출력이 JSON 객체였다면
실패한 인스턴스 경로를 최대 다섯 개까지 각 메시지와 함께(각각 200자 이내) 알려 줍니다.

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

복구한 출력도 여전히 잘못되면 리뷰는 ERROR가 되며, 저장된 오류에는
모델의 출력은 남지 않고 오류 코드만 남습니다. `max_tokens`와 `max_tool_calls`는
더 많은 도구가 실행되기 전에 클라이언트 쪽에서 적용되며, 어느 것도 제공자
요청 매개변수가 되지 않습니다.
