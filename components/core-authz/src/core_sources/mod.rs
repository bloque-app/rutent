//! Core's own answers to policy's questions (`T-020-55`, finding 4 of `M3`'s milestone review): what an input says
//! where the answer is Core's own state, read from its log and the advertisements its key register accepted, beside the
//! deployment's registers, which only the deployment can answer.
//!
//! # Whose answer is whose
//!
//! | Source | Answered by | From |
//! |---|---|---|
//! | `Requester`, `OrderOrigin` | Core | the order's record: the one its filing handed over before the log holds it ([`OpenedOrders`]), else the log's `order_opened`, else its acceptance |
//! | `OrderParties` | Core | the order's acceptance, as the log holds it; before it, the order's record, as above |
//! | `SegmentOrder` | Core | the plan that lists the segment: the order's `route_chosen` input, as the log holds it |
//! | `SegmentProvider` | Core | the effect that moved value on the segment, as the log holds it |
//! | `Statement` | Core | the stated entry naming the commitment: its maker's party, as the log attributed it |
//! | `Showing` | Core | the stated entry of a provider's result under the showing's commitment, about its order, segment and transaction, verified and received no later than the acceptance is decided |
//! | `Declarations` | Core | the reserve's advertisement the log's key register accepted last ([`AcceptedAdvertisements`]), its stated entry in the log: its reserve declaration |
//! | `Kept`, `KeptAbout` | Core | the log's stated entries, each naming a statement Core kept by its commitment |
//! | `ActsFor`, `AgentScope`, `Permissions`, `Account`, `Holder`, `Reaches`, `MeetsBelowOperator`, `OfOperator`, `Verifier`, `Provider`, `Consent`, `HoldingAccount` | the deployment ([`DeploymentRegisters`]) | its registers of actors, agents' scopes, permissions, accounts and their delegates, holding accounts and consents, and its verifiers and providers' answers (`F-030`) |
//!
//! The deployment is handed only what is its own: [`CoreSources`] answers every source of Core's itself and never
//! asks the deployment about one, so no assembly writes Core's v0 reading again, and none can answer it otherwise.
//! [`Registered`] can be built only here, for a source of the deployment's.
//!
//! # Failing closed
//!
//! Each answer is the one the scenario assembly's records gave before this row, or a refusal or not known: a log that
//! can't be read, an accepted advertisement that can't be read back, or an order register held elsewhere at the moment,
//! gives not known (`Reply::Unreachable`), and nothing moves on it; a record that holds nothing gives a missing input,
//! which the engine refuses as the order of another's (`T-023-01`). No answer here is a permit: each is only an input,
//! which the fixed rules and the deployment's policy rule over.
//!
//! # Readings
//!
//! - **An order's record.** An order a filing opens is authorised before the log holds anything of it, so its
//!   record is handed over first ([`OpenedOrders::opened`]) and dropped where it never opens
//!   ([`OpenedOrders::withdrawn`]), as core-orch's `FiledOrders` port says. Any other order's record is the log's:
//!   `order_opened`, as intake opened it, or its acceptance, for an order intake never opened (a linked order).
//! - **A reserve's declarations.** Only the advertisement the log's key register accepted last for the reserve is
//!   read, as core-audit's register holds it, by every rule it holds an advertisement to: never a stated advertisement
//!   it refused (a forged one, one another provider made naming the reserve, an older one replayed), whatever key it
//!   names. Where the register accepted none, or the one it accepted offers no reserve, the reserve declared nothing,
//!   and a decision resting on its declarations is refused as missing its input (the review of `8f7ddefd`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_contracts::core_internal::{Attribution, Entry, Parties, Receipt, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, RESULT_SHAPE, ReserveDeclaration};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine::Observed;

use crate::engine::{Need, Reply, Said, Source, Sources};
use crate::funds::ReadFailure;
use crate::roles::{AUTOMATIC_DEPOSIT, NO_USER_WITHDRAWAL};

/// Core's log, read: every entry it holds, each with its receipt, in log order, handed to `read` without a copy.
pub trait LogEntries {
    /// Hands the log's entries to `read` and answers what it gives, or why the log couldn't be read.
    ///
    /// # Errors
    ///
    /// Where the log can't be read now.
    fn read<T>(&mut self, read: impl FnOnce(&[(Receipt, Entry)]) -> T) -> Result<T, ReadFailure>;
}

/// An advertisement the log's key register accepted: the commitment of the stated entry that brought it, and the
/// reserve declaration it makes, where it offers a reserve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedAdvertisement {
    /// The commitment the log's stated entry names it by.
    pub commitment: Digest,
    /// Its reserve's declaration, where it offers a reserve.
    pub declaration: Option<ReserveDeclaration>,
}

/// Core's log's key register, read (`T-020-55`, the review of `8f7ddefd`): the advertisement of a provider the register
/// accepted last, by every rule it holds an advertisement to (core-audit's `AuditLog::accepted_advertisement`). A stated
/// advertisement the register refused is never one.
pub trait AcceptedAdvertisements {
    /// The advertisement of `provider` the register accepted last; `None` where it accepted none.
    ///
    /// # Errors
    ///
    /// Where the register can't be read now.
    fn accepted(&mut self, provider: &Id) -> Result<Option<AcceptedAdvertisement>, ReadFailure>;
}

/// The deployment's registers: what only the deployment can answer (`F-030` keeps them in production). Core asks it
/// only of a source of the deployment's, through [`Registered`], which nothing outside this module can build.
pub trait DeploymentRegisters {
    /// What the deployment's register says to `asked`.
    fn registered(&mut self, asked: &Registered<'_>) -> Reply<Said>;
}

/// One question to the deployment's registers: an input of a source that is the deployment's. Built only by
/// [`CoreSources`], and only for a source [`is_the_deployments`].
#[derive(Debug, Clone, Copy)]
pub struct Registered<'a> {
    need: &'a Need,
}

impl Registered<'_> {
    /// The input asked about, by the name the policy rule gives it: a login, a party, a provider.
    pub fn input(&self) -> &Id {
        &self.need.input
    }

    /// The register asked, and what it is asked of.
    pub fn source(&self) -> &Source {
        &self.need.from
    }
}

/// Whether `source` is the deployment's to answer, not Core's. Every source is named, so a new one is placed by
/// whoever adds it.
pub fn is_the_deployments(source: &Source) -> bool {
    match source {
        Source::ActsFor { .. }
        | Source::AgentScope { .. }
        | Source::Permissions { .. }
        | Source::Account(_)
        | Source::Holder { .. }
        | Source::Reaches { .. }
        | Source::MeetsBelowOperator { .. }
        | Source::OfOperator
        | Source::Verifier(_)
        | Source::Provider(_)
        | Source::Consent { .. }
        | Source::HoldingAccount { .. } => true,
        Source::Requester(_)
        | Source::OrderOrigin(_)
        | Source::OrderParties(_)
        | Source::SegmentOrder(_)
        | Source::SegmentProvider { .. }
        | Source::Statement(_)
        | Source::Showing { .. }
        | Source::Declarations(_)
        | Source::Kept(_)
        | Source::KeptAbout(_) => false,
    }
}

/// An order's record before the log holds it: who it is for and where its value leaves from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Opened {
    parties: Parties,
    origin_reserve: Id,
}

/// The orders a filing opened before the log holds anything of them, each with its record, as core-orch's
/// `FiledOrders` port hands them over. Shared: the assembly forwards that port's calls to a clone of the one
/// [`CoreSources`] holds.
#[derive(Debug, Clone, Default)]
pub struct OpenedOrders(Rc<RefCell<BTreeMap<String, Opened>>>);

impl OpenedOrders {
    /// The order `order` was opened on an automatic deposit, for `parties`, its value leaving from `origin_reserve`;
    /// its authorisation is asked next.
    pub fn opened(&self, order: &Id, parties: &Parties, origin_reserve: &Id) {
        self.0.borrow_mut().insert(
            order.as_str().to_owned(),
            Opened {
                parties: parties.clone(),
                origin_reserve: origin_reserve.clone(),
            },
        );
    }

    /// The order handed over by [`OpenedOrders::opened`] never opened: its record is dropped.
    pub fn withdrawn(&self, order: &Id) {
        self.0.borrow_mut().remove(order.as_str());
    }

    /// The record of `order`, if one was handed over; not known where the register is held elsewhere at the moment.
    fn of(&self, order: &Id) -> Result<Option<Opened>, ReadFailure> {
        self.0
            .try_borrow()
            .map(|orders| orders.get(order.as_str()).cloned())
            .map_err(|_| ReadFailure::Unavailable)
    }
}

/// Policy's sources as Core answers them: its own state from its log (`L`) and its log's key register (`A`), and the
/// deployment's registers (`R`) for what is the deployment's alone.
#[derive(Debug, Clone)]
pub struct CoreSources<L, A, R> {
    log: L,
    accepted: A,
    registers: R,
    opened: OpenedOrders,
}

fn said(yes: bool) -> Reply<Said> {
    Reply::Given(if yes { Said::Yes } else { Said::No })
}

impl<L, A, R> CoreSources<L, A, R> {
    /// Core's sources over its log, its log's key register and the deployment's registers.
    pub fn new(log: L, accepted: A, registers: R) -> Self {
        CoreSources {
            log,
            accepted,
            registers,
            opened: OpenedOrders::default(),
        }
    }

    /// The orders a filing hands over before the log holds them, shared.
    pub fn opened_orders(&self) -> OpenedOrders {
        self.opened.clone()
    }

    /// The deployment's registers.
    pub fn registers(&self) -> &R {
        &self.registers
    }
}

/// The parties and origin reserve of `order` as `entries` record it: intake's `order_opened`, else its acceptance.
fn logged_record(entries: &[(Receipt, Entry)], order: &Id) -> Option<Opened> {
    let mut accepted = None;
    for (_, entry) in entries {
        let Entry::Verified(v) = entry else { continue };
        match &v.what {
            What::OrderOpened {
                order: o,
                parties,
                origin_reserve,
                ..
            } if o == order => {
                return Some(Opened {
                    parties: (**parties).clone(),
                    origin_reserve: origin_reserve.clone(),
                });
            }
            What::Acceptance {
                order: o,
                parties,
                origin_reserve,
                ..
            } if o == order && accepted.is_none() => {
                accepted = Some(Opened {
                    parties: parties.clone(),
                    origin_reserve: origin_reserve.clone(),
                });
            }
            _ => {}
        }
    }
    accepted
}

/// The parties of `order`'s first acceptance in `entries`.
fn accepted_parties(entries: &[(Receipt, Entry)], order: &Id) -> Option<Parties> {
    entries.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::Acceptance {
                order: o, parties, ..
            } if o == order => Some(parties.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })
}

/// The order the first `route_chosen` in `entries` listing `segment` gives it to.
fn order_of_segment(entries: &[(Receipt, Entry)], segment: &Id) -> Option<String> {
    entries.iter().find_map(|(_, e)| {
        let Entry::Verified(v) = e else { return None };
        let What::Input { input } = &v.what else {
            return None;
        };
        match &input.observed {
            Observed::RouteChosen { parts, .. } => parts
                .iter()
                .flat_map(|p| &p.segments)
                .any(|s| s.segment_id.as_str() == segment.as_str())
                .then(|| input.subject.order.as_str().to_owned()),
            _ => None,
        }
    })
}

/// The provider the first move of value on `segment` of `order` in `entries` went to.
fn provider_of_segment(entries: &[(Receipt, Entry)], order: &Id, segment: &Id) -> Option<Id> {
    entries.iter().find_map(|(_, e)| {
        let Entry::Verified(v) = e else { return None };
        match &v.what {
            What::Effect {
                action:
                    rutent_contracts::core_internal::Action::MoveValue {
                        order: o,
                        segment: s,
                        provider: p,
                        ..
                    },
                ..
            } if o == order && s == segment => Some(p.clone()),
            _ => None,
        }
    })
}

/// The party of the maker of the first stated entry in `entries` naming `commitment`, as the log attributed it.
fn maker_of(entries: &[(Receipt, Entry)], commitment: &Digest) -> Option<Id> {
    entries.iter().find_map(|(_, e)| match e {
        Entry::Stated(s) if &s.commitment == commitment => {
            Some(s.attribution.maker().party.clone())
        }
        _ => None,
    })
}

/// Whether `entries` name a statement kept under `commitment` about `order`, or, unless `itself`, about no order.
fn kept_in(entries: &[(Receipt, Entry)], commitment: &Digest, order: &Id, itself: bool) -> bool {
    entries.iter().any(|(_, e)| match e {
        Entry::Stated(s) => {
            let about = s.about.as_ref().and_then(|a| a.order.as_ref());
            &s.commitment == commitment
                && match about {
                    Some(o) => o == order,
                    None => !itself,
                }
        }
        Entry::Verified(_) => false,
    })
}

/// Whether `entries` hold `showing` of `segment` of `order`: a provider's result, verified, stated under the
/// showing's commitment about that order, segment and transaction, and received no later than `decided_at`.
fn shown_in(
    entries: &[(Receipt, Entry)],
    order: &Id,
    segment: &Id,
    showing: &rutent_contracts::core_internal::ShowingNamed,
    decided_at: &Timestamp,
) -> bool {
    entries.iter().any(|(_, e)| match e {
        Entry::Stated(s) => {
            let about = s.about.as_ref();
            s.what.statement.as_str() == RESULT_SHAPE
                && matches!(s.attribution, Attribution::Verified { .. })
                && s.commitment == showing.commitment
                && about.and_then(|a| a.order.as_ref()) == Some(order)
                && about.and_then(|a| a.segment.as_ref()) == Some(segment)
                && about.and_then(|a| a.transaction.as_ref()) == Some(&showing.transaction)
                && s.received_at.unix_nanos() <= decided_at.unix_nanos()
        }
        Entry::Verified(_) => false,
    })
}

/// Whether `entries` hold a stated advertisement under `commitment`.
fn advertised_under(entries: &[(Receipt, Entry)], commitment: &Digest) -> bool {
    entries.iter().any(|(_, e)| {
        matches!(e, Entry::Stated(s)
            if &s.commitment == commitment && s.what.statement.as_str() == ADVERTISEMENT_SHAPE)
    })
}

impl<L: LogEntries, A: AcceptedAdvertisements, R> CoreSources<L, A, R> {
    /// The record of `order`: the one handed over, else the log's.
    fn record(&mut self, order: &Id) -> Result<Option<Opened>, ReadFailure> {
        if let Some(opened) = self.opened.of(order)? {
            return Ok(Some(opened));
        }
        self.log.read(|entries| logged_record(entries, order))
    }

    /// Whether `party` is `order`'s requester, beneficiary or payer: by its acceptance, else by its record.
    fn order_parties(&mut self, order: &Id, party: &Id) -> Result<Option<bool>, ReadFailure> {
        let parties = match self.log.read(|entries| accepted_parties(entries, order))? {
            Some(parties) => Some(parties),
            None => self.record(order)?.map(|r| r.parties),
        };
        // Its requester, beneficiary or payer, as the log's `customer-read-is-of-its-own-order` holds it: the
        // reserve a settlement account is at is none of them.
        Ok(parties.map(|p| &p.requester == party || &p.beneficiary == party || &p.payer == party))
    }

    /// The declaration in force of `reserve`: the one its advertisement the log's key register accepted last makes,
    /// where the log holds that advertisement's stated entry. Nothing where the register accepted none, or the one it
    /// accepted offers no reserve: no other advertisement, and no stated entry the register refused, is read.
    fn declaration(&mut self, reserve: &Id) -> Result<Option<ReserveDeclaration>, ReadFailure> {
        // A log that can't be read leaves the answer not known, as every other of Core's answers.
        self.log.read(|_| ())?;
        let Some(accepted) = self.accepted.accepted(reserve)? else {
            return Ok(None);
        };
        let logged = self
            .log
            .read(|entries| advertised_under(entries, &accepted.commitment))?;
        Ok(accepted.declaration.filter(|_| logged))
    }

    /// What Core says to `need`, a source of Core's own.
    fn cores(&mut self, need: &Need) -> Result<Reply<Said>, ReadFailure> {
        let input = &need.input;
        let known = |answer: Option<bool>| match answer {
            Some(yes) => said(yes),
            None => Reply::Missing,
        };
        Ok(match &need.from {
            Source::Requester(order) => {
                known(self.record(order)?.map(|r| &r.parties.requester == input))
            }
            Source::OrderOrigin(order) => {
                known(self.record(order)?.map(|r| &r.origin_reserve == input))
            }
            Source::OrderParties(order) => known(self.order_parties(order, input)?),
            Source::SegmentOrder(segment) => known(
                self.log
                    .read(|entries| order_of_segment(entries, segment))?
                    .map(|o| o == input.as_str()),
            ),
            Source::SegmentProvider { order, segment } => known(
                self.log
                    .read(|entries| provider_of_segment(entries, order, segment))?
                    .map(|p| &p == input),
            ),
            Source::Statement(commitment) => known(
                self.log
                    .read(|entries| maker_of(entries, commitment))?
                    .map(|m| &m == input),
            ),
            Source::Showing {
                order,
                segment,
                showing,
                decided_at,
            } => said(
                self.log
                    .read(|entries| shown_in(entries, order, segment, showing, decided_at))?,
            ),
            Source::Declarations(reserve) => match self.declaration(reserve)? {
                None => Reply::Missing,
                Some(d) => match input.as_str() {
                    NO_USER_WITHDRAWAL => said(!d.can_require_user_initiated_withdrawal),
                    AUTOMATIC_DEPOSIT => said(d.starts_routes_on_automatic_deposits),
                    // A reserve that published made each declaration either way: one it didn't make is a no.
                    _ => said(false),
                },
            },
            Source::Kept(commitment) => said(
                self.log
                    .read(|entries| kept_in(entries, commitment, input, false))?,
            ),
            Source::KeptAbout(commitment) => said(
                self.log
                    .read(|entries| kept_in(entries, commitment, input, true))?,
            ),
            // The deployment's: never asked here.
            _ => Reply::Missing,
        })
    }
}

impl<L: LogEntries, A: AcceptedAdvertisements, R: DeploymentRegisters> Sources
    for CoreSources<L, A, R>
{
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        if is_the_deployments(&need.from) {
            return self.registers.registered(&Registered { need });
        }
        // A log, an accepted advertisement or a register that can't be read now: not known, and nothing moves on it.
        self.cores(need).unwrap_or(Reply::Unreachable)
    }

    /// The parties of an order its filing handed over, or the log holds its record of, before its acceptance is in
    /// the log.
    fn order_record(&mut self, order: &Id) -> Reply<Parties> {
        match self.record(order) {
            Ok(Some(record)) => Reply::Given(record.parties),
            Ok(None) => Reply::Missing,
            Err(_) => Reply::Unreachable,
        }
    }
}

#[cfg(test)]
mod tests;
