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
    Deposit, NodeRef, Operation as ProviderOperation, Refusal, Statement, Terms,
};
use rutent_contracts::router_api::{Id, PolicyBounds, Timestamp};
use rutent_contracts::state_machine::SegmentState;
use rutent_prov::catalogue::Catalogue;
use rutent_prov::module::types::Events;

use super::journal::{Step, built};
use super::{Carrying, Parts, Resolutions, Resolved};
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
    Flight(Box<Flight>),
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

    /// The mover over `ports`, for one call of [`super::Stuck::carry_out`].
    pub fn on<'a>(&'a mut self, ports: Ports<'a>) -> Delivering<'a> {
        Delivering { mover: self, ports }
    }
}

/// A [`Mover`] over its ports.
pub struct Delivering<'a> {
    mover: &'a mut Mover,
    ports: Ports<'a>,
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
        let Some(Kept::Flight(flight)) = self.mover.kept.get(&key(effect)) else {
            return Err(NotTaken::NoMove);
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
        let inputs = match tuple.terms.required_inputs.as_slice() {
            [] => BTreeMap::new(),
            [only] => BTreeMap::from([(only.clone(), to.account.as_str().to_owned())]),
            _ => return Ok(None),
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
        flight: &mut Flight,
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
                    flight
                        .drains
                        .push(Step::new(built(core, what, &at, Outcome::Observed {})?));
                }
            }
            Err(()) => flight.uncarried = true,
        }
        Ok(())
    }
}

impl Resolutions for Delivering<'_> {
    fn resolve(
        &mut self,
        parts: &mut Parts<'_>,
        carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Result<Resolved, AppendFailure> {
        let at = key(&effect.receipt);
        let kept = match self.mover.kept.remove(&at) {
            Some(kept) => kept,
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
                // The platform wouldn't begin it: nothing passed out, so the move failed.
                Err(_) => flight.verdict = Some(Resolved::Failed),
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
            Self::keep(flight, parts, carrying.core, Drained::Events(events))?;
            Self::keep(flight, parts, carrying.core, Drained::Uses(uses))?;
        }
        if flight.verdict.is_none() {
            let (verdict, ends) = self.judge(parts, carrying, flight)?;
            flight.verdict = Some(verdict);
            if ends && flight.begun && !flight.ended {
                flight.ended = true;
                let record = self.ports.modules.end_segment(&identifier);
                self.ports.modules.end_calls(&identifier);
                if let Some(record) = record {
                    Self::keep(flight, parts, carrying.core, Drained::Record(record))?;
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
            // Refused before the module passed anything out: nothing moved.
            Answered::Refused(_) => return Ok((Resolved::Failed, true)),
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
            Ok(Counted::Failed)
            | Ok(Counted::Refused(Refusal::Final(_) | Refusal::LiftsAtKnownTime(_))) => {
                (Resolved::Failed, true)
            }
            // The provider's own word that it doesn't know, that it waits on the person, a refusal of another
            // kind, or an answer that doesn't count: the value may have moved, so the part is held.
            _ => (Resolved::Unknown, false),
        })
    }
}
