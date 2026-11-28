//! The measure records orchestration gives `Measures` (`REQ-AUD-7`, `T-020-31`): what a segment's ending gives, and
//! what a completed reversal gives, built where the ending happens and handed to the writer by
//! [`Segments::write_measures`](super::Segments::write_measures).
//!
//! **A measure is a detection** (`ADR-0009`). Building a record reads what the segment already has and moves nothing,
//! asks no one and appends nothing; a record that can't be written leaves the segment's outcome, its entries and what
//! it moved as they are without it. The records are kept here, each built once with the time of its ending, and
//! handed over by the caller's call: one the log can't take now waits for the next call, one it refuses is dropped
//! and counted ([`Measuring::refused`]).
//!
//! What each ending gives, for the provider the segment ran at, in its role as a provider:
//!
//! - **settled**: a quote against the executed price, the route's operation `amount_out` against what the segment
//!   gave out, in the operation's asset;
//! - **failed** (a failed result, or a withdrawal that failed and cancelled the part): a failure, its `market` absent,
//!   since no part holds a market figure in `M3`;
//! - **refused** by the provider, finally or until a known time: a refusal;
//! - **not handed over** (the platform wouldn't begin it) and every outcome that isn't final: nothing, since neither
//!   says anything of the provider's conduct.
//!
//! And for each answer the operation was evaluated with ([`hear`]): a dry run of the segment's own provider, against
//! how the segment ended, and counted towards its rate of no; a verifier's judgement the operation cites, counted
//! towards the verifier's rate of no, in the **verifier's** role, so a party that is both is measured apart in each
//! (`REQ-AUD-8`). Each answer is counted once for its order, whichever segments cite it.
//!
//! A record cites the entries it rests on: the segment's effect and its result as the provider stated it (a
//! provider's word stays in its stated entry); a reversal's, the confirmation it rests on and the input that
//! recorded it.

use std::collections::{BTreeSet, VecDeque};

use rutent_contracts::core_internal::calls::{AppendFailure, Measures};
use rutent_contracts::core_internal::{
    Measure, MeasureRecord, MeasuredRole, Receipt, Said, SegmentEnd,
};
use rutent_contracts::provider_role::{
    ConversionOutcome, Denomination, JudgementAnswer, ReserveMovementOutcome, ResultBody,
    TransferOutcome,
};
use rutent_contracts::router_api::{Amount, DecisionRecord, FigureValue, Id, Operation, Timestamp};

use super::{SegmentOutcome, journal};

/// What the route's operation priced, to be measured against what the segment came to.
#[derive(Debug, Clone)]
pub(super) struct Priced {
    /// What the operation said arrives, in `asset`.
    pub(super) quoted: Amount,
    /// The asset it arrives in.
    pub(super) asset: Denomination,
}

impl Priced {
    pub(super) fn of(operation: &Operation) -> Self {
        Priced {
            quoted: operation.amount_out.clone(),
            asset: operation.to.asset.clone(),
        }
    }
}

/// An answer the operation was evaluated with: a dry run of its own provider, or a verifier's judgement.
#[derive(Debug, Clone)]
pub(super) struct Heard {
    /// The snapshot figure it is, which counts once for its order.
    figure: Id,
    /// Whose answer it is.
    party: Id,
    /// The role it is measured in.
    role: MeasuredRole,
    /// What it said.
    said: Said,
    /// Whether it is a dry run, measured against how the segment ended too.
    dry_run: bool,
}

/// The answers `operation`'s figures hold in `record`: each dry run of the operation's own provider, in the
/// provider's role, and each verifier's judgement, in the verifier's.
pub(super) fn hear(record: &DecisionRecord, operation: &Operation) -> Vec<Heard> {
    let mut heard = Vec::new();
    for id in operation.figures.iter() {
        let Some(figure) = record.snapshot.figures.iter().find(|f| &f.figure_id == id) else {
            continue;
        };
        match &figure.value {
            FigureValue::DryRun { result, .. }
                if figure.origin.provider() == Some(&operation.provider) =>
            {
                if let Some(said) = dry_run_said(&result.statement) {
                    heard.push(Heard {
                        figure: id.clone(),
                        party: operation.provider.clone(),
                        role: MeasuredRole::Provider,
                        said,
                        dry_run: true,
                    });
                }
            }
            FigureValue::Eligibility { judgement, .. } => {
                if let Some(party) = Id::new(judgement.maker.as_str()) {
                    heard.push(Heard {
                        figure: id.clone(),
                        party,
                        role: MeasuredRole::Verifier,
                        said: match judgement.statement.answer {
                            JudgementAnswer::Yes => Said::Yes,
                            JudgementAnswer::No => Said::No,
                        },
                        dry_run: false,
                    });
                }
            }
            _ => {}
        }
    }
    heard
}

/// What a dry run's result said, or `None` for a result that isn't a dry run's answer.
fn dry_run_said(body: &ResultBody) -> Option<Said> {
    match body {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match r.outcome {
            ReserveMovementOutcome::DryRunAccepted(_) => Some(Said::Yes),
            ReserveMovementOutcome::Refused(_) => Some(Said::No),
            _ => None,
        },
        ResultBody::Transfer(r) => match r.outcome {
            TransferOutcome::DryRunAccepted(_) => Some(Said::Yes),
            TransferOutcome::Refused(_) => Some(Said::No),
            _ => None,
        },
        ResultBody::Conversion(r) => match r.outcome {
            ConversionOutcome::DryRunAccepted(_) => Some(Said::Yes),
            ConversionOutcome::Refused(_) => Some(Said::No),
            _ => None,
        },
        _ => None,
    }
}

/// One segment's ending.
pub(super) struct Ending<'a> {
    /// The order.
    pub(super) order: &'a Id,
    /// The segment.
    pub(super) segment: &'a Id,
    /// The provider it ran at.
    pub(super) provider: &'a Id,
    /// What its operation priced.
    pub(super) priced: &'a Priced,
    /// What it was evaluated with.
    pub(super) heard: &'a [Heard],
    /// How it ended.
    pub(super) outcome: &'a SegmentOutcome,
    /// The entries the records rest on.
    pub(super) cites: Vec<Receipt>,
    /// The entry that holds the answers heard, which their records cite too.
    pub(super) answers_rest_on: Option<Receipt>,
    /// When it ended, by Core's clock.
    pub(super) at: Timestamp,
}

/// The records kept for the writer, and the memory that keeps each ending, answer and reversal to one.
#[derive(Debug, Clone, Default)]
pub(super) struct Measuring {
    pending: VecDeque<MeasureRecord>,
    ended: BTreeSet<(String, String)>,
    answered: BTreeSet<(String, String)>,
    reversed: BTreeSet<(String, String)>,
    refused: usize,
}

impl Measuring {
    /// Keeps the records `ending` gives, once for its segment.
    pub(super) fn ended(&mut self, ending: &Ending<'_>) {
        let key = (
            ending.order.as_str().to_owned(),
            ending.segment.as_str().to_owned(),
        );
        let end = match ending.outcome {
            SegmentOutcome::Settled { .. } => SegmentEnd::Done,
            SegmentOutcome::Failed
            | SegmentOutcome::WithdrawalCancelled
            | SegmentOutcome::Refused(_) => SegmentEnd::Failed,
            _ => return,
        };
        if !self.ended.insert(key) || ending.cites.is_empty() {
            return;
        }
        let measured = match ending.outcome {
            SegmentOutcome::Settled { amount_out, .. } => Measure::Quote {
                quoted: ending.priced.quoted.clone(),
                executed: amount_out.clone(),
                asset: ending.priced.asset.clone(),
            },
            SegmentOutcome::Refused(_) => Measure::Refusal {},
            _ => Measure::Failure { market: None },
        };
        let record = |provider: &Id, role: MeasuredRole, measure: Measure| MeasureRecord {
            provider: provider.clone(),
            role,
            order: ending.order.clone(),
            segment: Some(ending.segment.clone()),
            measure,
            cites: ending.cites.clone(),
            at: ending.at.clone(),
        };
        self.pending
            .push_back(record(ending.provider, MeasuredRole::Provider, measured));
        for heard in ending.heard {
            let figure = (
                ending.order.as_str().to_owned(),
                heard.figure.as_str().to_owned(),
            );
            if !self.answered.insert(figure) {
                continue;
            }
            let mut cites = ending.cites.clone();
            if let Some(entry) = &ending.answers_rest_on
                && !cites.contains(entry)
            {
                cites.push(entry.clone());
            }
            let record = |measure| MeasureRecord {
                cites: cites.clone(),
                ..record(&heard.party, heard.role, measure)
            };
            if heard.dry_run {
                self.pending.push_back(record(Measure::DryRun {
                    said: heard.said,
                    outcome: end,
                }));
            }
            self.pending
                .push_back(record(Measure::Answer { said: heard.said }));
        }
    }

    /// Drops each record waiting for the writer that `written` says the log already holds (`T-020-46`): a Core that
    /// reopens keeps again only what a restart dropped.
    pub(super) fn drop_written(&mut self, written: impl Fn(&MeasureRecord) -> bool) {
        self.pending.retain(|record| !written(record));
    }

    /// Keeps the record of `order`'s `segment`'s completed reversal, once.
    pub(super) fn reversed(
        &mut self,
        order: &Id,
        segment: &Id,
        provider: &Id,
        at: Timestamp,
        cites: Vec<Receipt>,
    ) {
        let key = (order.as_str().to_owned(), segment.as_str().to_owned());
        if cites.is_empty() || !self.reversed.insert(key) {
            return;
        }
        self.pending.push_back(MeasureRecord {
            provider: provider.clone(),
            role: MeasuredRole::Provider,
            order: order.clone(),
            segment: Some(segment.clone()),
            measure: Measure::Reversal {},
            cites,
            at,
        });
    }

    /// Hands the records to `measures`, oldest first, and answers how many it took. A record the log can't take now
    /// is tried again within [`journal::APPEND_TRIES`], then kept, with the ones after it, for the next call; one it
    /// refuses is dropped and counted: it can never be written, and a measure that can't be written changes nothing.
    pub(super) fn write(&mut self, measures: &mut dyn Measures) -> usize {
        let mut written = 0;
        'records: while let Some(record) = self.pending.front() {
            for _ in 0..journal::APPEND_TRIES {
                match measures.measure(record) {
                    Ok(_) => {
                        written += 1;
                        self.pending.pop_front();
                        continue 'records;
                    }
                    Err(AppendFailure::Unavailable) => {}
                    Err(_) => {
                        self.refused += 1;
                        self.pending.pop_front();
                        continue 'records;
                    }
                }
            }
            break;
        }
        written
    }

    /// How many records wait for the writer.
    pub(super) fn waiting(&self) -> usize {
        self.pending.len()
    }

    /// How many records the log refused, which can never be written.
    pub(super) fn refused(&self) -> usize {
        self.refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rutent_contracts::router_api::DecisionRecord;
    use serde_json::json;

    fn id(text: &str) -> Id {
        Id::new(text).unwrap()
    }

    fn receipt(index: usize) -> Receipt {
        serde_json::from_value(json!({
            "log": "log-1",
            "index": index,
            "entry": { "algorithm": "sha-256", "value": format!("{index:064x}") },
        }))
        .unwrap()
    }

    /// The router-api fixture's record, its withdrawal at `reserve-bank-co` citing a verifier's judgement made by
    /// that same party: one party, a provider at the withdrawal and a verifier of the customer.
    fn record_with_its_provider_judging() -> (DecisionRecord, Operation) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../contracts/router-api/fixtures/valid/decision-record-split-of-one-part.json",
        );
        let mut envelope: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let record = &mut envelope["statement"];
        let mut judging = record["snapshot"]["figures"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["figure_id"] == "f-ex-b-eligibility")
            .unwrap()
            .clone();
        judging["figure_id"] = "f-w-eligibility".into();
        judging["origin"] = record["snapshot"]["figures"][0]["origin"].clone();
        judging["value"]["tuple"] = "f-w".into();
        judging["value"]["judgement"]["maker"] = "reserve-bank-co".into();
        record["snapshot"]["figures"]
            .as_array_mut()
            .unwrap()
            .push(judging);
        // Every operation on the withdrawal's tuple cites it, as the contract holds an operation to.
        for candidate in record["candidates"].as_array_mut().unwrap() {
            for part in candidate["parts"].as_array_mut().unwrap() {
                for operation in part["operations"].as_array_mut().unwrap() {
                    let figures = operation["figures"].as_array_mut().unwrap();
                    if figures.contains(&"f-w".into()) {
                        figures.push("f-w-eligibility".into());
                    }
                }
            }
        }
        let record: DecisionRecord = serde_json::from_value(record.clone()).unwrap();
        let operation = record.chosen().unwrap().parts[0].operations[0].clone();
        (record, operation)
    }

    // verifies: REQ-AUD-7
    #[test]
    fn a_verifiers_answer_and_the_same_partys_quote_as_provider_are_measured_apart() {
        let (record, operation) = record_with_its_provider_judging();
        let heard = hear(&record, &operation);
        let (order, segment) = (id("ord-1"), id("seg-1"));
        let priced = Priced::of(&operation);
        let outcome = SegmentOutcome::Settled {
            amount_out: operation.amount_out.clone(),
            evidence: id("ev-1"),
        };
        let mut measuring = Measuring::default();
        measuring.ended(&Ending {
            order: &order,
            segment: &segment,
            provider: &operation.provider,
            priced: &priced,
            heard: &heard,
            outcome: &outcome,
            cites: vec![receipt(1)],
            answers_rest_on: Some(receipt(9)),
            at: Timestamp::new("2026-11-18T10:00:00Z").unwrap(),
        });
        let kept: Vec<_> = measuring.pending.iter().collect();
        let party = "reserve-bank-co";
        // As a provider: its quote, and its dry run, which it answered yes to.
        assert!(kept.iter().any(|r| r.provider.as_str() == party
            && r.role == MeasuredRole::Provider
            && matches!(r.measure, Measure::Quote { .. })));
        let provider_answers = kept
            .iter()
            .filter(|r| {
                r.role == MeasuredRole::Provider && matches!(r.measure, Measure::Answer { .. })
            })
            .count();
        assert_eq!(provider_answers, 1, "its own dry run alone: {kept:?}");
        // As a verifier: its judgement of the customer, and nothing else.
        let verifier: Vec<_> = kept
            .iter()
            .filter(|r| r.role == MeasuredRole::Verifier)
            .collect();
        assert_eq!(verifier.len(), 1, "{verifier:?}");
        assert_eq!(verifier[0].provider.as_str(), party);
        assert_eq!(verifier[0].measure, Measure::Answer { said: Said::Yes });
        // It cites the entry that carries the judgement as well as the segment's own, and the quote cites no answer.
        assert_eq!(verifier[0].cites, vec![receipt(1), receipt(9)]);
        let quote = kept
            .iter()
            .find(|r| matches!(r.measure, Measure::Quote { .. }))
            .unwrap();
        assert_eq!(quote.cites, vec![receipt(1)]);

        // Another segment citing the same answers counts none of them again.
        let again = id("seg-2");
        measuring.ended(&Ending {
            order: &order,
            segment: &again,
            provider: &operation.provider,
            priced: &priced,
            heard: &heard,
            outcome: &outcome,
            cites: vec![receipt(2)],
            answers_rest_on: Some(receipt(9)),
            at: Timestamp::new("2026-11-18T10:00:01Z").unwrap(),
        });
        assert_eq!(
            measuring
                .pending
                .iter()
                .filter(|r| matches!(r.measure, Measure::Answer { .. }))
                .count(),
            2,
            "the provider's dry run and the verifier's judgement, once each"
        );
    }
}
