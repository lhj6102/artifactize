# Agent 응답 {#agent-responses}

GREEN이나 RED 결과에 무엇이 들어갈지는 `pass_schema`와 `fail_schema`로 선언합니다
([소유자 스키마](../declarations.md#owner-result-schemas)). 이 페이지는 Agent가 무엇을 돌려줘야
하는지, artifactize가 이를 어떻게 확인하는지, 실패하면 `run show`에서 무엇을 보게 되는지
설명합니다. 설정과 평소 사용법은 [Agent eval과 백엔드](../agent.md)를 보세요.

## 최종 출력과 검사 {#final-output-and-validation}

Agent는 먼저 도구를 호출할 수 있고, 그다음 provider의 구조화된 출력으로 최종 답을 냅니다. 이
출력은 [소유자 스키마](../declarations.md#owner-result-schemas)에 맞도록 제한됩니다. 저장되는
결과는 `"verdict":"GREEN"`이나 `"verdict":"RED"`와, 그 판정의 스키마가 허용하는 소유자 필드를
나란히 담은 객체 하나입니다. `session send`의 follow-up 답변은 일반 텍스트입니다.

artifactize가 직접 답을 소유자 스키마로 검사하므로, provider가 제한하지 못하는 키워드를
포함해 선언한 JSON Schema 키워드가 모두 적용됩니다. 최종 답이 잘못되면 같은 마감 시간 안에서
도구 없이 한 턴을 더 주며, 그 턴에는 Human 제출 오류처럼 오류와 함께 실패한 경로를 최대 다섯 개,
각각의 메시지(최대 200자)와 함께 알려 줍니다.

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

그 답도 잘못되면 리뷰는 ERROR이며, 저장되는 오류에는 모델 출력은 남기지 않고 오류 코드만
남습니다. `max_tokens`와 `max_tool_calls`는 artifactize가 세며, 예산을 다 쓰면 더 이상 도구
호출을 하지 않게 합니다. 이 값들은 provider에게 보내지 않습니다.
