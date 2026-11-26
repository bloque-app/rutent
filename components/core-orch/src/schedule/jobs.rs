//! Each job of Core's schedule, as Core runs it (`T-020-48`, the review of its first build, B2): the body of every job
//! is here, in `core-orch`, over [`Ports`], which the deployment answers and which decide nothing.
//!
//! # Whose is what
//!
//! | Job | Core's body here | What the deployment answers ([`Ports`]) |
//! |---|---|---|
//! | late answers | the transactions each segment was sent under, the answer kept once as its maker's statement (`Reversals::later`) | the platform's held late answers to those transactions |
//! | landing | `Segments::land_settled` | the register of holding accounts, asked through the platform ([`Landings`]) |
//! | measures, pairs, unbuilt pairs | `Segments::write_measures`, `write_pairs`, `write_unbuilt` | the log's comparisons and measure writer (`CMP-CORE-AUDIT`) |
//! | closing | which orders, the records of the routes each ran, `closing::report_logged_price` | the decision records Core's key signed, held to the log's digest |
//! | holding check | each account read once a check (`L3`), the reading built (account, provider, word, what the orders hold, time), checked; an account not read raised once a spell (`holding_not_read`) | the register's readings of public accounts, through the platform; the publishers' modules, asked through the platform in the operator's name |
//! | status questions | which segments are unknown, at whose cadence ([`super::Schedule`]), `Reversals::ask_unknown_status_again` | the platform, policy |
//! | stuck watch | which orders, their current route, `Stuck::watch` | the notices channel |
//! | stuck reports | only a stuck part, only the move's own provider's typed answer (`L5`), the mover as reporter, the kept statement named once | policy |
//! | waits | `Arrivals::end_expired` | intake's desk |
//! | notices | `Notices::notify` over the log as it stands | the notice key and the transport |
//! | checkpoints | due on the schedule fixed in advance | the checkpointer and the store it publishes to (`CMP-CORE-AUDIT`, `REQ-AUD-14`) |
//!
//! A job reaches Core's own parts (the segments, the stuck orders, the reversals, the arrivals, the prices, the notices)
//! only as the deployment lends them, beside the ports it reaches them with ([`Ports::with_segments`] and the rest): a
//! lending method builds the parts and hands them over, and adds nothing. No job's rule is the deployment's to write
//! again, and none of its answers moves value by itself: every value-moving path is the segments' own, landing
//! included (`Segments::settled_at_node`).
//!
//! **Fails closed.** A port that can't answer leaves its job's work for the next tick: a late answer not kept, a
//! reading not taken, a price not reported are each answered in the round and tried again; a job never runs on a
//! guess. A holding account whose module doesn't give its balance is raised to the operator, never left silent.

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{Comparisons, Measures, Operator, StuckOrders};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Difference, Entry, HoldingNotRead, HoldingReading, Receipt, Role, RouteEnd,
    What, Word,
};
use rutent_contracts::provider_role::{
    Denomination, HoldingBalance, HoldingBalanceOutcome, Operation, ResultBody,
};
use rutent_contracts::router_api::{Amount, DecisionRecord, Id, PositiveAmount};
use rutent_contracts::state_machine::PartPlan;
use rutent_prov::catalogue::{Catalogue, Publisher};
use rutent_prov::holding::BalanceSource;
use rutent_prov::holding::wallet::WalletError;
use rutent_prov::module::types::Handled;

use super::{
    Duties, HoldingChecked, LateTaken, Priced, Published, Reported, StatusTried, Unknown, Watched,
};
use crate::arrival::Arrivals;
use crate::closing::{PriceDesk, report_logged_price};
use crate::intake::Desk;
use crate::notice::{NoticeKey, Notices, Transport};
use crate::reversal::{At, Moving, Parts as ReversalParts, Reversals};
use crate::segments::{Landings, Live, Parts as SegmentParts, Says, Segments, UnbuiltPair};
use crate::snapshot::Clock;
use crate::stuck::{
    ConfirmingSegment, Moved, Parts as StuckParts, Reporting, ResolveError, Resolved, Stated,
    Stuck, Watch, Watching,
};

/// What Core holds of an order it started, which its route runs on: the record that chose the route, the plan, the
/// authorisation and origin, what the route delivers, the session, the inputs and the floor (`T-020-46` rebuilds it
/// from the log on reopening).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The router's recorded decision that chose the route.
    pub record: DecisionRecord,
    /// The order's plan: its parts and their segments.
    pub plan: Vec<PartPlan>,
    /// The authorisation the order's value moves under.
    pub authorisation: Id,
    /// The origin reserve.
    pub origin: Id,
    /// What the route ends at.
    pub route: RouteEnd,
    /// The user's session.
    pub session: Option<Id>,
    /// The order's inputs, by name.
    pub inputs: BTreeMap<String, String>,
    /// The least the customer takes at the destination, as its authorisation names it.
    pub floor: Option<PositiveAmount>,
}

/// Where a segment sits in its order's plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The part's place in the chosen candidate.
    pub at: usize,
    /// The part.
    pub part: Id,
    /// The part's segments, in the route's order.
    pub segments: Vec<Id>,
}

fn id(text: &str) -> Option<Id> {
    Id::new(text)
}

impl Started {
    /// The part `segment` is in, if the plan lists it.
    pub fn placed(&self, segment: &Id) -> Option<Placed> {
        self.plan.iter().enumerate().find_map(|(at, part)| {
            part.segments
                .iter()
                .any(|s| s.segment_id.as_str() == segment.as_str())
                .then(|| placed(at, part))
                .flatten()
        })
    }

    /// The order's first part.
    pub fn first(&self) -> Option<Placed> {
        self.plan.first().and_then(|part| placed(0, part))
    }

    /// The part of this order at `placed`, as the segments run it: `core` writes Core's entries and asks for each
    /// move, the operator signs, and `record_entry` is the log's entry of the record, where it is named.
    pub fn live<'a>(
        &'a self,
        order: &'a Id,
        placed: &'a Placed,
        actors: &'a Actors,
        record_entry: Option<&'a Receipt>,
    ) -> Live<'a> {
        Live {
            order,
            part: &placed.part,
            segments: &placed.segments,
            record: &self.record,
            at: placed.at,
            authorisation: &self.authorisation,
            origin_reserve: &self.origin,
            route: &self.route,
            caller: &actors.core,
            core: &actors.core,
            operator: &actors.operator,
            session: self.session.as_ref(),
            inputs: &self.inputs,
            floor: self.floor.as_ref(),
            record_entry,
        }
    }
}

fn placed(at: usize, part: &PartPlan) -> Option<Placed> {
    Some(Placed {
        at,
        part: id(part.part_id.as_str())?,
        segments: part
            .segments
            .iter()
            .map(|s| id(s.segment_id.as_str()))
            .collect::<Option<_>>()?,
    })
}

/// Who Core acts as, and the deployment's operator, in whose name Core signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actors {
    /// Core, acting on its own account.
    pub core: Actor,
    /// The operator.
    pub operator: Actor,
}

/// A public node's holding account, as the deployment's register reads it through the platform: its module's balance
/// beside the host's attribution of what each order holds there (`REQ-AUD-15`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRead {
    /// The node.
    pub node: String,
    /// The provider whose module holds the account.
    pub provider: String,
    /// The decimals of the account's asset.
    pub decimals: u8,
    /// What the register read, or why it couldn't.
    pub read: Result<rutent_prov::holding::HoldingReading, WalletError>,
}

/// The account a private node's publisher keeps for the value resting inside it, as the deployment's register of
/// holding accounts names it (`REQ-CAT-23`): it outlives a restart of Core, as the register does (`S1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublisherAccount {
    /// The private node.
    pub node: String,
    /// The publisher whose word its balance is.
    pub publisher: String,
    /// The module that answers for it, which the platform runs.
    pub module: String,
    /// The asset it holds.
    pub asset: String,
    /// The asset's decimals.
    pub decimals: u8,
}

/// What a job does with the stuck orders the deployment lends: the stuck orders, what they reach, policy and the
/// operator's channel.
pub type StuckLent<'f> =
    dyn FnMut(&mut Stuck, &mut StuckParts<'_>, &mut dyn StuckOrders, &mut dyn Operator) + 'f;

/// The deployment's ports, as Core's jobs reach them: what only the deployment can answer, and Core's own parts as the
/// deployment holds them, lent beside the ports they are reached with. No method decides anything: each reads, asks,
/// or lends, and every rule is the job's ([`Jobs`]).
pub trait Ports {
    /// Core's clock.
    fn clock(&self) -> &dyn Clock;
    /// Who Core acts as, and the operator.
    fn actors(&self) -> Actors;
    /// The orders Core started, each as it holds it, by order.
    fn started(&self) -> &BTreeMap<String, Started>;
    /// The log's entries, each with its receipt, as the log holds them now.
    fn history(&self) -> Vec<(Receipt, Entry)>;
    /// The decision record `id` of `order` as Core's key signed it, held to the digest the log names it by.
    fn decision_record(&self, order: &Id, id: &Id) -> Option<DecisionRecord>;
    /// Core's segments, to read.
    fn segments(&self) -> &Segments;
    /// Core's stuck orders, to read.
    fn stuck(&self) -> &Stuck;
    /// Core's reversals, to read.
    fn reversals(&self) -> &Reversals;
    /// Core's catalogue, to read.
    fn catalogue(&self) -> &Catalogue;
    /// A fresh identifier.
    fn fresh(&mut self) -> Id;

    /// The platform's late answers to the operations of `transactions`, each once: its transaction and its bytes.
    fn late_answers(&mut self, transactions: &BTreeSet<String>) -> Vec<(String, Vec<u8>)>;
    /// The register's public holding accounts, each read once through the platform.
    fn holding_readings(&mut self) -> Vec<AccountRead>;
    /// The register's private nodes' accounts.
    fn publisher_accounts(&self) -> Vec<PublisherAccount>;
    /// Hands `operation` to `module`, signed in the operator's name at Core's clock, through the platform.
    fn ask_as_operator(&mut self, module: &str, operation: Operation) -> Handled;
    /// Runs the checkpointer's ticks due and publishes what they issue (`REQ-AUD-14`); how many it published.
    ///
    /// # Errors
    ///
    /// Why it stopped.
    fn run_checkpoints(&mut self) -> Result<usize, String>;
    /// What the log took reaches the state machine.
    fn fed(&mut self);

    /// Lends the segments, with what they reach.
    fn with_segments(&mut self, with: &mut dyn FnMut(&mut Segments, &mut SegmentParts<'_>));
    /// Lends the reversals, with what they reach and what a question to a provider moves through.
    fn with_reversals(
        &mut self,
        with: &mut dyn FnMut(&mut Reversals, &mut ReversalParts<'_>, &mut Moving<'_>),
    );
    /// Lends the stuck orders, with what they reach, policy and the operator's channel.
    fn with_stuck(&mut self, with: &mut StuckLent<'_>);
    /// Lends the segments with the register of holding accounts, asked through the platform.
    fn with_landings(&mut self, with: &mut dyn FnMut(&mut Segments, &mut dyn Landings));
    /// Lends the segments with the log's comparisons and measure writer.
    fn with_audit(
        &mut self,
        with: &mut dyn FnMut(&mut Segments, &mut dyn Comparisons, &mut dyn Measures),
    );
    /// Lends the price desk, with the segments to read.
    fn with_prices(&mut self, with: &mut dyn FnMut(PriceDesk<'_>, &Segments));
    /// Lends the arrivals with intake's desk.
    fn with_arrivals(&mut self, with: &mut dyn FnMut(&mut Arrivals, &mut Desk<'_, '_>));
    /// Lends the notices with the notice key and the transport.
    fn with_notices(
        &mut self,
        with: &mut dyn FnMut(&mut Notices, &mut dyn NoticeKey, &mut dyn Transport),
    );
}

/// Core's jobs over the deployment's ports: the schedule's [`Duties`], each job's body Core's own.
pub struct Jobs<'p> {
    ports: &'p mut dyn Ports,
}

impl<'p> Jobs<'p> {
    /// Core's jobs over `ports`.
    pub fn new(ports: &'p mut dyn Ports) -> Self {
        Jobs { ports }
    }
}

/// The ids of the orders Core started, in order.
fn orders(ports: &dyn Ports) -> Vec<Id> {
    ports.started().keys().filter_map(|k| id(k)).collect()
}

/// The segments of `order` as Core follows them.
fn segments_of(ports: &dyn Ports, order: &Id) -> Vec<Id> {
    ports
        .segments()
        .order(order)
        .map(|followed| {
            followed
                .parts
                .iter()
                .flat_map(|p| p.segments.iter())
                .filter_map(|s| id(s.segment_id.as_str()))
                .collect()
        })
        .unwrap_or_default()
}

/// The route `order`'s part `first` runs on now, as Core's own record holds it, which a reopened Core rebuilds from its
/// log (`Segments::routes_ran`, the round-2 review of `T-020-48`, its B4): the last recalculated route of the part, its
/// record held through the decision records Core's key signed, and its segments those of the part's current route as Core
/// follows it after the ones that had settled before the recalculation, one for each operation of the record's route;
/// the first route where none was recalculated. `None` where a recalculated route's record isn't held, or its segments
/// aren't the route's (fails closed: the round says so).
fn current_route(
    ports: &dyn Ports,
    order: &Id,
    started: &Started,
    first: &Placed,
) -> Option<(DecisionRecord, Vec<Id>)> {
    let last = ports
        .segments()
        .routes_ran(order)
        .and_then(|(_, later)| later.iter().rev().find(|r| r.part == first.part).cloned());
    let Some(last) = last else {
        return Some((started.record.clone(), first.segments.clone()));
    };
    let record = ports.decision_record(order, &last.record)?;
    // The part's current route is what had settled before the recalculation, then the new route's segments.
    let operations = record.chosen()?.parts.first().operations.len();
    let segments = ports
        .segments()
        .order(order)?
        .parts
        .iter()
        .find(|p| p.part_id.as_str() == first.part.as_str())?
        .segments
        .iter()
        .skip(last.settled_before)
        .map(|s| id(s.segment_id.as_str()))
        .collect::<Option<Vec<_>>>()?;
    (segments.len() == operations).then_some((record, segments))
}

/// The receipt of the log's `decision_record` entry for `record`.
fn record_entry(history: &[(Receipt, Entry)], record: &DecisionRecord) -> Option<Receipt> {
    history.iter().find_map(|(receipt, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::DecisionRecord {
                decision_record, ..
            } if decision_record == record.record_id() => Some(receipt.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })
}

/// Who handed `bytes` to Core: the maker its envelope names, as the provider of the publisher the catalogue lists for
/// it (the operator's for a module it publishes).
fn handed_over_by(catalogue: &Catalogue, bytes: &[u8]) -> Option<Actor> {
    let acts = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v["maker"].as_str().and_then(id))?;
    let party = match catalogue.publishers().publisher(acts.as_str()) {
        Some(Publisher::Operator) => id("operator"),
        Some(Publisher::Party(publisher)) => id(&publisher),
        None => None,
    }
    .unwrap_or_else(|| acts.clone());
    Some(Actor {
        party,
        acts,
        role: Role::Provider,
        kind: ActorKind::Component,
    })
}

/// What a module's failure to give a balance is, as the raise names it.
fn not_read(why: &WalletError) -> HoldingNotRead {
    match why {
        WalletError::NotAnswered(_) => HoldingNotRead::NotAnswered {},
        WalletError::NotSucceeded | WalletError::NotKnown => HoldingNotRead::Refused {},
        WalletError::WrongResult | WalletError::Disagrees { .. } => {
            HoldingNotRead::AnotherAnswer {}
        }
        WalletError::Holding(_) | WalletError::NoHoldingAccount(_) => {
            HoldingNotRead::NotABalance {}
        }
    }
}

/// Whether the log holds a spell of `account` not read: a raise of it since its last check, so it is not raised again
/// until it is read (rebuilt from the log, so a restart raises no spell twice). And the account's last check.
fn spell_of(history: &[(Receipt, Entry)], account: &Id) -> (bool, Option<Receipt>) {
    let mut in_spell = false;
    let mut last = None;
    for (receipt, entry) in history {
        let Entry::Verified(v) = entry else { continue };
        match &v.what {
            What::HoldingCheck { account: a, .. } if a == account => {
                in_spell = false;
                last = Some(receipt.clone());
            }
            What::Raise { raise } => {
                if let Difference::HoldingNotRead { account: a, .. } = &raise.difference
                    && a == account
                {
                    in_spell = true;
                }
            }
            _ => {}
        }
    }
    (in_spell, last)
}

/// A holding reading of the contract's shape: the account named `holding-` and its node, what each order holds there
/// (each once, none at zero), at Core's clock. `None` where a field isn't one the contract carries.
#[allow(clippy::too_many_arguments)]
fn reading_of(
    node: &str,
    provider: &str,
    asset: &str,
    decimals: u8,
    balance: &str,
    word: Word,
    held: &[(String, String)],
    at: &rutent_contracts::router_api::Timestamp,
) -> Option<HoldingReading> {
    Some(HoldingReading {
        account: id(&format!("holding-{node}"))?,
        provider: id(provider)?,
        node: id(node)?,
        asset: Denomination {
            id: asset.to_owned(),
            decimals,
        },
        balance: Amount::new(balance)?,
        word,
        held_for: held
            .iter()
            .filter(|(_, amount)| amount != "0")
            .map(|(order, amount)| {
                Some(rutent_contracts::core_internal::Held {
                    order: id(order)?,
                    amount: PositiveAmount::new(amount.clone())?,
                })
            })
            .collect::<Option<_>>()?,
        at: at.clone(),
    })
}

/// One account to check: where it is, its module's provider, and what was read of it.
struct ToCheck {
    node: String,
    provider: String,
    reading: Result<HoldingReading, (HoldingNotRead, String)>,
}

impl Jobs<'_> {
    /// Every holding account read, each once this check (`REQ-AUD-15`, the review's `L3`): a public node's through the
    /// register, its module's balance beside the host's attribution; a private node's through its publisher's module,
    /// asked in the operator's name, its balance the publisher's word beside what Core's segments rest there.
    fn read_accounts(&mut self) -> Vec<ToCheck> {
        let Some(at) = self.ports.clock().now().timestamp() else {
            return Vec::new();
        };
        let mut checks = Vec::new();
        for read in self.ports.holding_readings() {
            let reading = match read.read {
                Ok(got) => {
                    let word = match got.source {
                        BalanceSource::Module => Word::Checked,
                        BalanceSource::Publisher(_) => Word::Publisher,
                    };
                    let held: Vec<(String, String)> = got
                        .held
                        .iter()
                        .map(|h| (h.order.clone(), h.amount.to_string()))
                        .collect();
                    reading_of(
                        &got.node,
                        &read.provider,
                        &got.asset,
                        read.decimals,
                        &got.balance.to_string(),
                        word,
                        &held,
                        &at,
                    )
                    .ok_or((
                        HoldingNotRead::NotABalance {},
                        "the reading isn't one the contract carries".to_owned(),
                    ))
                }
                Err(why) => Err((not_read(&why), format!("{why:?}"))),
            };
            checks.push(ToCheck {
                node: read.node,
                provider: read.provider,
                reading,
            });
        }
        let resting = self.ports.segments().resting_totals();
        for account in self.ports.publisher_accounts() {
            let reading = self.publisher_reading(&account, &resting, &at);
            checks.push(ToCheck {
                node: account.node,
                provider: account.module,
                reading,
            });
        }
        checks
    }

    /// The reading of a private node's account: its publisher's module's answer to the operator's `holding_balance`,
    /// under a fresh transaction of Core's, and what Core's segments rest at the node for each order.
    fn publisher_reading(
        &mut self,
        account: &PublisherAccount,
        resting: &BTreeMap<(String, String), Amount>,
        at: &rutent_contracts::router_api::Timestamp,
    ) -> Result<HoldingReading, (HoldingNotRead, String)> {
        let transaction = self.ports.fresh().as_str().to_owned();
        let operation = Operation::HoldingBalance(HoldingBalance {
            transaction: transaction.clone(),
            provider: account.module.clone(),
            node: account.node.clone(),
            asset: account.asset.clone(),
        });
        let answered = match self.ports.ask_as_operator(&account.module, operation) {
            Handled::Answered(result) => result.statement,
            Handled::Refused(why) => {
                return Err((
                    HoldingNotRead::NotAnswered {},
                    format!("the platform passed no answer: {why:?}"),
                ));
            }
        };
        let ResultBody::HoldingBalance(result) = answered else {
            return Err((
                HoldingNotRead::AnotherAnswer {},
                "the module answered another question".to_owned(),
            ));
        };
        if result.transaction != transaction {
            return Err((
                HoldingNotRead::AnotherAnswer {},
                "the answer names another transaction".to_owned(),
            ));
        }
        let HoldingBalanceOutcome::Answered(answer) = result.outcome else {
            return Err((HoldingNotRead::Refused {}, "the module refused".to_owned()));
        };
        let held: Vec<(String, String)> = resting
            .iter()
            .filter(|((node, _), _)| node == &account.node)
            .map(|((_, order), amount)| (order.clone(), amount.as_str().to_owned()))
            .collect();
        reading_of(
            &account.node,
            &account.module,
            &account.asset,
            account.decimals,
            &answer.balance,
            Word::Publisher,
            &held,
            at,
        )
        .ok_or((
            HoldingNotRead::NotABalance {},
            "the balance isn't an amount".to_owned(),
        ))
    }

    /// The stuck move `one` reported to its mover from what Core kept of its provider's answers (`AC-6`, `REQ-ORD-1`):
    /// only a segment of a part Core declared stuck, only an answer the segments judge the move's own provider's and
    /// saying failed or settled (typed, `L5`), the move's mover as the reporter, and the statement named by the entry
    /// it was kept as, once (no second entry of the same bytes). One already reported is not told again.
    fn report_kept_for(&mut self, one: &Unknown) -> Vec<Reported> {
        let Some(started) = self.ports.started().get(one.order.as_str()).cloned() else {
            return Vec::new();
        };
        let Some(placed) = started.placed(&one.segment) else {
            return Vec::new();
        };
        if !self.ports.stuck().is_stuck(&one.order, &placed.part) {
            return Vec::new();
        }
        let Some(moved) = self.ports.segments().unknown_move(&one.order, &one.segment) else {
            return Vec::new();
        };
        let kept: Vec<(Vec<u8>, Option<Receipt>)> = self
            .ports
            .reversals()
            .kept_statement_receipts(&one.order, &one.segment)
            .into_iter()
            .map(|(bytes, receipt)| (bytes.to_vec(), receipt.cloned()))
            .collect();
        let actors = self.ports.actors();
        let history = self.ports.history();
        let entry = record_entry(&history, &started.record);
        let mut reported = Vec::new();
        for (bytes, receipt) in kept {
            let live = started.live(&one.order, &placed, &actors, entry.as_ref());
            let mut late = None;
            self.ports.with_segments(&mut |segments, parts| {
                late = Some(match &receipt {
                    Some(kept) => segments.late_kept(parts, &live, &one.segment, &bytes, kept),
                    None => segments.late_statement(parts, &live, &one.segment, &bytes),
                });
            });
            let Some(Ok(late)) = late else { continue };
            let outcome = match late.says {
                Says::Failed => Resolved::Failed,
                Says::Settled => Resolved::Done,
                Says::Unknown | Says::Nothing => continue,
            };
            let history = self.ports.history();
            let mut taken = None;
            self.ports
                .with_stuck(&mut |stuck, parts, policy, _channel| {
                    taken = Some(stuck.report_segment_outcome(
                        parts,
                        policy,
                        Reporting {
                            order: &one.order,
                            part: &placed.part,
                            segment: &one.segment,
                            reporter: &moved.mover,
                        },
                        outcome,
                        &Stated::from(&late),
                        &history,
                    ));
                });
            let Some(taken) = taken else { continue };
            if matches!(taken, Err(ResolveError::AlreadyReported)) {
                continue;
            }
            reported.push(Reported {
                unknown: one.clone(),
                statement: late.receipt.clone(),
                taken: taken
                    .map_err(|why| format!("{why:?}"))
                    .and_then(|moved| match moved {
                        Moved::AwaitsConfirmation { report } => Ok(report),
                        other => Err(format!("{other:?}")),
                    }),
            });
        }
        self.ports.fed();
        reported
    }
}

/// Why a mover's reported failure wasn't confirmed.
#[derive(Debug)]
pub enum NotConfirmed {
    /// The order isn't one Core started, or doesn't plan the segment.
    NotStarted,
    /// The statement isn't taken as evidence of the move's outcome.
    NotEvidence(crate::segments::LateRefused),
    /// The stuck mover refused the confirmation.
    Refused(ResolveError),
    /// The deployment lent nothing.
    NotLent,
}

/// Confirms the failed outcome a mover reported for the unknown `segment` of `order` (`Stuck::confirm_segment_outcome`,
/// `AC-6`, the review's `L4`): `confirmer` is another actor than the reporter, `report` the report's version and the
/// statement it rests on (what the schedule's round told), and `other` another maker's statement about the move's
/// transaction, which bears the report out, taken as evidence once. Core records the move's final result, `failed`,
/// and the segment's `segment_failed`.
///
/// # Errors
///
/// If the order hasn't started, the statement isn't taken as evidence, or the confirmation is refused.
pub fn confirm_reported_failure(
    ports: &mut dyn Ports,
    order: &Id,
    segment: &Id,
    confirmer: &Actor,
    report: (u64, &Receipt),
    other: &[u8],
) -> Result<Moved, NotConfirmed> {
    let started = ports
        .started()
        .get(order.as_str())
        .cloned()
        .ok_or(NotConfirmed::NotStarted)?;
    let placed = started.placed(segment).ok_or(NotConfirmed::NotStarted)?;
    let actors = ports.actors();
    let history = ports.history();
    let entry = record_entry(&history, &started.record);
    let live = started.live(order, &placed, &actors, entry.as_ref());
    let mut late = None;
    ports.with_segments(&mut |segments, parts| {
        late = Some(segments.late_statement(parts, &live, segment, other));
    });
    let late = late
        .ok_or(NotConfirmed::NotLent)?
        .map_err(NotConfirmed::NotEvidence)?;
    let stated = Stated::from(&late);
    let history = ports.history();
    let mut moved = None;
    ports.with_stuck(&mut |stuck, parts, policy, _channel| {
        moved = Some(stuck.confirm_segment_outcome(
            parts,
            policy,
            ConfirmingSegment {
                order,
                part: &placed.part,
                segment,
                confirmer,
                outcome: Resolved::Failed,
                report: report.0,
                reported_with: report.1,
                evidence: &stated,
                core: &actors.core,
            },
            &history,
        ));
    });
    ports.fed();
    moved
        .ok_or(NotConfirmed::NotLent)?
        .map_err(NotConfirmed::Refused)
}

impl Duties for Jobs<'_> {
    fn late_answers(&mut self) -> Vec<LateTaken> {
        let mut taken = Vec::new();
        for order in orders(&*self.ports) {
            let Some(started) = self.ports.started().get(order.as_str()).cloned() else {
                continue;
            };
            let mut sent = BTreeMap::new();
            for segment in segments_of(&*self.ports, &order) {
                if let Some((transaction, _)) = self.ports.segments().sent(&order, &segment) {
                    sent.insert(transaction.as_str().to_owned(), segment);
                }
            }
            if sent.is_empty() {
                continue;
            }
            let transactions = sent.keys().cloned().collect();
            for (transaction, bytes) in self.ports.late_answers(&transactions) {
                let Some(segment) = sent.get(&transaction) else {
                    continue;
                };
                let Some(placed) = started.placed(segment) else {
                    continue;
                };
                let history = self.ports.history();
                let mut kept = Err("the reversals weren't lent".to_owned());
                self.ports.with_reversals(&mut |reversals, parts, _| {
                    reversals.follow(&order, &history);
                    // The platform hands over its provider's own late answer: the maker its envelope names presents it.
                    let Some(presented_by) = handed_over_by(parts.catalogue, &bytes) else {
                        kept = Err("the answer names no maker".to_owned());
                        return;
                    };
                    kept = reversals
                        .later(
                            parts,
                            At {
                                order: &order,
                                part: &placed.part,
                                segment,
                            },
                            &bytes,
                            &presented_by,
                        )
                        .map_err(|why| format!("{why:?}"));
                });
                self.ports.fed();
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
        let mut landed = 0;
        self.ports
            .with_landings(&mut |segments, landings| landed = segments.land_settled(landings));
        self.ports.fed();
        landed
    }

    fn write_measures(&mut self) -> usize {
        let mut written = 0;
        self.ports.with_audit(&mut |segments, _, measures| {
            written = segments.write_measures(measures);
        });
        written
    }

    fn write_pairs(&mut self) -> Result<usize, String> {
        let mut written = Err("the comparisons weren't lent".to_owned());
        self.ports.with_audit(&mut |segments, comparisons, _| {
            written = segments
                .write_pairs(comparisons)
                .map_err(|why| format!("{why:?}"));
        });
        written
    }

    fn close_completed(&mut self) -> Vec<Priced> {
        let mut priced = Vec::new();
        for order in orders(&*self.ports) {
            // The record of each route the order ran, as Core's key signed it and the log names it by digest.
            let mut records = BTreeMap::new();
            if let Some((first, later)) = self.ports.segments().routes_ran(&order) {
                let ids: Vec<Id> = std::iter::once(first.clone())
                    .chain(later.iter().map(|r| r.record.clone()))
                    .collect();
                for record in ids {
                    if let Some(held) = self.ports.decision_record(&order, &record) {
                        records.insert(record.as_str().to_owned(), held);
                    }
                }
            }
            let mut outcome = None;
            self.ports.with_prices(&mut |desk, segments| {
                let before = desk.prices.of(&order).is_some();
                outcome = Some((
                    before,
                    report_logged_price(desk, &order, segments, &|id: &Id| {
                        records.get(id.as_str()).cloned()
                    }),
                ));
            });
            match outcome {
                Some((before, Ok(Some(reported)))) => priced.push(Priced {
                    order: order.clone(),
                    reported: Ok(reported),
                    now: !before,
                }),
                Some((_, Err(why))) => priced.push(Priced {
                    order: order.clone(),
                    reported: Err(why),
                    now: false,
                }),
                Some((_, Ok(None))) | None => {}
            }
        }
        priced
    }

    fn check_holdings(&mut self) -> Vec<HoldingChecked> {
        let history = self.ports.history();
        let mut checked = Vec::new();
        for one in self.read_accounts() {
            let (reading, check, unread) = match one.reading {
                Ok(reading) => {
                    let mut check = Err("the comparisons weren't lent".to_owned());
                    self.ports.with_audit(&mut |_, comparisons, _| {
                        check = comparisons
                            .holding_reading(&reading)
                            .map_err(|why| format!("{why:?}"));
                    });
                    (Ok(reading), check, None)
                }
                Err((kind, why)) => {
                    // Raised to the operator, once in a spell of failure (fails closed: a module that keeps refusing
                    // hides no difference silently, `REQ-AUD-15`, `REQ-ORD-7`).
                    let raised = match (
                        id(&format!("holding-{}", one.node)),
                        id(&one.node),
                        id(&one.provider),
                    ) {
                        (Some(account), Some(node), Some(provider)) => {
                            let (in_spell, last) = spell_of(&history, &account);
                            if in_spell {
                                Err("raised already in this spell".to_owned())
                            } else {
                                let mut raised = Err("the comparisons weren't lent".to_owned());
                                self.ports.with_audit(&mut |_, comparisons, _| {
                                    raised = comparisons
                                        .holding_not_read(
                                            &account,
                                            &node,
                                            &provider,
                                            &kind,
                                            last.as_ref(),
                                        )
                                        .map_err(|why| format!("{why:?}"));
                                });
                                raised
                            }
                        }
                        _ => Err("the account isn't named by identifiers".to_owned()),
                    };
                    (Err(why.clone()), Err(why), Some(raised))
                }
            };
            checked.push(HoldingChecked {
                node: one.node,
                reading,
                check,
                not_read: unread,
            });
        }
        checked
    }

    /// Each private node a started order's plan passes, as Core's own records of the orders it runs name it, asked of
    /// each provider of the node's publisher that declares it (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`): each read is
    /// decided by policy and recorded as an effect, as closing's is ([`Segments::read_node_record`]), and the answer
    /// is taken as a record only as `closing::publishers::taken` allows. The deployment carries each question to the
    /// platform and chooses neither node nor provider; where policy refuses a read nothing is asked.
    fn read_publishers(&mut self) -> Vec<Published> {
        // (order, segment, node, bounds) for the first segment of each order's plan that passes each private node.
        let mut places: Vec<(Id, Id, String, rutent_contracts::router_api::PolicyBounds)> =
            Vec::new();
        for (order, started) in self.ports.started() {
            let Some(order) = id(order) else { continue };
            for segment in started.plan.iter().flat_map(|part| part.segments.iter()) {
                for end in [&segment.from.node, &segment.to.node] {
                    let name = match end {
                        rutent_contracts::provider_role::NodeRef::Inner(id) => id.as_str(),
                        rutent_contracts::provider_role::NodeRef::Outer(o) => o.network.as_str(),
                    };
                    let private = self
                        .ports
                        .catalogue()
                        .publishers()
                        .publisher_of_node(name)
                        .is_some();
                    let seen = places.iter().any(|(o, _, n, _)| *o == order && n == name);
                    if private
                        && !seen
                        && let Some(segment_id) = id(segment.segment_id.as_str())
                    {
                        places.push((
                            order.clone(),
                            segment_id,
                            name.to_owned(),
                            started.record.bounds.clone(),
                        ));
                    }
                }
            }
        }
        let actors = self.ports.actors();
        let mut wanted = Vec::new();
        for (order, segment, node, bounds) in places {
            for provider in crate::closing::publishers::serving(self.ports.catalogue(), &node) {
                wanted.push((
                    order.clone(),
                    segment.clone(),
                    node.clone(),
                    provider,
                    bounds.clone(),
                ));
            }
        }
        let mut answers = Vec::new();
        self.ports.with_segments(&mut |segments, parts| {
            for (order, segment, node, provider, bounds) in &wanted {
                let read = crate::segments::RecordRead {
                    order,
                    segment,
                    node,
                    provider,
                    bounds,
                    operator: &actors.operator,
                    core: &actors.core,
                };
                if let Some((question, bytes)) = segments.read_node_record(parts, &read) {
                    answers.push((node.clone(), question, bytes));
                }
            }
        });
        let at = self.ports.clock().now().floor();
        answers
            .into_iter()
            .filter_map(|(node, question, bytes)| {
                crate::closing::publishers::taken(
                    self.ports.catalogue(),
                    at,
                    &question.statement,
                    &bytes,
                )
                .ok()
                .map(|record| Published { node, record })
            })
            .collect()
    }

    fn unknown_segments(&self) -> Vec<Unknown> {
        let mut unknown = Vec::new();
        for order in orders(&*self.ports) {
            for segment in segments_of(&*self.ports, &order) {
                if let Some(moved) = self.ports.segments().unknown_move(&order, &segment) {
                    unknown.push(Unknown {
                        order: order.clone(),
                        segment,
                        provider: moved.provider.as_str().to_owned(),
                    });
                }
            }
        }
        unknown
    }

    fn ask_status_again(&mut self, unknown: &Unknown) -> StatusTried {
        let placed = self
            .ports
            .started()
            .get(unknown.order.as_str())
            .and_then(|s| s.placed(&unknown.segment));
        let Some(placed) = placed else {
            return StatusTried {
                unknown: unknown.clone(),
                asked: Err("the order didn't start".to_owned()),
                kept: None,
                status: None,
            };
        };
        let history = self.ports.history();
        let mut asked = Err("the reversals weren't lent".to_owned());
        self.ports.with_reversals(&mut |reversals, parts, moving| {
            reversals.follow(&unknown.order, &history);
            asked = match reversals.ask_unknown_status_again(
                parts,
                moving,
                At {
                    order: &unknown.order,
                    part: &placed.part,
                    segment: &unknown.segment,
                },
            ) {
                Ok(Some(asked)) => Ok(asked),
                Ok(None) => Err("no question was put".to_owned()),
                Err(why) => Err(format!("{why:?}")),
            };
        });
        self.ports.fed();
        StatusTried {
            unknown: unknown.clone(),
            kept: asked.as_ref().ok().and_then(|a| a.answer.clone()),
            status: asked.as_ref().ok().cloned(),
            asked: asked.map(|a| a.effect),
        }
    }

    fn watch(&mut self) -> Vec<Watched> {
        let core = self.ports.actors().core;
        let mut watched = Vec::new();
        for order in orders(&*self.ports) {
            let Some(started) = self.ports.started().get(order.as_str()) else {
                continue;
            };
            let Some(first) = started.first() else {
                continue;
            };
            let Some((record, segments)) = current_route(&*self.ports, &order, started, &first)
            else {
                // Fails closed: a route Core can't name now is answered, never timed on another route.
                watched.push(Watched {
                    order,
                    found: Err("the current route's record isn't held".to_owned()),
                });
                continue;
            };
            let mut found = Err("the stuck orders weren't lent".to_owned());
            self.ports.with_stuck(&mut |stuck, parts, _, channel| {
                found = stuck
                    .watch(
                        parts,
                        channel,
                        Watching {
                            order: &order,
                            part: &first.part,
                            segments: &segments,
                            record: &record,
                            at: 0,
                            core: &core,
                        },
                    )
                    .map_err(|why| format!("{why:?}"));
            });
            self.ports.fed();
            if !matches!(found, Ok(Watch::Running | Watch::AlreadyStuck)) {
                watched.push(Watched { order, found });
            }
        }
        watched
    }

    fn report_kept_answers(&mut self) -> Vec<Reported> {
        let mut reported = Vec::new();
        for one in self.unknown_segments() {
            reported.extend(self.report_kept_for(&one));
        }
        reported
    }

    fn end_expired(&mut self) -> Vec<Id> {
        let mut ended = Vec::new();
        self.ports.with_arrivals(&mut |arrivals, desk| {
            ended = arrivals.end_expired(desk).unwrap_or_default();
        });
        self.ports.fed();
        ended
    }

    fn unbuilt_pairs(&self) -> Vec<UnbuiltPair> {
        self.ports.segments().unbuilt_pairs().to_vec()
    }

    fn raise_unbuilt_pairs(&mut self) -> usize {
        let mut raised = 0;
        self.ports.with_audit(&mut |segments, comparisons, _| {
            raised = segments.write_unbuilt(comparisons).unwrap_or(0);
        });
        raised
    }

    fn notify(&mut self) -> Result<usize, String> {
        let history = self.ports.history();
        let mut sent = Err("the notices weren't lent".to_owned());
        self.ports.with_notices(&mut |notices, key, transport| {
            sent = notices
                .notify(&history, key, transport)
                .map_err(|why| format!("{why:?}"));
        });
        sent
    }

    fn checkpoints(&mut self) -> Result<usize, String> {
        self.ports.run_checkpoints()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Job, Schedule};
    use crate::snapshot::Reading;
    use rutent_contracts::core_internal::calls::AppendFailure;
    use rutent_contracts::core_internal::{MeasureRecord, PairNotBuilt, ReportPair};
    use rutent_contracts::module_interface::PlatformRefusal;
    use rutent_prov::holding::{HeldForOrder, HoldingReading as Read};

    struct At(u64);

    impl Clock for At {
        fn now(&self) -> Reading {
            Reading::at(self.0)
        }
    }

    /// What the log's comparisons were handed.
    #[derive(Default)]
    struct Told {
        readings: Vec<HoldingReading>,
        unread: Vec<(Id, Id, Id, HoldingNotRead, Option<Receipt>)>,
    }

    impl Comparisons for Told {
        fn report_pair(&mut self, _: &ReportPair) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn holding_reading(&mut self, reading: &HoldingReading) -> Result<Receipt, AppendFailure> {
            self.readings.push(reading.clone());
            Err(AppendFailure::Unavailable)
        }
        fn pair_not_built(
            &mut self,
            _: &Id,
            _: &Id,
            _: &PairNotBuilt,
        ) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn holding_not_read(
            &mut self,
            account: &Id,
            node: &Id,
            provider: &Id,
            why: &HoldingNotRead,
            last: Option<&Receipt>,
        ) -> Result<Receipt, AppendFailure> {
            self.unread.push((
                account.clone(),
                node.clone(),
                provider.clone(),
                why.clone(),
                last.cloned(),
            ));
            Err(AppendFailure::Unavailable)
        }
    }

    impl Measures for Told {
        fn measure(&mut self, _: &MeasureRecord) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }

    /// A deployment that answers only its ports: its register holds one public account, read through the platform,
    /// and one private node's, whose module gives no answer; it lends Core's parts to the log's comparisons alone, and
    /// no other (each other job finds them not lent, so nothing moves on it, `REQ-AUZ-5`).
    struct Bare {
        clock: At,
        started: BTreeMap<String, Started>,
        segments: Segments,
        stuck: Stuck,
        reversals: Reversals,
        catalogue: Catalogue,
        told: Told,
        read: usize,
        asked: Vec<String>,
        next: u32,
    }

    impl Ports for Bare {
        fn clock(&self) -> &dyn Clock {
            &self.clock
        }
        fn actors(&self) -> Actors {
            let actor = |party: &str| Actor {
                party: id(party).unwrap(),
                acts: id(party).unwrap(),
                role: Role::Core,
                kind: ActorKind::Component,
            };
            Actors {
                core: actor("rutent"),
                operator: actor("operator"),
            }
        }
        fn started(&self) -> &BTreeMap<String, Started> {
            &self.started
        }
        fn history(&self) -> Vec<(Receipt, Entry)> {
            Vec::new()
        }
        fn decision_record(&self, _: &Id, _: &Id) -> Option<DecisionRecord> {
            None
        }
        fn segments(&self) -> &Segments {
            &self.segments
        }
        fn stuck(&self) -> &Stuck {
            &self.stuck
        }
        fn reversals(&self) -> &Reversals {
            &self.reversals
        }
        fn catalogue(&self) -> &Catalogue {
            &self.catalogue
        }
        fn fresh(&mut self) -> Id {
            self.next += 1;
            id(&format!("fresh-{}", self.next)).unwrap()
        }
        fn late_answers(&mut self, _: &BTreeSet<String>) -> Vec<(String, Vec<u8>)> {
            Vec::new()
        }
        fn holding_readings(&mut self) -> Vec<AccountRead> {
            self.read += 1;
            vec![AccountRead {
                node: "bank-ledger".to_owned(),
                provider: "operator-wallet".to_owned(),
                decimals: 2,
                read: Ok(Read {
                    node: "bank-ledger".to_owned(),
                    asset: "USDC".to_owned(),
                    balance: 1500,
                    source: BalanceSource::Module,
                    held: vec![
                        HeldForOrder {
                            order: "ord-1".to_owned(),
                            amount: 1500,
                        },
                        HeldForOrder {
                            order: "ord-2".to_owned(),
                            amount: 0,
                        },
                    ],
                }),
            }]
        }
        fn publisher_accounts(&self) -> Vec<PublisherAccount> {
            vec![PublisherAccount {
                node: "rail".to_owned(),
                publisher: "rail-publisher".to_owned(),
                module: "rail-account".to_owned(),
                asset: "USDC".to_owned(),
                decimals: 2,
            }]
        }
        fn ask_as_operator(&mut self, module: &str, _: Operation) -> Handled {
            self.asked.push(module.to_owned());
            Handled::Refused(PlatformRefusal::Unavailable)
        }
        fn run_checkpoints(&mut self) -> Result<usize, String> {
            Err("the checkpointer can't be reached".to_owned())
        }
        fn fed(&mut self) {}
        fn with_segments(&mut self, _: &mut dyn FnMut(&mut Segments, &mut SegmentParts<'_>)) {}
        fn with_reversals(
            &mut self,
            _: &mut dyn FnMut(&mut Reversals, &mut ReversalParts<'_>, &mut Moving<'_>),
        ) {
        }
        fn with_stuck(&mut self, _: &mut StuckLent<'_>) {}
        fn with_landings(&mut self, _: &mut dyn FnMut(&mut Segments, &mut dyn Landings)) {}
        fn with_audit(
            &mut self,
            with: &mut dyn FnMut(&mut Segments, &mut dyn Comparisons, &mut dyn Measures),
        ) {
            let mut told = std::mem::take(&mut self.told);
            let mut measures = Told::default();
            with(&mut self.segments, &mut told, &mut measures);
            self.told = told;
        }
        fn with_prices(&mut self, _: &mut dyn FnMut(PriceDesk<'_>, &Segments)) {}
        fn with_arrivals(&mut self, _: &mut dyn FnMut(&mut Arrivals, &mut Desk<'_, '_>)) {}
        fn with_notices(
            &mut self,
            _: &mut dyn FnMut(&mut Notices, &mut dyn NoticeKey, &mut dyn Transport),
        ) {
        }
    }

    fn bare() -> Bare {
        Bare {
            clock: At(1_764_000_000),
            started: BTreeMap::new(),
            segments: Segments::new(),
            stuck: Stuck::new(),
            reversals: Reversals::new(),
            catalogue: Catalogue::new(),
            told: Told::default(),
            read: 0,
            asked: Vec::new(),
            next: 0,
        }
    }

    /// Every job's body is Core's: over a deployment that answers only its ports, the tick runs every job, and what
    /// each job makes of the answers is Core's rule, not the deployment's: the holding reading built (its account, its
    /// word, what each order holds, nothing at zero) from the register's one read of each account, the account whose
    /// module gives no answer raised as not read, each account asked once (`L3`); and a job whose parts or port the
    /// deployment can't lend or answer moves nothing and says so (`REQ-AUZ-5`: an input taken away is no permit).
    // verifies: REQ-AUZ-5
    #[test]
    fn every_job_of_the_schedule_is_cores_and_the_deployment_answers_only_its_ports() {
        let mut deployment = bare();
        let mut schedule = Schedule::new();
        let round = schedule.tick(1_764_000_000, &mut Jobs::new(&mut deployment));
        assert_eq!(round.ran, Job::ALL, "every job, in its order");

        // The holding check: one read of each account, the public one through the register, the private one through
        // its publisher's module, asked in the operator's name.
        assert_eq!(deployment.read, 1, "the register is read once a check");
        assert_eq!(
            deployment.asked,
            ["rail-account"],
            "the module is asked once"
        );
        let [reading] = deployment.told.readings.as_slice() else {
            panic!("one reading: {:?}", deployment.told.readings);
        };
        assert_eq!(reading.account.as_str(), "holding-bank-ledger");
        assert_eq!(reading.provider.as_str(), "operator-wallet");
        assert_eq!(reading.word, Word::Checked);
        assert_eq!(reading.balance.as_str(), "1500");
        assert_eq!(
            reading
                .held_for
                .iter()
                .map(|h| (h.order.as_str(), h.amount.amount().as_str()))
                .collect::<Vec<_>>(),
            [("ord-1", "1500")],
            "an order holding nothing there is not named"
        );
        assert_eq!(
            deployment.told.unread,
            [(
                id("holding-rail").unwrap(),
                id("rail").unwrap(),
                id("rail-account").unwrap(),
                HoldingNotRead::NotAnswered {},
                None,
            )],
            "the account its module didn't answer for is raised, citing nothing: it was never checked"
        );
        let rail = round
            .holding_of("rail")
            .expect("the private account is in the round");
        assert!(rail.reading.is_err() && rail.check.is_err() && rail.not_read.is_some());

        // What the deployment can't lend or answer moves nothing, and the round says so.
        assert_eq!(round.landed, 0);
        assert!(round.closed.is_empty() && round.ended.is_empty() && round.reported.is_empty());
        assert!(matches!(&round.notified, Some(Err(why)) if why.contains("lent")));
        assert!(matches!(&round.checkpoints, Some(Err(_))));
        assert_eq!(
            round.pairs,
            Some(Ok(0)),
            "the comparisons were lent: nothing waits"
        );
    }
}
