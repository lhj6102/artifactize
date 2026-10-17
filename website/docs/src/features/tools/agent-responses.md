# Agent responses

Wire formats and validation used by an Agent eval. For setup and everyday use, see
[Agent evals and backends](../agent.md).

## Final output and validation

The final answer uses each backend's structured output: Responses `text.format` for OpenAI
and Codex, and Messages `output_config.format` for Anthropic. The wire answer is an object
with a `result` field holding `verdict` followed by `verdictFields`, the owner fields.
Both alternatives put the same discriminator first, so constrained key ordering cannot
select a verdict merely because an owner field sorts before it. The stored result stays
a flat object containing `"verdict":"GREEN"` or `"verdict":"RED"` and permitted owner fields.
Tools remain available before the final answer, and follow-up answers remain plain text.

Every structured object is closed and every field is required; optional owner fields use
null placeholders, which are removed when the original field does not accept null.
Unsupported validation constraints become field descriptions and are still enforced by
local validation against the original owner schema. Values outside the providers' common
structural subset (such as open maps or recursive references) travel as JSON-encoded text
in that field and are decoded before validation. Native arrays and closed objects remain
structured, including item limits on Responses; Anthropic's unsupported item limits are
checked locally. If a Responses format would exceed its nesting, property or enum
limits, the whole verdict travels as JSON-encoded text inside `verdictFields` instead; the
same original owner schema still validates the decoded object.
One tools-disabled repair is allowed for invalid final output, within the original
deadline. The repair turn names the error and, when the output was a JSON object, up
to five failing instance paths with their messages, each bounded to 200 characters,
as a Human submission's error does:

```text
Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema
- instancePath "/covered": "R1" is not of type "array"
Return only one JSON object matching the schema.
```

If the repaired output is still invalid, the review is ERROR, and the saved error
keeps only the error code, never the model's output. `max_tokens` and `max_tool_calls` are
enforced client-side before further tools execute; neither becomes a provider
request parameter.

