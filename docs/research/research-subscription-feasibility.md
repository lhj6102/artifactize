# Subscription support feasibility (2026-10-03)

Spikes: spikes/subscription-feasibility (rig-results.txt, http-captures.json, claude-one/two.jsonl, mcp-events.jsonl).

## A. ChatGPT via official Sign in with ChatGPT + rig 0.43 OpenAI Responses — feasible with caveats
- Inference: bearer token to https://api.openai.com/v1/responses through rig's ordinary client; no fork/patch needed.
- Adapter must: set additional_params.store=false; stream; leave max_tokens/temperature unset; use SystemInstructionsPlacement::AllInstructions; treat `Ok` + Length/ContentFilter (response.incomplete) as failure; read response.error.code from the error body (normalized code is None); list models with a raw GET (rig list_models expects {data}, SIWC returns {models}; filter visibility=="list", use slug).
- Login (artifactize-owned): dynamic registration via auth.openai.com authorize (client_id=dynamic_agent_client -> issued oaiapp_ id), PKCE S256, loopback on 127.0.0.1 (no device flow), token exchange/refresh at auth.openai.com/api/accounts/oauth/token, revoke endpoint; access 1h, rotating refresh 30d; 0600 atomic storage; validate ID token via JWKS.
- Risks: eligibility not promised (403 subscription_sharing_user_not_eligible); no real SIWC account tested.

## B. Claude via official `claude` CLI (2.1.287) subprocess — feasible with caveats
- Flags: -p --output-format stream-json --verbose, --model <full id>, --effort, --max-turns, --mcp-config + --strict-mcp-config, --tools "", --allowedTools mcp__..., --permission-mode dontAsk, --system-prompt, --setting-sources "", --disable-slash-commands, --no-session-persistence; settings claudeMdExcludes ["**"], autoMemoryEnabled false, disableAllHooks true, switchModelsOnFlag false. Never --bare (refuses OAuth).
- Verified with 2 real haiku calls: init tools [] / only mcp tool; model id in stream; MCP server enforced budget (call 1 executed, call 2 refused); cwd untouched.
- Caveats: effort may be silently lowered; tools execute before final message_delta (no exact pre-tool identity/usage gate); malformed/unknown calls never reach MCP; repair = second tools-disabled invocation; cancel by process-group SIGTERM (exit 143).
- Policy: allowed for the unmodified binary with the user's own login; artifactize must not collect/store/intermediate Claude tokens or add its own Claude.ai login.
