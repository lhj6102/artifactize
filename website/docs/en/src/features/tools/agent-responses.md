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
keyword you declare is enforced, even those a provider cannot constrain. An invalid final
answer gets one more turn, without tools and within the same deadline, that names the
error and up to five failing paths with their messages (each at most 200 characters), as a
Human submission's error does:

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

If that answer is still invalid, the review is ERROR, and the saved error keeps only the
error code, never the model's output. `max_tokens` and `max_tool_calls` are counted by
artifactize, which stops further tool calls when a budget runs out; they are not sent to
the provider.
