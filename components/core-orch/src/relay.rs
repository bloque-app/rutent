//! Who passed a statement on (`REQ-AUD-2`): the one place Core's writers of a stated entry set `relayed_by`.
//!
//! A statement is recorded as its claimed maker's, unverified; where someone other than that maker presented it, the
//! entry says so apart from its maker, so a relay's copy of another party's signed statement is never recorded as if
//! its maker had presented it, and the log's restatement read can tell a maker's resend from a relay's copy
//! (`core-audit`'s `Statements` sets it the same way). Every writer of a stated entry takes who presented the statement
//! and calls [`relayed_by`], the only place in this crate that sets the field: a writer that set it to nothing itself
//! would lose the relay, and a test holds that none does.

use rutent_contracts::core_internal::{Actor, ActorKind, Role};
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::{Catalogue, Publisher};

/// The `relayed_by` of a stated entry claimed by `maker` and presented by `presented_by`: the presenter where it isn't
/// the maker, and nothing where the maker presented its own statement. Compared by the component acting (`acts`), as
/// `core-audit`'s `Statements` compares them.
pub(crate) fn relayed_by(maker: &Actor, presented_by: &Actor) -> Option<Actor> {
    (presented_by.acts != maker.acts).then(|| presented_by.clone())
}

/// The actor of a provider (or of a verifier, in `role`) that handed Core a statement: acting for the party the
/// catalogue says publishes it, or for the operator where the operator does, and for itself where it names none.
pub(crate) fn presenter(catalogue: &Catalogue, acts: &Id, role: Role) -> Actor {
    let party = match catalogue.publishers().publisher(acts.as_str()) {
        Some(Publisher::Operator) => Id::new("operator"),
        Some(Publisher::Party(publisher)) => Id::new(publisher),
        None => None,
    };
    Actor {
        party: party.unwrap_or_else(|| acts.clone()),
        acts: acts.clone(),
        role,
        kind: ActorKind::Component,
    }
}

/// The actor `bytes` claim made them: the maker their envelope names, as `actor_of` makes it an actor, or
/// `presented_by` where the envelope names none Core can read or names the presenter itself.
pub(crate) fn claimed_by(
    bytes: &[u8],
    presented_by: &Actor,
    actor_of: impl FnOnce(&Id) -> Actor,
) -> Actor {
    let named = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v.get("maker").and_then(|m| m.as_str()).and_then(Id::new));
    match named {
        Some(maker) if maker != presented_by.acts => actor_of(&maker),
        _ => presented_by.clone(),
    }
}
