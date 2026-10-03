## Summary

- **Adopt the Rig family, pinned initially to `rig-core = 0.43.0`.** It is the strongest currently verified Rust equivalent to Pi’s multiprovider layer. Add `rig-agent = 0.43.0` if artifactize wants an existing agent loop, and `rig-bedrock = 0.43.0` if Bedrock is required.
- This is an **adoption recommendation, not a drop-in compatibility claim**. Several CCDD behaviors must be relaxed or dropped; those changes are listed explicitly below.
- Rig’s reported-model identifier, optional usage counters, ChatGPT/Codex dialect, streaming, reasoning controls, and tool-result images are real source-level features—not merely README claims.
- A compiled, credential-free probe verified exact Codex request parameters, server-reported model identity, reported-zero usage, cache accounting, and ordered ordinary tool calls.
- The same probe proved a compatibility blocker: **Rig rejects duplicate provider tool-call IDs**, whereas CCDD deliberately budgets and executes those calls by source order.
- Other differences include Rig’s fresh per-request `session_id`, absence of a Pi-equivalent model/effort capability catalog, Anthropic tool-result `is_error` loss in the common representation, and Rig agent batch-commit behavior.
- **Authentication is the largest product limitation.** Rig supports OpenAI API keys, Anthropic API keys, AWS credentials, and a ChatGPT/Codex subscription backend. Its subscription implementation uses the legacy `chatgpt.com/backend-api/codex` service and Codex-style device OAuth—not the new official per-application Sign in with ChatGPT flow.
- Rig does **not** provide an out-of-the-box Anthropic subscription-OAuth implementation. Its Anthropic client sends `x-api-key`.
- If **official Sign in with ChatGPT only** and **Anthropic subscription access** remain mandatory, **none of the researched crates satisfies the complete requirement out of the box**. Choosing Rig does not resolve that gap.
- `genai` is the runner-up, but its string-only tool results, effort remapping, and streaming identity/usage normalization are poorer matches. `llm`, `aisdk`, and `llms-sdk` hide more information CCDD needs.
- `async-openai` is a strong OpenAI-only SDK, not a multiprovider substitute. No official general-purpose OpenAI or Anthropic Rust SDK was found in their current official SDK lists.
- The unofficial crates.io `pi-ai` remains excluded. `pi-core-rs` is closer behaviorally but experimental, unpublished, and targets Pi 0.84.4 rather than 1.0.0.
- No product code or user credentials were read or modified. This complete report is returned rather than saved because this worker’s instructions prohibit writing report files.

# Rust LLM Provider Research for artifactize

**As of 2026-10-03.** Final scope: select an existing Pi-like Rust crate, document its capabilities and the CCDD requirements that do not fit, and describe only a thin executor adapter. This report does not propose a new provider framework, OAuth implementation, or CLI-executor architecture.

## 1. Evidence and scope

### Local evidence index

The following absolute paths identify the CCDD references used below:

- **C1 — executor:** `ccdd@eddf8f7/src/executors/pi.ts`
- **C2 — credentials:** `ccdd@eddf8f7/src/executors/auth.ts`
- **C3 — final-result validation:** `ccdd@eddf8f7/src/executors/final-result.ts`
- **C4 — telemetry:** `ccdd@eddf8f7/src/executors/telemetry.ts`
- **C5 — executor tests:** `ccdd@eddf8f7/test/pi.test.ts`
- **C6 — wire tests:** `ccdd@eddf8f7/test/pi-wire.test.ts`
- **C7 — budget tests:** `ccdd@eddf8f7/test/review-budget.test.ts`
- **C8 — documented contracts:** `ccdd@eddf8f7/docs/contracts.md`
- **C9 — reviewer documentation:** `ccdd@eddf8f7/docs/reviewers.md`
- **C10 — tool conversion:** `ccdd@eddf8f7/src/tools/runner.ts`
- **C11 — offline diagnostic:** `ccdd@eddf8f7/src/project/load-check.ts`
- **C12 — identity caveat:** `ccdd@eddf8f7/src/executors/CONTEXT.md`
- **C13 — prompts:** `ccdd@eddf8f7/src/executors/prompt.ts`

A citation such as **C1:30–48** means those lines in the absolute file above.

CCDD 6.6.0 pins both npm Pi packages to **1.0.0**. Registry metadata dates those releases to **2026-10-01**, with Git commit `a13d35a742c6ef8462812a28fbe1d8c8b7431c32`. I inspected the extracted 1.0.0 package, not only the older installation. The actual package metadata under `/home/user/code/ccdd/node_modules/@earendil-works/pi-ai/package.json` reported **0.85.1** during this research, despite the task’s mention of 0.99.1. The old installation was therefore not treated as the parity reference.

Version and maintenance claims below use live crates.io metadata and GitHub repository metadata, not search snippets. This matters: search results incorrectly suggested Rig lacked a ChatGPT backend and reported an older Genai prerelease. Source inspection disproved both.

## 2. Requirements extracted from CCDD

| Requirement | Actual CCDD behavior and source |
|---|---|
| Provider/model catalog | Uses Pi’s built-in catalog; validates exact provider and model before transport. Needs protocol, reasoning support, thinking-level mapping, image-input capability, and provider/model compatibility information. **C1:19–48.** |
| Exact model selection | No unknown model substitution, aliases invented locally, or automatic fallback. Anthropic catalog fallback lists are explicitly cleared. **C1:30–48; C5:70–107,249–272.** |
| Exact reasoning application | Supported vocabulary is **`off`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`**—the task’s initial list omitted `max`. Supported Pi levels that map to a different named level are rejected. Adaptive Anthropic `minimal` aliases are rejected. Importantly, current CCDD allows `off` only for a non-reasoning model, even if a reasoning model’s upstream map contains an off representation. **C1:26–48.** |
| SSE streaming | Runs the model with `transport: 'sse'`; requires proper terminal completion, error and cancellation handling, not just text chunks. **C1:165–207,213–221; C6:107–149.** |
| Agent loop with scoped tools | Iterates assistant/tool turns using the admitted Artifact tools only. Tool JSON schemas and local argument validation remain authoritative. Unknown tools and invalid arguments must not become observations. **C1:128–149,165–183; C5:109–140.** |
| Ordered tool-call accounting | Tools execute sequentially. Every model-issued tool-call block counts before name resolution or argument validation. Accounting links calls by source order, not provider IDs. Duplicate IDs must not attribute the first call to the second call’s arguments. **C1:106–115,130–140,175–190; C7:71–89,112–119.** |
| Tool budget behavior | Within-budget calls run; the first excess call and later calls do not. Already performed observations remain recorded. Cancellation/deadline errors take precedence over budget errors. **C1:110–115,178–188; C8:518–532.** |
| Images in tool results | Tool results may contain actual base64 images, not just image URLs in the initial user prompt. CCDD rejects an image result for a catalog model lacking image input. JSON and launch content become text; images remain typed image blocks. **C1:140–148; C10:132–146,158–163.** |
| Conversation replay | Tool calls/results and opaque reasoning continuation data must survive subsequent turns. Anthropic’s historical tool definitions and Bedrock’s tool configuration have special repair requirements. **C1:167–174; C5:553–587,607–636; C6:131–147.** |
| Per-model-call usage | Emits one usage event per completed, identity-validated assistant message. Retains only present nonnegative safe-integer counters: input, output, cacheRead, cacheWrite, cacheWrite1h, reasoning, totalTokens. Does not record prices or content. **C1:190–206; C4:1–9; C5:297–320.** |
| Per-review usage and token budget | Sums the current attempt’s completed model turns, including repair. `maxTokens` means processed total tokens, including cache reads/writes—not a per-call output cap or dollar budget. Missing usage contributes zero to budget enforcement. Reasoning is a subset of output, and one-hour cache writes a subset of cache writes. **C1:198–206; C8:525–529,702–742.** |
| Response identity check before tools | Checks Pi’s provider, requested model, and any exposed `responseModel`; aborts before executing that response’s tool batch on mismatch. Applies again during repair. **C1:190–196,213–219; C5:274–294,485–502.** |
| Missing identity is unknown | CCDD does not require server attestation where Pi omitted it. The Codex parser’s missing response-model exposure is documented. Missing metadata is not proof of correctness, but is not itself rejected. **C9:75–81; C12, “Provider identity visibility.”** |
| Cancellation and deadline | A single deadline covers credentials, requests, tools, result checks and repair. Default timeout is 240 seconds. External cancellation, timeout and provider failure are operational errors, not verdicts. Cleanup always runs. **C1:83–105,213–225,253–272; C5:142–156,225–247,456–483.** |
| Read-only credential stores | API keys and OAuth entries may come from explicit external files; environment credentials are also supported through Pi. Credential paths must be absolute and outside reviewed input, including canonical/symlink checks. Files are bounded to 1 MiB. **C2:24–29,50–58,60–95.** |
| OAuth freshness and no refresh | Stored OAuth tokens expiring within five minutes are rejected. Store modification/deletion are forbidden. The legacy Codex adapter reads only `tokens.access_token`, derives expiry from its JWT, and never passes the shared refresh token to Pi. Conflicting Pi/Codex entries fail. **C2:68–73,96–130; C5:175–211.** |
| Final output is exactly one JSON value | The final assistant turn must be complete, successful, and free of tool calls. Concatenated text is parsed as a whole, bounded to 1 MiB UTF-8, and validated against the final schema. No code-fence stripping or JSON extraction from prose. **C1:213–225; C3:198–212; C8:537–548.** |
| One repair turn, no executable tools | Invalid JSON/schema or an owner result-check diagnostic gets at most one further assistant turn, same context/model/reasoning/deadline. Executable tools are removed. Normally wire `tool_choice` is none; Anthropic retains historical definitions; Bedrock retains definitions with auto but no executable tools. Any repair tool call cannot produce new evidence or another turn. **C1:227–249; C5:377–454,553–587,607–636.** |
| Bounded, content-free diagnostics | Schema diagnostics expose bounded schema locations/keywords, not instance values or response-derived keys. Optional telemetry cannot stall evaluation or replace a primary failure; writes have a 100 ms bound. Raw responses, prompts, reasoning and repair prompts are not persisted as evidence. **C3:100–105,128–179,182–228; C4:12–20; C8:578–599,702–725.** |
| Session identity | Stable per-review session identity across tools and repair; distinct reviews have distinct identities. OpenCode requires `x-opencode-session`. Codex wire tests require the same `session-id` across its turns. Identifiable client User-Agent, not accidental identity substitution. **C9:69–73; C6:57–105,131–147.** |
| Testability and offline diagnostics | Transport can be replaced while real validation, agent loop and tools remain. Load-check must not import providers or perform provider network calls. **C1:75–76; C11:97–104.** |

### Important docs/code discrepancy

C9 says GPT-6.1 Sol supports “minimal through max.” However, the **Pi 1.0.0 catalog maps `minimal` to `low`**, and C1:42 explicitly rejects that substitution. Therefore the actual CCDD acceptance rule is **low, medium, high, xhigh, max**, not minimal. The same issue applies to Astra and Luna’s minimal alias. Preserve actual validation behavior or explicitly change it; do not infer acceptance from prose alone.

The relevant pinned catalog is:

`(session scratchpad, not kept)/provider-sources/pi-ai-npm-1.0.0/package/dist/providers/data/openai-codex.json`

## 3. Current candidate landscape

### Versions, licensing and maintenance

“Active” below means observed recent publishing/push activity, not a guarantee of support. Dates are UTC registry publication dates. Latest prereleases are identified separately from stable releases.

| Candidate | Current release / date | License and observed maintenance | Assessment |
|---|---|---|---|
| **Rig family** | `rig-core`, `rig-agent`, `rig-bedrock` **0.43.0**, **2026-09-30** | MIT. Rig repository pushed **2026-10-02**. Rust **1.95+**. Substantial 0.43 API restructuring. [Core registry](https://crates.io/api/v1/crates/rig-core), [agent registry](https://crates.io/api/v1/crates/rig-agent), [Bedrock registry](https://crates.io/api/v1/crates/rig-bedrock), [release](https://github.com/0xPlaygrounds/rig/releases/tag/v0.43.0). | **Best existing multiprovider choice.** |
| **Genai, jeremychone** | Stable **0.6.5**, **2026-06-06**; newest **0.7.0-rc.1**, **2026-09-27** | MIT OR Apache-2.0. Repository pushed **2026-09-27**. [Registry](https://crates.io/api/v1/crates/genai), [repository](https://github.com/jeremychone/rust-genai). | Runner-up; lighter client, but normalization conflicts with CCDD exactness. |
| **llm, graniet** | **1.3.8**, **2026-04-19** | MIT. Repository last pushed **2026-06-06**. [Registry](https://crates.io/api/v1/crates/llm), [repository](https://github.com/graniet/llm). | Broad providers, weaker common response contract. |
| **async-openai** | **0.42.1**, **2026-09-28** | MIT. Repository pushed the same day. Declared Rust 1.75. [Registry](https://crates.io/api/v1/crates/async-openai), [repository](https://github.com/64bit/async-openai). | Strong direct OpenAI SDK; not multiprovider. |
| **misanthropic** | Stable **0.5.1**, **2024-11-30**; newest **1.0.0-alpha.20**, **2026-09-28** | MIT in crate metadata. Repository pushed **2026-10-01**. [Registry](https://crates.io/api/v1/crates/misanthropic), [repository](https://github.com/mdegans/misanthropic). | Modern Anthropic coverage is in an alpha line; not a Pi substitute. |
| **anthropic-sdk-rust** | **0.1.1**, **2025-06-11** | MIT. Registry points to **dimichgh/anthropic-sdk-rust**, not every similarly named GitHub project. Last push **2025-06-11**. [Registry](https://crates.io/api/v1/crates/anthropic-sdk-rust), [repository](https://github.com/dimichgh/anthropic-sdk-rust). | Old and materially incomplete for current CCDD needs. |
| **anthropic-rust-sdk** | **0.112.3**, **2026-07-20** | MIT; community AI-assisted synchronization from the TS SDK, not an official release. Last push **2026-07-20**. [Registry](https://crates.io/api/v1/crates/anthropic-rust-sdk), [repository](https://github.com/gaoyia/anthropic-rust-sdk). | Flexible raw fields, but young and single-provider. |
| **pi-core-rs, Onion-L** | Unpublished; package declares **0.84.4**; inspected commit **89890a17**, **2026-09-25** | MIT, `publish = false`, explicitly experimental. [Repository](https://github.com/Onion-L/pi-core-rs), [commit](https://github.com/Onion-L/pi-core-rs/commit/89890a17f75a09165c38b58a06d1a7ff71d87b07). | Closest nominal Pi port; unsuitable as the default production dependency now. |
| **aisdk** | **0.5.2**, **2026-02-25** | MIT. Repository pushed **2026-08-08**. [Registry](https://crates.io/api/v1/crates/aisdk), [repository](https://github.com/lazy-hq/aisdk). | Broad gateway list, but common API hides identity and has limited effort controls. |
| **llms-sdk** | **0.3.4**, **2026-09-23** | MIT. Repository pushed **2026-09-27**. Small/new project. [Registry](https://crates.io/api/v1/crates/llms-sdk), [repository](https://github.com/AstraBert/llms-sdk). | Clean small client, currently Chat Completions + Anthropic, not Responses/Codex. |
| **pi_agent_rust** | **0.6.1**, **2026-09-24** | **Non-standard MIT-with-rider**, with explicit restrictions concerning OpenAI/Anthropic and parties acting for them. Repository pushed **2026-10-02**. [Registry](https://crates.io/api/v1/crates/pi_agent_rust), [license](https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/LICENSE). | Do not treat as ordinary MIT. Exclude absent legal acceptance; large coding-agent application rather than clean default library choice. |
| **Official AWS SDK** | `aws-sdk-bedrockruntime` **1.148.0**, **2026-10-01** | Apache-2.0; official AWS SDK, frequent releases. [Registry](https://crates.io/api/v1/crates/aws-sdk-bedrockruntime), [repository](https://github.com/awslabs/aws-sdk-rust). | Good underlying Bedrock client; Rig already integrates it. |

Additional narrow subscription crates exist: `skg-provider-codex` **0.4.1** (2026-03-10, MIT OR Apache-2.0) and `dpc-tau-provider-codex` **0.2.1** (2026-09-29, MPL-2.0). They are tied to other frameworks and the Codex backend, not a general solution to this task. The latter emphasizes pooled WebSockets and its own auth/configuration machinery. Neither was demonstrated to provide official new SIWC support. [SKG docs](https://docs.rs/skg-provider-codex/0.4.1/skg_provider_codex/), [Tau docs](https://docs.rs/dpc-tau-provider-codex/0.2.1/tau_provider_codex/).

The already-rejected crates.io **`pi-ai` 1.0.0** remains excluded: it is not the official npm Pi project’s Rust distribution and lacks required Responses/Codex/Bedrock coverage.

### Official SDK status

OpenAI’s current official SDK page lists JavaScript/TypeScript, Python, .NET, Java, Go and Ruby; Rust is under **community libraries**, with `async-openai`. Anthropic’s page lists Python, TypeScript, C#, Go, Java, PHP and Ruby, not Rust. AWS does have an official Rust SDK.

Sources: [OpenAI libraries](https://developers.openai.com/api/docs/libraries), [Anthropic SDKs](https://platform.claude.com/docs/en/api/client-sdks).

“Claude Agent SDK for Rust” search hits generally refer to community wrappers around the Claude Code process, not an official Rust Messages SDK. They are not evaluated as provider-crate replacements here.

## 4. Capability comparison

### 4.1 Protocol and authentication coverage

**Native** means an implementation exists, not that live account access was tested. **Custom** means caller configuration/extra headers or a different low-level path, not out-of-box workflow support.

| Candidate | Anthropic Messages | OpenAI Responses | ChatGPT/Codex subscription backend | Anthropic subscription OAuth | Bedrock |
|---|---|---|---|---|---|
| **Rig 0.43** | Native, API key | Native, API key | **Native legacy Codex dialect**; injected access token or device OAuth/cache/refresh helper | **No native path**; normal Anthropic config hardcodes `x-api-key` | Native through `rig-bedrock`, AWS SDK credentials |
| **Genai 0.7 rc** | Native, API key/custom auth resolver | Native | No dedicated Codex/SIWC flow found | Custom headers are possible; no verified complete subscription compatibility | Native Converse support, including optional SigV4 feature and Bedrock API-key path |
| **llm 1.3.8** | Native | **Yes**—its OpenAI backend actually uses Responses | No dedicated subscription dialect found | No complete native path verified | AWS backend |
| **async-openai 0.42.1** | No native Messages adapter | Native | Token/base URL/header injection possible, but no Codex OAuth/dialect workflow | No | No Converse |
| **misanthropic alpha.20** | Native, API key | Not its core purpose; an OpenAI compatibility module is not a full multiprovider client | No | Normal request path sets `x-api-key`; no native subscription flow | No direct Bedrock adapter |
| **anthropic-sdk-rust 0.1.1** | Native | No | No | Configurable bearer auth, **not** complete subscription protocol support | No |
| **anthropic-rust-sdk 0.112.3** | Native | No | No | `auth_token` and custom headers, **not** complete subscription workflow | Explicitly excludes cloud-provider variants |
| **pi-core-rs** | Yes | Yes | Yes, Pi-style Codex support | Pi-style OAuth | Yes |
| **aisdk 0.5.2** | Yes | Yes | No dedicated path found | No complete native path verified | Its “Amazon Bedrock” module is **OpenAI-compatible**, not the Converse adapter CCDD uses |
| **llms-sdk 0.3.4** | Yes | **No; Chat Completions only** | No | No complete native path verified | No native Converse |

### 4.2 Fidelity and executor features

| Candidate | Streaming/tools | Reasoning exactness | Images, including tool results | Usage and response model | Credentials/cancellation | Fit |
|---|---|---|---|---|---|---|
| **Rig** | SSE; typed tool calls; companion agent runtime, sequential default | OpenAI enum includes none/minimal/low/medium/high/xhigh/max; Anthropic/provider fields via explicit parameters. No Pi-equivalent supported-level catalog | User images and typed tool-result images in Anthropic, Responses and Bedrock | True optional server model; optional counters; raw provider document; one-hour cache count requires raw inspection | Explicit credentials; stream/future drop cancels; optional OAuth helper mutates its store | **Best**, with explicit contract changes |
| **Genai rc** | SSE/tools; client rather than full Pi loop | **Maps levels**: minimal→low; unsupported xhigh/max→high in Anthropic helper | User multimodal content; **ToolResponse is a String**, not typed image blocks | Nonstream provider model available, but absent field may default to requested model; stream public result lacks reported model. New raw-frame sink can recover it. Some zero counters become None | Auth/target resolvers and extra headers/body; async stream drop; no full cancellation parity test here | More adaptation than Rig |
| **llm** | Streaming + tools; optional agent facilities | Common effort enum only low/medium/high | User images; tool-result representation does not model native image-result blocks adequately | `ChatResponse` trait exposes text/tools/thinking/usage **but no model or raw response getter**; detailed cache fidelity limited | Builder API keys/timeouts; drop-oriented cancellation, not tested here | Too much type erasure |
| **async-openai** | Raw Responses events/tools; no own loop required | Full current effort enum and typed/raw request controls | Responses function outputs can be text or content arrays, including images | Wire model String; optional overall usage; individual typed counters are not all optional. BYOT available | Custom key/base URL/headers/client; dropping receiver terminates background reader even if upstream idle | Excellent single-provider component, not Pi-like multiplexer |
| **misanthropic alpha** | Messages SSE, tool blocks and higher-level chat facilities | Adaptive/legacy thinking; low/medium/high/xhigh/max/custom effort | Native message blocks and tool-result content | Wire model and usage, TTL cache details; mandatory core counters can hide absence through defaults | API-key-first; drop-based async streams | Good specialized candidate, prerelease |
| **anthropic-sdk-rust** | Messages SSE/tools | Current adaptive effort parity not established | User images; actual message `tool_result.content` is `Option<String>`; higher-level helper converts images to `[Image]` text | Model/usage exposed; modern counter coverage limited | API key/bearer config; streaming cancellation not verified | Reject for this adoption |
| **anthropic-rust-sdk** | SSE and message events | Raw `thinking`, `output_config`, extra fields | Generic blocks allow native image/tool payloads | Model and core/cache usage exposed; limited typed usage detail | API key/auth-token/custom headers; async streams | Flexible but immature and not multiprovider |
| **pi-core-rs** | Pi-style streams, agent events, sequential option, cancellation tokens | Pi-like levels/map; catalog tied to older Pi target | Pi-like image/tool content | Pi-shaped usage and `response_model`; source search found response-model assignment only in Chat Completions path | Pi-style credentials/refresh machinery; own read-only store policy needed | Behavioral proximity, unacceptable maturity tradeoff for default |
| **aisdk** | Streaming/tools/agent facilities | Common enum low/medium/high | User images; tools return String/JSON rather than typed native image results | Common response contains contents+usage, not model identity; only selected counters | Configured API keys; async behavior; exact cancellation not verified | Convenience abstraction loses needed information |
| **llms-sdk** | Text/tool/reasoning SSE | Broad effort enum but provider mappings still need checking | User images; `ToolResultPart.result` is String | Common completed response has ID/message/usage **no model field** | Explicit request key; configurable retries | Small and insufficient protocol/fidelity coverage |

For all candidates, **the exact CCDD final-JSON and single-no-tools-repair policy is application behavior**, not something to assume from a library advertising “structured output.” Some extractors use tools or automatic retries internally, which changes CCDD’s semantics.

## 5. Why Rig wins—and what it actually supports

### 5.1 Genuine provider unification

Rig 0.43 exposes a shared completion request/response representation and a type-erased `DynModel<Completion>`, alongside concrete provider clients. Its registry includes OpenAI, Azure, DeepSeek, Groq, OpenRouter, Mistral-compatible endpoints, xAI, ChatGPT, Copilot and others; Anthropic-format and Gemini-format providers have their own native implementations. Bedrock is a separate companion crate.

This is meaningfully Pi-like: artifactize can call a single completion/stream interface rather than choose a different SDK surface at every call site.

Rig 0.43 also split provider/core machinery from the classic agent runtime. **Do not expect `rig-core` alone to contain the old high-level agent API.** Use the version-matched `rig-agent` companion for that. Its runtime includes hooks, request patches, a steppable run state, tool execution, retries and structured-output policies.

Sources: [Rig registry source](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/registry.rs), [Rig agent crate](https://docs.rs/rig-agent/0.43.0/rig_agent/), [0.43 release](https://github.com/0xPlaygrounds/rig/releases/tag/v0.43.0).

### 5.2 Authentication matrix for Rig

**OpenAI API keys:** native `OpenAIConfig::new(key)`; explicit Responses versus Chat Completions selection; custom endpoint configuration. No need to read environment variables if artifactize supplies the credential explicitly.

**ChatGPT/Codex subscription:** native `chatgpt::DIALECT`:

- Base URL: `https://chatgpt.com/backend-api/codex`.
- Requests go to `/responses`.
- Bearer access token and optional `ChatGPT-Account-Id`.
- SSE, `store:false`, encrypted reasoning replay support.
- Direct token injection works without invoking its auth helper.
- Optional `AuthSource::OAuth` uses Codex-style device endpoints, a fixed Codex client ID, cached records, and refresh-token handling.
- That helper **writes** the cache even in some non-refresh cases, such as filling in the account ID. It is not a read-only CCDD credential-store adapter.

**Official new Sign in with ChatGPT:** not the same feature. Current official docs require per-app registration and public `https://api.openai.com/v1/responses`; they explicitly say not to use ChatGPT backend-api endpoints. I did not find this registration/auth workflow in Rig 0.43 or the other surveyed crates. Rig’s ordinary OpenAI transport can send an externally obtained bearer token, but that is transport capability—not an integrated SIWC solution.

**Anthropic:** native API-key support. `AnthropicConfig` has `api_key`, base URL, version and beta headers, but its header builder directly emits `x-api-key`. There is no native subscription-OAuth mode equivalent to Pi’s provider.

**Bedrock:** use `rig-bedrock`; credentials and request signing come from the AWS SDK. This is not Anthropic consumer-subscription access.

Sources: [ChatGPT dialect](https://docs.rs/rig-core/0.43.0/rig_core/providers/chatgpt/index.html), [auth source](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/chatgpt/auth/native.rs), [Anthropic wire](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/anthropic/wire.rs), [official SIWC inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).

### 5.3 Response identity and usage: verified strengths

Rig’s common `CompletionResponse` contains:

- `provider: String` — the configured provider descriptor;
- `model: Option<String>` — **the model actually named in the provider response**, not the requested model;
- `usage` with optional counters;
- `raw: serde_json::Value` for provider-specific details;
- terminal reason and separate message/response/request IDs.

This is the right shape for CCDD’s identity checks. Note that `provider` is adapter identity, not independent server attestation. Map artifactize’s public provider ID deliberately: Rig calls its Codex dialect **`chatgpt`**, not `openai-codex`.

Rig’s usage contract differs from Pi’s: **Rig input includes cached reads and writes**, while Pi’s `input` is uncached input and those counts are separate. This is a mapping issue, not an excuse to double-count. The normalized Rig usage lacks a separate one-hour cache-write field, but the Anthropic raw response retains it; the probe confirmed this.

Raw does not mean lossless bytes: Rig explicitly warns that typed parsing can omit unmodeled fields. Keep raw response content transient and extract only approved counters.

Sources: [CompletionResponse and Usage](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/completion/request.rs#L175-L214), [usage semantics](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/completion/request.rs#L389-L427), [Responses terminal model capture](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/openai/responses_api/streaming.rs#L875-L885).

### 5.4 Streaming, tools, reasoning, images and cancellation

- `Model::stream` returns a stream whose final fold exposes completion metadata; incomplete terminal state is not automatically a valid answer.
- Stream events have ordered part identifiers and raw argument deltas.
- Common tool results support **text, JSON and images**. The Anthropic, Responses and Bedrock converters handle image results.
- OpenAI’s reasoning enum includes all named levels through `Max`; Anthropic reasoning is carried through explicit provider request fields.
- Rig accepts explicit model strings, including models absent from its convenience constants.
- Dropping a stream cancels its retained request; stopping polling only pauses it. Artifactize still owns the review deadline and cancellation of its local tools.
- `rig-agent` tool concurrency defaults to one; `.tool_concurrency(1)` makes intent explicit. It has hooks before requests and after completed model turns.

Sources: [stream lifecycle](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/streaming/mod.rs), [tool-result types](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/completion/message.rs#L296-L328), [reasoning enum](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/openai/responses_api/mod.rs#L1831-L1844).

## 6. CCDD requirements to relax or drop when adopting Rig

These are **behavior changes**, not hidden implementation details. They should become explicit artifactize requirements and tests.

| CCDD requirement to relax/drop | Rig behavior | Impact and minimal integration note |
|---|---|---|
| Full offline Pi catalog validation | Rig has provider registry/model listing and some capability information, not Pi’s complete per-model reasoning/image/thinking map | Unknown or unsupported model/effort may fail at request construction or provider response rather than identical preflight diagnostics. Retain only a small application-level supported-profile policy if desired. |
| Generic cross-provider reasoning-level equivalence | Controls are provider-specific; Rig does not prove every level is exact for every model | Do not promise all seven levels on every provider. Expose only explicitly supported settings; never silently substitute effort. |
| Duplicate tool-call IDs processed in source order | Rig’s core fold raises `DuplicateCallId` | The malformed turn becomes an execution error before earlier calls in that turn run. This is fail-closed, but differs from CCDD’s “first call ran, then budget exceeded” regression. No thin-hook fix after normalization. |
| Every malformed tool-call block reaches budget accounting | Some malformed arguments or names are rejected during decoding/normalization | Some turns fail as provider-response errors rather than counted recoverable tool errors. Accept the stricter boundary or do not adopt unmodified Rig. |
| Stable Codex session header across review turns | Rig generates a fresh **`session_id`** per request; CCDD tests expect stable **`session-id`** | Session correlation/cache-affinity semantics change. The spelling also differs; do not claim wire parity. |
| Native `opencode-go` wrapper parity | No dedicated Pi-equivalent OpenCode Go wrapper found in Rig’s registry | Generic compatible endpoint use does not automatically supply stable `x-opencode-session` or every model-specific reasoning replay quirk. Drop first-release parity for this provider. |
| Typed tool-result error status reaches Anthropic | Common Rig `ToolResult` has no `is_error`; Anthropic conversion sets `is_error: None` | Error text can still reach the model, but not the native error flag. Keep local observation/error truth independent of this wire representation. |
| Pi token-field semantics unchanged | Rig input includes cache traffic; total semantics are normalized; no common `cacheWrite1h` | Either document Rig semantics or use a thin telemetry mapping. Do not add cache counts to Rig total again. |
| Read-only stores while using library OAuth login helper | Rig OAuth helper may read, refresh and write its own cache | Keep read-only execution only by using explicit supplied access tokens; the OAuth helper itself cannot be called a read-only store adapter. |
| Official SIWC subscription auth out of the box | Rig’s built-in subscription flow is legacy Codex, not per-app SIWC | If official SIWC-only remains mandatory, subscription support is **not satisfied by this crate**. Defer that feature pending a suitable existing integration. |
| Anthropic subscription OAuth out of the box | Rig supports API keys, not Pi-style subscription OAuth | Use direct API-key/Bedrock access in the crate-backed executor or defer subscription support. Do not advertise bearer injection as complete support. |
| Rig agent emits each successful tool execution before a later batch failure | `rig-agent` batches commit/surface all-or-nothing, even with sequential execution | Earlier tools may have physically run but lack committed batch events when a later one fails. Record Artifact observations in the Artifact-tool wrapper, not solely from Rig committed events. |
| Pi’s recoverable unknown/invalid-tool behavior | Rig agent invalid-call handling is fail-fast by default and has its own retry/repair/skip rules | Accept Rig error behavior unless a small hook can express the chosen artifactize policy; do not claim Pi event-level parity. |
| Pi retry timing and diagnostics | Rig classifies errors and its agent offers retry policy, but it is not Pi’s exact retry schedule or error-text classifier | Keep bounded host timeout and stable artifactize error categories; drop byte-for-byte diagnostic/retry parity. |
| Pi-specific wire defaults | Codex dialect strips several parameters, including `text`, output caps and sampling fields; Pi sends different defaults such as text verbosity | Request bodies will not match Pi snapshots. Judge output behavior and supported contract, not byte identity. |

Two policies **need not be relaxed** merely because Rig is adopted:

1. **No model fallback.** Configure one explicit model and do not install routing/fallback hooks. Reject any exposed server-model mismatch before tools/verdict acceptance.
2. **Strict final JSON with one repair.** Keep artifactize’s existing validator and single-repair policy. Do not replace it with an unrestricted extractor retry or JSON-from-prose convenience parser.

The latter requires a small explicit integration test around the selected Rig agent path; no such full end-to-end repair integration was compiled in this research.

### Subscription policy qualification

Current Anthropic documentation permits customers to run the **unmodified official Claude Code binary** under stated conditions, but says developers should use API keys or supported cloud providers and restricts collecting/intermediating subscription credentials for third-party applications. Pi 1.0’s OAuth adapter supplies Claude Code-specific identity headers and a Claude Code system line. That is not a feature to assume any generic Rust bearer-token client legitimately reproduces.

This report therefore does **not** recommend a custom imitation of that path. It records that Rig lacks it and asks the product owner to accept or defer the requirement. [Current Anthropic policy](https://code.claude.com/docs/en/legal-and-compliance).

## 7. Credential-free spike results

A throwaway crate was created and run successfully with **Rust 1.98.1**, using published **Rig 0.43.0** and mocked HTTP streams. It made no authenticated requests and never opened user auth files.

Files:

- `(session scratchpad, not kept)/spikes/llm-provider-probe/Cargo.toml`
- `(session scratchpad, not kept)/spikes/llm-provider-probe/src/main.rs`
- `(session scratchpad, not kept)/spikes/llm-provider-probe/Cargo.lock`

Verified assertions:

1. Direct synthetic access-token/account injection encodes `https://chatgpt.com/backend-api/codex/responses`.
2. An arbitrary exact model string, `gpt-6.1-sol`, is preserved.
3. `reasoning.effort = xhigh`, `store = false`, `stream = true` survive serialization.
4. A Codex terminal SSE event reporting `CODEX-SERVER-MODEL` produces that actual value in `CompletionResponse.model`, despite a different requested model.
5. Reported output/reasoning zeroes remain `Some(0)`; default unreported usage remains `None`.
6. Anthropic input 10 + cache read 3 + cache creation 2 becomes Rig input **15**; output 5 produces total **20**.
7. Anthropic raw response preserves `ephemeral_1h_input_tokens = 2`.
8. Two ordinary tool calls with distinct IDs retain argument order.
9. Two calls with the **same provider ID** produce **`DuplicateCallId`**. The final probe asserts this error as a demonstrated parity gap, not as a passing CCDD behavior.

The probe was initially corrected after accidentally trying a crate-private configuration method; its final public-API path uses the connected model’s public `wire.encode`. The final run completed successfully.

Not established by these tests: real account authorization, actual model entitlement, subscription policy compliance, real provider acceptance of image results, full repair lifecycle, Bedrock behavior under credentials, or end-to-end network cancellation timing.

## 8. Recommendation and thin integration sketch

### Recommended dependency choice

**Choose Rig, not a Pi-named experimental port.**

- Primary multiprovider library: **`rig-core = "=0.43.0"`**.
- Existing agent loop if wanted: **`rig-agent = "=0.43.0"`**.
- Optional Bedrock: **`rig-bedrock = "=0.43.0"`**.
- Enable Rig’s **`reqwest`** transport feature for real HTTP; 0.43 no longer includes an HTTP transport merely by depending on the core. Select TLS features deliberately.
- Keep the Rig family version-aligned, commit the lockfile, and run adapter contract tests before upgrading. The 0.43 release contains substantial breaking changes; old blog examples are unreliable.

### What the thin adapter does

1. **Construct one explicit Rig provider/model** from artifactize’s configured provider, model and supplied credential; map public provider naming, notably Rig `chatgpt` versus the old Pi `openai-codex` label.
2. **Translate existing prompts and Artifact tool definitions/results** into Rig’s request/message types. Preserve image blocks and keep tool execution delegated to the existing Artifact runner.
3. **Use Rig streaming or its existing agent runner**, with tool concurrency one and no model-routing/fallback hooks. Do not build another SSE/parser/provider layer.
4. **At completed-turn boundaries**, inspect actual response model, stop reason and usage before allowing tools or accepting a verdict. Convert usage semantics once, and persist only bounded telemetry.
5. **Wrap the request/run with artifactize cancellation and deadline**, dropping the Rig operation when cancelled and cancelling the Artifact runner separately.
6. **Retain artifactize’s final JSON/schema check and one permitted repair**, configuring no executable tools for that continuation. Keep automatic extractor behavior out of the contract unless its retry/tool policy is deliberately accepted.

If artifactize uses `rig-agent`, its current extension surface includes `AgentHook::on_completion_call`, `on_model_turn_finished`, `on_invalid_tool_call`, run-local state, and request patches. These are the appropriate integration points. The first spike should confirm the exact hook ordering for identity/budget rejection; simply subscribing to emitted text or post-commit tool events is insufficient.

**This adapter is not supposed to recreate Pi’s catalog, credential management, provider parsers, or fallback heuristics.** Where Rig does not supply a requested feature—especially the two subscription requirements—record that feature as unsupported rather than silently building a second framework.

### Why not Genai instead?

Genai is attractive for a small general chat client and now has broad protocols, including Responses and Bedrock. But its native `ToolResponse` is still a string, its Anthropic effort helper deliberately remaps unsupported levels, its common streaming result does not carry server-model identity, and its usage normalization can turn reported zeroes into absence. The 0.7 raw-frame sink helps, but recovering all those semantics starts to defeat the purpose of adopting its abstraction.

### Why not pi-core-rs?

It is closer to Pi in vocabulary and agent events, including sequential execution before argument preparation. But adopting it commits artifactize to an unpublished experimental project targeting an older upstream version, with a stale catalog and incomplete response identity exposure. It is useful as a reference or future watch candidate, not the recommended dependency baseline.

### Why not async-openai plus an Anthropic crate?

That is a reasonable direct-SDK engineering strategy, but it does not meet the latest request to adopt **one existing Pi-like multiprovider abstraction**. Rig already provides the stronger common boundary and a matching agent runtime.

## 9. Decisions still needed

1. **Authentication gate:** Is it acceptable to ship Rig-backed OpenAI API-key, Anthropic API-key and Bedrock support first? Rig’s legacy subscription backend is not the official SIWC workflow. None of the surveyed candidates solves official SIWC plus Anthropic subscription access end-to-end.
2. **Malformed tool batches:** Accept Rig’s fail-closed rejection of duplicate IDs and certain malformed calls, rather than CCDD’s call-order execution/budget behavior? This is a concrete tested incompatibility.
3. **Catalog policy:** Drop Pi’s comprehensive offline catalog validation in favor of explicit model strings and provider rejection, or keep a small declared supported-profile list? A complete Pi catalog replacement is outside thin integration.
4. **Reasoning UI:** Which levels will artifactize expose per provider/model? Do not advertise a generic level that the selected library/provider will remap.
5. **Usage schema:** Preserve old Pi field semantics with a small conversion or adopt Rig’s inclusive input semantics? Either is viable; silently changing the meaning is not.
6. **Tool errors and batch evidence:** Accept native `is_error` loss and rely on artifactize’s local observation records? If using `rig-agent`, keep the actual tool runner—not Rig’s all-or-nothing batch events—as the evidence source.
7. **Initial provider set:** Defer `opencode-go` until its session headers, reasoning history and endpoint-specific behavior are verified? A generic OpenAI-compatible URL alone is not demonstrated parity.
8. **Upgrade tolerance:** Pin and test Rig family releases together; 0.43 is promising but newly released and structurally different from earlier versions.
9. **Final repair acceptance test:** Confirm exactly one tool-disabled continuation, same model/deadline, and no hidden extractor retries before calling the migration complete.

## Sources

### Package releases and maintenance

- [Rig core registry](https://crates.io/api/v1/crates/rig-core)
- [Rig agent registry](https://crates.io/api/v1/crates/rig-agent)
- [Rig Bedrock registry](https://crates.io/api/v1/crates/rig-bedrock)
- [Rig 0.43 release](https://github.com/0xPlaygrounds/rig/releases/tag/v0.43.0)
- [Rig repository metadata](https://api.github.com/repos/0xPlaygrounds/rig)
- [Genai registry](https://crates.io/api/v1/crates/genai)
- [Genai repository](https://github.com/jeremychone/rust-genai)
- [llm registry](https://crates.io/api/v1/crates/llm)
- [llm repository](https://github.com/graniet/llm)
- [async-openai registry](https://crates.io/api/v1/crates/async-openai)
- [async-openai repository](https://github.com/64bit/async-openai)
- [misanthropic registry](https://crates.io/api/v1/crates/misanthropic)
- [anthropic-sdk-rust registry](https://crates.io/api/v1/crates/anthropic-sdk-rust)
- [anthropic-rust-sdk registry](https://crates.io/api/v1/crates/anthropic-rust-sdk)
- [pi-core-rs repository](https://github.com/Onion-L/pi-core-rs)
- [aisdk registry](https://crates.io/api/v1/crates/aisdk)
- [llms-sdk registry](https://crates.io/api/v1/crates/llms-sdk)
- [pi_agent_rust registry](https://crates.io/api/v1/crates/pi_agent_rust)
- [AWS Bedrock runtime registry](https://crates.io/api/v1/crates/aws-sdk-bedrockruntime)
- [npm Pi AI 1.0.0](https://registry.npmjs.org/@earendil-works/pi-ai/1.0.0)
- [npm Pi Agent Core 1.0.0](https://registry.npmjs.org/@earendil-works/pi-agent-core/1.0.0)

### API and implementation references

- [Rig ChatGPT 0.43 docs](https://docs.rs/rig-core/0.43.0/rig_core/providers/chatgpt/index.html)
- [Rig pinned response types and usage](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/completion/request.rs)
- [Rig duplicate-call rejection](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/operation/completion.rs#L498-L503)
- [Rig Anthropic tool conversion](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/anthropic/completion.rs#L883-L915)
- [Rig request session headers](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/providers/openai/wire.rs#L1202-L1220)
- [Genai 0.7 rc source](https://docs.rs/crate/genai/0.7.0-rc.1/source/)
- [Genai pinned effort remapping](https://github.com/jeremychone/rust-genai/blob/0a3184c01d4d94b3f40ef5ef4dc99451b2df3fd3/src/adapter/adapters/anthropic/ant_reasoning.rs)
- [async-openai 0.42.1 docs](https://docs.rs/async-openai/0.42.1/async_openai/)
- [SKG Codex docs](https://docs.rs/skg-provider-codex/0.4.1/skg_provider_codex/)
- [Tau Codex docs](https://docs.rs/dpc-tau-provider-codex/0.2.1/tau_provider_codex/)
- [OpenAI official/community SDK list](https://developers.openai.com/api/docs/libraries)
- [Anthropic official SDK list](https://platform.claude.com/docs/en/api/client-sdks)
- [Official SIWC open-source overview](https://developers.openai.com/siwc/token-sharing-open-source)
- [Official SIWC models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [Official SIWC preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
- [Official SIWC devkit](https://github.com/openai/sign-in-with-chatgpt-devkit)
- [Anthropic authentication and credential policy](https://code.claude.com/docs/en/legal-and-compliance)

### Research artifacts available locally

- Release snapshot: `(session scratchpad, not kept)/provider-sources/research-registry-data.json`
- GitHub maintenance snapshot: `(session scratchpad, not kept)/provider-sources/research-github-data.json`
- Downloaded crate and npm sources: `(session scratchpad, not kept)/provider-sources`
- Reproducible offline probe: `(session scratchpad, not kept)/spikes/llm-provider-probe`

**Bottom line:** Rig is the existing crate family to adopt if artifactize accepts its documented boundary and the explicit CCDD relaxations above. It is not an out-of-box solution for both requested subscription-auth products; that remains a scope decision, not a missing line of configuration.