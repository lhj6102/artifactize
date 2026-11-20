//! Streaming, attempt evidence, budgets, and the review turn's retry lifecycle.
use super::*;

impl Client {
    /// One turn, sent again while an attempt fails with a rate limit or a transient error
    /// before producing any output or usage, or with a failure of the provider's output
    /// ([`Failure::output_failed`]): at any turn, since every request replays the whole
    /// conversation up to its last tool results. A retry waits as long as the provider's
    /// `Retry-After` asks, or a short backoff, within the review's deadline. Every attempt
    /// keeps its counters, and its tokens count against the review's `maxTokens`: an output
    /// failure is not sent again once they reach it. A delivered response that fails
    /// [`validate_response`] for another reason fails its attempt and is not retried.
    pub async fn turn(
        &self,
        request: &CompletionRequest,
        context: Turn<'_>,
        attempts: &mut Vec<Attempt>,
    ) -> Result<CompletionResponse, Failure> {
        self.turn_observed(request, context, attempts, &mut |_| {})
            .await
    }

    /// Optional display delivery leaves the provider/retry/budget contract unchanged.
    pub async fn turn_observed(
        &self,
        request: &CompletionRequest,
        context: Turn<'_>,
        attempts: &mut Vec<Attempt>,
        observe: &mut impl FnMut(crate::agent::session::Delivery),
    ) -> Result<CompletionResponse, Failure> {
        for attempt in 1..=ATTEMPTS {
            if context.cancellation.is_cancelled() {
                return Err(Failure::cancelled());
            }
            if Instant::now() >= context.deadline {
                return Err(Failure::timeout());
            }
            let usage = Arc::new(ReportedUsage::default());
            let observed = AdapterContext::new(
                usage.clone(),
                Subject::default(),
                format!("turn-{}", context.number),
            );
            let mut emitted = false;
            let mut tail = degenerate::Tail::default();
            let sink = delivery::Sink::default();
            // Task-local registration encloses opening AND every poll, not just request creation.
            let (result, partial) = sink
                .scope(async {
                    let cancelled = || Classified::from(Failure::cancelled());
                    let timeout = || Classified::from(Failure::timeout());
                    let deadline = context.deadline;
                    let opened = tokio::select! {
                        biased;
                        _ = context.cancellation.cancelled() => return (Err(cancelled()), None),
                        _ = tokio::time::sleep_until(deadline) => return (Err(timeout()), None),
                        opened = self.stream(request.clone(), observed) => opened,
                    };
                    let mut stream = match opened {
                        Ok(stream) => stream,
                        Err(failure) => return (Err(failure), None),
                    };
                    let mut cadence = tokio::time::interval(delivery::CADENCE);
                    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    let read = loop {
                        tokio::select! {
                            biased;
                            _ = context.cancellation.cancelled() => break Err(cancelled()),
                            _ = tokio::time::sleep_until(deadline) => break Err(timeout()),
                            _ = cadence.tick() => {
                                for event in sink.take(context.number, attempt) {
                                    observe(event);
                                }
                            }
                            item = stream.next() => match item {
                                Some(Ok(item)) => {
                                    emitted = true;
                                    sink.normalized(&item);
                                    if let Item::Event(StreamEvent::Text { text, .. }) = &item
                                        && let Err(failure) = tail.push(text)
                                    {
                                        break Err(Classified::from(failure));
                                    }
                                    if sink.ready() {
                                        for event in sink.take(context.number, attempt) {
                                            observe(event);
                                        }
                                    }
                                }
                                Some(Err(error)) => break Err(self.classify(&error, time::OffsetDateTime::now_utc())),
                                None => break Ok(()),
                            }
                        }
                    };
                    for event in sink.take(context.number, attempt) {
                        observe(event);
                    }
                    let partial = stream.partial();
                    let result = match read {
                        Ok(()) => stream.finish().await.map_err(|error| self.classify(&error, time::OffsetDateTime::now_utc())),
                        Err(error) => Err(error),
                    };
                    let final_response = result.as_ref().ok().unwrap_or(&partial);
                    if result.is_ok() {
                        sink.final_response(&final_response.choice);
                    }
                    for event in sink.take(context.number, attempt) {
                        observe(event);
                    }
                    if let Err(error) = &result {
                        observe(crate::agent::session::Delivery {
                            turn: context.number,
                            attempt,
                            block: crate::types::DeliveryBlock::attempt(),
                            kind: crate::agent::session::DeliveryKind::Text,
                            text: error.message.clone(),
                            state: crate::agent::session::DeliveryState::Interrupted,
                        });
                    }
                    (result, Some(partial))
                })
                .await;
            let mut counters = usage.0.lock().unwrap().clone();
            if let Some(partial) = &partial {
                add_cache_usage(&mut counters, partial);
            }
            if let Ok(response) = &result {
                add_cache_usage(&mut counters, response);
            }
            let used_tokens = counters.values().any(|value| *value > 0);
            // rig totals include cache reads/writes; absent usage is zero only for enforcement.
            let spent = result
                .as_ref()
                .ok()
                .or(partial.as_ref())
                .and_then(|response| response.usage.total_tokens)
                .unwrap_or(0);
            *context.tokens_used = context.tokens_used.saturating_add(spent);
            let result = result.and_then(|response| {
                validate_response(&response, context.model)
                    .map(|()| response)
                    .map_err(Classified::from)
            });
            attempts.push(Attempt {
                turn: context.number,
                attempt,
                usage: counters.into(),
                error: result.as_ref().err().map(|failure| failure.message.clone()),
                error_code: result.as_ref().err().map(|failure| failure.code),
            });
            let failure = match result {
                Ok(response) => return Ok(response),
                Err(failure) => failure,
            };
            // A stream that broke off after its output began was a sound request.
            let failure = if failure.code == Code::Transient && (emitted || used_tokens) {
                Classified {
                    output_failed: true,
                    ..failure
                }
            } else {
                failure
            };
            // A failed output is already paid for; sending the same request again repeats
            // only this turn, and the next answer rarely fails the same way.
            let retry = if failure.output_failed {
                context
                    .max_tokens
                    .is_none_or(|limit| *context.tokens_used < limit)
            } else {
                failure.code.retryable() && !emitted && !used_tokens
            };
            if !retry || attempt == ATTEMPTS {
                return Err(failure.into());
            }
            let wait = failure
                .retry_after
                .unwrap_or(RETRY_BACKOFF_BASE * (1 << (attempt - 1)));
            // A wait too long to add to now is past any deadline.
            if failure.retry_after.is_some()
                && Instant::now()
                    .checked_add(wait)
                    .is_none_or(|end| end >= context.deadline)
            {
                return Err(Failure::new(
                    failure.code,
                    format!(
                        "{}. The provider asked to retry after {:.1} s, past the review deadline.",
                        failure.message.trim_end_matches('.'),
                        wait.as_secs_f64()
                    ),
                ));
            }
            tokio::select! {
                _ = context.cancellation.cancelled() => return Err(Failure::cancelled()),
                _ = tokio::time::sleep_until(context.deadline) => return Err(Failure::timeout()),
                _ = tokio::time::sleep(wait) => {},
            }
        }
        unreachable!()
    }
}

pub struct Turn<'a> {
    pub number: usize,
    /// The model the request asked for; a response from another model fails its attempt.
    pub model: &'a ModelId,
    pub deadline: Instant,
    pub cancellation: &'a CancellationToken,
    /// The review's `maxTokens`, if any.
    pub max_tokens: Option<u64>,
    /// The tokens the review has spent; every attempt of the turn adds its own.
    pub tokens_used: &'a mut u64,
}

fn add_cache_usage(
    counters: &mut std::collections::BTreeMap<String, u64>,
    response: &CompletionResponse,
) {
    insert_counter(
        counters,
        "cacheWriteTokens",
        response.usage.cache_creation_input_tokens,
    );
    insert_counter(
        counters,
        "cacheWrite1hTokens",
        response
            .raw
            .pointer("/usage/cache_creation/ephemeral_1h_input_tokens")
            .and_then(Value::as_u64),
    );
}

/// A delivered response is usable only from the requested model and when it finished. One
/// stopped at the output cap (`Length`) is a failure of the provider's output.
fn validate_response(response: &CompletionResponse, model: &ModelId) -> Result<(), Failure> {
    if response
        .model
        .as_deref()
        .is_some_and(|actual| actual != model.as_str())
    {
        return Err(Failure::new(
            Code::ProviderError,
            format!(
                "Provider model mismatch: requested {:?}, received {:?}.",
                model.as_str(),
                response.model
            ),
        ));
    }
    let finish = response.finish_reason();
    if !matches!(finish, Some(FinishReason::Stop | FinishReason::ToolCalls)) {
        let failure = Failure::new(
            Code::ProviderError,
            format!("Provider response was incomplete: {finish:?}."),
        );
        return Err(if matches!(finish, Some(FinishReason::Length)) {
            failure.of_output()
        } else {
            failure
        });
    }
    Ok(())
}
