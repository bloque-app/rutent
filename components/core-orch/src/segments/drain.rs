//! What the platform and the proxy hand over, as the log's `hand_over` entries (`REQ-MDL-8`, `REQ-MDL-6`,
//! `REQ-MDL-5`; core-internal's README, "The handed-over items").
//!
//! Every item of one drain (one call of `take_events`, one of `take_uses`, or one segment's record) becomes one entry
//! naming that drain and its place in it, so two identical items of one drain are two entries, and an entry appended
//! again is still one. Nothing is dropped: an item the contract can't carry (an identifier the platform names that
//! isn't one, say) is refused with the whole drain kept, never skipped, so it fails loudly rather than leaving a gap.
//!
//! prov's enums map to the contract's tokens with exhaustive matches: a variant prov adds fails this build until it is
//! named here (core-internal's README, "The proxy's and the platform's reasons are tokens").

use rutent_contracts::core_internal::{
    HandedOver, LookupOutcome, Move, MoveReason, Part, StateKey, Token, UseOf, UseOutcome, What,
};
use rutent_contracts::module_interface::PartName;
use rutent_contracts::provider_role::SafeInteger;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::module::lifecycle::{self, SegmentRecord};
use rutent_prov::module::types::{self, Events, LimitKind, Metered, Scope};
use rutent_prov::proxy::{self, Refusal, ResolveError, Use};

/// Why a drain couldn't be written as entries: the item at this place names something the contract can't carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncarried {
    /// The item's place in its drain.
    pub position: usize,
    /// What it names that the contract can't carry.
    pub what: &'static str,
}

/// The items of one platform drain, in its order: outbound call records, cross-module calls, secret lookups, state
/// reads and bounds hit. An operation that ended with no answer (`Events::unknown`) has no item in the contract; it
/// is carried into its segment's outcome instead (a gap for a later `F-020` row, in the feature's notes).
pub(super) fn of_events(events: &Events) -> Result<Vec<HandedOver>, Uncarried> {
    let Events {
        records,
        lookups,
        state_reads,
        calls,
        limits,
        unknown: _,
    } = events;
    let mut items = Vec::new();
    for record in records {
        items.push(HandedOver::OutboundCall {
            record: record.clone(),
        });
    }
    for call in calls {
        items.push(HandedOver::CrossModuleCall { call: call.clone() });
    }
    for lookup in lookups {
        let position = items.len();
        items.push(HandedOver::SecretLookup {
            provider: id(&lookup.provider, position)?,
            part: part(lookup.part),
            lookup: lookup.lookup.clone(),
            outcome: lookup.outcome.map(|o| match o {
                types::LookupOutcome::Resolved => LookupOutcome::Resolved {},
                types::LookupOutcome::Refused(reason) => LookupOutcome::Refused { reason },
            }),
        });
    }
    for read in state_reads {
        let position = items.len();
        items.push(HandedOver::StateRead {
            provider: id(&read.provider, position)?,
            part: part(read.part),
            transaction: id(&read.transaction, position)?,
            key: StateKey::new(read.key.clone()).ok_or(Uncarried {
                position,
                what: "a state key",
            })?,
            found: read.found,
        });
    }
    for hit in limits {
        let position = items.len();
        items.push(HandedOver::Limit {
            provider: id(&hit.provider, position)?,
            transaction: id(&hit.transaction, position)?,
            limit: token(limit(hit.hit.kind), position)?,
            scope: hit
                .hit
                .scope
                .map(|s| token(scope(s), position))
                .transpose()?,
        });
    }
    Ok(items)
}

/// The items of one proxy drain, in its order: each use, resolved or refused, with its transaction.
pub(super) fn of_uses(uses: &[Use]) -> Result<Vec<HandedOver>, Uncarried> {
    let mut items = Vec::new();
    for (position, used) in uses.iter().enumerate() {
        let of = match &used.of {
            proxy::UseOf::Secret(name) => UseOf::Secret { name: name.clone() },
            proxy::UseOf::StatementKey(_) => UseOf::StatementKey {},
        };
        let outcome = match &used.outcome {
            Ok(()) => UseOutcome::Resolved {},
            Err(refusal) => UseOutcome::Refused {
                reason: token(refused(refusal), position)?,
            },
        };
        items.push(HandedOver::SecretUse {
            provider: id(used.provider.as_str(), position)?,
            module_version: used
                .module_version
                .as_ref()
                .map(|v| id(v.as_str(), position))
                .transpose()?,
            transaction: id(used.transaction.as_str(), position)?,
            operation: used.operation.clone(),
            of,
            outcome,
        });
    }
    Ok(items)
}

/// The one item of a segment's record, its moves dated by the caller's clock.
pub(super) fn of_record(record: &SegmentRecord) -> Result<HandedOver, Uncarried> {
    let moves = record
        .moves
        .iter()
        .map(|m| {
            Ok(Move {
                from: id(&m.from, 0)?,
                to: id(&m.to, 0)?,
                reason: match m.reason {
                    lifecycle::MoveReason::Stale => MoveReason::Stale,
                },
                at: m.at.rfc3339().and_then(Timestamp::new).ok_or(Uncarried {
                    position: 0,
                    what: "a move's time",
                })?,
            })
        })
        .collect::<Result<Vec<_>, Uncarried>>()?;
    Ok(HandedOver::SegmentRecord {
        transaction: id(&record.transaction, 0)?,
        provider: id(&record.provider, 0)?,
        began_on: id(&record.began_on, 0)?,
        runs_on: id(&record.runs_on, 0)?,
        moves,
    })
}

/// One drain's items as `hand_over` entries, each naming the drain and its place.
pub(super) fn entries(items: Vec<HandedOver>, drain: &Id) -> Result<Vec<What>, Uncarried> {
    items
        .into_iter()
        .enumerate()
        .map(|(position, item)| {
            Ok(What::HandOver {
                item,
                drain: drain.clone(),
                position: SafeInteger::try_from(position as u64).map_err(|_| Uncarried {
                    position,
                    what: "a place in a drain",
                })?,
            })
        })
        .collect()
}

fn id(text: &str, position: usize) -> Result<Id, Uncarried> {
    Id::new(text).ok_or(Uncarried {
        position,
        what: "an identifier",
    })
}

fn token(text: &'static str, position: usize) -> Result<Token, Uncarried> {
    Token::new(text).ok_or(Uncarried {
        position,
        what: "a token",
    })
}

fn part(part: PartName) -> Part {
    match part {
        PartName::Sandboxed => Part::Sandboxed,
        PartName::Outside => Part::Outside,
    }
}

fn limit(kind: LimitKind) -> &'static str {
    match kind {
        LimitKind::Metered(metered) => match metered {
            Metered::OutboundCalls => "outbound_calls",
            Metered::OutboundBytes => "outbound_bytes",
            Metered::StateReads => "state_reads",
            Metered::StateBytes => "state_bytes",
            Metered::OutsideRequests => "outside_requests",
            Metered::ModuleCalls => "module_calls",
            Metered::SecretLookups => "secret_lookups",
        },
        LimitKind::CallDepth => "call_depth",
        LimitKind::KeyLength => "key_length",
        LimitKind::NameLength => "name_length",
        LimitKind::MessageSize => "message_size",
        LimitKind::Deadline => "deadline",
        LimitKind::Fuel => "fuel",
        LimitKind::Memory => "memory",
    }
}

fn scope(scope: Scope) -> &'static str {
    match scope {
        Scope::Operation => "operation",
        Scope::Provider => "provider",
    }
}

fn refused(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::NoRole => "no_role",
        Refusal::NoRelease => "no_release",
        Refusal::NotAccepted => "not_accepted",
        Refusal::ReleaseOfAnotherProvider => "release_of_another_provider",
        Refusal::AdvertisementOfAnotherProvider => "advertisement_of_another_provider",
        Refusal::AdvertisementOfAnotherDeployment => "advertisement_of_another_deployment",
        Refusal::NotTheOperatorsRelease => "not_the_operators_release",
        Refusal::AdvertisementOfAnOperatorsModule => "advertisement_of_an_operators_module",
        Refusal::UnaddressableName => "unaddressable_name",
        Refusal::NoRequestInFlight => "no_request_in_flight",
        Refusal::RequestOfAnotherProvider => "request_of_another_provider",
        Refusal::NotTheRequestInFlight => "not_the_request_in_flight",
        Refusal::UndeclaredOperation => "undeclared_operation",
        Refusal::UndeclaredSecret => "undeclared_secret",
        Refusal::StatementKeyNotAdvertised => "statement_key_not_advertised",
        Refusal::StatementKeyForAdvertising => "statement_key_for_advertising",
        Refusal::StatementKeyIsOperatorKey => "statement_key_is_operator_key",
        Refusal::NotFound => "not_found",
        Refusal::Store(store) => match store {
            ResolveError::InvalidName => "store_invalid_name",
            ResolveError::NotFound => "store_not_found",
            ResolveError::Denied => "store_denied",
            ResolveError::Unavailable { .. } => "store_unavailable",
            ResolveError::Unexpected { .. } => "store_unexpected",
            ResolveError::Malformed => "store_malformed",
            ResolveError::Transport(_) => "store_transport",
        },
    }
}
