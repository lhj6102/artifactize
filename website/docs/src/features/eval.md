# Eval

An **eval** is one check on an Artifact. Give it a stable local ID, a title that
says what passes, and a profile that says who or what reviews it. For example,
`[evals.tests]` on the `library` Artifact has the full ID `library/tests`.

The three executing kinds share the same lifecycle and reuse rules:

| Kind | Reviewer | Verdict |
|---|---|---|
| [Agent](agent.md) | An exact model through a chosen backend, using declared read-only tools | GREEN or RED with schema-checked owner fields |
| [Human](human.md) | A person who claims the waiting request and submits a sign-off | GREEN or RED with schema-checked owner fields |
| [Runtime](runtime.md) | A declared program and argument vector | Exit 0 is GREEN; another exit code is RED |

A failed invocation or invalid model result is **ERROR**, not RED. RED means the
check actually judged the Artifact and rejected it. Each kind can be reused
while its [inputs and criteria](fingerprint.md#the-reuse-key) still match.

Write the review criteria in `payload.instruction`. For Agent and Human evals,
`pass_schema` and `fail_schema` describe the owner's result fields; artifactize
adds the verdict. See [Owner-result schemas](declarations.md#owner-result-schemas)
for the closed envelope, reserved names and the verdict-only default. Runtime
evals use their command's exit status instead of owner schemas. Named `profile_variants` let a caller select an alternative execution
profile without editing the judgment; see [Declarations](declarations.md).

A fourth, non-executing kind, [dependency](artifact.md#dependency-evals), expresses
readiness over other Artifacts. It derives its state from evidence, never runs a
reviewer and has no cached verdict of its own.

Declare only real inputs. An eval's observation scope starts with its own target
and its explicit instruction/Runtime-operand references, then recursively admits
the children and mounts of every such Artifact. It does not follow a referenced
Artifact's own eval instructions into more dependencies. All admitted Artifacts
can contribute tools. Tool operands do not add new inputs on their own.

Observation access is broader than the [reuse key](fingerprint.md#the-reuse-key):
that key covers the target, its direct children/mounts and this eval's explicit
inputs, not every recursively reachable observation. Ordinary execution gates are
also separate: they are shared at the [Artifact/component level](run.md#shared-execution-gates),
not confined to this eval's own references. See [Artifact](artifact.md) for declarations.

`artifactize verify library/tests --wait` starts a [Run](run.md) for one eval.
Use `artifactize status library/tests` first to see what can reuse or needs review;
status computes fingerprints but does not execute evals.
