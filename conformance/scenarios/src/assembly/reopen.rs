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

    /// Reopens Core on the log the disk holds through Core's own entry point (`rutent_core_orch::schedule::reopen`, the
    /// review of #355's round 2, M3), which rebuilds every part in its own order; the assembly hands it only the
    /// deployment's pieces (the clock, the log's and the evidence store's handles, policy's register of authorisations,
    /// the host's accounts, the installed advertisements and their grant terms, the platform, fresh ids) and then holds
    /// what it answers, tells its own registers (policy's sources, the filings, the requests and the linked routes) and
    /// lets Core's run resume.
    ///
    /// # Errors
    ///
    /// As Core refuses to reopen.
    pub(super) fn reopen_core(&mut self) -> Result<(), AssemblyError> {
        use rutent_core_orch::schedule::reopen::{Deployment, Kept, Platform, Relist, reopen};
        struct Store<'a>(&'a mut LogsRead);
        impl Kept for Store<'_> {
            fn bytes(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
                let kept = KeptStatements::kept(self.0, commitment).ok()??;
                rutent_core_audit::receipts::store::bearing_out(&kept)
            }
            fn statements(&mut self) -> &mut dyn KeptStatements {
                self.0
            }
        }
        struct Platformed<'a> {
            fleet: &'a mut crate::sim::Fleet,
            holdings: &'a Rc<RefCell<rutent_prov::holding::wallet::Holdings>>,
            bounds: &'a rutent_contracts::router_api::PolicyBounds,
            clock: &'a SharedClock,
        }
        impl Platform for Platformed<'_> {
            fn cutovers(&mut self) -> &mut dyn rutent_core_orch::settings::Cutovers {
                &mut self.fleet.platform
            }
            fn releases(
                &mut self,
                with: &mut dyn FnMut(&dyn rutent_core_orch::segments::lifecycle::Releases),
            ) {
                let releases = self.fleet.releases();
                with(&releases);
            }
            fn landings(
                &mut self,
                with: &mut dyn FnMut(&mut dyn rutent_core_orch::segments::Landings),
            ) {
                let at = self.clock.instant();
                self.fleet.platform.world.set_clock(at);
                let Some(made_at) = rutent_core_orch::snapshot::Clock::now(self.clock).timestamp()
                else {
                    return;
                };
                let mut key = RealKey::assembly();
                let operator = rutent_core_orch::stuck::OperatorSigns::new(
                    &mut key,
                    self.bounds,
                    kit::operator().acts,
                    made_at,
                );
                let mut holdings = self.holdings.borrow_mut();
                let mut landing = holding::Landing {
                    holdings: &mut holdings,
                    platform: &mut self.fleet.platform,
                    operator: &operator,
                };
                with(&mut landing);
            }
        }
        let history: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        let installed = self.installed.clone();
        for one in &installed {
            self.record_release(one);
        }
        let advertised: Vec<(String, Vec<u8>)> = installed
            .iter()
            .map(|one| {
                (
                    one.advertisement.statement().maker.clone(),
                    one.advertisement.bytes().to_vec(),
                )
            })
            .collect();
        let register = Rc::clone(&self.log);
        let accepted = move |maker: &str| {
            register
                .borrow()
                .accepted_advertisement(maker)
                .map(|a| a.commitment)
        };
        let terms = |provider: &str, decider: &str, at: rutent_prov::catalogue::Instant| {
            installed
                .iter()
                .find(|one| one.advertisement.statement().statement.provider == provider)
                .map(|one| kit::grant_terms(one, decider, at))
        };
        let mut joined = |catalogue: &mut Catalogue, bytes: &[u8]| {
            let Ok(ad) = serde_json::from_slice::<
                Statement<rutent_contracts::provider_role::Advertisement>,
            >(bytes) else {
                return;
            };
            if let Some(one) = installed
                .iter()
                .find(|one| one.advertisement.statement().maker == ad.maker)
            {
                kit::joined(catalogue, &ad.statement, &one.spec);
            }
        };
        // The host's holding accounts and the publishers' accounts, as the deployment keeps them, listed first.
        let holdings = Rc::clone(&self.holdings);
        let registers = Rc::clone(&self.deployed.registers);
        let mut listed = |catalogue: &mut Catalogue| {
            let holdings = holdings.borrow();
            for (node, _) in holdings.attributions() {
                let Some(provider) = holdings.account(&node).map(|a| a.provider().to_owned())
                else {
                    continue;
                };
                let _ = catalogue.publish_operator_module(&provider);
                let _ = catalogue.add_public_node(&node);
                let _ = catalogue.register_public_holding(&node, &provider);
            }
            for account in registers.borrow().publisher_accounts.values() {
                let _ = catalogue.register_private_holding(
                    &account.node,
                    &account.publisher,
                    &account.module,
                );
            }
        };
        let logged: Vec<Entry> = history.iter().map(|(_, e)| e.clone()).collect();
        let mut in_force =
            |order: &Id, authorisation: &Id| authorised(&logged, order, authorisation);
        let core_actor = kit::core();
        let reopened = reopen(
            &history,
            &mut Deployment {
                clock: &self.clock,
                core: &core_actor,
                log: &mut self.general,
                kept: &mut Store(&mut self.kept),
                in_force: &mut in_force,
                catalogue: &mut self.catalogue,
                listed: &mut listed,
                relisting: Relist {
                    accepted: &accepted,
                    installed: &advertised,
                    terms: &terms,
                    tuple_id: &kit::tuple_id,
                    joined: &mut joined,
                },
                platform: &mut Platformed {
                    fleet: &mut self.fleet,
                    holdings: &self.holdings,
                    bounds: &self.bounds,
                    clock: &self.clock,
                },
                fresh: &mut self.fresh,
                erasing: &mut super::erasure::Bound(&self.bound),
            },
        )
        .map_err(|e| {
            use rutent_core_orch::schedule::reopen::ReopenRefused;
            use rutent_core_orch::settings::listing::RelistRefused;
            AssemblyError::Listing(match e {
                ReopenRefused::Catalogue(RelistRefused::NotReplayed { change, provider }) => format!(
                    "the log holds a {change:?} of {} that Core's catalogue rebuild doesn't replay",
                    provider.as_str()
                ),
                ReopenRefused::Catalogue(RelistRefused::Uncommitted(maker)) => format!(
                    "the advertisement installed for {maker} isn't one the log committed to: no commitment's bytes are its"
                ),
                other => format!("Core didn't reopen: {other:?}"),
            })
        })?;
        // Core's parts, held.
        self.signer.records = reopened.records;
        self.settings = reopened.settings;
        self.bounds = self.settings.bounds(&self.base_bounds);
        self.closing = reopened.closing;
        self.holds = reopened.holds;
        self.issued = reopened.issued;
        self.reported = reopened.prices;
        self.intake = reopened.intake;
        self.arrivals = reopened.arrivals;
        self.segments = reopened.segments;
        self.exposures = reopened.exposures;
        self.hand_on = reopened.hand_on;
        self.stuck = reopened.stuck;
        self.life.reversals = reopened.reversals;
        self.schedule = reopened.schedule;
        // The deployment's own registers, told what Core rebuilt: each order still waiting for its customer told to
        // policy's sources and the filings again (`T-020-51`, `T-020-55`), and intake's requests.
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
        let _ = self.feed();
        // What each started order runs on, rebuilt by Core's own resume from the parts, all rebuilt now (`T-020-61`).
        rutent_core_orch::schedule::resume(&mut super::schedule::Due(self));
        self.reaccept_linked(&history);
        Ok(())
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
