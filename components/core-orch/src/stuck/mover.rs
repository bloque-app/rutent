//! The production mover of a stuck order's resolution (`T-020-16`): a [`Resolutions`] that carries a decided `deliver`
//! out on the platform, in place of [`super::Unbuilt`], which fails every one.
//!
//! It runs only on an effect the log admitted on a stuck move's permit, so by the time it is called policy has
//! permitted both the decision and the move, by two actors, and the log has held the effect to the decision. It takes
//! everything it moves from that effect's action: the provider (`resolution.by`), the destination (`to`), the asset,
//! the amount and the transaction, which the decide step named and the log holds to be the resolution's own. Nothing
//! comes from the caller. Where the value rests, the inner node of the provider's that Core's own record of the part
//! says it went to, is read from Core's record of the operation it signed for the part's withdrawal
//! ([`crate::segments::Segments::sent`]), never from the call.
//!
//! **What it does, in this order, each step once** ([`Delivering::resolve`]):
//! 1. writes the operation, a deposit from that node to the destination's outer node at the amount decided, with the
//!    destination's account under the one input the tuple requires, signed with the operator key, and appends it as
//!    the operator's question about the order and the transaction, naming no segment
//!    (`stuck-resolution-transaction-is-unbound-or-its-orders`);
//! 2. begins the provider's segment of that transaction on the platform, as any segment goes;
//! 3. hands the operation to the provider's module, once;
//! 4. keeps what came back (the platform's events, the proxy's uses, the provider's answer) before any step that can
//!    fail, and appends it: the hand-over entries, the segment's record where it ended, and the answer as the
//!    provider's stated entry about the order and the transaction (`REQ-MDL-8`);
//! 5. answers [`Resolved`]: `Done` only for a result that counts and says it settled; `Failed` only where nothing
//!    passed out (the platform refused it, or Core could not write the operation) or the provider says it failed or
//!    refused finally; `Unknown` for everything else, so that a part is never freed for a second payment on an
//!    answer that may hide a movement.
//!
//! A call that stopped because the log couldn't take something is made again with the same effect and goes on from
//! where it stopped: the question, the begin and the hand-over are each done once, and what the provider answered is
//! kept, never asked for again (`INV-8`).
//!
//! **Fail closed, before anything is asked.** No node of the provider's holds the part's value, the provider
//! advertises no deposit tuple from that node to the destination's outer node, the tuple needs inputs no account
//! can fill, or the asset or its unit differs from the tuple's: the move answers `Failed`, and the platform is
//! handed nothing.
//!
//! **Gaps**: the mover's state is kept in memory, as [`super::Stuck`] and [`crate::segments::Segments`] are; the
//! production assembly keeps all three durably (`F-030`). A drain the contract can't carry leaves the answer
//! `Unknown`, with the drain kept whole and the part held. The calls the provider's module makes are named for no
//! route: a call it makes is refused (`REQ-MDL-21`), which fails closed.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, GeneralEntry};
use rutent_contracts::core_internal::{About, Action, Actor, Effect, Outcome, Receipt, Resolution};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{
    Deposit, NodeRef, Operation as ProviderOperation, Refusal, ReserveMovementOutcome, ResultBody,
    Statement, Terms,
};
use rutent_contracts::router_api::{Id, PolicyBounds, PositiveAmount, Timestamp};
use rutent_contracts::state_machine::{OperationKind, SegmentState};
use rutent_prov::catalogue::Catalogue;
use rutent_prov::holding::wallet::{
    HandedOnStart, StartError, WalletError, WithdrawalRecord, proof_of_withdrawal,
};
use rutent_prov::module::types::Events;

use super::handing::{AcceptedRoute, HandingOn};
use super::is_receipt_of;
use super::journal::{Step, built};
use super::{Carrying, Parts, Resolutions, Resolved};
use crate::hand_on::{proves_start, resting};
use crate::rounds::OperatorKey;
use crate::segments::{
    Answered, Counted, Drained, Modules, Unappended, Units, Uses, drain_entries, judge, maker,
    sign_operation, stated_entry,
};

/// What the mover reaches outside Core's own records: the platform it hands the operation to, the credential proxy's
/// records of use, the evidence store, the catalogue its tuples and keys are read from, and what signs the operation.
/// The assembly gives each; the log, the clock, the identifiers and the segments come with every call, in
/// [`Parts`].
pub struct Ports<'a> {
    /// The module platform.
    pub modules: &'a mut dyn Modules,
    /// The credential proxy's records of use.
    pub uses: &'a mut dyn Uses,
    /// The evidence store, which keeps the operation and the answer as they were signed and received.
    pub evidence: &'a mut dyn Evidence,
    /// The catalogue: the provider's advertised tuple, and the keys a result is attributed by.
    pub catalogue: &'a Catalogue,
    /// The operator key, which signs the operation.
    pub operator_key: &'a mut dyn OperatorKey,
    /// The bounds whose deployment and protocol version the operation is made for.
    pub bounds: &'a PolicyBounds,
    /// The operator, whose question it is.
    pub operator: &'a Actor,
}

/// One move, from its operation written until its outcome is answered.
#[derive(Debug, Clone)]
struct Flight {
    provider: Id,
    transaction: Id,
    statement: Statement<ProviderOperation>,
    units: Units,
    terms: Terms,
    question: Step,
    begun: bool,
    carried: bool,
    answered: Option<Answered>,
    /// The first verdict on what came back, kept: a later try reads the same.
    verdict: Option<Resolved>,
    ended: bool,
    /// The answer's entry, built once, where the answer can be recorded.
    answer: Option<Step>,
    /// What the platform and the proxy handed over, and the segment's record, each item once, in the order taken.
    drains: Vec<Step>,
    /// A drain the contract can't carry, which holds the answer unknown.
    uncarried: bool,
}

/// A move that was refused before anything was asked, or written: its answer, kept.
#[derive(Debug, Clone)]
enum Kept {
    Refused,
    /// A re-entry for an effect this mover holds no record of: what its predecessor handed over is unknown.
    Lost,
    Flight(Box<Flight>),
    /// A hand-on's withdrawal from the holding account.
    Hand(Box<HandFlight>),
}

/// One hand-on's withdrawal, from its route held to the records until its outcome is answered (`T-020-17`).
#[derive(Debug, Clone)]
struct HandFlight {
    provider: Id,
    linked: Id,
    amount: PositiveAmount,
    route: AcceptedRoute,
    /// The operator's question the log recorded attributed, which the wallet was handed.
    question: Option<Step>,
    /// Whether the wallet was handed the operator's statement: from then it may hold the withdrawal.
    asked: bool,
    verdict: Option<Resolved>,
    start: Option<HandedOnStart>,
    /// What the wallet was shown: its own transaction, which the question was recorded under, and the operator's
    /// signed statement.
    shown: Option<Shown>,
    /// The wallet's signed answer, kept as the holder's stated entry about the linked order ([`Mover::statements`],
    /// by these bytes), and the receipt of that entry once the log recorded it attributed.
    answer: Option<Vec<u8>>,
    statement: Option<Receipt>,
    /// The start the holding account's stated answer proved, taken as a known outcome ([`Delivering::statement`]):
    /// what [`Mover::withdrawn`] gives for a withdrawal left unknown, once its answer is on record attributed.
    known: Option<(HandedOnStart, Receipt)>,
    /// The holding account said, in a statement Core took, that the withdrawal failed: nothing starts on it again,
    /// whatever else it said.
    failed: bool,
    /// What the platform and the proxy handed over, each item once, in the order taken.
    drains: Vec<Step>,
    uncarried: bool,
}

/// What a holding account's wallet was shown for a hand-on: the transaction it names its own withdrawal by, which is
/// not the one the decision named, and the operator's signed statement of it.
#[derive(Debug, Clone)]
struct Shown {
    transaction: Id,
}

/// A part's value withdrawn from its holding account for a linked order's route, and so ready to be handed on
/// ([`crate::hand_on::HandOn::hand_on`], then [`crate::hand_on::HandOn::prove_start`]).
#[derive(Debug, Clone, Copy)]
pub struct Withdrawn<'a> {
    /// The linked order.
    pub linked: &'a Id,
    /// The route the withdrawal was made for: the one the hand-on is to take, so the two never part.
    pub route: &'a AcceptedRoute,
    /// The holding account's own record of the withdrawal.
    pub start: &'a HandedOnStart,
    /// The receipt of the operator's question the wallet answered, which the log holds attributed.
    pub question: Option<&'a Receipt>,
    /// The receipt of the holding account's own stated answer, which the log holds attributed: what the
    /// `holding_withdrawal` cites as its record ([`crate::hand_on::Start::statement`], `REQ-ORD-40`).
    pub statement: Option<&'a Receipt>,
}

/// The production [`Resolutions`]' state: each move it carried out or is carrying out, by its effect's receipt, and
/// the statements it took as evidence of an outcome. The production assembly keeps it durably with [`super::Stuck`]
/// (`F-030`).
#[derive(Debug, Clone, Default)]
pub struct Mover {
    kept: BTreeMap<String, Kept>,
    /// Each statement taken as evidence ([`Delivering::statement`]), built once however often it is tried: its bytes
    /// are kept in the evidence store when its entry is built.
    statements: BTreeMap<(String, Vec<u8>), Step>,
}

/// A provider's statement, taken as evidence of a move's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stated {
    /// The receipt of its stated entry, the evidence a report or a confirmation of the outcome names.
    pub receipt: Receipt,
    /// What it says of the move, where it is the move's own provider's statement of the move's own deposit: `Done` for
    /// a result that counts and says it settled, `Failed` for one that counts and says it failed or refused finally,
    /// `Unknown` for one that says it doesn't know. `None` of another provider's statement, which bears out
    /// nothing of the move itself, and only backs a confirmation as another maker's.
    pub outcome: Option<Resolved>,
}

impl From<&crate::segments::Late> for Stated {
    fn from(late: &crate::segments::Late) -> Self {
        use crate::segments::Says;
        Stated {
            receipt: late.receipt.clone(),
            outcome: match late.says {
                Says::Settled => Some(Resolved::Done),
                Says::Failed => Some(Resolved::Failed),
                Says::Unknown => Some(Resolved::Unknown),
                Says::Nothing => None,
            },
        }
    }
}

/// Why a statement wasn't taken as evidence of a move's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotTaken {
    /// The effect names no move this mover carried out.
    NoMove,
    /// Its signature doesn't verify under a key its maker published for statements, or it is of another deployment or
    /// protocol version: it is attributable to no one.
    Unattributable,
    /// It isn't a result in its envelope, or names no transaction.
    Unreadable,
    /// It is not about the transaction the move went under.
    OfAnotherTransaction,
    /// The move's own provider's statement of the move's own deposit, which doesn't count: it answers another
    /// operation, or its amounts break the rules against the one Core signed.
    NotCounted,
    /// The log or the evidence store refused or failed an append.
    Append(AppendFailure),
}

impl From<AppendFailure> for NotTaken {
    fn from(failure: AppendFailure) -> Self {
        NotTaken::Append(failure)
    }
}

impl Mover {
    /// A mover with nothing in hand.
    pub fn new() -> Self {
        Self::default()
    }

    /// The mover over `ports`, for one call of [`super::Stuck::carry_out`]. It carries out a `deliver`; a hand-on
    /// needs [`Delivering::handing_on`], and fails without it.
    pub fn on<'a>(&'a mut self, ports: Ports<'a>) -> Delivering<'a> {
        Delivering {
            mover: self,
            ports,
            hand: None,
        }
    }

    /// The withdrawal made for the hand-on `effect` carried out, once it is done (`T-020-17`): what the hand-on
    /// takes its route from, and what proves the linked order's start. `None` for anything else, and for a
    /// withdrawal whose outcome is unknown.
    pub fn withdrawn(&self, effect: &Receipt) -> Option<Withdrawn<'_>> {
        let Some(Kept::Hand(flight)) = self.kept.get(&key(effect)) else {
            return None;
        };
        // Done on the holding account's own record, kept as its stated answer; or left unknown and later known done
        // on that answer ([`Delivering::statement`]). Never on a start the log holds no answer for.
        if flight.failed {
            return None;
        }
        let (start, statement) = match (&flight.verdict, &flight.known) {
            (Some(Resolved::Done), _) => (flight.start.as_ref()?, flight.statement.as_ref()?),
            (_, Some((start, statement))) => (start, statement),
            _ => return None,
        };
        Some(Withdrawn {
            linked: &flight.linked,
            route: &flight.route,
            start,
            question: flight.question.as_ref().and_then(|q| q.receipt.as_ref()),
            statement: Some(statement),
        })
    }
}

/// A [`Mover`] over its ports.
pub struct Delivering<'a> {
    mover: &'a mut Mover,
    ports: Ports<'a>,
    hand: Option<HandingOn<'a>>,
}

impl<'a> Delivering<'a> {
    /// Gives the mover what a hand-on needs: the holding accounts and the linked orders' accepted routes. Without
    /// them every hand-on fails, with nothing handed over.
    pub fn handing_on(mut self, hand: HandingOn<'a>) -> Self {
        self.hand = Some(hand);
        self
    }
}

/// Whether the platform's refusal proves that nothing of the operation was passed out. Anything else, and any refusal
/// added later, doesn't: it is read as the outcome unknown.
fn nothing_passed_out(refusal: PlatformRefusal) -> bool {
    matches!(
        refusal,
        PlatformRefusal::MalformedOutput
            | PlatformRefusal::OversizedOutput
            | PlatformRefusal::LimitReached
            | PlatformRefusal::Unavailable
            | PlatformRefusal::NotHandled
            | PlatformRefusal::VersionExploited
            | PlatformRefusal::StatementKeyUnavailable
            | PlatformRefusal::StatementKeyMisconfigured
    )
}

/// The inputs a tuple that requires `required` takes, the destination's account filling the one it names; none where
/// the account can't fill what it requires.
fn inputs_for(required: &[String], account: &Id) -> Option<BTreeMap<String, String>> {
    match required {
        [] => Some(BTreeMap::new()),
        [only] => Some(BTreeMap::from([(
            only.clone(),
            account.as_str().to_owned(),
        )])),
        _ => None,
    }
}

fn key(receipt: &Receipt) -> String {
    serde_json::to_string(receipt).unwrap_or_default()
}

/// Core's clock, as a timestamp.
fn now(parts: &Parts<'_>) -> Result<Timestamp, AppendFailure> {
    parts
        .clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(super::NO_TIME))
}

/// The inner node of `provider`'s the value of `part` rests at, from Core's record of what it signed for the part's
/// settled segments: the destination of the latest one that took the value into a node of `provider`'s.
fn resting_node(
    parts: &Parts<'_>,
    order: &Id,
    part: &Id,
    provider: &Id,
    asset: &str,
) -> Option<String> {
    let followed = parts.segments.order(order)?;
    let part = followed
        .parts
        .iter()
        .find(|p| p.part_id.as_str() == part.as_str())?;
    let mut node = None;
    for segment in &part.segments {
        if segment.provider.as_str() != provider.as_str() || segment.state != SegmentState::Settled
        {
            continue;
        }
        let Some(id) = Id::new(segment.segment_id.as_str()) else {
            continue;
        };
        let Some((_, operation)) = parts.segments.sent(order, &id) else {
            continue;
        };
        node = match operation {
            ProviderOperation::Withdrawal(w) if w.asset == asset => Some(w.destination.clone()),
            ProviderOperation::Transfer(t) if t.asset == asset => Some(t.destination.clone()),
            ProviderOperation::Conversion(c) if c.to == asset => Some(c.node.clone()),
            _ => node,
        };
    }
    node
}

impl Delivering<'_> {
    /// Takes a provider's statement about the transaction a move went under as evidence of its outcome
    /// (`REQ-ORD-1`): the real outcome of a move left unknown, which a reporter reports and another actor confirms,
    /// each naming a statement ([`super::Stuck::known_outcome`], [`super::Stuck::confirm_outcome`]).
    ///
    /// Core checks it: it is attributable (its signature verifies under a statements key its maker published, in
    /// this deployment and protocol version), it is a result, and it names the move's transaction; the move's own
    /// provider's statement is judged as the segments judge one, against the operation Core signed. It is kept
    /// in the log as the maker's stated entry about the order and the transaction, naming no segment, once however
    /// often it is tried, and only the log's own rules say that it follows the move and was made after it
    /// (`outcome-confirmation-cites-verified-evidence`). It moves nothing, and the platform is handed nothing.
    pub fn statement(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Receipt,
        bytes: &[u8],
    ) -> Result<Stated, NotTaken> {
        let flight = match self.mover.kept.get(&key(effect)) {
            Some(Kept::Flight(flight)) => flight,
            Some(Kept::Hand(_)) => return self.hand_statement(parts, carrying, effect, bytes),
            _ => return Err(NotTaken::NoMove),
        };
        let (provider, transaction) = (flight.provider.clone(), flight.transaction.clone());
        let (operation, terms, units) = (
            flight.statement.statement.clone(),
            flight.terms.clone(),
            flight.units,
        );
        let at = parts.clock.now().floor();
        self.ports
            .catalogue
            .verify_statement(bytes, at)
            .map_err(|_| NotTaken::Unattributable)?;
        let body: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| NotTaken::Unreadable)?;
        if body["shape"] != rutent_contracts::provider_role::RESULT_SHAPE {
            return Err(NotTaken::Unreadable);
        }
        match body["statement"]["transaction"].as_str() {
            Some(named) if named == transaction.as_str() => {}
            Some(_) => return Err(NotTaken::OfAnotherTransaction),
            None => return Err(NotTaken::Unreadable),
        }
        let outcome = if body["maker"] == provider.as_str() {
            Some(
                match judge(
                    self.ports.catalogue,
                    at,
                    &provider,
                    &operation,
                    &terms,
                    None,
                    units,
                    bytes,
                ) {
                    Ok(Counted::Succeeded { .. }) => Resolved::Done,
                    Ok(Counted::Failed)
                    | Ok(Counted::Refused(Refusal::Final(_) | Refusal::LiftsAtKnownTime(_))) => {
                        Resolved::Failed
                    }
                    Ok(_) => Resolved::Unknown,
                    Err(_) => return Err(NotTaken::NotCounted),
                },
            )
        } else {
            None
        };
        let slot = (key(effect), bytes.to_vec());
        if !self.mover.statements.contains_key(&slot) {
            let about = About {
                order: Some(carrying.order.clone()),
                segment: None,
                transaction: Some(transaction),
            };
            let entry = stated_entry(
                self.ports.evidence,
                &now(parts)?,
                bytes,
                maker(self.ports.catalogue, bytes),
                Some(about),
            )
            .map_err(|why| match why {
                Unappended::Append(failure) => NotTaken::Append(failure),
                Unappended::Unrecordable => NotTaken::Unreadable,
            })?;
            let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
            self.mover.statements.insert(slot.clone(), Step::new(entry));
        }
        let step = self
            .mover
            .statements
            .get_mut(&slot)
            .ok_or(NotTaken::NoMove)?;
        let receipt = step.append_stated(parts.log)?;
        // A statement the log recorded unattributed rests on no key: it is evidence of nothing, and no entry of the
        // log's may rest on it.
        if is_receipt_of(&receipt, &step.entry) {
            return Err(NotTaken::Unattributable);
        }
        Ok(Stated { receipt, outcome })
    }

    /// Writes the move's flight: its operation signed and its question built, once. `None` where nothing can be
    /// asked, so the move fails before the platform is handed anything.
    fn write(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Result<Option<Flight>, AppendFailure> {
        let Action::ResolveStuck {
            resolution:
                Resolution::Deliver {
                    by,
                    to,
                    asset,
                    amount,
                },
            transaction,
            ..
        } = &effect.action
        else {
            // A hand-on is the hand-on's (`T-020-17`): this mover moves nothing of it.
            return Ok(None);
        };
        let Some(node) = resting_node(parts, carrying.order, carrying.part, by, &asset.id) else {
            return Ok(None);
        };
        let tuple = self
            .ports
            .catalogue
            .accepted_advertisement(by.as_str())
            .and_then(|ad| ad.statement.offers.reserve.as_ref())
            .and_then(|reserve| {
                reserve.tuples.iter().find(|t| {
                    t.origin == NodeRef::Inner(node.clone())
                        && t.destination == NodeRef::Outer(to.node.clone())
                        && t.asset == asset.id
                })
            });
        let Some(tuple) = tuple else {
            return Ok(None);
        };
        if tuple.decimals.origin != asset.decimals {
            return Ok(None);
        }
        // The destination's account fills the one input the tuple requires; a tuple that needs more can't be
        // filled from what the decision names.
        let Some(inputs) = inputs_for(&tuple.terms.required_inputs, &to.account) else {
            return Ok(None);
        };
        let operation = ProviderOperation::Deposit(Deposit {
            transaction: transaction.as_str().to_owned(),
            provider: by.as_str().to_owned(),
            origin: node,
            destination: to.node.clone(),
            asset: asset.id.clone(),
            amount: amount.amount().as_str().to_owned(),
            dry_run: false,
            inputs,
        });
        let at = now(parts)?;
        let Ok(signed) = sign_operation(
            self.ports.operator_key,
            self.ports.bounds,
            &self.ports.operator.acts,
            &at,
            operation,
        ) else {
            return Ok(None);
        };
        let Ok(statement) = serde_json::from_slice::<Statement<ProviderOperation>>(&signed) else {
            return Ok(None);
        };
        let about = About {
            order: Some(carrying.order.clone()),
            segment: None,
            transaction: Some(transaction.clone()),
        };
        let question = match stated_entry(
            self.ports.evidence,
            &at,
            &signed,
            self.ports.operator.clone(),
            Some(about),
        ) {
            Ok(entry) => entry,
            Err(crate::segments::Unappended::Append(failure)) => return Err(failure),
            Err(crate::segments::Unappended::Unrecordable) => return Ok(None),
        };
        let question = GeneralEntry::try_from(question).map_err(AppendFailure::Refused)?;
        Ok(Some(Flight {
            provider: by.clone(),
            transaction: transaction.clone(),
            statement,
            units: Units {
                origin_decimals: tuple.decimals.origin,
                out_decimals: tuple.decimals.destination,
            },
            terms: tuple.terms.clone(),
            question: Step::new(question),
            begun: false,
            carried: false,
            answered: None,
            verdict: None,
            ended: false,
            answer: None,
            drains: Vec::new(),
            uncarried: false,
        }))
    }

    /// Keeps one drain whole, as the steps of its items, once.
    fn keep(
        drains: &mut Vec<Step>,
        uncarried: &mut bool,
        parts: &mut Parts<'_>,
        core: &Actor,
        raw: Drained,
    ) -> Result<(), AppendFailure> {
        if raw.is_empty() {
            return Ok(());
        }
        let at = now(parts)?;
        let id = parts.fresh.id();
        let whats = raw
            .items()
            .map_err(|_| ())
            .and_then(|items| drain_entries(items, &id).map_err(|_| ()));
        match whats {
            Ok(whats) => {
                for what in whats {
                    drains.push(Step::new(built(core, what, &at, Outcome::Observed {})?));
                }
            }
            Err(()) => *uncarried = true,
        }
        Ok(())
    }
}

impl Resolutions for Delivering<'_> {
    /// The provider's own answer to the move, as the log holds it attributed, once the move is done on it: the
    /// evidence the part's `stuck_resolved` rests on (`REQ-ORD-39`). A move that was never answered, or whose
    /// answer the log recorded unattributed (its key unknown to it), has none.
    fn evidence_of(&self, effect: &Receipt) -> Option<Receipt> {
        let Some(Kept::Flight(flight)) = self.mover.kept.get(&key(effect)) else {
            return None;
        };
        if flight.verdict != Some(Resolved::Done) || flight.uncarried {
            return None;
        }
        let answer = flight.answer.as_ref()?;
        let receipt = answer.receipt.as_ref()?;
        (!is_receipt_of(receipt, &answer.entry)).then(|| receipt.clone())
    }

    fn may_have_handed_over(&self, effect: &Receipt) -> bool {
        match self.mover.kept.get(&key(effect)) {
            Some(Kept::Flight(flight)) => flight.carried,
            Some(Kept::Hand(flight)) => flight.asked,
            Some(Kept::Refused) => false,
            Some(Kept::Lost) | None => true,
        }
    }

    fn resolve(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Result<Resolved, AppendFailure> {
        if let Action::ResolveStuck {
            resolution: Resolution::HandOn { .. },
            ..
        } = &effect.action
        {
            return self.resolve_hand_on(parts, carrying, effect);
        }
        let at = key(&effect.receipt);
        let kept = match self.mover.kept.remove(&at) {
            Some(kept) => kept,
            // Acted on already, and this mover knows nothing of it (a restart, a fresh mover): the provider may hold
            // the operation, so it is never handed over again, and the outcome is unknown.
            None if carrying.again => Kept::Lost,
            None => match self.write(parts, carrying, effect)? {
                Some(flight) => Kept::Flight(Box::new(flight)),
                None => Kept::Refused,
            },
        };
        match kept {
            Kept::Refused => {
                self.mover.kept.insert(at, Kept::Refused);
                Ok(Resolved::Failed)
            }
            Kept::Lost | Kept::Hand(_) => {
                self.mover.kept.insert(at, Kept::Lost);
                Ok(Resolved::Unknown)
            }
            Kept::Flight(mut flight) => {
                let result = self.advance(parts, carrying, effect, &mut flight);
                self.mover.kept.insert(at, Kept::Flight(flight));
                result
            }
        }
    }
}

impl Delivering<'_> {
    /// Takes the move on from where it stopped (see the module's docs).
    fn advance(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
        flight: &mut Flight,
    ) -> Result<Resolved, AppendFailure> {
        // The question is appended before anything is handed over; one the log refuses backs no move: nothing
        // passed out, so it failed.
        if flight.verdict.is_none() && !flight.carried {
            match flight.question.append_stated(parts.log) {
                // The log records a statement attributed: its receipt is of the entry it recorded. One that is the
                // digest of the entry as built was recorded unattributed, its signing key unknown to the log, and
                // would reach the provider as a question the log can't tie to the operator, leaving the effect
                // open with no result. It backs no move.
                Ok(receipt) if is_receipt_of(&receipt, &flight.question.entry) => {
                    flight.verdict = Some(Resolved::Failed);
                }
                Ok(_) => {}
                Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable),
                Err(_) => {
                    flight.verdict = Some(Resolved::Failed);
                }
            }
        }
        let identifier =
            Identifier::new(flight.transaction.as_str()).ok_or(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("a transaction is an identifier"),
            ))?;
        if flight.verdict.is_none() && !flight.begun {
            match self
                .ports
                .modules
                .begin_segment(&flight.provider, &identifier)
            {
                Ok(_) => flight.begun = true,
                // The platform wouldn't begin it. Failed only for a refusal that proves nothing passed out; a
                // transaction it says began already (`DuplicateTransaction`) means the operation may be with the
                // provider.
                Err(refusal) => {
                    flight.verdict = Some(if nothing_passed_out(refusal) {
                        Resolved::Failed
                    } else {
                        Resolved::Unknown
                    });
                }
            }
        }
        if flight.verdict.is_none() && !flight.carried {
            flight.carried = true;
            let answered =
                self.ports
                    .modules
                    .carry_out(effect, &flight.provider, &flight.statement);
            // Everything that came back is kept before any step that can fail.
            let events: Events = self.ports.modules.take_events();
            let uses = self.ports.uses.take_uses();
            flight.answered = Some(answered);
            let (drains, uncarried) = (&mut flight.drains, &mut flight.uncarried);
            Self::keep(
                drains,
                uncarried,
                parts,
                carrying.core,
                Drained::Events(events),
            )?;
            Self::keep(drains, uncarried, parts, carrying.core, Drained::Uses(uses))?;
        }
        if flight.verdict.is_none() {
            let (verdict, ends) = self.judge(parts, carrying, flight)?;
            flight.verdict = Some(verdict);
            if ends && flight.begun && !flight.ended {
                flight.ended = true;
                let record = self.ports.modules.end_segment(&identifier);
                self.ports.modules.end_calls(&identifier);
                if let Some(record) = record {
                    let (drains, uncarried) = (&mut flight.drains, &mut flight.uncarried);
                    Self::keep(
                        drains,
                        uncarried,
                        parts,
                        carrying.core,
                        Drained::Record(record),
                    )?;
                }
            }
        }
        // What came back reaches the log before the effect's result: the platform's items, then the answer.
        for step in &mut flight.drains {
            step.append(parts.log)?;
        }
        if let Some(step) = flight.answer.as_mut() {
            step.append_stated(parts.log)?;
        }
        let verdict = flight.verdict.unwrap_or(Resolved::Unknown);
        // A drain Core can't write leaves the outcome unknown: what the platform passed out isn't on record.
        Ok(if flight.uncarried && verdict != Resolved::Failed {
            Resolved::Unknown
        } else {
            verdict
        })
    }

    /// What the answer says, once: the verdict, and whether it ends the segment. The answer is kept as the provider's
    /// statement about the order and the transaction, whether or not it counts.
    fn judge(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        flight: &mut Flight,
    ) -> Result<(Resolved, bool), AppendFailure> {
        let answered = flight.answered.clone().unwrap_or(Answered::Refused(
            // Nothing recorded of an answer: the platform wasn't asked, which is not an outcome either way.
            PlatformRefusal::OutcomeUnknown,
        ));
        let bytes = match answered {
            // The module passed something out, then ended with no answer.
            Answered::Refused(PlatformRefusal::OutcomeUnknown) => {
                return Ok((Resolved::Unknown, false));
            }
            // Refused before the module passed anything out: nothing moved, where the refusal says so.
            Answered::Refused(refusal) if nothing_passed_out(refusal) => {
                return Ok((Resolved::Failed, true));
            }
            Answered::Refused(_) => return Ok((Resolved::Unknown, false)),
            Answered::Bytes(bytes) => bytes,
        };
        let counted = judge(
            self.ports.catalogue,
            parts.clock.now().floor(),
            &flight.provider,
            &flight.statement.statement,
            &flight.terms,
            None,
            flight.units,
            &bytes,
        );
        let about = About {
            order: Some(carrying.order.clone()),
            segment: None,
            transaction: Some(flight.transaction.clone()),
        };
        let at = now(parts)?;
        match stated_entry(
            self.ports.evidence,
            &at,
            &bytes,
            maker(self.ports.catalogue, &bytes),
            Some(about),
        ) {
            Ok(entry) => {
                let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
                flight.answer = Some(Step::new(entry));
            }
            Err(Unappended::Append(failure)) => return Err(failure),
            // An answer that can't be recorded counts for nothing, and its outcome is unknown.
            Err(Unappended::Unrecordable) => return Ok((Resolved::Unknown, false)),
        }
        Ok(match counted {
            Ok(Counted::Succeeded { .. }) => (Resolved::Done, true),
            // The provider's own word that it failed or refused is its word alone: a provider that paid could say so
            // to be paid twice. The segment ends, but the outcome is unknown until another party confirms it
            // ([`Stuck::known_outcome`], [`Stuck::confirm_outcome`]).
            Ok(Counted::Failed)
            | Ok(Counted::Refused(Refusal::Final(_) | Refusal::LiftsAtKnownTime(_))) => {
                (Resolved::Unknown, true)
            }
            // The provider's own word that it doesn't know, that it waits on the person, a refusal of another
            // kind, or an answer that doesn't count: the value may have moved, so the part is held.
            _ => (Resolved::Unknown, false),
        })
    }
}

impl Delivering<'_> {
    /// Carries out a hand-on (`T-020-17`): the holding account's withdrawal for the linked order, over the linked
    /// order's accepted route, in this order, each step once:
    /// 1. the route is held to Core's own records: the linked order's own accepted route, one part, starting at the
    ///    inner node where Core's records say the part's value rests, in its asset, with no withdrawal before it, of
    ///    an order Core doesn't follow; the node's holding account is the provider the decision names, and the value
    ///    is all of what the part holds. Anything else fails, with the wallet asked nothing;
    /// 2. the operator's statement of the withdrawal is checked against what was decided, and recorded as the
    ///    operator's question about the linked order and the wallet's transaction, **attributed**, before the wallet
    ///    is handed it; one the log can't record attributed is never handed over;
    /// 3. the wallet is asked, once, through [`super::Withdrawals`];
    /// 4. what the platform and the proxy handed over is kept, and appended;
    /// 5. the answer: `Done` only for the holding account's own record of the withdrawal, which proves the start
    ///    ([`proves_start`]); `Failed` only where Core itself saw that nothing passed out (the route refused, the
    ///    question not recorded, the platform's refusal proving it); `Unknown` for everything else, the wallet's own
    ///    word that it failed included.
    fn resolve_hand_on(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Result<Resolved, AppendFailure> {
        let at = key(&effect.receipt);
        let kept = match self.mover.kept.remove(&at) {
            Some(kept) => kept,
            // Acted on already, and this mover knows nothing of it: the wallet may have been handed the
            // withdrawal, so it is never handed over again, and the outcome is unknown.
            None if carrying.again => Kept::Lost,
            None => match self.plan_hand(parts, carrying, effect) {
                Some(flight) => Kept::Hand(Box::new(flight)),
                None => Kept::Refused,
            },
        };
        match kept {
            Kept::Refused => {
                self.mover.kept.insert(at, Kept::Refused);
                Ok(Resolved::Failed)
            }
            Kept::Hand(mut flight) => {
                let result = self.withdraw(parts, carrying, effect, &mut flight);
                self.mover.kept.insert(at, Kept::Hand(flight));
                result
            }
            other => {
                // A deliver's flight under a hand-on's effect can't be; and a lost move stays lost.
                self.mover.kept.insert(at, Kept::Lost);
                let _ = other;
                Ok(Resolved::Unknown)
            }
        }
    }

    /// Holds the hand-on to Core's records before the wallet is asked anything; `None` where it can't be asked.
    fn plan_hand(
        &self,
        parts: &Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Option<HandFlight> {
        let hand = self.hand.as_ref()?;
        let Action::ResolveStuck {
            resolution:
                Resolution::HandOn {
                    by,
                    linked_order,
                    asset,
                    amount,
                },
            ..
        } = &effect.action
        else {
            return None;
        };
        // All of what the part holds, where Core's own records say it rests, in the asset decided.
        let (at, held) = resting(parts.segments, carrying.order, carrying.part)?;
        if at.asset != *asset || held != *amount {
            return None;
        }
        let NodeRef::Inner(node) = &at.node else {
            return None;
        };
        // The linked order's own accepted route, an order Core doesn't follow yet: one part, starting where the
        // value sits with no withdrawal.
        if parts.segments.order(linked_order).is_some() {
            return None;
        }
        let route = hand.routes.accepted(linked_order)?;
        if route.order != *linked_order {
            return None;
        }
        let [only] = route.plan.as_slice() else {
            return None;
        };
        let first = only.segments.first()?;
        if first.kind == OperationKind::Withdrawal
            || first.from != at
            || first.provider.as_str() != by.as_str()
        {
            return None;
        }
        // The customer's own acceptance of the linked order, which the log holds, its decision record in the log,
        // and the plan within what the acceptance says yes to, as the hand-on holds them: all before the holding
        // account is asked anything, since a withdrawal done can't be taken back. The operator never chooses where
        // the money goes (`REQ-ORD-43`).
        let entries = hand.log.entries();
        let requester = crate::hand_on::requester_of(&entries, carrying.order)?;
        crate::hand_on::linked_accepted(
            &entries,
            requester,
            linked_order,
            &route.decision_record,
            &route.acceptance,
            &route.plan,
        )
        .ok()?;
        // The node's holding account is the provider the decision names.
        if hand.withdrawals.holder(node).as_deref() != Some(by.as_str()) {
            return None;
        }
        Some(HandFlight {
            provider: by.clone(),
            linked: linked_order.clone(),
            amount: amount.clone(),
            route,
            question: None,
            asked: false,
            verdict: None,
            start: None,
            shown: None,
            answer: None,
            statement: None,
            known: None,
            failed: false,
            drains: Vec::new(),
            uncarried: false,
        })
    }

    /// Takes the withdrawal on from where it stopped: asks the wallet once, keeps what came back, appends it.
    fn withdraw(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
        flight: &mut HandFlight,
    ) -> Result<Resolved, AppendFailure> {
        // The withdrawal rests on the effect the log admitted on a permit: nothing is asked without it.
        // (This parameter is read only here, and `entry_points`' scan of `core-authz` wants it among the
        // parameters of a function that asks the wallet.)
        debug_assert!(matches!(effect.action, Action::ResolveStuck { .. }));
        // A call that handed the wallet the statement and stopped before its answer was kept is never made again:
        // the wallet may hold the withdrawal.
        if flight.asked && flight.verdict.is_none() {
            flight.verdict = Some(Resolved::Unknown);
        }
        if flight.verdict.is_none() {
            let Some(hand) = self.hand.as_mut() else {
                // Nothing to ask the wallet through now: where it was asked, what it did is unknown.
                flight.verdict = Some(if flight.asked {
                    Resolved::Unknown
                } else {
                    Resolved::Failed
                });
                return Ok(flight.verdict.unwrap_or(Resolved::Unknown));
            };
            let Some(first) = flight
                .route
                .plan
                .first()
                .and_then(|part| part.segments.first())
                .cloned()
            else {
                flight.verdict = Some(Resolved::Failed);
                return Ok(Resolved::Failed);
            };
            let NodeRef::Inner(node) = first.from.node.clone() else {
                flight.verdict = Some(Resolved::Failed);
                return Ok(Resolved::Failed);
            };
            let amount = flight.amount.amount().as_str().to_owned();
            let order = carrying.order.as_str().to_owned();
            let (provider, linked) = (flight.provider.clone(), flight.linked.clone());
            let asset = first.from.asset.id.clone();
            let (mut sent, mut unavailable) = (false, false);
            let mut question: Option<Step> = None;
            let mut shown: Option<Shown> = None;
            let mut asked = |statement: &Statement<ProviderOperation>| -> bool {
                // What the operator signed is what was decided: the holding account's own withdrawal, in full.
                let ProviderOperation::Withdrawal(w) = &statement.statement else {
                    return false;
                };
                let exact = w.provider == provider.as_str()
                    && w.destination == node
                    && w.asset == asset
                    && w.amount == amount
                    && !w.dry_run;
                let (Some(transaction), Ok(bytes), Ok(at)) = (
                    Id::new(&w.transaction),
                    serde_json::to_vec(statement),
                    now(parts),
                ) else {
                    return false;
                };
                if !exact {
                    return false;
                }
                let wallets_transaction = transaction.clone();
                let about = About {
                    order: Some(linked.clone()),
                    segment: None,
                    transaction: Some(transaction),
                };
                let entry = match stated_entry(
                    &mut *self.ports.evidence,
                    &at,
                    &bytes,
                    self.ports.operator.clone(),
                    Some(about),
                ) {
                    Ok(entry) => entry,
                    Err(Unappended::Append(AppendFailure::Unavailable)) => {
                        unavailable = true;
                        return false;
                    }
                    Err(_) => return false,
                };
                let Ok(entry) = GeneralEntry::try_from(entry) else {
                    return false;
                };
                let mut step = Step::new(entry);
                match step.append_stated(parts.log) {
                    // One that is the digest of the entry as built was recorded unattributed, its signing key
                    // unknown to the log: no question the log can tie to the operator.
                    Ok(receipt) if is_receipt_of(&receipt, &step.entry) => false,
                    Ok(_) => {
                        sent = true;
                        question = Some(step);
                        shown = Some(Shown {
                            transaction: wallets_transaction,
                        });
                        true
                    }
                    Err(AppendFailure::Unavailable) => {
                        unavailable = true;
                        false
                    }
                    Err(_) => false,
                }
            };
            let answered = hand.withdrawals.withdraw(
                &flight.route.plan[0].segments,
                &order,
                &amount,
                &mut asked,
            );
            flight.asked = sent;
            if let Some(step) = question {
                flight.question = Some(step);
            }
            flight.shown = shown;
            match answered {
                Ok(start) if sent => {
                    // The record proves the start, and the wallet's own signed answer, which the host keeps as the
                    // holder's stated entry about the linked order, says the same of the wallet's own transaction.
                    let transaction = flight.shown.as_ref().map(|s| s.transaction.clone());
                    let said = transaction.as_ref().and_then(|t| {
                        said(
                            &start.answer,
                            &flight.provider,
                            t,
                            &node,
                            &first,
                            carrying.order,
                        )
                    });
                    let proven = proves_start(
                        &start.record,
                        &flight.provider,
                        &first,
                        carrying.order,
                        flight.amount.amount(),
                    )
                    .is_ok()
                        && start.reserve == flight.provider.as_str()
                        && start.node == node
                        && start.first_segment == first.segment_id
                        && transaction.as_ref().map(Id::as_str) == Some(start.transaction.as_str())
                        && said == Some(Said::Withdrew(start.record.clone()));
                    // Core keeps the wallet's answer whatever it says: it is the holder's word, for a report and a
                    // confirmation of the outcome to name. One that can't be kept leaves the outcome unknown.
                    let kept = self.keep_answer(parts, effect, flight, &start);
                    flight.verdict = Some(if proven && kept {
                        Resolved::Done
                    } else {
                        // The wallet says it paid out, in a record that proves nothing of this start, or in an
                        // answer Core can't keep.
                        Resolved::Unknown
                    });
                    flight.start = Some(start);
                }
                // The account paid out and the operator's question was never recorded: Core can't vouch for it.
                Ok(_) => flight.verdict = Some(Resolved::Unknown),
                Err(_) if !sent => {
                    if unavailable {
                        // The log couldn't take the question now: nothing was handed over, and the withdrawal is
                        // asked again on the next call.
                        return Err(AppendFailure::Unavailable);
                    }
                    // The route refused, or the question not recorded: nothing passed out, as Core itself saw.
                    flight.verdict = Some(Resolved::Failed);
                }
                // Failed only where the platform's own refusal proves that nothing passed out; the wallet's word
                // that it failed, and every other refusal, may hide a withdrawal.
                Err(StartError::Wallet(WalletError::NotAnswered(refusal)))
                    if nothing_passed_out(refusal) =>
                {
                    flight.verdict = Some(Resolved::Failed);
                }
                Err(_) => flight.verdict = Some(Resolved::Unknown),
            }
            // What came back is kept once, after the verdict is: a step that can't be taken now leaves the outcome
            // unknown, never the wallet asked again.
            if sent {
                let events: Events = self.ports.modules.take_events();
                let uses = self.ports.uses.take_uses();
                let (drains, uncarried) = (&mut flight.drains, &mut flight.uncarried);
                let kept = Self::keep(
                    drains,
                    uncarried,
                    parts,
                    carrying.core,
                    Drained::Events(events),
                )
                .and_then(|()| {
                    Self::keep(drains, uncarried, parts, carrying.core, Drained::Uses(uses))
                });
                if kept.is_err() {
                    flight.uncarried = true;
                }
            }
        }
        for step in &mut flight.drains {
            step.append(parts.log)?;
        }
        // The holder's answer reaches the log before the effect's result, and only one recorded attributed, the
        // wallet's own word under its own key, backs a done: one the log can't tie to the wallet is no record.
        if let Some(bytes) = flight.answer.clone()
            && let Some(step) = self
                .mover
                .statements
                .get_mut(&(key(&effect.receipt), bytes))
        {
            let receipt = step.append_stated(parts.log)?;
            if is_receipt_of(&receipt, &step.entry) {
                if flight.verdict == Some(Resolved::Done) {
                    flight.verdict = Some(Resolved::Unknown);
                }
            } else {
                flight.statement = Some(receipt);
            }
        }
        let verdict = flight.verdict.unwrap_or(Resolved::Unknown);
        Ok(if flight.uncarried && verdict != Resolved::Failed {
            Resolved::Unknown
        } else {
            verdict
        })
    }

    /// Builds the holding account's own answer as its stated entry about the linked order and the wallet's own
    /// transaction, once, for the effect `effect`: the record the linked order's start rests on
    /// (`holding-withdrawal-rests-on-its-record`) and what a hand-on's outcome is reported on. False where it can't
    /// be built, so no done rests on it.
    fn keep_answer(
        &mut self,
        parts: &Parts<'_>,
        effect: &Effect,
        flight: &mut HandFlight,
        start: &HandedOnStart,
    ) -> bool {
        let (Some(transaction), Ok(at)) = (Id::new(&start.transaction), now(parts)) else {
            return false;
        };
        let about = About {
            order: Some(flight.linked.clone()),
            segment: None,
            transaction: Some(transaction),
        };
        let slot = (key(&effect.receipt), start.answer.clone());
        if !self.mover.statements.contains_key(&slot) {
            let Ok(entry) = stated_entry(
                &mut *self.ports.evidence,
                &at,
                &start.answer,
                maker(self.ports.catalogue, &start.answer),
                Some(about),
            ) else {
                return false;
            };
            let Ok(entry) = GeneralEntry::try_from(entry) else {
                return false;
            };
            self.mover.statements.insert(slot, Step::new(entry));
        }
        flight.answer = Some(start.answer.clone());
        true
    }

    /// Takes a statement about the wallet's own withdrawal as evidence of a hand-on's outcome (`REQ-ORD-1`,
    /// `T-020-26`), as [`Delivering::statement`] takes a deliver's: attributable, a result, naming the transaction
    /// the wallet ran the withdrawal under (which the question was recorded under, not the one the decision named),
    /// and, where the holding account made it, judged against the withdrawal Core asked for. Kept once as the
    /// maker's stated entry about the linked order, and nothing is handed to the platform. Its outcome is the holding
    /// account's own word: `Done` only for a record that proves the start; its failure, or one it can't say, frees
    /// nothing until another party confirms it.
    fn hand_statement(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Receipt,
        bytes: &[u8],
    ) -> Result<Stated, NotTaken> {
        let Some(Kept::Hand(flight)) = self.mover.kept.get(&key(effect)) else {
            return Err(NotTaken::NoMove);
        };
        // A wallet that was never handed the statement was asked nothing: no answer of its is a move's.
        let Some(shown) = flight.shown.clone() else {
            return Err(NotTaken::NoMove);
        };
        let (provider, linked) = (flight.provider.clone(), flight.linked.clone());
        let Some(first) = flight.route.plan.first().and_then(|p| p.segments.first()) else {
            return Err(NotTaken::NoMove);
        };
        let (first, amount) = (first.clone(), flight.amount.clone());
        let NodeRef::Inner(node) = first.from.node.clone() else {
            return Err(NotTaken::NoMove);
        };
        let at = parts.clock.now().floor();
        self.ports
            .catalogue
            .verify_statement(bytes, at)
            .map_err(|_| NotTaken::Unattributable)?;
        let body: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| NotTaken::Unreadable)?;
        if body["shape"] != rutent_contracts::provider_role::RESULT_SHAPE {
            return Err(NotTaken::Unreadable);
        }
        match body["statement"]["transaction"].as_str() {
            Some(named) if named == shown.transaction.as_str() => {}
            Some(_) => return Err(NotTaken::OfAnotherTransaction),
            None => return Err(NotTaken::Unreadable),
        }
        let mut proved: Option<WithdrawalRecord> = None;
        let outcome = if body["maker"] == provider.as_str() {
            Some(
                match said(
                    bytes,
                    &provider,
                    &shown.transaction,
                    &node,
                    &first,
                    carrying.order,
                ) {
                    Some(Said::Withdrew(record)) => {
                        // A record that says it succeeded and proves nothing of this start bears out nothing.
                        proves_start(&record, &provider, &first, carrying.order, amount.amount())
                            .map_err(|_| NotTaken::NotCounted)?;
                        proved = Some(record);
                        Resolved::Done
                    }
                    Some(Said::Failed) => Resolved::Failed,
                    Some(Said::Unknown) => Resolved::Unknown,
                    None => return Err(NotTaken::NotCounted),
                },
            )
        } else {
            None
        };
        let slot = (key(effect), bytes.to_vec());
        if !self.mover.statements.contains_key(&slot) {
            let about = About {
                order: Some(linked),
                segment: None,
                transaction: Some(shown.transaction.clone()),
            };
            let entry = stated_entry(
                self.ports.evidence,
                &now(parts)?,
                bytes,
                maker(self.ports.catalogue, bytes),
                Some(about),
            )
            .map_err(|why| match why {
                Unappended::Append(failure) => NotTaken::Append(failure),
                Unappended::Unrecordable => NotTaken::Unreadable,
            })?;
            let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
            self.mover.statements.insert(slot.clone(), Step::new(entry));
        }
        let step = self
            .mover
            .statements
            .get_mut(&slot)
            .ok_or(NotTaken::NoMove)?;
        let receipt = step.append_stated(parts.log)?;
        // A statement the log recorded unattributed rests on no key: it is evidence of nothing.
        if is_receipt_of(&receipt, &step.entry) {
            return Err(NotTaken::Unattributable);
        }
        if outcome == Some(Resolved::Failed)
            && let Some(Kept::Hand(flight)) = self.mover.kept.get_mut(&key(effect))
        {
            flight.failed = true;
            flight.known = None;
        }
        if let (Some(record), Some(Kept::Hand(flight))) =
            (proved, self.mover.kept.get_mut(&key(effect)))
        {
            flight.known = Some((
                HandedOnStart {
                    reserve: provider.as_str().to_owned(),
                    node,
                    first_segment: first.segment_id.clone(),
                    record,
                    transaction: shown.transaction.as_str().to_owned(),
                    answer: bytes.to_vec(),
                },
                receipt.clone(),
            ));
        }
        Ok(Stated { receipt, outcome })
    }
}

/// What a holding account's own signed answer says of the withdrawal asked of it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Said {
    /// It succeeded, for the whole amount asked, with the wallet's own record of it.
    Withdrew(WithdrawalRecord),
    /// The wallet says it failed or refused finally.
    Failed,
    /// It says it can't tell, or says something that isn't an outcome of the withdrawal.
    Unknown,
}

/// What `bytes`, a signed result, says of the withdrawal under `transaction` at `node` for `order`: `None` where it
/// isn't the `provider`'s own result of that withdrawal.
fn said(
    bytes: &[u8],
    provider: &Id,
    transaction: &Id,
    node: &str,
    first: &rutent_contracts::state_machine::SegmentPlan,
    order: &Id,
) -> Option<Said> {
    let signed: Statement<ResultBody> = serde_json::from_slice(bytes).ok()?;
    if signed.maker != provider.as_str() {
        return None;
    }
    let ResultBody::Withdrawal(result) = signed.statement else {
        return None;
    };
    if result.transaction != transaction.as_str() {
        return None;
    }
    Some(match result.outcome {
        ReserveMovementOutcome::Succeeded(settled) => {
            let reference =
                proof_of_withdrawal(provider.as_str(), transaction.as_str(), &settled.evidence)
                    .ok();
            let moved = settled.amount_out.parse::<u128>().ok();
            match (reference, moved) {
                (Some(reference), Some(amount)) if settled.amount_in == settled.amount_out => {
                    Said::Withdrew(WithdrawalRecord {
                        by: provider.as_str().to_owned(),
                        node: node.to_owned(),
                        asset: first.from.asset.id.clone(),
                        order: order.as_str().to_owned(),
                        amount,
                        reference,
                    })
                }
                _ => Said::Unknown,
            }
        }
        ReserveMovementOutcome::Failed(_) => Said::Failed,
        ReserveMovementOutcome::Refused(refused)
            if matches!(
                refused.refusal,
                Refusal::Final(_) | Refusal::LiftsAtKnownTime(_)
            ) =>
        {
            Said::Failed
        }
        _ => Said::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tuple that requires no input takes none, one takes the destination's account, and more than one can't be
    /// filled from what the decision names, so the move fails before anything is asked.
    // verifies: REQ-AUZ-4
    #[test]
    fn only_a_tuple_whose_inputs_the_account_can_fill_is_asked() {
        let account = Id::new("acct-2").unwrap();
        assert_eq!(inputs_for(&[], &account), Some(BTreeMap::new()));
        assert_eq!(
            inputs_for(&["beneficiary.bank_account".to_owned()], &account),
            Some(BTreeMap::from([(
                "beneficiary.bank_account".to_owned(),
                "acct-2".to_owned()
            )]))
        );
        assert_eq!(
            inputs_for(&["a".to_owned(), "b".to_owned()], &account),
            None
        );
    }

    /// Only a refusal that proves nothing passed out is read as nothing having moved.
    // verifies: INV-2
    #[test]
    fn only_a_refusal_proving_nothing_passed_out_is_read_as_nothing_moved() {
        assert!(nothing_passed_out(PlatformRefusal::Unavailable));
        assert!(!nothing_passed_out(PlatformRefusal::DuplicateTransaction));
        assert!(!nothing_passed_out(PlatformRefusal::OutcomeUnknown));
    }
}
