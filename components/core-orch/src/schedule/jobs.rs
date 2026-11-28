//! Each job of Core's schedule, as Core runs it (`T-020-48`, the review of its first build, B2): the body of every job
//! is here, in `core-orch`, over [`Ports`], which the deployment answers and which decide nothing.
//!
//! # Whose is what
//!
//! | Job | Core's body here | What the deployment answers ([`Ports`]) |
//! |---|---|---|
//! | late answers | the transactions each segment was sent under, the answer kept once as its maker's statement (`Reversals::later`), and acknowledged to the platform once kept or refused for good | the platform's held late answers to those transactions, which it hands over again until acknowledged |
//! | landing | `Segments::land_settled` | the register of holding accounts, asked through the platform ([`Landings`]) |
//! | measures, pairs, unbuilt pairs | `Segments::write_measures`, `write_pairs`, `write_unbuilt` | the log's comparisons and measure writer (`CMP-CORE-AUDIT`) |
//! | closing | which orders, the records of the routes each ran (intake's, each held to the log's digest), `closing::report_logged_price` | nothing more |
//! | holding check | each account of the register read once a check (`L3`) by a decided read ([`Segments::read_holding_balance`]), the reading built (account, provider, word, what Core's segments rest there, time) and checked, a public node's beside the wallet host's own record as a second comparison; an account not read, or the catalogue's holder the register doesn't name, raised once a spell (`holding_not_read`) | the register of holding accounts and the host's record of each; the platform, which carries the question to the module |
//! | status questions | which segments are unknown, at whose cadence ([`super::Schedule`]), `Reversals::ask_unknown_status_again` | the platform, policy |
//! | stuck watch | which orders, their current route, `Stuck::watch`; a route Core can't name declared stuck (L7) | the notices channel |
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

use rutent_contracts::core_internal::calls::{
    AppendFailure, Authorisations, Comparisons, Measures, Operator, StuckOrders,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Authorisation, Difference, Entry, HoldingNotRead, HoldingReading, Receipt,
    Role, What, Word,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    Denomination, HoldingBalanceOutcome, ResultBody, Statement, Tracking,
};
use rutent_contracts::router_api::{Amount, DecisionRecord, Id, PolicyBounds, PositiveAmount};
use rutent_prov::catalogue::{Catalogue, Publisher};
use rutent_prov::holding::HeldForOrder;

use super::{
    Duties, HoldingChecked, Late, LateTaken, Priced, Published, Reported, StatusTried, Unknown,
    WalletCompared, Watched,
};
use crate::arrival::Arrivals;
use crate::closing::{PriceDesk, report_logged_price};
use crate::hand_on::{HandOn, Parts as HandParts};
use crate::intake::Desk;
use crate::intake::Intake;
use crate::notice::{NoticeKey, Notices, Transport};
use crate::recalc::{Parts as RecalcParts, Recalculations};
use crate::reversal::{At, Moving, Parts as ReversalParts, ReversalError, Reversals};
use crate::segments::{
    Balance, BalanceRead, Landings, Parts as SegmentParts, RepeatOf, Repeated, Says, Segments,
    UnbuiltPair,
};
use crate::snapshot::Clock;
use crate::snapshot::Price;
use crate::stuck::{
    ConfirmingSegment, Moved, Parts as StuckParts, Reporting, ResolveError, Resolved, Stated,
    Stuck, Watch, Watching,
};
use crate::stuck::{Delivering, Mover};

pub use crate::intake::{Placed, Started};

fn id(text: &str) -> Option<Id> {
    Id::new(text)
}

/// Who Core acts as, and the deployment's operator, in whose name Core signs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actors {
    /// Core, acting on its own account.
    pub core: Actor,
    /// The operator.
    pub operator: Actor,
}

/// A public node's holding account, as the deployment's register of holding accounts names it (`REQ-CAT-23`): the module
/// that holds it and its asset, with the decimals the register gives the asset, beside the wallet host's own record of
/// what each order holds there (`REQ-AUD-15`). The balance is not here: Core reads it from the module, as a decided read
/// ([`Segments::read_holding_balance`]), and compares it with what its own segments rest at the node
/// ([`Segments::resting_totals`]), keeping the host's record as a second comparison (`T-020-63`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicAccount {
    /// The node.
    pub node: String,
    /// The provider whose module holds the account.
    pub provider: String,
    /// The asset the account holds.
    pub asset: String,
    /// The decimals of the account's asset, as the register gives them.
    pub decimals: u8,
    /// What the wallet host's own record says each order holds there, in order name order.
    pub attribution: Vec<HeldForOrder>,
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

/// What a job does with the recalculations the deployment lends: the recalculations, what they reach, and the prices.
pub type RecalculationsLent<'f> =
    dyn FnMut(&mut Recalculations, &mut RecalcParts<'_>, &[Price]) + 'f;

/// What a stuck command does with what the deployment lends to carry a move out: the stuck orders, what they reach,
/// policy, and the production mover with what a hand-on's withdrawal reaches.
pub type MoverLent<'f> =
    dyn FnMut(&mut Stuck, &mut StuckParts<'_>, &mut dyn StuckOrders, &mut Delivering<'_>) + 'f;

/// The platform as a reversal reaches it: the modules a reversal asked of a failed provider is carried out through, and
/// the providers' modules a reserve's confirmation question is handed to, exactly as Core signed it (`REQ-ORD-3`).
pub trait ReversalPlatform {
    /// The module platform, which carries out a decided reversal.
    fn modules(&mut self) -> &mut dyn crate::segments::Modules;
    /// The providers' modules, asked a question that moves nothing.
    fn providers(&mut self) -> &mut dyn crate::rounds::Providers;
}

/// What a reversal step reaches besides the reversals' parts: the operator key a question is signed with, policy's one
/// decision call, the platform, the stuck orders a dispute is declared through, and the register of actors that binds
/// an operator's login to the party it states (`REQ-ORD-3`, `REQ-ORD-11`, `REQ-ORD-30`, `T-020-61` item 10).
pub struct ReversalDesk<'a> {
    /// The operator key.
    pub key: &'a mut dyn crate::rounds::OperatorKey,
    /// Policy's one decision call.
    pub decisions: &'a mut dyn rutent_contracts::core_internal::calls::Decisions,
    /// The platform.
    pub platform: &'a mut dyn ReversalPlatform,
    /// The stuck orders.
    pub stuck: &'a mut Stuck,
    /// The register of actors.
    pub actors: &'a mut dyn crate::intake::Actors,
    /// The public records, read through Core's adapter for each listed network's reader in this pass: a reversal back
    /// into a held public node is confirmed on its network's record (`T-020-60` (6)).
    pub records: &'a dyn crate::closing::PublicRecords,
}

/// What a reversal step does with what the deployment lends: the reversals, what they reach, and the desk.
pub type ReversalsLent<'f> =
    dyn FnMut(&mut Reversals, &mut ReversalParts<'_>, &mut ReversalDesk<'_>) + 'f;

/// The deployment's ports, as Core's jobs reach them: what only the deployment can answer, and Core's own parts as the
/// deployment holds them, lent beside the ports they are reached with. No method decides anything: each reads, asks,
/// or lends, and every rule is the job's ([`Jobs`]).
pub trait Ports {
    /// Core's clock.
    fn clock(&self) -> &dyn Clock;
    /// Who Core acts as, and the operator.
    fn actors(&self) -> Actors;
    /// The log's entries, each with its receipt, as the log holds them now.
    fn history(&self) -> Vec<(Receipt, Entry)>;
    /// The bytes the evidence store keeps behind `commitment`, as the log's own read gives them: what Core reads its
    /// decision records back from, held to the log's digests (`T-020-61`).
    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>>;
    /// The authorisation `authorisation` of `order` with the terms policy's register holds in force for it, as policy
    /// reads it from the log: `None` where it holds it in force no more, or holds none (`T-020-61`).
    fn in_force(&mut self, order: &Id, authorisation: &Id) -> Option<Authorisation>;
    /// Core's intake, to read: the orders Core started and what each runs on (`T-020-61`).
    fn intake(&self) -> &Intake;
    /// Core's arrivals, to read: the filings an order filed on an automatic deposit starts on.
    fn arrivals(&self) -> &Arrivals;
    /// Core's stuck mover, to read: the hand-on withdrawals it carried out.
    fn mover(&self) -> &Mover;
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

    /// The bounds Core's questions are made in: the deployment's and protocol version, as the operator's settings leave
    /// them.
    fn bounds(&self) -> PolicyBounds;
    /// The platform's late answers to the operations of `transactions`: its transaction and its bytes. The platform holds
    /// each until Core says it kept it ([`Ports::late_kept`]), so one Core couldn't record now (the log down) comes again
    /// the next tick, a restart included, and is never dropped (`NFR-6`, `T-020-63`).
    fn late_answers(&mut self, transactions: &BTreeSet<String>) -> Vec<(String, Vec<u8>)>;
    /// That Core has done with the late answer to `transaction`: kept, or refused for good. The platform hands it over no
    /// more.
    fn late_kept(&mut self, transaction: &str);
    /// The register's public holding accounts, with the wallet host's own record of each. Core reads their balances
    /// itself, through [`Ports::with_segments`].
    fn public_accounts(&self) -> Vec<PublicAccount>;
    /// The register's private nodes' accounts.
    fn publisher_accounts(&self) -> Vec<PublisherAccount>;
    /// Runs the checkpointer's ticks due and publishes what they issue (`REQ-AUD-14`); how many it published.
    ///
    /// # Errors
    ///
    /// Why it stopped.
    fn run_checkpoints(&mut self) -> Result<usize, String>;
    /// What the log took reaches the state machine.
    fn fed(&mut self);

    /// Lends intake, to hold what an order Core started runs on (`T-020-61`).
    fn with_intake(&mut self, with: &mut dyn FnMut(&mut Intake));
    /// Lends policy's authorisations, to register a linked order's authorisation as its acceptance names it.
    fn with_authorisations(&mut self, with: &mut dyn FnMut(&mut dyn Authorisations));
    /// Lends the recalculations, with what a recalculation reaches and the prices Core holds (`REQ-ORD-22`).
    fn with_recalculations(&mut self, with: &mut RecalculationsLent<'_>);
    /// Lends the hand-ons, with what a hand-on reaches (`REQ-ORD-43`).
    fn with_hand_on(&mut self, with: &mut dyn FnMut(&mut HandOn, &mut HandParts<'_>));
    /// Lends the stuck orders with the production mover, policy and what a hand-on's withdrawal reaches: what a decided
    /// move of a stuck order is carried out through (`REQ-AUZ-4`).
    fn with_mover(&mut self, with: &mut MoverLent<'_>);
    /// Lends the segments, with what they reach.
    fn with_segments(&mut self, with: &mut dyn FnMut(&mut Segments, &mut SegmentParts<'_>));
    /// Lends the reversals, with what they reach and what a question to a provider moves through.
    fn with_reversals(
        &mut self,
        with: &mut dyn FnMut(&mut Reversals, &mut ReversalParts<'_>, &mut Moving<'_>),
    );
    /// Lends the reversals with what a reversal step reaches: the operator key, policy, the platform, the stuck orders
    /// and the register of actors ([`ReversalDesk`]).
    fn with_reversal_desk(&mut self, with: &mut ReversalsLent<'_>);
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

    /// The ports, to read.
    pub(super) fn ports(&self) -> &dyn Ports {
        &*self.ports
    }

    /// The ports, to ask and lend through.
    pub(super) fn ports_mut(&mut self) -> &mut dyn Ports {
        &mut *self.ports
    }
}

/// The ids of the orders Core started, in order.
fn orders(ports: &dyn Ports) -> Vec<Id> {
    ports.intake().started_orders()
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
/// record intake's, held to the log's digest when it was taken (`T-020-61`), and its segments those of the part's current route as Core
/// follows it after the ones that had settled before the recalculation, one for each operation of the record's route;
/// the first route where none was recalculated. `None` where a recalculated route's record isn't held, or its segments
/// aren't the route's (fails closed: the round says so).
pub(super) fn current_route(
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
    let record = ports.intake().decision_record(&last.record)?.clone();
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
pub(super) fn record_entry(
    history: &[(Receipt, Entry)],
    record: &DecisionRecord,
) -> Option<Receipt> {
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

/// Whether the log holds a spell of `account` not read: a raise of it since its last check, so it is not raised again
/// until it is read (rebuilt from the log, so a restart raises no spell twice). And the account's last check by
/// `provider`, the module that holds it now: a check another module gave before the account moved to this one is not
/// its check, and a raise citing it would be refused (`raise-of-a-holding-not-read-cites-its-accounts-check`), so the
/// spell would never be recorded and the account raised at every try (`T-020-60`).
fn spell_of(history: &[(Receipt, Entry)], account: &Id, provider: &Id) -> (bool, Option<Receipt>) {
    let mut in_spell = false;
    let mut last = None;
    for (receipt, entry) in history {
        let Entry::Verified(v) = entry else { continue };
        match &v.what {
            What::HoldingCheck {
                account: a,
                provider: p,
                ..
            } if a == account && p == provider => {
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
    /// The account's balance beside what Core's segments rest at the node, or why it couldn't be read.
    reading: Result<HoldingReading, (HoldingNotRead, String)>,
    /// A public node's balance beside the wallet host's own record, where that record's total isn't Core's: the same
    /// balance, a second comparison (`T-020-63`).
    wallet: Option<HoldingReading>,
}

/// What Core's segments rest at `node` in `asset` for each order (`Segments::resting_totals_by_asset`): each order once, as
/// its name and its amount. A holding account holds one asset, so it is held to its own (`T-020-67`).
fn held_by(
    resting: &BTreeMap<(String, String, String), Amount>,
    node: &str,
    asset: &str,
) -> Vec<(String, String)> {
    resting
        .iter()
        .filter(|((at, _, held_asset), _)| at == node && held_asset == asset)
        .map(|((_, order, _), amount)| (order.clone(), amount.as_str().to_owned()))
        .collect()
}

/// Why an account is not read where Core's clock gives no time to stamp the reading with.
fn not_timed() -> (HoldingNotRead, String) {
    (
        HoldingNotRead::NotTimed {},
        "Core's clock gave no time to stamp a reading with".to_owned(),
    )
}

/// The sum of `amounts`, whole units, where each is a plain number: `None` where one is past a `u128`.
fn total<'a>(mut amounts: impl Iterator<Item = &'a str>) -> Option<u128> {
    amounts.try_fold(0u128, |sum, a| sum.checked_add(a.parse::<u128>().ok()?))
}

impl Jobs<'_> {
    /// Every holding account of the deployment's register read once, each once this check (`REQ-AUD-15`, the review's
    /// `L3`): a public node's from its wallet, a private node's from its publisher's module, each balance asked as a
    /// decided read and set beside what Core's own segments rest at the node (`INV-1`'s second half); a public node's
    /// beside the wallet host's own record too, where that record's total differs from Core's, since where the two are
    /// the same the second comparison can't say another thing. A holding account the catalogue lists that the register
    /// doesn't name is not read, and says so; a clock that gives no time reads none, and says so for each (`T-020-63`).
    fn read_accounts(&mut self, only: Option<&BTreeSet<String>>) -> Vec<ToCheck> {
        let at = self.ports.clock().now().timestamp();
        let wanted = |node: &str| only.is_none_or(|nodes| nodes.contains(node));
        let public: Vec<PublicAccount> = self
            .ports
            .public_accounts()
            .into_iter()
            .filter(|a| wanted(&a.node))
            .collect();
        let private: Vec<PublisherAccount> = self
            .ports
            .publisher_accounts()
            .into_iter()
            .filter(|a| wanted(&a.node))
            .collect();
        let registered_all: BTreeSet<String> = self
            .ports
            .public_accounts()
            .into_iter()
            .map(|a| a.node)
            .chain(self.ports.publisher_accounts().into_iter().map(|a| a.node))
            .collect();
        let resting = self.ports.segments().resting_totals_by_asset();
        let mut checks = Vec::new();
        for account in public {
            let reading = match &at {
                Some(at) => self
                    .read_balance(&account.node, &account.provider, &account.asset)
                    .and_then(|balance| {
                        let core = held_by(&resting, &account.node, &account.asset);
                        reading_of(
                            &account.node,
                            &account.provider,
                            &account.asset,
                            account.decimals,
                            &balance,
                            Word::Checked,
                            &core,
                            at,
                        )
                        .map(|reading| (reading, balance))
                        .ok_or((
                            HoldingNotRead::NotABalance {},
                            "the balance isn't an amount".to_owned(),
                        ))
                    }),
                None => Err(not_timed()),
            };
            let (reading, wallet) = match reading {
                Ok((reading, balance)) => {
                    let host: Vec<(String, String)> = account
                        .attribution
                        .iter()
                        .map(|h| (h.order.clone(), h.amount.to_string()))
                        .collect();
                    let core: Vec<(String, String)> =
                        held_by(&resting, &account.node, &account.asset);
                    let same = total(host.iter().map(|(_, a)| a.as_str()))
                        == total(core.iter().map(|(_, a)| a.as_str()))
                        && total(core.iter().map(|(_, a)| a.as_str())).is_some();
                    let wallet = (!same)
                        .then(|| {
                            at.as_ref().and_then(|at| {
                                reading_of(
                                    &account.node,
                                    &account.provider,
                                    &account.asset,
                                    account.decimals,
                                    &balance,
                                    Word::Checked,
                                    &host,
                                    at,
                                )
                            })
                        })
                        .flatten();
                    (Ok(reading), wallet)
                }
                Err(why) => (Err(why), None),
            };
            checks.push(ToCheck {
                node: account.node,
                provider: account.provider,
                reading,
                wallet,
            });
        }
        for account in private {
            let reading = match &at {
                Some(at) => self
                    .read_balance(&account.node, &account.module, &account.asset)
                    .and_then(|balance| {
                        reading_of(
                            &account.node,
                            &account.module,
                            &account.asset,
                            account.decimals,
                            &balance,
                            Word::Publisher,
                            &held_by(&resting, &account.node, &account.asset),
                            at,
                        )
                        .ok_or((
                            HoldingNotRead::NotABalance {},
                            "the balance isn't an amount".to_owned(),
                        ))
                    }),
                None => Err(not_timed()),
            };
            checks.push(ToCheck {
                node: account.node,
                provider: account.module,
                reading,
                wallet: None,
            });
        }
        // The register is the definition of what is checked: a holding account the catalogue lists, or a node where Core's
        // own segments rest value, which the register names in neither list, is raised as not read, never silently left
        // out, whether or not value rests there (`T-020-63`, `T-020-67`).
        let mut unregistered: BTreeSet<String> =
            resting.keys().map(|(node, _, _)| node.clone()).collect();
        unregistered.extend(
            self.ports
                .catalogue()
                .nodes()
                .holders()
                .map(|(node, _)| node.to_owned()),
        );
        for node in &unregistered {
            if registered_all.contains(node) || !wanted(node) {
                continue;
            }
            let provider = self.ports.catalogue().nodes().holding(node).map_or_else(
                || "unregistered".to_owned(),
                |holder| holder.reference.clone(),
            );
            checks.push(ToCheck {
                node: node.clone(),
                provider,
                reading: Err((
                    HoldingNotRead::NotRegistered {},
                    "the register of holding accounts doesn't name a holding account the catalogue lists, or a node where Core rests value"
                        .to_owned(),
                )),
                wallet: None,
            });
        }
        self.ports.fed();
        checks
    }

    /// Carries the own operation of each segment of `order` left unknown out again, where it is due its provider's status
    /// cadence (`due`), the provider declares a repeat safe and no status query to ask where it stands by, and policy
    /// permits it: a decided effect, as the original move was (`REQ-AUZ-1`, `T-020-63`). Which segments were tried, whether
    /// or not the platform had an answer: a refusal by policy waits its cadence as an answer does, and a failure to record
    /// it is tried again next tick.
    fn repeat_unknown(
        &mut self,
        order: &Id,
        sent: &BTreeMap<String, Id>,
        due: &dyn Fn(&Unknown) -> bool,
    ) -> Vec<Unknown> {
        let core = self.ports.actors().core;
        let mut tried = Vec::new();
        for (transaction, segment) in sent {
            let Some(moved) = self.ports.segments().unknown_move(order, segment) else {
                continue;
            };
            let unknown = Unknown {
                order: order.clone(),
                segment: segment.clone(),
                provider: moved.provider.as_str().to_owned(),
            };
            let declared = self.ports.catalogue().declared_by(&unknown.provider);
            let repeatable = declared.is_some_and(|d| {
                d.safe_retry == Some(true) && !d.tracking.contains(&Tracking::StatusQuery)
            });
            let (Some(tx), true) = (id(transaction), repeatable && due(&unknown)) else {
                continue;
            };
            let mut outcome = Repeated::Failed;
            self.ports.with_segments(&mut |segments, parts| {
                outcome = segments.carry_again(
                    parts,
                    &RepeatOf {
                        order,
                        segment,
                        provider: &moved.provider,
                        transaction: &tx,
                        core: &core,
                    },
                );
            });
            if outcome != Repeated::Failed {
                tried.push(unknown);
            }
        }
        self.ports.fed();
        tried
    }

    /// The balance `provider`'s module states for the account of `node` in `asset`: the read is decided and recorded as
    /// an effect before the module is asked ([`Segments::read_holding_balance`]), and the answer is taken only as the
    /// answer to that question: a result of the transaction asked under, stating a balance. Why it wasn't read, else.
    fn read_balance(
        &mut self,
        node: &str,
        provider: &str,
        asset: &str,
    ) -> Result<String, (HoldingNotRead, String)> {
        let actors = self.ports.actors();
        let bounds = self.ports.bounds();
        let mut read = Balance::Failed;
        self.ports.with_segments(&mut |segments, parts| {
            read = segments.read_holding_balance(
                parts,
                &BalanceRead {
                    node,
                    provider,
                    asset,
                    bounds: &bounds,
                    operator: &actors.operator,
                    core: &actors.core,
                },
            );
        });
        let (transaction, bytes) = match read {
            Balance::Denied => {
                return Err((
                    HoldingNotRead::NotPermitted {},
                    "policy didn't permit the read".to_owned(),
                ));
            }
            Balance::Failed => {
                return Err((
                    HoldingNotRead::NotAnswered {},
                    "the platform passed no answer".to_owned(),
                ));
            }
            Balance::Answered(transaction, bytes) => (transaction, bytes),
        };
        let Ok(answered) = serde_json::from_slice::<Statement<ResultBody>>(&bytes) else {
            return Err((
                HoldingNotRead::AnotherAnswer {},
                "the answer isn't a result".to_owned(),
            ));
        };
        let ResultBody::HoldingBalance(result) = answered.statement else {
            return Err((
                HoldingNotRead::AnotherAnswer {},
                "the module answered another question".to_owned(),
            ));
        };
        if result.transaction != transaction.as_str() {
            return Err((
                HoldingNotRead::AnotherAnswer {},
                "the answer names another transaction".to_owned(),
            ));
        }
        match result.outcome {
            HoldingBalanceOutcome::Answered(answer) => Ok(answer.balance),
            HoldingBalanceOutcome::Refused(_) => {
                Err((HoldingNotRead::Refused {}, "the module refused".to_owned()))
            }
        }
    }

    /// The stuck move `one` reported to its mover from what Core kept of its provider's answers (`AC-6`, `REQ-ORD-1`):
    /// only a segment of a part Core declared stuck, only an answer the segments judge the move's own provider's and
    /// saying failed or settled (typed, `L5`), the move's mover as the reporter, and the statement named by the entry
    /// it was kept as, once (no second entry of the same bytes). One already reported is not told again.
    fn report_kept_for(&mut self, one: &Unknown) -> Vec<Reported> {
        let Some(started) = self.ports.intake().started(&one.order).cloned() else {
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
        .intake()
        .started(order)
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
    fn run_orders(&mut self) -> super::run::Runs {
        Jobs::run_orders(self)
    }

    fn late_answers(&mut self, due: &dyn Fn(&Unknown) -> bool) -> Late {
        let mut taken = Vec::new();
        let mut repeated = Vec::new();
        for order in orders(&*self.ports) {
            let Some(started) = self.ports.intake().started(&order).cloned() else {
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
            repeated.extend(self.repeat_unknown(&order, &sent, due));
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
                // Whether it is worth handing over again: nothing was recorded for want of the reversals, the log or the
                // segment's own answer, none of which is the answer's fault.
                let mut again = true;
                self.ports.with_reversals(&mut |reversals, parts, _| {
                    reversals.follow(&order, &history);
                    // The platform hands over its provider's own late answer: the maker its envelope names presents it.
                    let Some(presented_by) = handed_over_by(parts.catalogue, &bytes) else {
                        kept = Err("the answer names no maker".to_owned());
                        again = false;
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
                        .map_err(|why| {
                            again = matches!(
                                why,
                                ReversalError::NotAnswered
                                    | ReversalError::Append(AppendFailure::Unavailable)
                            );
                            format!("{why:?}")
                        });
                    if kept.is_ok() {
                        again = false;
                    }
                });
                self.ports.fed();
                // Kept, or refused for good: the platform hands it over no more. One Core couldn't record now (the log
                // down, the segment's answer not yet taken) stays with the platform, which hands it over again the next
                // tick, so it is never dropped (`T-020-63`, `NFR-6`).
                if !again {
                    self.ports.late_kept(&transaction);
                }
                taken.push(LateTaken {
                    order: order.clone(),
                    segment: segment.clone(),
                    kept,
                });
            }
        }
        Late { taken, repeated }
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
                    if let Some(held) = self.ports.intake().decision_record(&record) {
                        records.insert(record.as_str().to_owned(), held.clone());
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

    fn check_holdings(&mut self, only: Option<&BTreeSet<String>>) -> Vec<HoldingChecked> {
        let history = self.ports.history();
        let mut checked = Vec::new();
        for one in self.read_accounts(only) {
            let (reading, check, unread, wallet) = match one.reading {
                Ok(reading) => {
                    let mut check = Err("the comparisons weren't lent".to_owned());
                    self.ports.with_audit(&mut |_, comparisons, _| {
                        check = comparisons
                            .holding_reading(&reading)
                            .map_err(|why| format!("{why:?}"));
                    });
                    // The wallet host's own record, beside the same balance (`T-020-63`).
                    let wallet = one.wallet.map(|reading| {
                        let mut check = Err("the comparisons weren't lent".to_owned());
                        self.ports.with_audit(&mut |_, comparisons, _| {
                            check = comparisons
                                .holding_reading(&reading)
                                .map_err(|why| format!("{why:?}"));
                        });
                        WalletCompared { reading, check }
                    });
                    (Ok(reading), check, None, wallet)
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
                            let (in_spell, last) = spell_of(&history, &account, &provider);
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
                    (Err(why.clone()), Err(why), Some(raised), None)
                }
            };
            checked.push(HoldingChecked {
                node: one.node,
                reading,
                check,
                not_read: unread,
                against_wallet: wallet,
            });
        }
        checked
    }

    /// Each private node an order Core started is still unsettled at, asked of each provider of the node's publisher
    /// that declares it (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`): for each such order, node and provider, once in a run,
    /// about the transactions of that order's segments there, and the read names that order and one of its segments. An
    /// order whose segments at the node have all settled (or had their reversal completed) is read no more: closing kept
    /// its record when it settled, so the job's reads, and the log's entries for them, are bounded by the orders in flight
    /// and not by those ever started. Each read is decided by policy and recorded as an effect, as closing's is
    /// ([`Segments::read_node_record`]), and its answer is taken as a record only as `closing::publishers::taken` allows.
    ///
    /// **What it serves.** The records go to the round ([`Round::published`](super::Round::published)) and no further: they are what the
    /// publishers state, now, of the orders in flight through their nodes, for the operator's view and for what reads a
    /// round. Closing's books are closing's own: it reads for itself, for each segment it grades, and keeps what it
    /// cites. The deployment carries each question to the platform and chooses neither node, provider nor order; where
    /// policy refuses a read nothing is asked.
    fn read_publishers(&mut self) -> Vec<Published> {
        use rutent_contracts::state_machine::SegmentState;
        use std::collections::BTreeMap;
        // For each order and node it is unsettled at: a segment of it there and the transactions of its segments there.
        let mut places: Vec<(
            Id,
            String,
            Id,
            rutent_contracts::router_api::PolicyBounds,
            BTreeSet<String>,
        )> = Vec::new();
        for order in orders(&*self.ports) {
            let (Some(started), Some(followed)) = (
                self.ports.intake().started(&order),
                self.ports.segments().order(&order),
            ) else {
                continue;
            };
            let mut here: BTreeMap<String, (Id, BTreeSet<String>, bool)> = BTreeMap::new();
            for segment in followed.parts.iter().flat_map(|part| part.segments.iter()) {
                let Some(segment_id) = id(segment.segment_id.as_str()) else {
                    continue;
                };
                // Only a segment handed over has anything to ask about.
                let Some(transaction) = self
                    .ports
                    .segments()
                    .statement_of(&order, &segment_id)
                    .and_then(|s| crate::closing::publishers::transaction_of(&s.statement))
                else {
                    continue;
                };
                let open = !matches!(
                    segment.state,
                    SegmentState::Settled | SegmentState::ReversalComplete
                );
                for end in [&segment.from.node, &segment.to.node] {
                    let name = match end {
                        rutent_contracts::provider_role::NodeRef::Inner(id) => id.as_str(),
                        rutent_contracts::provider_role::NodeRef::Outer(o) => o.network.as_str(),
                    };
                    if self
                        .ports
                        .catalogue()
                        .publishers()
                        .publisher_of_node(name)
                        .is_none()
                    {
                        continue;
                    }
                    let entry = here
                        .entry(name.to_owned())
                        .or_insert_with(|| (segment_id.clone(), BTreeSet::new(), false));
                    entry.1.insert(transaction.clone());
                    entry.2 |= open;
                }
            }
            for (node, (segment, about, open)) in here {
                if open {
                    places.push((
                        order.clone(),
                        node,
                        segment,
                        started.record.bounds.clone(),
                        about,
                    ));
                }
            }
        }
        let actors = self.ports.actors();
        let mut wanted = Vec::new();
        for (order, node, segment, bounds, about) in places {
            let about: Vec<Id> = about.iter().filter_map(|t| id(t)).collect();
            for provider in crate::closing::publishers::serving(self.ports.catalogue(), &node) {
                wanted.push((
                    order.clone(),
                    segment.clone(),
                    node.clone(),
                    provider,
                    bounds.clone(),
                    about.clone(),
                ));
            }
        }
        let mut answers = Vec::new();
        self.ports.with_segments(&mut |segments, parts| {
            for (order, segment, node, provider, bounds, about) in &wanted {
                let read = crate::segments::RecordRead {
                    order,
                    segment,
                    node,
                    provider,
                    about,
                    bounds,
                    operator: &actors.operator,
                    core: &actors.core,
                };
                // Held to its own due time and bound, and not asked once the tick has taken the record
                // (`T-020-67`).
                if let Some(crate::segments::Read::Answered(question, bytes, received)) =
                    segments.read_node_record_on_tick(parts, &read)
                {
                    // What the tick read is the grading's books' too: a segment that waits long is graded with the record
                    // the publisher has by then, whatever the grading's own reads came to.
                    let wanted = crate::segments::RecordWanted {
                        node: node.clone(),
                        provider: provider.clone(),
                        about: about.iter().map(|t| t.as_str().to_owned()).collect(),
                    };
                    let fed = parts.grading.record_read(
                        order,
                        &wanted,
                        Some((&question, bytes.as_slice())),
                        &received,
                    );
                    answers.push((
                        order.clone(),
                        segment.clone(),
                        node.clone(),
                        provider.clone(),
                        question,
                        bytes,
                        fed,
                    ));
                }
            }
        });
        let at = self.ports.clock().now().floor();
        let mut published = Vec::new();
        let mut taken = Vec::new();
        for (order, segment, node, provider, question, bytes, fed) in answers {
            if let Ok(record) = crate::closing::publishers::taken(
                self.ports.catalogue(),
                at,
                &question.statement,
                &bytes,
            ) {
                if fed {
                    let about = wanted
                        .iter()
                        .find(|(o, s, n, p, ..)| {
                            *o == order && *s == segment && *n == node && *p == provider
                        })
                        .map(|(.., about)| about.clone())
                        .unwrap_or_default();
                    taken.push((order.clone(), segment, node.clone(), provider, about));
                }
                published.push(Published {
                    order,
                    node,
                    record,
                });
            }
        }
        self.ports.with_segments(&mut |segments, _| {
            for (order, segment, node, provider, about) in &taken {
                segments.note_record_taken(order, segment, node, provider, about);
            }
        });
        published
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
            .intake()
            .started(&unknown.order)
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
            let Some(started) = self.ports.intake().started(&order) else {
                continue;
            };
            let Some(first) = started.first() else {
                continue;
            };
            let Some((record, segments)) = current_route(&*self.ports, &order, started, &first)
            else {
                // Fails closed: a route Core can't name now is never timed on another route. Its part is declared stuck,
                // for the operator, on the last route the log names for it (#316's round-3 L7, `T-020-61`).
                let found = self.unnamed_route_stuck(&order, &first.part);
                watched.push(Watched { order, found });
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
    use rutent_prov::holding::HeldForOrder;

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

    /// A deployment that answers only its ports: its register holds one public account, with the wallet host's record of
    /// what its orders hold there, and one private node's; it lends Core's parts to the log's comparisons alone, and no
    /// other (each other job finds them not lent, so nothing moves on it, and no balance is read, `REQ-AUZ-5`).
    struct Bare {
        exposures: crate::snapshot::Exposures,
        issued: crate::rounds::Issued,
        clock: At,
        intake: Intake,
        arrivals: Arrivals,
        mover: Mover,
        segments: Segments,
        stuck: Stuck,
        reversals: Reversals,
        catalogue: Catalogue,
        told: Told,
        next: u32,
        /// The nodes the register names: the public account's and the private node's, unless left out.
        register: bool,
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
        fn history(&self) -> Vec<(Receipt, Entry)> {
            Vec::new()
        }
        fn kept(&mut self, _: &Digest) -> Option<Vec<u8>> {
            None
        }
        fn in_force(&mut self, _: &Id, _: &Id) -> Option<Authorisation> {
            None
        }
        fn intake(&self) -> &Intake {
            &self.intake
        }
        fn arrivals(&self) -> &Arrivals {
            &self.arrivals
        }
        fn mover(&self) -> &Mover {
            &self.mover
        }
        fn with_intake(&mut self, _: &mut dyn FnMut(&mut Intake)) {}
        fn with_authorisations(&mut self, _: &mut dyn FnMut(&mut dyn Authorisations)) {}
        fn with_recalculations(&mut self, _: &mut RecalculationsLent<'_>) {}
        fn with_hand_on(&mut self, _: &mut dyn FnMut(&mut HandOn, &mut HandParts<'_>)) {}
        fn with_mover(&mut self, _: &mut MoverLent<'_>) {}
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
        fn bounds(&self) -> PolicyBounds {
            serde_json::from_str(include_str!(
                "../../../../contracts/router-api/fixtures/valid/policy-bounds-without-splitting.json"
            ))
            .expect("bounds")
        }
        fn late_answers(&mut self, _: &BTreeSet<String>) -> Vec<(String, Vec<u8>)> {
            Vec::new()
        }
        fn late_kept(&mut self, _: &str) {}
        fn public_accounts(&self) -> Vec<PublicAccount> {
            if !self.register {
                return Vec::new();
            }
            vec![PublicAccount {
                node: "bank-ledger".to_owned(),
                provider: "operator-wallet".to_owned(),
                asset: "USDC".to_owned(),
                decimals: 2,
                attribution: vec![
                    HeldForOrder {
                        order: "ord-1".to_owned(),
                        amount: 1500,
                    },
                    HeldForOrder {
                        order: "ord-2".to_owned(),
                        amount: 0,
                    },
                ],
            }]
        }
        fn publisher_accounts(&self) -> Vec<PublisherAccount> {
            if !self.register {
                return Vec::new();
            }
            vec![PublisherAccount {
                node: "rail".to_owned(),
                publisher: "rail-publisher".to_owned(),
                module: "rail-account".to_owned(),
                asset: "USDC".to_owned(),
                decimals: 2,
            }]
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
        fn with_reversal_desk(&mut self, _: &mut ReversalsLent<'_>) {}
        fn with_stuck(&mut self, with: &mut StuckLent<'_>) {
            // The stuck orders and what they reach, over a log, a policy and a channel that take nothing.
            let (mut log, mut policy, mut channel) = (Refuses, Refuses, Refuses);
            let mut fresh = Drawn(0);
            let mut parts = StuckParts {
                clock: &self.clock,
                log: &mut log,
                fresh: &mut fresh,
                issued: &self.issued,
                segments: &mut self.segments,
                exposures: &mut self.exposures,
            };
            with(&mut self.stuck, &mut parts, &mut policy, &mut channel);
        }
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

    /// A log, a policy and an operator's channel that take nothing.
    struct Refuses;

    impl rutent_contracts::core_internal::calls::Log for Refuses {
        fn append(
            &mut self,
            _: &rutent_contracts::core_internal::calls::GeneralEntry,
        ) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }

    impl StuckOrders for Refuses {
        fn decide_stuck(
            &mut self,
            _: &rutent_contracts::core_internal::StuckDecide,
        ) -> Result<rutent_contracts::core_internal::Decision, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn move_stuck(
            &mut self,
            _: &rutent_contracts::core_internal::StuckMove,
        ) -> Result<rutent_contracts::core_internal::Decision, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn report_outcome(
            &mut self,
            _: &rutent_contracts::core_internal::OutcomeReport,
        ) -> Result<rutent_contracts::core_internal::Decision, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn confirm_outcome(
            &mut self,
            _: &rutent_contracts::core_internal::OutcomeConfirmation,
        ) -> Result<rutent_contracts::core_internal::Decision, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }

    impl Operator for Refuses {
        fn raise(
            &mut self,
            _: &rutent_contracts::core_internal::Raise,
        ) -> rutent_contracts::core_internal::calls::Delivery {
            rutent_contracts::core_internal::calls::Delivery::Undelivered
        }
    }

    /// Fresh identifiers, counted.
    struct Drawn(u32);

    impl crate::rounds::Fresh for Drawn {
        fn id(&mut self) -> Id {
            self.0 += 1;
            id(&format!("drawn-{}", self.0)).unwrap()
        }
    }

    fn bare() -> Bare {
        Bare {
            exposures: crate::snapshot::Exposures::new(),
            issued: crate::rounds::Issued::default(),
            clock: At(1_764_000_000),
            intake: Intake::new(),
            arrivals: Arrivals::default(),
            mover: Mover::default(),
            segments: Segments::new(),
            stuck: Stuck::new(),
            reversals: Reversals::new(),
            catalogue: Catalogue::new(),
            told: Told::default(),
            next: 0,
            register: true,
        }
    }

    /// The operator's decide step names any part of a split order (`T-020-61`, the review's M4): over a deployment that
    /// lends a split order of three parts, its second part stuck, Core's entry point takes the part Core holds stuck,
    /// never the first, and goes on to that part's own checks. The deployment's log takes nothing, so the decision stops
    /// at the first of them that needs the log's records, the part's two ends; an order whose parts none is stuck is
    /// not stuck at all.
    // verifies: REQ-AUZ-4, AC-6
    #[test]
    fn stuck_command_decides_the_stuck_part_of_a_split_order() {
        use crate::stuck::{ResolveError, decide_stuck};
        use rutent_contracts::core_internal::Resolution;
        use rutent_contracts::state_machine::RecordedInput;

        let chosen: RecordedInput = serde_json::from_str(include_str!(
            "../../../../contracts/state-machine/fixtures/valid/recorded-input-route-chosen-three-parts.json"
        ))
        .expect("a route of three parts");
        let mut stuck_at: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/state-machine/fixtures/valid/recorded-input-stuck-declared-over-time.json"
        ))
        .expect("a stuck declaration");
        stuck_at["subject"]["part"] = "p2".into();
        let stuck_at: RecordedInput = serde_json::from_value(stuck_at).expect("of the second part");
        let rutent_contracts::state_machine::Observed::RouteChosen { parts, .. } = &chosen.observed
        else {
            panic!("the route chosen");
        };
        let order = id("ord-1").unwrap();
        let signed: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/router-api/fixtures/valid/decision-record-split-of-one-part.json"
        ))
        .expect("a signed record");
        let record: rutent_contracts::router_api::DecisionRecord =
            serde_json::from_value(signed["statement"].clone()).expect("a record");
        let started = |plan| {
            Started {
            record: record.clone(),
            plan,
            authorisation: id("auth-1").unwrap(),
            origin: id("reserve-bank-co").unwrap(),
            route: serde_json::from_value(serde_json::json!({
                "destination": { "reserve": "reserve-wallet", "node": { "network": "polygon" }, "account": "0xabc" },
                "to_asset": { "id": "USDC", "decimals": 6 },
                "delivers": "100",
            }))
            .expect("the route's end"),
            session: None,
            inputs: BTreeMap::new(),
            floor: None,
        }
        };

        let mut deployment = bare();
        assert!(
            deployment
                .segments
                .follow_recorded(&deployment.clock, &[chosen.clone(), stuck_at])
        );
        deployment
            .intake
            .hold_started(&order, started(parts.clone()));
        deployment
            .stuck
            .plant_stuck(&order, &id("p2").unwrap(), Reading::at(1_763_000_000));
        let decider = Bare::actors(&deployment).operator;
        let resolution: Resolution = serde_json::from_value(serde_json::json!({
            "kind": "hand_on", "by": "reserve-wallet", "linked_order": "linked-1",
            "asset": { "id": "USDC", "decimals": 6 }, "amount": "100",
        }))
        .unwrap_or_else(|e| panic!("a resolution: {e}"));
        let decided = decide_stuck(&mut deployment, &order, &decider, &resolution, &[]);
        assert_eq!(
            decided.err(),
            Some(ResolveError::NoEnds),
            "the second part, stuck, is decided: it goes past its selection to its own ends"
        );

        // Neither of the other parts is stuck: an order with only those is not stuck.
        let mut deployment = bare();
        assert!(
            deployment
                .segments
                .follow_recorded(&deployment.clock, std::slice::from_ref(&chosen))
        );
        deployment
            .intake
            .hold_started(&order, started(parts.clone()));
        assert_eq!(
            decide_stuck(&mut deployment, &order, &decider, &resolution, &[]).err(),
            Some(ResolveError::NotStuck)
        );
    }

    /// Every job's body is Core's: over a deployment that answers only its ports, the tick runs every job, and what
    /// each job makes of the answers is Core's rule, not the deployment's: each account of the register is read by Core's
    /// own decided read, which the deployment here can't lend (no balance is asked of any module), so each is raised as
    /// not read, once, citing nothing, and never read as zero; and a job whose parts or port the deployment can't lend or
    /// answer moves nothing and says so (`REQ-AUZ-5`: an input taken away is no permit).
    // verifies: REQ-AUZ-5
    #[test]
    fn every_job_of_the_schedule_is_cores_and_the_deployment_answers_only_its_ports() {
        let mut deployment = bare();
        let mut schedule = Schedule::new();
        let round = schedule.tick(1_764_000_000, &mut Jobs::new(&mut deployment));
        assert_eq!(round.ran, Job::ALL, "every job, in its order");

        // The holding check: the register's two accounts, neither read, each raised, citing nothing: it was never
        // checked. No reading is built from a balance Core wasn't given.
        assert!(
            deployment.told.readings.is_empty(),
            "no balance, no reading"
        );
        let mut unread = deployment.told.unread.clone();
        unread.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        assert_eq!(
            unread,
            [
                (
                    id("holding-bank-ledger").unwrap(),
                    id("bank-ledger").unwrap(),
                    id("operator-wallet").unwrap(),
                    HoldingNotRead::NotAnswered {},
                    None,
                ),
                (
                    id("holding-rail").unwrap(),
                    id("rail").unwrap(),
                    id("rail-account").unwrap(),
                    HoldingNotRead::NotAnswered {},
                    None,
                ),
            ],
            "the accounts nobody answered for are raised, citing nothing: they were never checked"
        );
        for node in ["bank-ledger", "rail"] {
            let one = round.holding_of(node).expect("the account is in the round");
            assert!(one.reading.is_err() && one.check.is_err() && one.not_read.is_some());
            assert!(one.against_wallet.is_none());
        }

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

    /// A clock that gives no time to stamp a reading with doesn't skip the accounts: each is raised as not read, and
    /// nothing is asked of any module (`INV-1`, `REQ-AUD-15`, `T-020-63`; the review of `T-020-48`'s first build).
    // verifies: INV-1
    #[test]
    fn account_the_clock_cannot_time_is_raised_as_not_read() {
        let mut deployment = bare();
        // Past the year 9999: a time no timestamp states.
        deployment.clock = At(u64::MAX / 2);
        let mut schedule = Schedule::new();
        let round = schedule.tick(1_764_000_000, &mut Jobs::new(&mut deployment));
        assert_eq!(round.holding.len(), 2, "both accounts: {:?}", round.holding);
        for one in &round.holding {
            assert!(one.reading.is_err() && one.check.is_err(), "{one:?}");
            assert!(one.not_read.is_some(), "raised, not skipped: {one:?}");
        }
        let mut unread: Vec<_> = deployment
            .told
            .unread
            .iter()
            .map(|(account, _, _, why, _)| (account.as_str().to_owned(), why.clone()))
            .collect();
        unread.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            unread,
            [
                (
                    "holding-bank-ledger".to_owned(),
                    HoldingNotRead::NotTimed {}
                ),
                ("holding-rail".to_owned(), HoldingNotRead::NotTimed {}),
            ]
        );
    }

    /// A node the catalogue lists a holding account at that the deployment's register doesn't name is raised as not
    /// read whether or not Core's own segments rest value there: the register is the definition of what is checked, and
    /// what it leaves out is said, not skipped. Nothing of its module is asked (`REQ-AUD-15`, `T-020-63`, `T-020-67`: the
    /// first build raised it only where value rested).
    // verifies: REQ-AUD-15
    #[test]
    fn holder_with_nothing_of_cores_resting_in_it_is_raised_though_the_register_does_not_name_it() {
        let mut deployment = bare();
        deployment.register = false;
        deployment
            .catalogue
            .publish_operator_module("operator-wallet")
            .expect("the operator publishes the wallet");
        deployment
            .catalogue
            .add_public_node("bank-ledger")
            .expect("a public node");
        deployment
            .catalogue
            .register_public_holding("bank-ledger", "operator-wallet")
            .expect("its holding account");
        let mut schedule = Schedule::new();
        let round = schedule.tick(1_764_000_000, &mut Jobs::new(&mut deployment));
        assert_eq!(round.holding.len(), 1, "{:?}", round.holding);
        assert_eq!(round.holding[0].node, "bank-ledger");
        assert!(round.holding[0].reading.is_err(), "{:?}", round.holding);
        assert_eq!(deployment.told.unread.len(), 1);
    }
}

#[cfg(test)]
mod held_by_tests {
    use super::*;

    fn amount(text: &str) -> Amount {
        Amount::new(text).expect("an amount")
    }

    /// At a node resting two assets, each holding account is compared with what rests in its own asset, not the sum of both:
    /// a short in one is not hidden behind an over in the other, nor a difference raised for the sum (`REQ-AUD-6`,
    /// `INV-1`, `T-020-67`).
    // verifies: REQ-AUD-6
    #[test]
    fn holding_check_at_a_node_resting_two_assets_compares_each_account_with_its_own_asset() {
        let resting: BTreeMap<(String, String, String), Amount> = [
            (("ledger", "ord-1", "COP"), "500"),
            (("ledger", "ord-1", "USDC"), "70"),
            (("ledger", "ord-2", "COP"), "100"),
            (("other", "ord-1", "COP"), "9"),
        ]
        .into_iter()
        .map(|((n, o, a), v)| ((n.to_owned(), o.to_owned(), a.to_owned()), amount(v)))
        .collect();
        assert_eq!(
            held_by(&resting, "ledger", "COP"),
            [
                ("ord-1".to_owned(), "500".to_owned()),
                ("ord-2".to_owned(), "100".to_owned())
            ]
        );
        assert_eq!(
            held_by(&resting, "ledger", "USDC"),
            [("ord-1".to_owned(), "70".to_owned())]
        );
        assert!(held_by(&resting, "ledger", "EUR").is_empty());
    }
}
