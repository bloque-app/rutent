//! Core opened again on its disk rebuilds its own state from the log (`T-020-46`, finding 3 of `M3`'s milestone
//! review; `NFR-6`, `INV-8`, `REQ-ORD-8`).
//!
//! The log is Core's durable record. A Core that reopens reads back from it, beside what `T-020-09` already rebuilt
//! (the grading's `Closing::recorded`, the held quotes' `Holds::recorded`, the catalogue's listing and the proven
//! starts):
//!
//! - the transactions it asked under ([`Issued::recorded`]);
//! - the holds reserves gave of its quotes, each kept again from the log and the evidence store (`Holds::reopened`,
//!   `T-020-57`);
//! - intake's orders and acceptances ([`Intake::reopened`]), and policy's sources of the orders' parties from them;
//! - the segments ([`Segments::reopened`]): the orders followed, the effects acted on, each segment handed over and
//!   its outcome, and the value in flight held in [`Exposures`];
//! - the stuck orders ([`Stuck::reopened`]): the declarations, the kept decisions, what moved and the moves in motion;
//! - what each started order needs to run on: its decision record, its plan, its authorisation as the log holds it in
//!   force, its origin and session from its `order_opened`, its inputs from the operator's statement of them that
//!   `order_opened` names by commitment, and what its route delivers; a linked order's from its acceptance, the
//!   authorisation policy registered for it and the first order's route end;
//! - the routes linked orders were accepted on, from their acceptances and the records their routes were chosen by,
//!   under the segment ids `accept_linked` derives;
//!
//! - the hand-ons the log holds half of ([`rutent_core_orch::hand_on::HandOn::reopened`]);
//! - the landings the segments owed, asked of the holding accounts, whose host record goes on beside the platform
//!   (`Segments::reconcile_landings`).
//!
//! The decision records are Core's: it signs each with the operator key, keeps it in the evidence store as the
//! operator's stated entry and names it in the log by digest. A Core that reopens reads them back from the store and
//! holds each to the log's digest before a route runs on it; nothing of them goes on in memory.

use rutent_contracts::core_internal::calls::KeptStatements;
use rutent_contracts::module_interface::Digest;
use rutent_core_orch::segments::Reopening;

use super::*;

/// What a reopened Core reads beside its log: the decision records its key signed, held to the log's digests, and
/// what the evidence store keeps.
struct Reading<'a> {
    signed: &'a BTreeMap<String, Vec<u8>>,
    entries: &'a [(Receipt, Entry)],
    kept: &'a mut LogsRead,
}

impl Reopening for Reading<'_> {
    fn record(&self, id: &Id) -> Option<DecisionRecord> {
        held_record(self.signed, self.entries, id)
    }

    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
        let kept = KeptStatements::kept(self.kept, commitment).ok()??;
        rutent_core_audit::receipts::store::bearing_out(&kept)
    }

    fn in_force(&mut self, order: &Id, authorisation: &Id) -> Option<Authorisation> {
        let logged: Vec<Entry> = self.entries.iter().map(|(_, e)| e.clone()).collect();
        authorised(&logged, order, authorisation)
    }
}

/// The decision record `id` Core's key signed, held to the digest the log names it by.
pub(super) fn held_record(
    signed: &BTreeMap<String, Vec<u8>>,
    entries: &[(Receipt, Entry)],
    id: &Id,
) -> Option<DecisionRecord> {
    let recorded = signed.get(id.as_str())?;
    let named = entries.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::DecisionRecord {
                decision_record,
                record,
                ..
            } if decision_record == id => Some(record.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })?;
    let canonical = rutent_contracts::provider_role::to_canonical(
        &serde_json::from_slice::<serde_json::Value>(recorded).ok()?,
    )
    .ok()?;
    (named.value.as_str() == rutent_prov::module::wire::sha256(&canonical))
        .then(|| record_in(recorded))
        .flatten()
}

impl Core {
    /// The host's holding accounts, which go on beside the platform, listed again in the reopened catalogue: each
    /// wallet a module the operator publishes, its node public, the way to its account recorded, as
    /// [`Core::install_wallet`] listed it (`REQ-CAT-23`, `T-020-46`).
    pub(super) fn relist_holdings(&mut self) {
        let accounts: Vec<(String, String)> = {
            let holdings = self.holdings.borrow();
            holdings
                .attributions()
                .into_iter()
                .filter_map(|(node, _)| {
                    let provider = holdings.account(&node)?.provider().to_owned();
                    Some((node, provider))
                })
                .collect()
        };
        for (node, provider) in accounts {
            let _ = self.catalogue.publish_operator_module(&provider);
            let _ = self.catalogue.add_public_node(&node);
            let _ = self.catalogue.register_public_holding(&node, &provider);
        }
    }

    /// The decision records Core signed, read back from the evidence store (`T-020-46`): each the operator's stated
    /// entry of router-api's decision-record shape, its kept bytes the record as signed, kept under the record's id
    /// where the log's `decision_record` entry names those bytes by digest. A record the store no longer keeps, or
    /// whose bytes aren't the ones the log names, is not read, and what rests on it fails closed.
    fn records_in_the_store(&mut self, history: &[(Receipt, Entry)]) -> BTreeMap<String, Vec<u8>> {
        let mut records = BTreeMap::new();
        for (_, entry) in history {
            let Entry::Stated(stated) = entry else {
                continue;
            };
            if stated.what.statement.as_str() != DECISION_RECORD_SHAPE {
                continue;
            }
            let Some(bytes) = KeptStatements::kept(&mut self.kept, &stated.commitment)
                .ok()
                .flatten()
                .and_then(|kept| rutent_core_audit::receipts::store::bearing_out(&kept))
            else {
                continue;
            };
            let Some(record) = record_in(&bytes) else {
                continue;
            };
            let id = record.record_id().as_str().to_owned();
            let mut one = BTreeMap::new();
            one.insert(id.clone(), bytes.clone());
            if held_record(&one, history, record.record_id()).is_some() {
                records.insert(id, bytes);
            }
        }
        records
    }

    /// Intake rebuilt from the log as a Core that reopened would rebuild it were the statement kept behind
    /// `commitment` unreadable (a test hook, `T-020-46`): the evidence store answers nothing for it, so an order whose
    /// inputs it holds is held, failing closed. The rest of Core's state is not changed.
    pub fn intake_reopened_without(&mut self, commitment: &Digest) {
        struct Without<'a> {
            reading: Reading<'a>,
            unreadable: &'a Digest,
        }
        impl Reopening for Without<'_> {
            fn record(&self, id: &Id) -> Option<DecisionRecord> {
                self.reading.record(id)
            }
            fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
                if commitment == self.unreadable {
                    return None;
                }
                self.reading.kept(commitment)
            }
        }
        let history: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        let mut reading = Without {
            reading: Reading {
                signed: &self.signer.records,
                entries: &history,
                kept: &mut self.kept,
            },
            unreadable: commitment,
        };
        self.intake = Intake::reopened(&history, &mut reading);
    }

    /// The value in flight a Core that reopened would hold were none of its decision records readable (a control for
    /// the ceilings' failing closed, `T-020-46`): each segment handed over held at the value its signed operation
    /// states. Core's own state is not changed.
    pub fn exposures_rebuilt_without_records(
        &mut self,
    ) -> Vec<(
        Id,
        rutent_contracts::provider_role::Denomination,
        rutent_contracts::router_api::Amount,
    )> {
        struct Blind<'a> {
            kept: &'a mut LogsRead,
        }
        impl Reopening for Blind<'_> {
            fn record(&self, _: &Id) -> Option<DecisionRecord> {
                None
            }
            fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
                let kept = KeptStatements::kept(self.kept, commitment).ok()??;
                rutent_core_audit::receipts::store::bearing_out(&kept)
            }
        }
        let history: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        let mut exposures = Exposures::new();
        let _ = Segments::reopened(
            &self.clock,
            &self.catalogue,
            &history,
            &mut Blind {
                kept: &mut self.kept,
            },
            &mut exposures,
        );
        exposures.totals()
    }

    /// Rebuilds Core's own state from the log the disk holds (see the module's docs).
    pub(super) fn rebuild_from_log(&mut self) {
        let history: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        // The decision records Core signed, from the evidence store, never from memory.
        self.signer.records = self.records_in_the_store(&history);
        // The quotes reserves hold, kept again by Core from the log and what the evidence store keeps behind each
        // hold, and which order redeemed which (`T-020-57`).
        self.holds = {
            let mut reading = Reading {
                signed: &self.signer.records,
                entries: &history,
                kept: &mut self.kept,
            };
            rutent_core_orch::quote::Holds::reopened(&history, &mut reading)
        };
        self.issued = Issued::recorded(&history);
        // The executed prices reported, so each order's is reported once (`REQ-QTE-7`).
        self.reported = rutent_core_orch::quote::Prices::recorded(&history);
        self.intake = {
            let mut reading = Reading {
                signed: &self.signer.records,
                entries: &history,
                kept: &mut self.kept,
            };
            Intake::reopened(&history, &mut reading)
        };
        // The arrivals, the deposits held and the waits for a customer, rebuilt from the log as well (`T-020-51`): each
        // order still waiting for its customer is told to policy's sources and to the assembly's filings again.
        self.arrivals = rutent_core_orch::arrival::Arrivals::reopened(&history, &mut self.kept);
        for (_, entry) in &history {
            let Entry::Verified(VerifiedEntry {
                what:
                    What::Shown {
                        order,
                        awaiting: Some(_),
                        ..
                    },
                ..
            }) = entry
            else {
                continue;
            };
            if let Some(record) = self.arrivals.record(order) {
                // The filing's record, as arrivals rebuilt it from the log, handed to Core's sources again as the
                // filing handed it over when the order opened (`T-020-55`).
                self.sources.opened_orders().opened(
                    &record.order,
                    &record.parties,
                    &record.origin_reserve,
                );
            }
            if let Some(filing) = self.arrivals.filing_of(order) {
                self.filings
                    .insert(order.as_str().to_owned(), filing.clone());
            }
        }
        let orders: Vec<Id> = self.intake.orders().filter_map(Id::new).collect();
        for order in &orders {
            if let Some(request) = self.intake.request_of(order) {
                self.requests
                    .insert(order.as_str().to_owned(), request.clone());
            }
        }
        let mut exposures = Exposures::new();
        let segments = {
            let mut reading = Reading {
                signed: &self.signer.records,
                entries: &history,
                kept: &mut self.kept,
            };
            Segments::reopened(
                &self.clock,
                &self.catalogue,
                &history,
                &mut reading,
                &mut exposures,
            )
        };
        self.segments = segments;
        self.exposures = exposures;
        // The routes handed on whose start the holding account's record proved (`T-020-26`), and the hand-ons the log
        // holds half of.
        self.recover_starts();
        self.hand_on = {
            let mut reading = Reading {
                signed: &self.signer.records,
                entries: &history,
                kept: &mut self.kept,
            };
            rutent_core_orch::hand_on::HandOn::reopened(&history, &self.segments, &mut reading)
        };
        // The landings the segments owed: asked of the holding accounts, never made blind (`INV-1`).
        self.reconcile_landings();
        self.stuck = Stuck::reopened(&self.segments, &self.clock, &history, &mut self.exposures);
        // The statements kept about each segment after its answer, from the log and the evidence store (`T-020-61`, item 6).
        self.life.reversals =
            rutent_core_orch::reversal::Reversals::reopened(&history, &mut |commitment| {
                let kept = KeptStatements::kept(&mut self.kept, commitment).ok()??;
                rutent_core_audit::receipts::store::bearing_out(&kept)
            });
        // A resolution done whose part's `stuck_resolved` the log doesn't hold is finished, as a Core that ran on
        // would have (`T-020-46`).
        {
            let mut parts = StuckParts {
                clock: &self.clock,
                log: &mut self.general,
                fresh: &mut self.fresh,
                issued: &self.issued,
                segments: &mut self.segments,
                exposures: &mut self.exposures,
            };
            let _ = self.stuck.finish_reopened(&mut parts, &kit::core());
        }
        let _ = self.feed();
        // What each started order runs on, rebuilt by Core's own resume from the log, the held gate Core's (`T-020-61`):
        // intake rebuilt its own orders' above; the filed and linked ones, and any intake couldn't, are built here.
        rutent_core_orch::schedule::resume(&mut super::schedule::Due(self));
        self.reaccept_linked(&history);
        // The erasures decided and done that the log holds no record of, settled (`Unsettled::settle`'s resume).
        self.settle_reopened_erasures();
    }

    /// The routes the linked orders were accepted on, rebuilt from the log (`Core::accepted_in_the_log`): every
    /// acceptance of an order intake never opened, an order a hand-on links to (`T-020-46`).
    fn reaccept_linked(&mut self, history: &[(Receipt, Entry)]) {
        let opened: Vec<&Id> = history
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what: What::OrderOpened { order, .. },
                    ..
                }) => Some(order),
                _ => None,
            })
            .collect();
        let linked: Vec<Id> = history
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what: What::Acceptance { order, .. },
                    ..
                }) if !opened.contains(&order) => Some(order.clone()),
                _ => None,
            })
            .collect();
        for order in linked {
            let route = rutent_core_orch::hand_on::accepted_in_the_log(
                history,
                &mut |commitment| {
                    let kept = KeptStatements::kept(&mut self.kept, commitment).ok()??;
                    rutent_core_audit::receipts::store::bearing_out(&kept)
                },
                &order,
            );
            if let Some(route) = route {
                self.linked.insert(order.as_str().to_owned(), route);
            }
        }
    }
}
