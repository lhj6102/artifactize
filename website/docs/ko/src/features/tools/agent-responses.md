# Agent 응답 {#agent-responses}

GREEN이나 RED 결과에 무엇이 들어갈지는 `pass_schema`와 `fail_schema`로 선언합니다
([소유자 스키마](../declarations.md#owner-result-schemas)). 이 페이지는 Agent가 무엇을 돌려줘야
하는지, artifactize가 이를 어떻게 확인하는지, 실패하면 `run show`에서 무엇을 보게 되는지
설명합니다. 설정 방법과 기본 사용법은 [Agent eval과 백엔드](../agent.md)를 보세요.

## 최종 출력과 검사 {#final-output-and-validation}

Agent는 먼저 도구를 호출할 수 있고, 그다음 provider의 구조화된 출력으로 최종 답을 냅니다. 이
출력은 [소유자 스키마](../declarations.md#owner-result-schemas)에 맞도록 제한됩니다. 저장되는
결과는 `"verdict":"GREEN"`이나 `"verdict":"RED"`와, 그 판정의 스키마가 허용하는 소유자 필드를
함께 담은 객체 하나입니다. `session send`의 follow-up 답변은 일반 텍스트입니다.

provider가 검증하지 못하는 키워드도 artifactize가 소유자 스키마로 직접 검사하므로, 선언한 JSON Schema 제약 조건이 모두 적용됩니다. 답이 스키마에 맞지 않으면 Agent에게 무엇이 잘못됐는지 알리고 도구를 쓰지 않은 채 한 번 더 답하게 합니다. Agent는 이미 모은 정보로 답해야 하며, 이때
도구를 요청해도 리뷰는 `INVALID_RESULT`로 끝납니다. 저장된 대화에 그 메시지가 나옵니다.

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

그 답도 잘못되면 리뷰는 `INVALID_RESULT`와 함께 ERROR로 끝납니다.
[`session show`](../../cli/index.md#agent-sessions)로 Agent가 무엇을 답했는지 읽고, 스키마와 지시문이 서로 맞도록 고친 다음 다시 verify하세요. 예를 들어 각 필드에 무엇이 들어가야 하는지 지시문에 적을 수 있습니다. artifactize는 `max_tokens`와 `max_tool_calls`에 따라 토큰 수와 도구 호출 수를 세며, 예산을 다 쓰면 더 이상 도구를 호출하지 못하게 합니다. 이 값들은 provider에게 보내지 않습니다.
