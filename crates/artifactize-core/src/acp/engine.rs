//! Unified ACP Chat turn engine (`C-CHAT-ACP-UNIFIED`, `C-ONE-TURN-OWNERSHIP`, `C-ACP-EDGE-ISOLATION`).
//!
//! Coordinates Workspace Trust verification, warm pool reuse (`Same-Child Load` vs
//! `New-Child Cold Load` vs `session/new` + 128 KiB Handoff), one-turn `VerifyBridge` isolation,
//! SQLite single-transaction Permission CAS with pauseable `TURN_TIMEOUT`, cross-chunk secret
//! redaction, and bounded display-event persistence.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant as StdInstant},
};

use rig_core::message::Message;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::{
    agent::{
        self, CHAT_TURN_TIMEOUT,
        error::{Code, Failure},
        session::{
            AcpPermissionEvent, AcpPlanEvent, AcpToolCallEvent, AcpToolUpdateEvent, Delivery,
            DeliveryKind, DeliveryState, HandoffEvent, Kind, Recorder,
        },
    },
    chat::{Outcome, ToolKind, Turn, tools::Tools},
    llm::stdio_mcp::{self, RpcId},
    platform, repository, runs,
    store::{
        Receipts,
        chat::{NewPermission, Owner, PermissionDelivery},
    },
    types::{DeliveryBlock, SessionId},
};

use super::{
    AcpBinding, AgentId, ConfigCategory, ConfigChoice, ConfigOption, ConfigSelection,
    PermissionStatus, PlanStep, Problem, ToolDiff, ToolLocation, ToolStatus, WarmOwner,
    catalog::{self, AgentSpec},
    handoff,
    pool::{Pool, TakeOutcome, WarmChild},
    redactor::StreamRedactor,
    trust::{self, LauncherFingerprint, TrustRequirement, TrustScope},
    verify_bridge::{BridgeCall, VerifyBridge},
    wire::{
        FINAL_ANSWER_BYTES, IncomingPermission, OpenedSession, PromptFrame, SessionEvent,
        StopReason, TURN_DISPLAY_EVENTS, TURN_PERSISTED_BYTES, bound_text,
    },
};

/// Poll interval for cross-window Stop and Permission decisions.
const CAS_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Pre-checked ACP launch context verified BEFORE a Chat turn is accepted into JSONL.
pub(crate) struct VerifiedAcpLaunch {
    pub state: PathBuf,
    pub scope: TrustScope,
    pub spec: AgentSpec,
    pub fingerprint: LauncherFingerprint,
    pub runtime_fingerprint: String,
}

impl VerifiedAcpLaunch {
    /// Re-hash the executable bytes on disk and re-verify Workspace Trust immediately before
    /// spawning or reusing an ACP child in `worktree` (`C-TRUST-BEFORE-WORKTREE`).
    pub(crate) fn reverify_before_exec(&self, worktree: &Path) -> Result<(), Problem> {
        let current =
            LauncherFingerprint::compute(&self.fingerprint.resolved_executable, &self.spec.args)
                .map_err(Problem::Failed)?;
        // The scope is read again from the checkout the worktree holds now: another repository
        // put in its place is not covered by the consent given for the one prepared.
        let identity = repository::identify(worktree);
        let scope = TrustScope::for_worktree(worktree, &identity).map_err(Problem::Failed)?;
        if scope != self.scope {
            return Err(Problem::TrustRequired(Box::new(TrustRequirement {
                agent_id: self.spec.id.clone(),
                agent_name: self.spec.name.clone(),
                disclosure: scope.disclosure().to_owned(),
                scope,
                fingerprint: current,
                reason: format!(
                    "The worktree's checkout changed before {} started; confirm Workspace Trust for it.",
                    self.spec.name
                ),
            })));
        }
        match trust::check_trust(
            Some(&self.state),
            Some(worktree),
            &self.spec.id,
            &self.spec.name,
            &scope,
            &current,
        )
        .map_err(Problem::Failed)?
        {
            Ok(()) if current == self.fingerprint => Ok(()),
            Ok(()) => Err(Problem::TrustRequired(Box::new(TrustRequirement {
                agent_id: self.spec.id.clone(),
                agent_name: self.spec.name.clone(),
                scope: self.scope.clone(),
                fingerprint: current,
                disclosure: self.scope.disclosure().to_owned(),
                reason: format!(
                    "{} executable changed between preparation and launch; re-confirm Workspace Trust.",
                    self.spec.name
                ),
            }))),
            Err(requirement) => Err(Problem::TrustRequired(Box::new(requirement))),
        }
    }
}

/// Verify executable resolution, direct SHA-256 hashing, and explicit Workspace Trust consent
/// before accepting a turn into JSONL or spawning an ACP process in `worktree`.
pub(crate) fn verify_launch(
    state: &Path,
    worktree: &Path,
    identity: &repository::Identity,
    agent_id: &AgentId,
) -> Result<Result<VerifiedAcpLaunch, TrustRequirement>, String> {
    let spec = catalog::resolve_spec(agent_id, Some(state), Some(worktree))?;
    let resolved = trust::resolve_executable(
        &spec.program,
        Some(worktree),
        spec.reject_worktree_executable,
    )?;
    let fingerprint = LauncherFingerprint::compute(&resolved, &spec.args)?;
    let scope = TrustScope::for_worktree(worktree, identity)?;
    match trust::check_trust(
        Some(state),
        Some(worktree),
        &spec.id,
        &spec.name,
        &scope,
        &fingerprint,
    )? {
        Ok(()) => {
            // A warm child or a remote session started with other variables (a secret since
            // replaced, say) is not this launch's: their digest is part of the fingerprint.
            let runtime_fingerprint = fingerprint
                .runtime_fingerprint(catalog::auth_generation(), &spec.environment_digest());
            Ok(Ok(VerifiedAcpLaunch {
                state: state.to_path_buf(),
                scope,
                spec,
                fingerprint,
                runtime_fingerprint,
            }))
        }
        Err(requirement) => Ok(Err(requirement)),
    }
}

/// Result returned to `Chat::finish` so `finish_chat` can atomically advance `settled_send`
/// and `warm_owner` with `generation` CAS.
pub(crate) struct AcpTurnOutcome {
    pub result: Result<String, Failure>,
    pub generation: u64,
    pub warm_owner: Option<WarmOwner>,
    /// Whether the agent's session ended quiescent, so a later turn may resume it with
    /// `session/load`; a turn that ended with a Verify call still running is not.
    pub resumable: bool,
}

/// Inputs required to execute one ACP Chat turn.
pub(crate) struct AcpTurnInput<'a> {
    pub session_id: &'a SessionId,
    pub worktree: &'a Path,
    pub identity: &'a repository::Identity,
    pub moved_from: Option<&'a Path>,
    pub send: usize,
    pub prior_turns: &'a [Turn],
    pub last_outcome: Option<Outcome>,
    pub previous_engine_label: Option<&'a str>,
    pub user_text: &'a str,
    pub requested_config: &'a [ConfigSelection],
    pub binding: Option<AcpBinding>,
    pub launch: VerifiedAcpLaunch,
    pub context: &'a runs::Context,
    pub tools: &'a Tools<'a>,
    pub receipts: &'a Receipts,
    pub owner: &'a Owner,
    pub pool: &'a Arc<Pool>,
    pub permission_notify: &'a Arc<Notify>,
    pub turn_cancel: &'a CancellationToken,
}

/// Execute one ACP Chat turn with full Trust, Pool, one-turn VerifyBridge, Permission CAS,
/// and secret redaction guarantees.
pub(crate) async fn run_turn(input: AcpTurnInput<'_>, recorder: &mut Recorder) -> AcpTurnOutcome {
    let current_warm_owner = match WarmOwner::current() {
        Ok(owner) => owner,
        Err(error) => {
            return AcpTurnOutcome {
                result: Err(Failure::from(error)),
                resumable: false,
                generation: input.binding.as_ref().map_or(1, |b| b.generation),
                warm_owner: None,
            };
        }
    };

    // A target change since the turn was prepared bumps the stored generation: a warm child is
    // reused, and a remote session loaded, only from the binding as it is stored now.
    let stored = match input.receipts.acp_binding(input.session_id).await {
        Ok(stored) => stored,
        Err(error) => {
            return AcpTurnOutcome {
                result: Err(Failure::new(Code::AgentError, error)),
                resumable: false,
                generation: input.binding.as_ref().map_or(1, |b| b.generation),
                warm_owner: None,
            };
        }
    };
    let mut generation = stored.as_ref().map_or(1, |b| b.generation);
    // The generation the stored binding holds: persisting this turn's session compares it.
    let mut expected_generation = stored.as_ref().map(|b| b.generation);
    let settled_send = stored.as_ref().map_or(0, |b| b.settled_send);
    let mut binding = stored;
    let prior_sends = input.send.saturating_sub(1);

    // Check warm pool for Path A candidate (also retires stale / digest-mismatched child).
    let warm_candidate = match input.pool.take_for_turn(
        input.session_id,
        binding.as_ref(),
        current_warm_owner,
        &input.launch.spec.id,
        &input.launch.fingerprint.executable_sha256,
        &input.launch.runtime_fingerprint,
    ) {
        TakeOutcome::Warm(child) => Some(*child),
        TakeOutcome::Retired {
            stale,
            digest_mismatch,
        } => {
            let stale_gen = stale.generation;
            let _ = stale.terminate().await;
            let _ = input
                .receipts
                .clear_acp_warm_owner(input.session_id, stale_gen, current_warm_owner)
                .await;
            if digest_mismatch {
                if let Ok(bumped) = input.receipts.bump_acp_generation(input.session_id).await {
                    generation = bumped;
                    expected_generation = expected_generation.map(|_| bumped);
                }
                binding = None;
            }
            None
        }
        TakeOutcome::Absent => None,
    };

    // Determine whether `session/load` is permitted under Path A (Same-Child) or Path B (New-Child).
    let common_load_ok = binding.as_ref().is_some_and(|b| {
        b.remote_session_id.is_some()
            && b.settled_send == prior_sends
            && input.last_outcome == Some(Outcome::Completed)
            && b.agent_id == input.launch.spec.id
            && b.worktree == input.worktree
            && b.checkout == *input.identity
            && b.runtime_fingerprint == input.launch.runtime_fingerprint
    });

    let mut child: Option<WarmChild> = None;
    let mut load_remote_id: Option<String> = None;

    if let Some(warm) = warm_candidate {
        let same_child_ok = common_load_ok
            && input.launch.spec.supports_same_child_load
            && binding
                .as_ref()
                .is_some_and(|b| b.warm_owner == Some(current_warm_owner));
        if same_child_ok {
            load_remote_id = binding.as_ref().and_then(|b| b.remote_session_id.clone());
        }
        child = Some(warm);
    } else if let Some(b) = &binding {
        match b.warm_owner {
            None if common_load_ok => {
                // Path B: New-Child Cold Load (previous child cleanly exited/evicted).
                load_remote_id = b.remote_session_id.clone();
            }
            Some(_) => {
                // Another live process or dead app process owned the warm child:
                // refuse session/load, increment generation, and open session/new + Handoff.
                if let Ok(bumped) = input.receipts.bump_acp_generation(input.session_id).await {
                    generation = bumped;
                    expected_generation = expected_generation.map(|_| bumped);
                }
            }
            None => {}
        }
    }

    // Reusing a warm child or loading its session needs the binding they come from to be this
    // turn's: claim its generation first. A target change since the turn read it fails the claim,
    // so neither is used and the turn opens a session of its own.
    if child.is_some() || load_remote_id.is_some() {
        let claimed = match expected_generation {
            Some(expected) => input
                .receipts
                .claim_acp_generation(input.session_id, expected)
                .await
                .ok()
                .flatten(),
            None => None,
        };
        if let Some(claimed) = claimed {
            generation = claimed;
            expected_generation = Some(claimed);
            if let Some(warm) = child.as_mut() {
                warm.generation = claimed;
            }
        } else {
            if let Some(warm) = child.take() {
                let _ = warm.terminate().await;
            }
            load_remote_id = None;
            let stored = input
                .receipts
                .acp_binding(input.session_id)
                .await
                .ok()
                .flatten();
            generation = stored.as_ref().map_or(1, |b| b.generation);
            expected_generation = stored.as_ref().map(|b| b.generation);
        }
    }

    // Open a fresh one-turn Verify MCP bridge (disarmed until `session/prompt`).
    let bridge = match VerifyBridge::start(stdio_mcp::Server::new(input.tools.verify_only_list())) {
        Ok(bridge) => bridge,
        Err(error) => {
            if let Some(warm) = child {
                let _ = warm.terminate().await;
            }
            return AcpTurnOutcome {
                result: Err(error),
                resumable: false,
                generation,
                warm_owner: None,
            };
        }
    };

    let mcp_servers = if input.launch.spec.mcp_stdio {
        vec![bridge.mcp_server(input.context.executable())]
    } else {
        Vec::new()
    };

    // Re-verify executable hash & Workspace Trust right before warm reuse or fresh spawn.
    let (mut child, is_fresh_child) = match child {
        Some(warm) => {
            if let Err(problem) = input.launch.reverify_before_exec(input.worktree) {
                let _ = warm.terminate().await;
                bridge.stop().await;
                return AcpTurnOutcome {
                    result: Err(problem.into()),
                    resumable: false,
                    generation,
                    warm_owner: None,
                };
            }
            (warm, false)
        }
        None => {
            match WarmChild::spawn(
                input.session_id.clone(),
                &input.launch,
                generation,
                input.worktree,
            )
            .await
            {
                Ok(spawned) => (spawned, true),
                Err(problem) => {
                    bridge.stop().await;
                    return AcpTurnOutcome {
                        result: Err(problem.into()),
                        resumable: false,
                        generation,
                        warm_owner: None,
                    };
                }
            }
        }
    };

    let redactor = StreamRedactor::new(input.launch.spec.known_secrets());

    let turn_res = execute_on_child(
        &input,
        &mut child,
        is_fresh_child,
        load_remote_id.as_deref(),
        &mcp_servers,
        bridge,
        generation,
        current_warm_owner,
        (expected_generation, settled_send),
        redactor.clone(),
        recorder,
    )
    .await;

    match turn_res {
        Ok(completed) => {
            if completed.can_park_warm {
                child.mark_quiescent(Some(completed.remote_session_id));
                let evicted = input.pool.park(child);
                for victim in evicted {
                    let v_id = victim.session_id.clone();
                    let v_gen = victim.generation;
                    let _ = victim.terminate().await;
                    let _ = input
                        .receipts
                        .clear_acp_warm_owner(&v_id, v_gen, current_warm_owner)
                        .await;
                }
                AcpTurnOutcome {
                    result: Ok(completed.answer),
                    generation,
                    warm_owner: Some(current_warm_owner),
                    resumable: true,
                }
            } else {
                let _ = child.terminate().await;
                AcpTurnOutcome {
                    result: Ok(completed.answer),
                    resumable: false,
                    generation,
                    warm_owner: None,
                }
            }
        }
        Err((failure, stderr_classify)) => {
            let stderr_bytes = child.terminate().await;
            let mut final_failure = if stderr_classify && failure.code == Code::ProviderError {
                let stderr_text = platform::output_text(&stderr_bytes);
                let redacted_stderr = redactor.redact_all(&stderr_text);
                if redacted_stderr.contains(super::wire::AUTH_LINK_MARKER) {
                    Problem::SignedOut {
                        agent: input.launch.spec.name.clone(),
                        hint: input.launch.spec.install_hint.clone(),
                    }
                    .into()
                } else {
                    Problem::classify_text(
                        &input.launch.spec.name,
                        &input.launch.spec.install_hint,
                        &redacted_stderr,
                    )
                    .map(Failure::from)
                    .unwrap_or(failure)
                }
            } else {
                failure
            };
            final_failure.message = redactor.redact_all(&final_failure.message);
            AcpTurnOutcome {
                result: Err(final_failure),
                resumable: false,
                generation,
                warm_owner: None,
            }
        }
    }
}

struct CompletedTurn {
    answer: String,
    remote_session_id: String,
    can_park_warm: bool,
}

#[expect(
    clippy::too_many_arguments,
    reason = "Turn execution binds the prepared ACP child, bridge, CAS receipts and recorder in one linear lifecycle"
)]
async fn execute_on_child(
    input: &AcpTurnInput<'_>,
    child: &mut WarmChild,
    is_fresh_child: bool,
    load_remote_id: Option<&str>,
    mcp_servers: &[super::wire::McpServer],
    mut bridge: VerifyBridge,
    generation: u64,
    current_warm_owner: WarmOwner,
    (expected_prev_gen, settled_send): (Option<u64>, usize),
    redactor: StreamRedactor,
    recorder: &mut Recorder,
) -> Result<CompletedTurn, (Failure, bool)> {
    let cancel = input.turn_cancel;
    if cancel.is_cancelled() {
        bridge.stop().await;
        return Err((Failure::cancelled(), false));
    }

    // 1. Initialize if fresh child.
    let capabilities = if is_fresh_child {
        let init = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                bridge.stop().await;
                return Err((Failure::cancelled(), false));
            }
            res = child.connection.initialize() => res.map_err(|p| (Failure::from(p), true))?,
        };
        init.capabilities
    } else {
        child.connection.capabilities().cloned().unwrap_or_default()
    };

    // 2. Open session: try `session/load` when eligible, otherwise `session/new` (+ Handoff).
    let mut used_load = false;
    let mut load_fallback_reason: Option<String> = None;
    let opened: OpenedSession = if let Some(remote_id) = load_remote_id
        && capabilities.load_session
    {
        let attempt = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                bridge.stop().await;
                return Err((Failure::cancelled(), false));
            }
            res = child.connection.session_load(remote_id, input.worktree, mcp_servers) => res,
        };
        match attempt {
            Ok(session) => {
                used_load = true;
                session
            }
            Err(
                problem @ (Problem::SignedOut { .. }
                | Problem::Quota { .. }
                | Problem::RateLimit { .. }),
            ) => {
                bridge.stop().await;
                return Err((Failure::from(problem), true));
            }
            Err(problem) => {
                load_fallback_reason = Some(redactor.redact_all(&Failure::from(problem).message));
                // Fall back to `session/new` + Handoff cleanly on the same child and surface the reset explicitly.
                let sys = capabilities
                    .system_prompt_append
                    .then_some(handoff::ACP_CHAT_GUARDRAILS);
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {
                        bridge.stop().await;
                        return Err((Failure::cancelled(), false));
                    }
                    res = child.connection.session_new(input.worktree, mcp_servers, sys) => {
                        res.map_err(|p| (Failure::from(p), true))?
                    }
                }
            }
        }
    } else {
        let sys = capabilities
            .system_prompt_append
            .then_some(handoff::ACP_CHAT_GUARDRAILS);
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                bridge.stop().await;
                return Err((Failure::cancelled(), false));
            }
            res = child.connection.session_new(input.worktree, mcp_servers, sys) => {
                res.map_err(|p| (Failure::from(p), true))?
            }
        }
    };

    // Persist `remote_session_id` with `generation` CAS.
    let updated_binding = AcpBinding {
        agent_id: input.launch.spec.id.clone(),
        worktree: input.worktree.to_path_buf(),
        checkout: input.identity.clone(),
        remote_session_id: Some(opened.session_id.clone()),
        generation,
        settled_send,
        runtime_fingerprint: input.launch.runtime_fingerprint.clone(),
        warm_owner: Some(current_warm_owner),
    };
    // A failed CAS means the binding changed since this turn read it, a target change among
    // others: the session it opened is not this Chat's any more, so nothing is prompted on it.
    match input
        .receipts
        .save_acp_binding_cas(input.session_id, expected_prev_gen, updated_binding)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            bridge.stop().await;
            return Err((
                Failure::new(
                    Code::AgentError,
                    "This Chat's agent changed while the turn was starting; send the message again."
                        .to_owned(),
                ),
                false,
            ));
        }
        Err(error) => {
            bridge.stop().await;
            return Err((Failure::new(Code::AgentError, error), false));
        }
    }

    // 3. Apply configOptions in dependency order (model -> mode -> thought_level -> rest).
    // Aborts on mode mismatch before `session/prompt` (never silently downgrades approval level).
    let applied = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            bridge.stop().await;
            return Err((Failure::cancelled(), false));
        }
        res = child.connection.apply_config_options(&opened, input.requested_config) => {
            res.map_err(|p| (Failure::from(p), false))?
        }
    };
    for notice in &applied.notices {
        recorder.event(Kind::Notice {
            text: redactor.redact_all(notice),
        });
    }
    if let Some(reason) = load_fallback_reason {
        let notice = if input.prior_turns.is_empty() {
            format!(
                "Could not resume prior {} session ({reason}); started a fresh ACP session.",
                input.launch.spec.name
            )
        } else {
            format!(
                "Could not resume prior {} session ({reason}); started a fresh ACP session with a deterministic summary of {} prior turn(s).",
                input.launch.spec.name,
                input.prior_turns.len()
            )
        };
        recorder.event(Kind::Notice { text: notice });
    }
    let _ = input
        .receipts
        .update_chat_turn_meta(
            input.session_id,
            &input.owner.token,
            redact_options(&applied.live_options, &redactor),
            bridge.state(),
        )
        .await;

    // 4. Build prompt: cold starts inject deterministic 128 KiB Handoff (redacted) & record Handoff event.
    let prompt_text = if used_load {
        input.user_text.to_owned()
    } else {
        let handoff_text =
            handoff::build_handoff(input.prior_turns, input.moved_from, input.worktree)
                .map(|text| redactor.redact_all(&text));
        if let Some(prev_label) = input.previous_engine_label
            && prev_label != input.launch.spec.name
            && !input.prior_turns.is_empty()
        {
            recorder.event(Kind::Handoff(HandoffEvent {
                from: prev_label.to_owned(),
                to: input.launch.spec.name.clone(),
                summary: format!(
                    "Continued conversation in {} with a deterministic summary of {} prior turn(s).",
                    input.launch.spec.name,
                    input.prior_turns.len()
                ),
            }));
        }
        handoff::compose_prompt(
            input.user_text,
            handoff_text.as_deref(),
            !capabilities.system_prompt_append,
        )
    };

    // 5. Arm the one-turn Verify bridge and issue `session/prompt`.
    bridge.arm_turn();
    let prompt_req_id = match child
        .connection
        .start_prompt(&opened.session_id, &prompt_text)
        .await
    {
        Ok(id) => id,
        Err(problem) => {
            bridge.stop().await;
            return Err((Failure::from(problem), true));
        }
    };

    // 6. Main event loop: streams updates, handles VerifyBridge tool calls, handles SQLite
    //    Permission CAS with pauseable TURN_TIMEOUT, and enforces persistence bounds.
    let mut text_redactor = redactor.clone();
    let mut thought_redactor = redactor.clone();
    let mut state = TurnAccumulator::new(input.launch.spec.name.clone());
    let mut pending_perms: BTreeMap<String, PendingPermissionSlot> = BTreeMap::new();
    let mut remaining_budget = CHAT_TURN_TIMEOUT;
    let mut budget_started: Option<StdInstant> = Some(StdInstant::now());
    let mut cas_poll = tokio::time::interval(CAS_POLL_INTERVAL);
    cas_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_bridge_state = bridge.state();
    // What the Chat view shows of the agent's options: redacted, as everything it sent is.
    let mut live_options = redact_options(&applied.live_options, &redactor);
    let chosen = applied.effective_selections;

    let loop_result: Result<StopReason, Failure> = loop {
        // Pause TURN_TIMEOUT while any PENDING permission row is open.
        if pending_perms.is_empty() {
            if budget_started.is_none() {
                budget_started = Some(StdInstant::now());
            }
        } else if let Some(started) = budget_started.take() {
            remaining_budget = remaining_budget.saturating_sub(started.elapsed());
        }

        let sleep_deadline = budget_started.map(|started| {
            let left = remaining_budget.saturating_sub(started.elapsed());
            tokio::time::Instant::now() + left
        });

        let timeout_fut = async {
            match sleep_deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending::<()>().await,
            }
        };

        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                break Err(Failure::cancelled());
            }
            () = timeout_fut => {
                break Err(Failure::timeout());
            }
            () = input.permission_notify.notified(), if !pending_perms.is_empty() => {
                if let Err(error) = process_pending_permissions(
                    input,
                    &mut child.connection,
                    &mut pending_perms,
                    recorder,
                ).await {
                    break Err(error);
                }
            }
            _ = cas_poll.tick() => {
                if input.receipts.chat_stop_requested(input.session_id, input.owner).await == Ok(true) {
                    cancel.cancel();
                    break Err(Failure::cancelled());
                }
                if !pending_perms.is_empty()
                    && let Err(error) = process_pending_permissions(
                        input,
                        &mut child.connection,
                        &mut pending_perms,
                        recorder,
                    ).await
                {
                    break Err(error);
                }
                let current_bridge_state = bridge.state();
                if current_bridge_state != last_bridge_state {
                    last_bridge_state = current_bridge_state;
                    let _ = input.receipts.update_chat_turn_meta(
                        input.session_id,
                        &input.owner.token,
                        live_options.clone(),
                        current_bridge_state,
                    ).await;
                }
            }
            call = bridge.next() => {
                match call {
                    Some(call) => {
                        let current_bridge_state = bridge.state();
                        if current_bridge_state != last_bridge_state {
                            last_bridge_state = current_bridge_state;
                            let _ = input.receipts.update_chat_turn_meta(
                                input.session_id,
                                &input.owner.token,
                                live_options.clone(),
                                current_bridge_state,
                            ).await;
                        }
                        if let Err(error) = handle_bridge_call(
                            input,
                            call,
                            &redactor,
                            &mut state,
                            recorder,
                            cancel,
                        ).await {
                            break Err(error);
                        }
                    }
                    None => {
                        break Err(Failure::from("The ACP Verify MCP bridge stopped unexpectedly.".to_owned()));
                    }
                }
            }
            frame = child.connection.next_prompt_frame(prompt_req_id, &opened.session_id) => {
                match frame {
                    Ok(PromptFrame::Update(event)) => {
                        // The turn runs under the mode the user chose, or not at all.
                        let changed = match &event {
                            SessionEvent::ConfigOptionsUpdated { options } => chosen_modes_stand(&chosen, options),
                            SessionEvent::ModeChanged { mode_id } => chosen_mode_is(&chosen, mode_id),
                            _ => Ok(()),
                        };
                        if let Err(problem) = changed {
                            break Err(Failure::from(problem));
                        }
                        if let SessionEvent::ConfigOptionsUpdated { options } = &event {
                            live_options = redact_options(options, &redactor);
                            let _ = input.receipts.update_chat_turn_meta(
                                input.session_id,
                                &input.owner.token,
                                live_options.clone(),
                                bridge.state(),
                            ).await;
                        }
                        state.apply_event(
                            event,
                            &redactor,
                            &mut text_redactor,
                            &mut thought_redactor,
                            recorder,
                        );
                    }
                    Ok(PromptFrame::Permission(req)) => {
                        if let Err(error) = admit_permission(
                            input,
                            &mut child.connection,
                            req,
                            &redactor,
                            &mut pending_perms,
                            recorder,
                        ).await {
                            break Err(error);
                        }
                    }
                    Ok(PromptFrame::Finished(reason)) => {
                        break Ok(reason);
                    }
                    Err(problem) => {
                        break Err(Failure::from(problem));
                    }
                }
            }
        }
    };

    // Flush trailing redactor windows and record final assistant prose in `Kind::Message`.
    state.flush_all(&mut text_redactor, &mut thought_redactor, recorder);

    // Disarm, then count: a call is counted before it checks the turn (verify_bridge), so one
    // that got in is counted here and one that did not is refused. Any call still in flight when
    // a clean StopReason arrives is a protocol desync, and the child is not kept warm.
    bridge.disarm_turn();
    let in_flight_at_end = bridge.in_flight_calls();

    // Cancel any leftover pending permissions before draining or stopping: their rows first,
    // so no responder can select one the agent is then told was cancelled.
    if !pending_perms.is_empty() {
        let _ = input
            .receipts
            .cancel_open_chat_permissions(input.session_id, &input.owner.token)
            .await;
    }
    for (_, slot) in std::mem::take(&mut pending_perms) {
        let _ = child
            .connection
            .send_permission_cancelled(slot.rpc_id)
            .await;
        recorder.event(Kind::AcpPermission(AcpPermissionEvent {
            permission_id: slot.permission_id,
            title: slot.title,
            tool_kind: slot.tool_kind,
            summary: slot.summary,
            detail: slot.detail,
            options: slot.options,
            status: PermissionStatus::Cancelled,
            selected_option_id: None,
        }));
    }

    // Immediately retire the one-turn bridge listener and token before any next turn can arm.
    bridge.stop().await;

    match loop_result {
        Ok(StopReason::EndTurn) => {
            if state.answer.is_empty() {
                return Err((
                    Failure::new(
                        Code::ProviderError,
                        format!("{} ended without a Chat answer.", input.launch.spec.name),
                    ),
                    true,
                ));
            }
            if let Err(error) = recorder.check_saved() {
                return Err((Failure::from(error), false));
            }
            // Redacted whole: a secret split across text on either side of a tool call joins
            // again here.
            let answer = redactor.redact_all(&state.answer);
            let (bounded_answer, _) = bound_text(&answer, FINAL_ANSWER_BYTES);
            let can_park_warm = in_flight_at_end == 0;
            Ok(CompletedTurn {
                answer: bounded_answer,
                remote_session_id: opened.session_id,
                can_park_warm,
            })
        }
        Ok(StopReason::Cancelled) => Err((Failure::cancelled(), false)),
        Ok(StopReason::Refusal) => Err((
            Failure::from(Problem::Quota {
                agent: input.launch.spec.name.clone(),
            }),
            false,
        )),
        Ok(StopReason::MaxTokens | StopReason::MaxTurnRequests) => Err((
            Failure::new(
                Code::ProviderBudgetExceeded,
                format!(
                    "{} ended the turn at its output or request limit.",
                    input.launch.spec.name
                ),
            ),
            false,
        )),
        Ok(StopReason::Other) => Err((
            Failure::new(
                Code::ProviderError,
                format!("{} ended without a Chat answer.", input.launch.spec.name),
            ),
            true,
        )),
        Err(failure) => {
            if failure.code == Code::Cancelled {
                child.connection.send_cancel(&opened.session_id).await;
                let _ = child
                    .connection
                    .drain_after_cancel(prompt_req_id, &opened.session_id)
                    .await;
            }
            Err((failure, true))
        }
    }
}

struct PendingPermissionSlot {
    permission_id: String,
    rpc_id: RpcId,
    title: String,
    tool_kind: ToolKind,
    summary: String,
    detail: Option<String>,
    options: Vec<super::PermissionOption>,
    /// The ID shown for each option, paired with the agent's own, which only the reply carries.
    agent_option_ids: Vec<(String, String)>,
}

async fn admit_permission(
    input: &AcpTurnInput<'_>,
    conn: &mut super::wire::Connection,
    req: IncomingPermission,
    redactor: &StreamRedactor,
    pending: &mut BTreeMap<String, PendingPermissionSlot>,
    recorder: &mut Recorder,
) -> Result<(), Failure> {
    let permission_id = agent::uuid()?;
    let title = redactor.redact_all(&req.title);
    let summary = redactor.redact_all(&req.summary);
    let detail = req.detail.as_deref().map(|d| redactor.redact_all(d));
    // What the transcript and the store keep is redacted, IDs included; an ID that redaction
    // makes equal to an earlier one gets its position, so each option stays its own.
    let mut agent_option_ids: Vec<(String, String)> = Vec::new();
    let options: Vec<_> = req
        .options
        .into_iter()
        .enumerate()
        .map(|(index, option)| {
            let mut shown = redactor.redact_all(&option.option_id);
            if agent_option_ids
                .iter()
                .any(|(earlier, _)| *earlier == shown)
            {
                shown = format!("{shown}#{}", index + 1);
            }
            agent_option_ids.push((shown.clone(), option.option_id.clone()));
            super::PermissionOption {
                option_id: shown,
                name: redactor.redact_all(&option.name),
                ..option
            }
        })
        .collect();
    let rpc_request_id = match &req.rpc_id {
        RpcId::Number(n) => n.to_string(),
        RpcId::Text(s) => s.clone(),
    };
    // A request with no option left is cancelled at once, with no row to clean up later.
    let admitted = !options.is_empty()
        && input
            .receipts
            .insert_chat_permission(NewPermission {
                session_id: input.session_id.clone(),
                owner_token: input.owner.token.clone(),
                permission_id: permission_id.clone(),
                rpc_request_id,
                title: title.clone(),
                tool_kind: req.tool_kind,
                summary: summary.clone(),
                detail: detail.clone(),
                options: options.clone(),
                created_at: crate::broker::now(),
            })
            .await
            .map_err(Failure::from)?;

    if !admitted {
        let _ = conn.send_permission_cancelled(req.rpc_id).await;
        recorder.event(Kind::AcpPermission(AcpPermissionEvent {
            permission_id,
            title,
            tool_kind: req.tool_kind,
            summary,
            detail,
            options,
            status: PermissionStatus::Cancelled,
            selected_option_id: None,
        }));
        recorder.notify();
        return Ok(());
    }

    recorder.event(Kind::AcpPermission(AcpPermissionEvent {
        permission_id: permission_id.clone(),
        title: title.clone(),
        tool_kind: req.tool_kind,
        summary: summary.clone(),
        detail: detail.clone(),
        options: options.clone(),
        status: PermissionStatus::Pending,
        selected_option_id: None,
    }));
    recorder.notify();

    pending.insert(
        permission_id.clone(),
        PendingPermissionSlot {
            permission_id,
            rpc_id: req.rpc_id,
            title,
            tool_kind: req.tool_kind,
            summary,
            detail,
            options,
            agent_option_ids,
        },
    );
    Ok(())
}

async fn process_pending_permissions(
    input: &AcpTurnInput<'_>,
    conn: &mut super::wire::Connection,
    pending: &mut BTreeMap<String, PendingPermissionSlot>,
    recorder: &mut Recorder,
) -> Result<(), Failure> {
    let ids: Vec<String> = pending.keys().cloned().collect();
    for perm_id in ids {
        let Some((rpc_id, agent_ids)) = pending
            .get(&perm_id)
            .map(|slot| (slot.rpc_id.clone(), slot.agent_option_ids.clone()))
        else {
            continue;
        };
        let mut problem = None;
        let delivery = input
            .receipts
            .deliver_chat_permission(
                input.session_id,
                &input.owner.token,
                &perm_id,
                |option_id| async {
                    // The agent hears its own ID back for the option the person chose.
                    let agent_id = agent_ids
                        .into_iter()
                        .find(|(shown, _)| *shown == option_id)
                        .map_or(option_id, |(_, agent)| agent);
                    match conn.send_permission_selected(rpc_id, &agent_id).await {
                        Ok(()) => true,
                        Err(error) => {
                            problem = Some(error);
                            false
                        }
                    }
                },
            )
            .await
            .map_err(Failure::from)?;
        let (status, selected_option_id) = match delivery {
            PermissionDelivery::StillPending => continue,
            PermissionDelivery::Delivered { option_id } => {
                (PermissionStatus::Delivered, Some(option_id))
            }
            PermissionDelivery::Expired { option_id } => {
                (PermissionStatus::Expired, Some(option_id))
            }
            PermissionDelivery::Cancelled => (PermissionStatus::Cancelled, None),
        };
        let Some(slot) = pending.remove(&perm_id) else {
            continue;
        };
        if status == PermissionStatus::Cancelled {
            let _ = conn.send_permission_cancelled(slot.rpc_id).await;
        }
        recorder.event(Kind::AcpPermission(AcpPermissionEvent {
            permission_id: slot.permission_id,
            title: slot.title,
            tool_kind: slot.tool_kind,
            summary: slot.summary,
            detail: slot.detail,
            options: slot.options,
            status,
            selected_option_id,
        }));
        recorder.notify();
        if status == PermissionStatus::Expired {
            return Err(problem.map_or_else(
                || {
                    Failure::from(format!(
                        "{} did not take the permission reply in time.",
                        input.launch.spec.name
                    ))
                },
                Failure::from,
            ));
        }
    }
    Ok(())
}

async fn handle_bridge_call(
    input: &AcpTurnInput<'_>,
    call: BridgeCall,
    redactor: &StreamRedactor,
    state: &mut TurnAccumulator,
    recorder: &mut Recorder,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    let call_id = agent::uuid()?;
    let summary = format!("{} {}", call.name, call.arguments);
    let summary_short = summary.chars().take(120).collect::<String>();
    let redacted_args = redactor.redact_all(&call.arguments.to_string());

    state.record_display(
        recorder,
        Kind::AcpToolCall(AcpToolCallEvent {
            call_id: call_id.clone(),
            name: call.name.clone(),
            title: call.name.clone(),
            tool_kind: ToolKind::Verify,
            status: ToolStatus::Running,
            summary: redactor.redact_all(&summary_short),
            arguments: redacted_args,
            locations: Vec::new(),
            output: None,
            diff: None,
            output_truncated: false,
        }),
    );

    let durable_start = call.name == "verify_start";
    let invoke = input.tools.call_acp_verify(
        input.session_id,
        input.send,
        &call.name,
        call.arguments,
        if durable_start {
            CancellationToken::new()
        } else {
            cancel.clone()
        },
    );
    let called = if durable_start {
        // Once Verify admits a Run, Stop must not suppress its durable `RunReference`.
        invoke.await
    } else {
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(Failure::cancelled()),
            res = invoke => res,
        }
    };

    if let Some(run_id) = called.run {
        recorder.event(Kind::RunReference { run: run_id });
    }
    let failed = called.result.is_error();
    let result_text = serde_json::to_string(&called.result).unwrap_or_default();
    let redacted_output = redactor.redact_all(&result_text);
    let (bounded_output, truncated) = bound_text(&redacted_output, super::wire::TOOL_OUTPUT_BYTES);

    state.record_display(
        recorder,
        Kind::AcpToolUpdate(AcpToolUpdateEvent {
            call_id,
            title: None,
            tool_kind: Some(ToolKind::Verify),
            status: Some(if failed {
                ToolStatus::Failed
            } else {
                ToolStatus::Completed
            }),
            summary: None,
            locations: None,
            output: Some(bounded_output),
            diff: None,
            output_truncated: truncated,
            failed: Some(failed),
            exit_code: None,
        }),
    );

    let reply = stdio_mcp::call_reply(&call.id, called.result);
    let _ = call.reply.send(reply);
    Ok(())
}

/// Tracks streaming prose/thoughts and enforces per-turn display persistence bounds
/// (`TURN_DISPLAY_EVENTS = 2,000`, `TURN_PERSISTED_BYTES = 8 MiB`).
struct TurnAccumulator {
    agent_name: String,
    segment: usize,
    pending_text: String,
    answer: String,
    display_events: usize,
    persisted_bytes: usize,
    cap_noticed: bool,
}

impl TurnAccumulator {
    fn new(agent_name: String) -> Self {
        Self {
            agent_name,
            segment: 0,
            pending_text: String::new(),
            answer: String::new(),
            display_events: 0,
            persisted_bytes: 0,
            cap_noticed: false,
        }
    }

    fn turn_index(&self) -> usize {
        self.segment + 1
    }

    fn record_display(&mut self, recorder: &mut Recorder, kind: Kind) {
        let approx_bytes = serde_json::to_string(&kind).map_or(256, |s| s.len());
        if self.display_events >= TURN_DISPLAY_EVENTS
            || self.persisted_bytes + approx_bytes > TURN_PERSISTED_BYTES
        {
            if !self.cap_noticed {
                self.cap_noticed = true;
                recorder.event(Kind::Notice {
                    text: format!(
                        "{} exceeded the per-turn display cap ({TURN_DISPLAY_EVENTS} events / 8 MiB); further intermediate tool updates are omitted.",
                        self.agent_name
                    ),
                });
            }
            return;
        }
        self.display_events += 1;
        self.persisted_bytes += approx_bytes;
        recorder.event(kind);
    }

    fn push_text(&mut self, chunk: &str, recorder: &mut Recorder) {
        if chunk.is_empty() {
            return;
        }
        if self.answer.len() < FINAL_ANSWER_BYTES {
            let room = FINAL_ANSWER_BYTES.saturating_sub(self.answer.len());
            let (slice, _) = bound_text(chunk, room);
            self.answer.push_str(&slice);
        }
        let remaining_persisted = TURN_PERSISTED_BYTES
            .saturating_sub(self.persisted_bytes.saturating_add(self.pending_text.len()));
        if remaining_persisted > 0 {
            let (slice, _) = bound_text(chunk, remaining_persisted);
            self.pending_text.push_str(&slice);
        }
        let turn = self.turn_index();
        self.record_display(
            recorder,
            Kind::Delivery(Delivery {
                turn,
                attempt: 1,
                block: DeliveryBlock::text(0),
                kind: DeliveryKind::Text,
                text: chunk.to_owned(),
                state: DeliveryState::Delta,
            }),
        );
    }

    fn push_thought(&mut self, chunk: &str, recorder: &mut Recorder) {
        if chunk.is_empty() {
            return;
        }
        let turn = self.turn_index();
        self.record_display(
            recorder,
            Kind::Delivery(Delivery {
                turn,
                attempt: 1,
                block: DeliveryBlock::summary("acp-thought", 0),
                kind: DeliveryKind::Summary,
                text: chunk.to_owned(),
                state: DeliveryState::Delta,
            }),
        );
    }

    fn flush_prose_segment(&mut self, recorder: &mut Recorder) {
        if !self.pending_text.is_empty() {
            let text = std::mem::take(&mut self.pending_text);
            let remaining = TURN_PERSISTED_BYTES.saturating_sub(self.persisted_bytes);
            if remaining == 0 || self.display_events >= TURN_DISPLAY_EVENTS {
                if !self.cap_noticed {
                    self.cap_noticed = true;
                    recorder.event(Kind::Notice {
                        text: format!(
                            "{} exceeded the per-turn display cap ({TURN_DISPLAY_EVENTS} events / 8 MiB); further output is omitted.",
                            self.agent_name
                        ),
                    });
                }
                return;
            }
            let (bounded, truncated) = bound_text(&text, remaining);
            self.display_events += 1;
            self.persisted_bytes = self.persisted_bytes.saturating_add(bounded.len());
            recorder.message(self.turn_index(), &Message::assistant(bounded), false);
            if truncated && !self.cap_noticed {
                self.cap_noticed = true;
                recorder.event(Kind::Notice {
                    text: format!(
                        "{} exceeded the per-turn display cap ({TURN_DISPLAY_EVENTS} events / 8 MiB); further output is omitted.",
                        self.agent_name
                    ),
                });
            }
            self.segment += 1;
        }
    }

    fn flush_all(
        &mut self,
        text_redactor: &mut StreamRedactor,
        thought_redactor: &mut StreamRedactor,
        recorder: &mut Recorder,
    ) {
        let tail_thought = thought_redactor.flush();
        self.push_thought(&tail_thought, recorder);
        let tail_text = text_redactor.flush();
        self.push_text(&tail_text, recorder);
        self.flush_prose_segment(recorder);
    }

    fn apply_event(
        &mut self,
        event: SessionEvent,
        redactor: &StreamRedactor,
        text_redactor: &mut StreamRedactor,
        thought_redactor: &mut StreamRedactor,
        recorder: &mut Recorder,
    ) {
        match event {
            SessionEvent::AgentMessageChunk { text } => {
                let tail_thought = thought_redactor.release();
                self.push_thought(&tail_thought, recorder);
                let safe = text_redactor.push(&text);
                self.push_text(&safe, recorder);
            }
            SessionEvent::AgentThoughtChunk { text } => {
                let safe = thought_redactor.push(&text);
                self.push_thought(&safe, recorder);
            }
            SessionEvent::ToolCall {
                call_id,
                title,
                tool_kind,
                status,
                summary,
                locations,
                output,
                diff,
                output_truncated,
            } => {
                let tail_text = text_redactor.release();
                self.push_text(&tail_text, recorder);
                self.flush_prose_segment(recorder);
                let redacted_diff = diff.map(|d| redact_diff(d, redactor));
                let redacted_output = output.as_deref().map(|o| redactor.redact_all(o));
                let title = redactor.redact_all(&title);
                self.record_display(
                    recorder,
                    Kind::AcpToolCall(AcpToolCallEvent {
                        call_id: redactor.redact_all(&call_id),
                        name: title.clone(),
                        title,
                        tool_kind,
                        status,
                        summary: redactor.redact_all(&summary),
                        arguments: String::new(),
                        locations: redact_locations(locations, redactor),
                        output: redacted_output,
                        diff: redacted_diff,
                        output_truncated,
                    }),
                );
            }
            SessionEvent::ToolCallUpdate {
                call_id,
                title,
                tool_kind,
                status,
                summary,
                locations,
                output,
                diff,
                output_truncated,
            } => {
                let redacted_diff = diff.map(|d| redact_diff(d, redactor));
                let redacted_output = output.as_deref().map(|o| redactor.redact_all(o));
                let failed = status.map(|s| s == ToolStatus::Failed);
                self.record_display(
                    recorder,
                    Kind::AcpToolUpdate(AcpToolUpdateEvent {
                        call_id: redactor.redact_all(&call_id),
                        title: title.as_deref().map(|t| redactor.redact_all(t)),
                        tool_kind,
                        status,
                        summary: summary.as_deref().map(|s| redactor.redact_all(s)),
                        locations: locations.map(|locations| redact_locations(locations, redactor)),
                        output: redacted_output,
                        diff: redacted_diff,
                        output_truncated,
                        failed,
                        exit_code: None,
                    }),
                );
            }
            SessionEvent::Plan { steps } => {
                let steps = steps
                    .into_iter()
                    .map(|step| PlanStep {
                        content: redactor.redact_all(&step.content),
                        priority: step.priority.as_deref().map(|p| redactor.redact_all(p)),
                        ..step
                    })
                    .collect();
                self.record_display(recorder, Kind::AcpPlan(AcpPlanEvent { steps }));
            }
            SessionEvent::Notice { text } => {
                self.record_display(
                    recorder,
                    Kind::Notice {
                        text: redactor.redact_all(&text),
                    },
                );
            }
            SessionEvent::ConfigOptionsUpdated { .. } | SessionEvent::ModeChanged { .. } => {}
        }
    }
}

/// The agent's options as the Chat view and the transcript may hold them: every name, value and
/// description it sent redacted.
pub(super) fn redact_options(
    options: &[ConfigOption],
    redactor: &StreamRedactor,
) -> Vec<ConfigOption> {
    options
        .iter()
        .map(|option| ConfigOption {
            id: redactor.redact_all(&option.id),
            name: redactor.redact_all(&option.name),
            category: option.category,
            current_value: redactor.redact_all(&option.current_value),
            choices: option
                .choices
                .iter()
                .map(|choice| ConfigChoice {
                    value: redactor.redact_all(&choice.value),
                    name: redactor.redact_all(&choice.name),
                    description: choice
                        .description
                        .as_deref()
                        .map(|text| redactor.redact_all(text)),
                    bypass_warning: choice.bypass_warning,
                })
                .collect(),
        })
        .collect()
}

/// The chosen modes, by option ID.
fn chosen_modes(chosen: &[ConfigSelection]) -> impl Iterator<Item = &ConfigSelection> {
    chosen
        .iter()
        .filter(|selection| selection.category == Some(ConfigCategory::Mode))
}

/// Whether every mode the user chose is still current in `options`: an agent may drop or change
/// one mid-turn, and the turn never goes on under another mode.
fn chosen_modes_stand(chosen: &[ConfigSelection], options: &[ConfigOption]) -> Result<(), Problem> {
    for selection in chosen_modes(chosen) {
        let actual = options
            .iter()
            .find(|option| option.id == selection.id && option.category == ConfigCategory::Mode)
            .map(|option| option.current_value.clone());
        if actual.as_deref() != Some(selection.value.as_str()) {
            return Err(Problem::ModeMismatch {
                requested: selection.value.clone(),
                actual,
            });
        }
    }
    Ok(())
}

/// Whether the mode the agent reports switching to is the one the user chose, if they chose one.
fn chosen_mode_is(chosen: &[ConfigSelection], mode_id: &str) -> Result<(), Problem> {
    match chosen_modes(chosen).next() {
        Some(selection) if selection.value != mode_id => Err(Problem::ModeMismatch {
            requested: selection.value.clone(),
            actual: Some(mode_id.to_owned()),
        }),
        _ => Ok(()),
    }
}

fn redact_locations(locations: Vec<ToolLocation>, redactor: &StreamRedactor) -> Vec<ToolLocation> {
    locations
        .into_iter()
        .map(|location| ToolLocation {
            path: redactor.redact_all(&location.path),
            ..location
        })
        .collect()
}

fn redact_diff(diff: ToolDiff, redactor: &StreamRedactor) -> ToolDiff {
    ToolDiff {
        path: redactor.redact_all(&diff.path),
        old_text: diff.old_text.as_deref().map(|t| redactor.redact_all(t)),
        new_text: redactor.redact_all(&diff.new_text),
        truncated: diff.truncated,
    }
}
