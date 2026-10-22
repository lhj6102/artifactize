# Agent responses

Declare what a GREEN or RED result holds with `pass_schema` and `fail_schema`
([owner schemas](../declarations.md#owner-result-schemas)). This page says what the Agent
must return, how artifactize checks it, and what you see in `run show` when it fails. For
setup and everyday use, see [Agent evals and backends](../agent.md).

## Final output and validation

The Agent may call its tools first, then gives its final answer in the provider's
structured output, constrained to your [owner schemas](../declarations.md#owner-result-schemas).
The saved result is one object with `"verdict":"GREEN"` or `"verdict":"RED"` and the owner
fields that verdict's schema allows, side by side. Follow-up answers from
`session send` are plain text.

Artifactize validates the answer against the owner schema itself, so every JSON Schema
keyword you declare is enforced, even those a provider cannot constrain. If the answer
does not match, the Agent is told what was wrong and gets one more chance. The saved
conversation shows that message:

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

If that answer is still invalid, the review ends ERROR with `INVALID_RESULT`. Read what
the Agent answered with [`session show`](../../cli/index.md#agent-sessions), make the schema
and the instruction agree (for example, say in the instruction what each field must
contain), then verify again. `max_tokens` and `max_tool_calls` are counted by
artifactize, which stops further tool calls when a budget runs out; they are not sent to
the provider.
