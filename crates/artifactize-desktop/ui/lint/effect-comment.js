// Every `$effect` and `$effect.pre` has a `// EFFECT:` comment on the line directly above it,
// saying why nothing safer (`$derived`, `{@attach}`, `createSubscriber`, an event handler)
// does the job. The checklist item is S-EFFECT-ESCAPE.

/** The `$effect(...)` or `$effect.pre(...)` call a statement is made of, or null. */
function effectCall(statement) {
  if (statement.type !== "ExpressionStatement" || statement.expression.type !== "CallExpression") return null;
  const callee = statement.expression.callee;
  if (callee.type === "Identifier" && callee.name === "$effect") return callee.name;
  if (
    callee.type === "MemberExpression" &&
    !callee.computed &&
    callee.object.type === "Identifier" &&
    callee.object.name === "$effect" &&
    callee.property.type === "Identifier" &&
    callee.property.name === "pre"
  ) {
    return "$effect.pre";
  }
  return null;
}

export default {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      missing: "{{ name }} needs a `// EFFECT:` comment on the line directly above it, saying why nothing safer does the job.",
    },
  },
  create(context) {
    const source = context.sourceCode;
    return {
      ExpressionStatement(statement) {
        const name = effectCall(statement);
        if (name === null) return;
        const above = source.getCommentsBefore(statement).at(-1);
        const explained =
          above !== undefined &&
          above.type === "Line" &&
          above.loc.end.line === statement.loc.start.line - 1 &&
          above.value.trim().startsWith("EFFECT:");
        if (!explained) context.report({ node: statement, messageId: "missing", data: { name } });
      },
    };
  },
};
