//! Core's schedule, as the assembly wires it (`T-020-48`, finding 4 of `M3`'s milestone review of 2026-11-24).
//!
//! `core-orch`'s [`Schedule`](rutent_core_orch::schedule::Schedule) decides what is due; this module is its port, [`Duties`], over the parts the assembly
//! holds: the platform, the holding accounts, the segments, the stuck orders and the log. [`Core::tick`] is the one
//! entry point, the one call the deployment keeps (on a timer), and the one call a scenario makes on Core's clock. The
//! assembly exposes no holding reading, landing, measure or late-answer call of its own: each is a job of the tick, and
//! the readings it takes are taken through the platform and never handed in.
//!
//! **A private node's account.** At a public node the account's module is the operator's plain wallet, registered in
//! [`Holdings`](rutent_prov::holding::wallet::Holdings). At a private node it is its publisher's module, whose answer
//! to the operator's `holding_balance` question is the publisher's own word, labelled so (`REQ-AUD-15`): its reading is
//! taken the same way, and what the orders hold there is what Core's segments rest at the node. No simulated provider
//! stands in for a publisher's holding account, so a scenario installs the plain wallet as one
//! ([`Core::install_publisher_account`]).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::HoldingReading;
use rutent_contracts::provider_role::{
    HoldingBalance, HoldingBalanceOutcome, Operation, ResultBody,
};
use rutent_contracts::router_api::Id;
use rutent_core_orch::schedule::{
    Duties, HoldingChecked, Job, LateTaken, Published, Reported, Round, StatusTried, Unknown,
};
use rutent_core_orch::segments::{Live, Says, UnbuiltPair};
use rutent_core_orch::snapshot::Clock as _;
use rutent_core_orch::stuck::{
    OperatorSigns, Parts as StuckParts, Reporting, ResolveError, Resolved, Stated,
};
use rutent_prov::module::types::Handled;
use serde_json::json;

use super::{AssemblyError, Core, Running, SegmentParts};
use crate::kit::{self, RealKey};
use crate::sim::InstalledWallet;
use rutent_core_orch::segments::NoProxy;

/// The decimals of every asset the simulated world moves (`kit`): an account's reading is stated in them.
const DECIMALS: u8 = 2;

/// The account a private node's publisher keeps for the value resting inside the node.
#[derive(Debug, Clone)]
pub struct PublisherAccount {
    /// The module that answers for the account: the platform runs it.
    pub module: String,
    /// The publisher whose word its balance is.
    pub publisher: String,
    /// The asset it holds.
    pub asset: String,
    sent: u64,
}

impl Core {
    /// Core's own clock, in whole seconds: what the schedule's cadences count in. Reading it moves nothing.
    pub(super) fn clock_seconds(&self) -> u64 {
        u64::try_from(self.clock.peek() / 1_000_000_000).unwrap_or(u64::MAX)
    }

    /// One tick of Core's schedule (`T-020-48`): runs each job that is due on Core's clock, over the parts Core holds,
    /// and answers what ran and what it came to. The deployment calls it on a timer; a scenario calls it where the
    /// deployment's timer would have. A job not yet due is left for a later tick.
    pub fn tick(&mut self) -> Round {
        let now = self.clock_seconds();
        let mut schedule = std::mem::take(&mut self.schedule);
        let round = schedule.tick(now, &mut Due(self));
        self.schedule = schedule;
        round
    }

    /// Sets how often `job` runs (the operator's setting of the job's cadence; `T-020-49` decides it).
    pub fn set_cadence(&mut self, job: Job, seconds: u64) {
        self.schedule.cadences_mut().set(job, seconds);
    }

    /// Sets how often a segment unknown at `provider` is asked its status again.
    pub fn set_status_cadence(&mut self, provider: &str, seconds: u64) {
        self.schedule
            .cadences_mut()
            .set_status_of(provider, seconds);
    }

    /// Installs the plain wallet as the account `publisher` keeps for the private node `node`, holding `asset`: the
    /// platform runs it, the catalogue records it as the node's holding account, and the schedule reads it as the
    /// publisher's word. The node must be one a provider joined to the catalogue declares private.
    ///
    /// # Errors
    ///
    /// If the catalogue refuses the node or the way to its account.
    pub fn install_publisher_account(
        &mut self,
        node: &str,
        publisher: &str,
        asset: &str,
    ) -> Result<InstalledWallet, AssemblyError> {
        let wallet = self.fleet.install_wallet(&format!("{publisher}-account"));
        self.catalogue
            .register_private_holding(node, publisher, &wallet.provider)
            .map_err(|e| AssemblyError::Listing(format!("the publisher's account: {e:?}")))?;
        self.publisher_accounts.insert(
            node.to_owned(),
            PublisherAccount {
                module: wallet.provider.clone(),
                publisher: publisher.to_owned(),
                asset: asset.to_owned(),
                sent: 0,
            },
        );
        Ok(wallet)
    }

    /// The reading of the private node `node`'s account: what its publisher's module states of its balance, asked of it
    /// through the platform as the operator's question, and what Core's segments rest there for each order.
    fn publisher_reading(&mut self, node: &str) -> Result<HoldingReading, String> {
        let account = self
            .publisher_accounts
            .get_mut(node)
            .ok_or_else(|| format!("{node} has no publisher's account"))?;
        account.sent += 1;
        let account = account.clone();
        let at = self.clock.instant();
        self.fleet.platform.world.set_clock(at);
        let made_at = self
            .clock
            .now()
            .timestamp()
            .ok_or("the clock can't be written as a timestamp")?;
        let transaction = format!("{}-balance-{}", account.module, account.sent);
        let operation = Operation::HoldingBalance(HoldingBalance {
            transaction: transaction.clone(),
            provider: account.module.clone(),
            node: node.to_owned(),
            asset: account.asset.clone(),
        });
        let mut key = RealKey::assembly();
        let operator = OperatorSigns::new(&mut key, &self.bounds, kit::operator().acts, made_at);
        let answered = match self.fleet.platform.handle(
            &account.module,
            &rutent_prov::holding::wallet::Operator::statement(&operator, operation),
        ) {
            Handled::Answered(result) => result.statement,
            Handled::Refused(why) => return Err(format!("the module didn't answer: {why:?}")),
        };
        let ResultBody::HoldingBalance(result) = answered else {
            return Err("the module answered another question".to_owned());
        };
        if result.transaction != transaction {
            return Err("the answer names another transaction".to_owned());
        }
        let HoldingBalanceOutcome::Answered(answer) = result.outcome else {
            return Err("the module refused".to_owned());
        };
        let held_for: Vec<_> = self
            .segments
            .resting_totals()
            .into_iter()
            .filter(|((at, _), amount)| at == node && amount.as_str() != "0")
            .map(|((_, order), amount)| json!({ "order": order, "amount": amount.as_str() }))
            .collect();
        serde_json::from_value(json!({
            "account": format!("holding-{node}"),
            "provider": account.module,
            "node": node,
            "asset": { "id": account.asset, "decimals": DECIMALS },
            "balance": answer.balance,
            "word": "publisher",
            "held_for": held_for,
            "at": self.clock.timestamp(),
        }))
        .map_err(|e| format!("not a reading: {e}"))
    }
}

/// The tick's reach: [`Duties`] over a [`Core`].
struct Due<'a>(&'a mut Core);

impl Duties for Due<'_> {
    fn late_answers(&mut self) -> Vec<LateTaken> {
        let core = &mut *self.0;
        let orders: Vec<Id> = core
            .started
            .keys()
            .filter_map(|k| Id::new(k.as_str()))
            .collect();
        let mut taken = Vec::new();
        for order in orders {
            let Some(followed) = core.segments.order(&order).cloned() else {
                continue;
            };
            let mut sent = BTreeMap::new();
            for segment in followed.parts.iter().flat_map(|p| p.segments.iter()) {
                let id = kit::id(segment.segment_id.as_str());
                if let Some((transaction, _)) = core.segments.sent(&order, &id) {
                    sent.insert(transaction.as_str().to_owned(), id);
                }
            }
            if sent.is_empty() {
                continue;
            }
            let transactions = sent.keys().cloned().collect();
            core.fleet.platform.world.set_clock(core.clock.instant());
            for (_, transaction, bytes) in core.fleet.late_answers(&transactions) {
                let Some(segment) = sent.get(&transaction) else {
                    continue;
                };
                let kept = core
                    .late_delivery(&order, segment, &bytes)
                    .map_err(|why| format!("{why:?}"));
                taken.push(LateTaken {
                    order: order.clone(),
                    segment: segment.clone(),
                    kept,
                });
            }
        }
        taken
    }

    fn land_settled(&mut self) -> usize {
        self.0.land_settled_now()
    }

    fn write_measures(&mut self) -> usize {
        self.0.write_measures_now()
    }

    fn close_completed(&mut self) -> Vec<Id> {
        let orders: Vec<Id> = self
            .0
            .started
            .keys()
            .filter_map(|k| Id::new(k.as_str()))
            .collect();
        orders
            .into_iter()
            .filter(|order| self.0.close(order))
            .collect()
    }

    fn check_holdings(&mut self) -> Vec<HoldingChecked> {
        let core = &mut *self.0;
        let public: Vec<String> = core
            .holdings
            .borrow()
            .attributions()
            .into_iter()
            .map(|(node, _)| node)
            .collect();
        let private: Vec<String> = core.publisher_accounts.keys().cloned().collect();
        let mut checked = Vec::new();
        for node in public.into_iter().chain(private) {
            let reading = if core.publisher_accounts.contains_key(&node) {
                core.publisher_reading(&node)
            } else {
                core.holding_reading(&node, DECIMALS)
                    .map_err(|why| format!("{why:?}"))
            };
            let check = match &reading {
                Ok(reading) => core
                    .check_holding(reading)
                    .map_err(|why| format!("{why:?}")),
                Err(why) => Err(why.clone()),
            };
            checked.push(HoldingChecked {
                node,
                reading,
                check,
            });
        }
        checked
    }

    fn read_publishers(&mut self) -> Vec<Published> {
        let core = &mut *self.0;
        core.readings.refresh(&core.fleet.platform.world);
        let mut nodes: Vec<String> = core
            .installed
            .iter()
            .flat_map(|one| {
                one.advertisement
                    .statement()
                    .statement
                    .private_nodes
                    .clone()
            })
            .collect();
        nodes.sort();
        nodes.dedup();
        nodes
            .into_iter()
            .flat_map(|node| {
                core.readings
                    .published_in(&node)
                    .into_iter()
                    .map(move |record| Published {
                        node: node.clone(),
                        record,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn unknown_segments(&self) -> Vec<Unknown> {
        let core = &*self.0;
        let mut unknown = Vec::new();
        for order in core.started.keys().filter_map(|k| Id::new(k.as_str())) {
            let Some(followed) = core.segments.order(&order) else {
                continue;
            };
            for segment in followed.parts.iter().flat_map(|p| p.segments.iter()) {
                let id = kit::id(segment.segment_id.as_str());
                if let Some(moved) = core.segments.unknown_move(&order, &id) {
                    unknown.push(Unknown {
                        order: order.clone(),
                        segment: id,
                        provider: moved.provider.as_str().to_owned(),
                    });
                }
            }
        }
        unknown
    }

    fn ask_status_again(&mut self, unknown: &Unknown) -> StatusTried {
        let core = &mut *self.0;
        let at = (
            unknown.order.as_str().to_owned(),
            unknown.segment.as_str().to_owned(),
        );
        core.life.status_asks.remove(&at);
        let asked = match core.part_of(&unknown.order) {
            Ok(part) => {
                core.ask_unknown_among(
                    &unknown.order,
                    &part,
                    std::slice::from_ref(&unknown.segment),
                    true,
                );
                match core.life.status_asks.get(&at) {
                    Some(Ok(asked)) => Ok(asked.clone()),
                    Some(Err(why)) => Err(why.clone()),
                    None => Err("no question was put".to_owned()),
                }
            }
            Err(why) => Err(format!("{why:?}")),
        };
        StatusTried {
            unknown: unknown.clone(),
            kept: asked.as_ref().ok().and_then(|a| a.answer.clone()),
            asked: asked.map(|a| a.effect),
        }
    }

    fn report_kept_answers(&mut self) -> Vec<Reported> {
        let unknown = self.unknown_segments();
        let mut reported = Vec::new();
        for one in unknown {
            reported.extend(self.0.report_kept_for(&one));
        }
        reported
    }

    fn end_expired(&mut self) -> Vec<Id> {
        self.0.end_expired_waits().unwrap_or_default()
    }

    fn unbuilt_pairs(&self) -> Vec<UnbuiltPair> {
        self.0.segments.unbuilt_pairs().to_vec()
    }
}

impl Core {
    /// Confirms the failed outcome a mover reported for the unknown `segment` of `order` (`Stuck::confirm_segment_outcome`,
    /// `AC-6`): `confirmer` is another actor than the reporter, `report` the report's version and `reported_with` the
    /// statement it rests on (what the schedule's round told, [`Reported`]), and `other` another maker's statement about
    /// the move's transaction, which bears the report out. Core records the move's final result, `failed`, and the
    /// segment's `segment_failed`.
    ///
    /// # Errors
    ///
    /// If the order hasn't started, the statement isn't taken as evidence, or the confirmation is refused.
    pub fn confirm_reported_failure(
        &mut self,
        order: &Id,
        segment: &Id,
        confirmer: &rutent_contracts::core_internal::Actor,
        report: (u64, &rutent_contracts::core_internal::Receipt),
        other: &[u8],
    ) -> Result<rutent_core_orch::stuck::Moved, super::StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(super::StepError::NotNow("the order started"))?;
        let (at, part) = started
            .plan
            .iter()
            .enumerate()
            .find(|(_, p)| {
                p.segments
                    .iter()
                    .any(|s| s.segment_id.as_str() == segment.as_str())
            })
            .ok_or(super::StepError::NotNow("the segment is in a part"))?;
        let part_id = kit::id(part.part_id.as_str());
        let segments: Vec<Id> = part
            .segments
            .iter()
            .map(|s| kit::id(s.segment_id.as_str()))
            .collect();
        let (core, operator) = (kit::core(), kit::operator());
        let record_entry = self.record_entry_of(&started.record);
        let live = Live {
            order,
            part: &part_id,
            segments: &segments,
            record: &started.record,
            at,
            authorisation: &started.authorisation,
            origin_reserve: &started.origin,
            route: &started.route,
            caller: &core,
            core: &core,
            operator: &operator,
            session: started.session.as_ref(),
            inputs: &started.inputs,
            floor: started.floor.as_ref(),
            record_entry: record_entry.as_ref(),
        };
        let late = self
            .take_late(&live, segment, other)
            .map_err(|_| super::StepError::NotNow("the statement is taken as evidence"))?;
        let stated = Stated::from(&late);
        let mut policy = self.policy.clone();
        let history: Vec<_> = self.log.borrow().entries().to_vec();
        let mut parts = StuckParts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        let moved = self.stuck.confirm_segment_outcome(
            &mut parts,
            &mut policy,
            rutent_core_orch::stuck::ConfirmingSegment {
                order,
                part: &part_id,
                segment,
                confirmer,
                outcome: Resolved::Failed,
                report: report.0,
                reported_with: report.1,
                evidence: &stated,
                core: &core,
            },
            &history,
        );
        self.feed()?;
        moved.map_err(super::StepError::Resolve)
    }

    /// What Core kept of its provider's answers about the unknown segment `one`, handed to its stuck mover: a
    /// statement the provider's own result, which says the move failed, is kept as evidence (`Segments::late_statement`)
    /// and reported as the move's outcome by the mover, the actor whose request policy permitted it, through the stuck
    /// mover's own report (`Stuck::report_segment_outcome`), which settles nothing until another actor confirms it
    /// (`AC-6`, `REQ-ORD-1`). Only a segment of a part Core declared stuck can be reported; one already reported is
    /// not told again.
    fn report_kept_for(&mut self, one: &Unknown) -> Vec<Reported> {
        let Some(started) = self.started.get(one.order.as_str()).cloned() else {
            return Vec::new();
        };
        let Some((at, part)) = started.plan.iter().enumerate().find(|(_, p)| {
            p.segments
                .iter()
                .any(|s| s.segment_id.as_str() == one.segment.as_str())
        }) else {
            return Vec::new();
        };
        let part_id = kit::id(part.part_id.as_str());
        if !self.stuck.is_stuck(&one.order, &part_id) {
            return Vec::new();
        }
        let Some(moved) = self.segments.unknown_move(&one.order, &one.segment) else {
            return Vec::new();
        };
        let kept: Vec<Vec<u8>> = self
            .life
            .reversals
            .kept_statements(&one.order, &one.segment)
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        let segments: Vec<Id> = part
            .segments
            .iter()
            .map(|s| kit::id(s.segment_id.as_str()))
            .collect();
        let (core, operator) = (kit::core(), kit::operator());
        let record_entry = self.record_entry_of(&started.record);
        let live = Live {
            order: &one.order,
            part: &part_id,
            segments: &segments,
            record: &started.record,
            at,
            authorisation: &started.authorisation,
            origin_reserve: &started.origin,
            route: &started.route,
            caller: &core,
            core: &core,
            operator: &operator,
            session: started.session.as_ref(),
            inputs: &started.inputs,
            floor: started.floor.as_ref(),
            record_entry: record_entry.as_ref(),
        };
        let mut reported = Vec::new();
        for bytes in kept {
            // Only an answer that says the move failed is taken to the mover; any other is Core's already, kept once.
            let failed = serde_json::from_slice::<serde_json::Value>(&bytes)
                .is_ok_and(|v| v["statement"]["outcome"]["status"] == "failed");
            if !failed {
                continue;
            }
            let late = self.take_late(&live, &one.segment, &bytes);
            let Ok(late) = late else { continue };
            if late.says != Says::Failed {
                continue;
            }
            let mut policy = self.policy.clone();
            let history: Vec<_> = self.log.borrow().entries().to_vec();
            let mut parts = StuckParts {
                clock: &self.clock,
                log: &mut self.general,
                fresh: &mut self.fresh,
                issued: &self.issued,
                segments: &mut self.segments,
                exposures: &mut self.exposures,
            };
            let taken = self.stuck.report_segment_outcome(
                &mut parts,
                &mut policy,
                Reporting {
                    order: &one.order,
                    part: &part_id,
                    segment: &one.segment,
                    reporter: &moved.mover,
                },
                Resolved::Failed,
                &Stated::from(&late),
                &history,
            );
            if matches!(taken, Err(ResolveError::AlreadyReported)) {
                continue;
            }
            reported.push(Reported {
                unknown: one.clone(),
                statement: late.receipt.clone(),
                taken: taken
                    .map_err(|why| format!("{why:?}"))
                    .and_then(|moved| match moved {
                        rutent_core_orch::stuck::Moved::AwaitsConfirmation { report } => Ok(report),
                        other => Err(format!("{other:?}")),
                    }),
            });
        }
        let _ = self.feed();
        reported
    }

    /// Takes `bytes`, the provider's later statement about the unknown `segment` of `live`'s order, as evidence of what
    /// the move came to, kept once as the provider's stated entry (`Segments::late_statement`).
    fn take_late(
        &mut self,
        live: &Live<'_>,
        segment: &Id,
        bytes: &[u8],
    ) -> Result<rutent_core_orch::segments::Late, rutent_core_orch::segments::LateRefused> {
        let readings = self.readings.clone();
        readings.refresh(&self.fleet.platform.world);
        let mut running = Running {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
            faults: &mut self.faults,
            feed: &mut self.feed,
            log: &self.log,
            swap_to: self.swap_to.as_deref_mut(),
            readings: &readings,
        };
        let mut grading = rutent_core_orch::closing::Evidenced {
            catalogue: &self.catalogue,
            records: &readings,
            books: &readings,
            closing: &mut self.closing,
        };
        let mut decisions = self.policy.clone();
        let mut channel = rutent_core_orch::notice::Channel {
            notices: &mut self.notices,
            key: &mut self.notice_signer,
            transport: &mut self.wire,
        };
        let mut parts = SegmentParts {
            clock: &self.clock,
            catalogue: &self.catalogue,
            log: &mut self.general,
            evidence: &mut self.keeper,
            decisions: &mut decisions,
            modules: &mut running,
            uses: &mut NoProxy,
            grading: &mut grading,
            operator_key: &mut self.signer,
            fresh: &mut self.fresh,
            issued: &self.issued,
            exposures: &mut self.exposures,
            channel: &mut channel,
        };
        self.segments
            .late_statement(&mut parts, live, segment, bytes)
    }
}
