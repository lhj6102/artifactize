//! The graph's evaluation and the scheduling order of the evidence they were made from.
use std::{collections::BTreeMap, sync::Arc};

use crate::{
    config::EvalId,
    graph::{Evaluation, Evidence, Graph},
    store::Request,
};

/// What a scheduler pass computed from its evidence, kept until the evidence changes. Most
/// passes change no evidence (a reconcile tick while a sign-off waits, a hint about another
/// Run), and evaluating a large graph is most of what a pass costs, so those passes reuse the
/// last evaluation and order instead of computing them again.
#[derive(Default)]
pub(super) struct Evaluated<'a> {
    evaluation: Option<(Key, Arc<Evaluation<'a>>)>,
    order: Option<(Gathered, Arc<[usize]>)>,
}

/// The evidence a pass has gathered, by eval.
type Gathered = BTreeMap<EvalId, Evidence>;

/// The inputs an evaluation is made from: the evidence and whether errors block.
type Key = (Gathered, bool);

impl<'a> Evaluated<'a> {
    /// `graph`'s evaluation of `evidence` for a Run, as [`Graph::evaluate_for_run`] makes it.
    pub(super) fn evaluation(
        &mut self,
        graph: &'a Graph<'_>,
        evidence: &BTreeMap<EvalId, Evidence>,
        errors_block: bool,
    ) -> Arc<Evaluation<'a>> {
        match &self.evaluation {
            Some(((kept, blocks), evaluation)) if kept == evidence && *blocks == errors_block => {
                evaluation.clone()
            }
            _ => {
                let evaluation = Arc::new(graph.evaluate_for_run(evidence, errors_block));
                self.evaluation = Some(((evidence.clone(), errors_block), evaluation.clone()));
                evaluation
            }
        }
    }

    /// The order a pass considers `requests` in, as [`super::modes::order`] makes it. The
    /// requests of a Run never change, so only the evidence decides it.
    pub(super) fn order(
        &mut self,
        graph: &Graph<'_>,
        requests: &[Request],
        evidence: &BTreeMap<EvalId, Evidence>,
    ) -> Arc<[usize]> {
        match &self.order {
            Some((kept, order)) if kept == evidence => order.clone(),
            _ => {
                let order: Arc<[usize]> = super::modes::order(graph, requests, evidence).into();
                self.order = Some((evidence.clone(), order.clone()));
                order
            }
        }
    }
}
