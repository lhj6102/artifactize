//! Settle completed tasks and promote or withhold provisional results.
use super::*;

impl<'a> Scheduler<'a, '_> {
    pub(super) async fn settle_provisional(
        &mut self,
        evaluated: &mut evaluated::Evaluated<'a>,
        evidence: &BTreeMap<crate::config::EvalId, Evidence>,
    ) -> Result<(), String> {
        let evaluation = evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
        for request in self.requests.iter_mut() {
            if evaluation.evals[request.eval_id.as_str()].status == EvalStatus::Provisional
                && let crate::store::RequestState::Completed { result, at } = &request.state
            {
                request.state = crate::store::RequestState::Provisional {
                    result: result
                        .clone()
                        .try_into()
                        .expect("provisional evidence is GREEN"),
                    at: *at,
                };
                self.receipts.save_request(request).await?;
            } else if evaluation.evals[request.eval_id.as_str()].status == EvalStatus::Green
                && let crate::store::RequestState::Provisional { result, at } = &request.state
            {
                request.state = crate::store::RequestState::completed(result.clone().into(), *at);
                self.receipts.save_request(request).await?;
            }
        }
        Ok(())
    }

    /// Settle a finished task: a stopped one goes back to waiting or blocked, any other gives
    /// its request's evidence.
    pub(super) async fn settle(
        &mut self,
        result: Result<Result<(usize, Request), String>, tokio::task::JoinError>,
        admissions: &mut [Admission],
        evidence: &mut BTreeMap<crate::config::EvalId, Evidence>,
        stops: &Stops,
    ) -> Result<(), String> {
        let (index, mut request) = crate::task::joined(result).map_err(|e| e.to_string())??;
        let Admission::Running { stop, .. } =
            std::mem::replace(&mut admissions[index], Admission::Idle)
        else {
            unreachable!("a joined task belongs to a running request");
        };
        match stop {
            Some(modes::Stop::Blocked) => {
                admissions[index] = Admission::Blocked;
                let evaluation = self
                    .graph
                    .evaluate_for_run(evidence, self.run.modes.speculate);
                let eval = &evaluation.evals[request.eval_id.as_str()];
                request.state = crate::store::RequestState::pending(
                    crate::types::RequestStatus::Blocked,
                    Some(now()),
                )
                .expect("blocked state");
                request.blocked_by = eval.blocked_by.iter().copied().map(Into::into).collect();
                request.blocked_reason = Some(crate::project::verify::gate_reason(eval));
                self.receipts.block_request(&request).await?;
                self.requests[index] = request;
                return Ok(());
            }
            Some(modes::Stop::Yield) => {
                request.state =
                    crate::store::RequestState::pending(crate::types::RequestStatus::Queued, None)
                        .expect("queued state");
                request.execution_id = None;
                request.started_at = None;
                request.blocked_reason = Some("Speculative work yielded to a ready eval.".into());
                self.receipts.save_request(&request).await?;
                self.requests[index] = request;
                return Ok(());
            }
            None => {}
        }
        if let Some(remote) = &self.remote {
            remote.publish_request(self.receipts, &request).await?;
        }
        let stopped = stops.all();
        if stopped.len() != self.run.stopped_backends.len() {
            self.run.stopped_backends = stopped;
            self.receipts.save_run(self.run).await?;
        }
        if !request.status().is_human_wait() {
            evidence.insert(request.eval_id.clone(), settled(&request));
        }
        self.requests[index] = request;
        Ok(())
    }
}

/// The evidence a request that stopped waiting gives its eval. A sign-off that a newer request
/// for its eval in the same working tree took over is unanswered here, as if nobody had
/// claimed it; any other failure is an operational error.
pub(super) fn settled(request: &Request) -> Evidence {
    match request.status() {
        crate::types::RequestStatus::Green => Evidence::Current(Verdict::Green),
        crate::types::RequestStatus::Red => Evidence::Current(Verdict::Red),
        _ if superseded(request) => Evidence::Deferred,
        _ => Evidence::OperationalError,
    }
}
