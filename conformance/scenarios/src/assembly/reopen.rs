//! Core opened again on its disk rebuilds its own state from the log (`T-020-46`, finding 3 of `M3`'s milestone
//! review; `NFR-6`, `INV-8`, `REQ-ORD-8`).
//!
//! The log is Core's durable record. A Core that reopens reads back from it, beside what `T-020-09` already rebuilt
//! (the grading's `Closing::recorded`, the held quotes' `Holds::recorded`, the catalogue's listing and the proven
//! starts):
//!
//! - the transactions it asked under ([`Issued::recorded`]);
//! - intake's orders and acceptances ([`Intake::reopened`]), and policy's sources of the orders' parties from them;
//! - the segments ([`Segments::reopened`]): the orders followed, the effects acted on, each segment handed over and
//!   its outcome, and the value in flight held in [`Exposures`];
//! - the stuck orders ([`Stuck::reopened`]): the declarations, the kept decisions, what moved and the moves in motion;
//! - what each started order needs to run on: its decision record, its plan, its authorisation as the log holds it in
//!   force, its origin and session from its `order_opened`, its inputs from the operator's statement of them that
//!   `order_opened` names by commitment, and what its route delivers.
//!
//! - the hand-ons the log holds half of ([`rutent_core_orch::hand_on::HandOn::reopened`]);
//! - the landings the segments owed, asked of the holding accounts, whose host record goes on beside the platform
//!   (`Segments::reconcile_landings`).
//!
//! The decision records are the operator key's: Core signs each with it, and the log names it by digest. The key and
//! the records it signed go on across the reopening, as the platform and the clock do, and each is held to the log's
//! digest before a route runs on it.

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
        let accounts: Vec<(String, String)> = self
            .holdings
            .attributions()
            .into_iter()
            .filter_map(|(node, _)| {
                let provider = self.holdings.account(&node)?.provider().to_owned();
                Some((node, provider))
            })
            .collect();
        for (node, provider) in accounts {
            let _ = self.catalogue.publish_operator_module(&provider);
            let _ = self.catalogue.add_public_node(&node);
            let _ = self.catalogue.register_public_holding(&node, &provider);
        }
    }

    /// Rebuilds Core's own state from the log the disk holds (see the module's docs).
    pub(super) fn rebuild_from_log(&mut self) {
        let history: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        self.issued = Issued::recorded(&history);
        self.intake = {
            let mut reading = Reading {
                signed: &self.signer.records,
                entries: &history,
                kept: &mut self.kept,
            };
            Intake::reopened(&history, &mut reading)
        };
        let orders: Vec<Id> = self.intake.orders().filter_map(Id::new).collect();
        for order in &orders {
            if let Some(record) = self.intake.record(order) {
                self.records
                    .orders
                    .borrow_mut()
                    .insert(order.as_str().to_owned(), record);
            }
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
        self.hand_on = rutent_core_orch::hand_on::HandOn::reopened(&history);
        // The landings the segments owed: asked of the holding accounts, never made blind (`INV-1`).
        self.reconcile_landings();
        self.stuck = Stuck::reopened(&self.segments, &self.clock, &history, &mut self.exposures);
        self.restart_orders(&history);
    }

    /// What each order the log started needs to run on, from the log and the records Core's key signed.
    fn restart_orders(&mut self, history: &[(Receipt, Entry)]) {
        let logged: Vec<Entry> = history.iter().map(|(_, e)| e.clone()).collect();
        for (_, entry) in history {
            let Entry::Verified(VerifiedEntry {
                what: What::Input { input },
                ..
            }) = entry
            else {
                continue;
            };
            let Observed::RouteChosen {
                decision_record,
                parts,
                handed_on_from: None,
            } = &input.observed
            else {
                continue;
            };
            let Some(order) = Id::new(input.subject.order.as_str()) else {
                continue;
            };
            if let Some(started) = self.restarted(history, &logged, &order, decision_record, parts)
            {
                self.started.insert(order.as_str().to_owned(), started);
            }
        }
    }

    fn restarted(
        &self,
        history: &[(Receipt, Entry)],
        logged: &[Entry],
        order: &Id,
        decision_record: &rutent_contracts::state_machine::Id,
        plan: &[PartPlan],
    ) -> Option<Started> {
        let record = held_record(
            &self.signer.records,
            history,
            &Id::new(decision_record.as_str())?,
        )?;
        let whats = || {
            logged.iter().filter_map(|e| match e {
                Entry::Verified(v) => Some(&v.what),
                Entry::Stated(_) => None,
            })
        };
        let authorisation = whats().find_map(|w| match w {
            What::Acceptance {
                order: o,
                authorisation,
                ..
            } if o == order => Some(authorisation.clone()),
            _ => None,
        })?;
        let authorised = authorised(logged, order, &authorisation)?;
        let (origin, session, destination) = whats().find_map(|w| match w {
            What::OrderOpened {
                order: o,
                origin_reserve,
                session,
                destination,
                ..
            } if o == order => Some((origin_reserve.clone(), session.clone(), destination.clone())),
            _ => None,
        })?;
        // What the route delivers: as its moves name it, or as the quote shown priced it from the record.
        let delivers = whats()
            .find_map(|w| match w {
                What::Effect {
                    action:
                        Action::MoveValue {
                            order: o, route, ..
                        },
                    ..
                } if o == order => Some(route.delivers.clone()),
                _ => None,
            })
            .or_else(|| {
                let candidate = record.chosen()?;
                let floor = authorised.floor.as_ref().map(|f| f.amount().clone());
                rutent_core_orch::quote::show(&record, candidate, floor.as_ref())
                    .ok()
                    .map(|shown| shown.arrives)
            })?;
        // The order's inputs, as intake read them back from the operator's statement of them (`T-020-46`), the
        // destination's account among them.
        let mut inputs = self
            .intake
            .request_of(order)
            .map(|request| request.inputs.clone())
            .unwrap_or_default();
        inputs.insert(
            DESTINATION_INPUT.to_owned(),
            destination.account.as_str().to_owned(),
        );
        Some(Started {
            record,
            plan: plan.to_vec(),
            authorisation,
            origin,
            route: RouteEnd {
                destination: authorised.destination.clone(),
                to_asset: authorised.to_asset.clone(),
                delivers,
            },
            session: Some(session),
            inputs,
            floor: authorised.floor.clone(),
        })
    }
}
