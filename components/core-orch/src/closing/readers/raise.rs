//! The raise that tells the operator a listed network didn't start (`T-020-60` (2), `REQ-CAT-22`, `DEC-28`): a
//! network whose reader is unreachable or fails Core's start-up check is held off every route, and Core raises it to
//! the operator in the log, citing the listing its reader was held to (`raise-of-a-reader-not-started-cites-its-listing`).
//! It moves nothing (ADR-0009).
//!
//! **Once in a spell.** A network is raised the first time a check finds it not started, and not again until its reader
//! passes a check: the spell then ends, and a later failure raises again. The spell is Core's own record, held for as
//! long as Core's process lives, so a reopened Core raises every network still not started once more: the operator is
//! told again after a restart (a default, failing towards telling).

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Action, Actor, CatalogueChange, Difference, Entry, Outcome, Raise, ReaderNotStarted, Receipt,
    TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::router_api::Id;

use super::{NotStarted, Unreadable};
use crate::segments::RaiseDesk;

/// The networks raised in the spell now running, by name: Core's own record (see the module's docs).
#[derive(Debug, Clone, Default)]
pub struct Raised {
    networks: BTreeSet<String>,
}

impl Raised {
    /// Raises each network of `not_started` that isn't raised in its spell yet, citing the last listing of it in
    /// `history`, and ends the spell of every network `listed` names that `not_started` doesn't: its reader passed.
    /// Answers the receipt of each raise appended. A network the log holds no listing of, or one not checked yet
    /// ([`NotStarted::Unchecked`]), isn't raised. A refused raise is answered, the network left out of its spell, so
    /// the next check tries again.
    ///
    /// # Errors
    ///
    /// The first append the log refused or couldn't take now; every other network is still tried.
    pub fn raise(
        &mut self,
        listed: &[String],
        not_started: &BTreeMap<String, NotStarted>,
        history: &[(Receipt, Entry)],
        desk: RaiseDesk<'_>,
    ) -> Result<Vec<Receipt>, AppendFailure> {
        let RaiseDesk {
            log,
            clock,
            operator,
            core,
            ..
        } = desk;
        for network in listed {
            if !not_started.contains_key(network) {
                self.networks.remove(network);
            }
        }
        let mut first = Ok(());
        let mut appended = Vec::new();
        for (network, why) in not_started {
            if self.networks.contains(network) {
                continue;
            }
            let (Some(why), Some(listing)) = (reason(why), listing_of(history, network)) else {
                continue;
            };
            let raised = (|| {
                let at = clock
                    .now()
                    .timestamp()
                    .ok_or(AppendFailure::Refused(Violation(
                        "Core's clock reads a time a timestamp can hold",
                    )))?;
                let raise = Raise {
                    raise: raise_id(history, network)?,
                    difference: Difference::ReaderNotStarted {
                        network: Id::new(network.as_str())
                            .ok_or(AppendFailure::Refused(Violation("a network is an id")))?,
                        why,
                    },
                    cites: vec![listing],
                    at,
                };
                raise.check().map_err(AppendFailure::Refused)?;
                let delivered = matches!(operator.raise(&raise), Delivery::Delivered);
                let outcome = if delivered {
                    Outcome::Done {}
                } else {
                    Outcome::Failed {}
                };
                let entry = built(core, &raise, outcome)?;
                append(log, &entry)
            })();
            match raised {
                Ok(receipt) => {
                    self.networks.insert(network.clone());
                    appended.push(receipt);
                }
                Err(failure) => {
                    if first.is_ok() {
                        first = Err(failure);
                    }
                }
            }
        }
        first.map(|()| appended)
    }

    /// Whether `network` is raised in the spell now running.
    pub fn is_raised(&self, network: &str) -> bool {
        self.networks.contains(network)
    }
}

/// Why `network` didn't start, as the raise names it: `None` for one not checked yet.
pub fn reason(why: &NotStarted) -> Option<ReaderNotStarted> {
    Some(match why {
        NotStarted::ListingBreaksItsRules => ReaderNotStarted::ListingBreaksItsRules,
        NotStarted::OtherIdentity => ReaderNotStarted::OtherIdentity,
        NotStarted::OtherTrust => ReaderNotStarted::OtherTrust,
        NotStarted::Unchecked => return None,
        NotStarted::Unread(unreadable) => match unreadable {
            Unreadable::NoHost => ReaderNotStarted::NoHost,
            Unreadable::OtherCa => ReaderNotStarted::OtherCa,
            Unreadable::Unreachable | Unreadable::NotStarted | Unreadable::NotListed => {
                ReaderNotStarted::Unreachable
            }
            Unreadable::NoAnswer => ReaderNotStarted::NoAnswer,
            Unreadable::Malformed | Unreadable::Unstated => ReaderNotStarted::Malformed,
            Unreadable::NotSigned => ReaderNotStarted::NotSigned,
            Unreadable::Refused(_) => ReaderNotStarted::Refused,
            Unreadable::Reader(_) => ReaderNotStarted::ReaderUnreadable,
        },
    })
}

/// The receipt of the last decided listing of `network` in `history`: the `list_public_network` effect the log holds
/// done.
pub fn listing_of(history: &[(Receipt, Entry)], network: &str) -> Option<Receipt> {
    let done = |effect: &Receipt| {
        history.iter().any(|(_, e)| {
            matches!(e, Entry::Verified(VerifiedEntry {
                what: What::Result { effect: done },
                outcome: Outcome::Done {},
                ..
            }) if done == effect)
        })
    };
    history
        .iter()
        .rev()
        .find_map(|(receipt, entry)| match entry {
            Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change: CatalogueChange::ListPublicNetwork,
                                public_network: Some(listed),
                                ..
                            },
                        ..
                    },
                ..
            }) if listed.network.as_str() == network && done(receipt) => Some(receipt.clone()),
            _ => None,
        })
}

/// The raise's id: `reader-not-started-{network}-{n}`, `n` counting the raises of the network the log holds, so the
/// raise draws nothing from Core's seeded source (a scenario whose network fails mid-run shifts no later id).
fn raise_id(history: &[(Receipt, Entry)], network: &str) -> Result<Id, AppendFailure> {
    let before = history
        .iter()
        .filter(|(_, e)| {
            matches!(e, Entry::Verified(VerifiedEntry {
                what: What::Raise { raise },
                ..
            }) if matches!(&raise.difference,
                Difference::ReaderNotStarted { network: n, .. } if n.as_str() == network))
        })
        .count();
    Id::new(format!("reader-not-started-{network}-{}", before + 1))
        .ok_or(AppendFailure::Refused(Violation("a raise's id is an id")))
}

fn built(core: &Actor, raise: &Raise, outcome: Outcome) -> Result<GeneralEntry, AppendFailure> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: core.clone(),
        what: What::Raise {
            raise: raise.clone(),
        },
        when: When {
            at: raise.at.clone(),
            source: TimeSource::CoreClock,
        },
        outcome,
    });
    entry.check().map_err(AppendFailure::Refused)?;
    GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)
}

/// Appends the raise's entry, tried again while the log can't take it now (`INV-8`).
fn append(log: &mut dyn Log, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..crate::segments::APPEND_TRIES {
        match log.append(entry) {
            Ok(receipt) => return Ok(receipt),
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}
