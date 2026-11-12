//! The citation table: every relation an entry must hold to earlier entries of the log, each with the `SPEC.md` ids
//! that demand it, and the one engine, [`admit_entry`], that reads it.
//!
//! A row names what in the citing entry points back ([`Relation::cites`]), what kind of earlier entry must answer it
//! ([`Relation::answers`]), and the fields that must match. An `exists` row is refused when no earlier entry answers
//! it; a `none other` row when one does. `contracts/core-internal/README.md` lists the same table, and
//! `tests/core-internal.rs` holds an admitted and a refused case for every row and every matched field.

use alloc::vec::Vec;

use super::Violation;
use super::standing::{decided_part, standing};
use super::types::*;
use crate::provider_role::EvidenceGrade;
use crate::router_api::Id;
use crate::state_machine::Observed;

/// Whether a row asks for an earlier entry that answers it, or for none that does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantifier {
    /// Some earlier entry must answer each citation.
    Exists,
    /// No earlier entry, other than the citing entry itself retried, may answer it.
    NoneOther,
}

/// One citation an entry makes under a row: the receipt it names, where it names one, and which of its citations it
/// is (an index into a list, such as a statement among several).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cite {
    /// The receipt cited, where the citation is by receipt; `None` where the answering entry is found by its fields.
    pub receipt: Option<Receipt>,
    /// Which citation of the entry this is.
    pub index: usize,
}

/// How an earlier entry answers a citation: the relation table's own answer, not policy's [`Answer`](super::Answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// It isn't the kind of entry, or doesn't carry the key, the row asks for.
    NotTarget,
    /// It is, but the row's field at this index doesn't match.
    Mismatch(usize),
    /// It answers the citation.
    Holds,
}

/// The log's history: each entry it holds with its receipt, in order.
pub type History = [(Receipt, Entry)];

/// The citations an entry makes under a row, given the history: a verified entry's, or, for a row of
/// [`STATED_RELATIONS`], a stated entry's.
pub type Cites<C = VerifiedCiting> = fn(&C, &History) -> Vec<Cite>;

/// How `target`, an earlier entry, answers a citation `entry` makes, given the whole history.
pub type Answers<C = VerifiedCiting> = fn(&C, &Cite, &Entry, &History) -> Match;

/// A verified entry as the rows of [`RELATIONS`] read it: with the maker this deployment's operator signs its own
/// statements as, so a row can tell the operator's own statements from a module's it publishes. It reads as the
/// entry itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCiting {
    /// The verified entry.
    pub entry: VerifiedEntry,
    /// The maker the deployment's operator signs as.
    pub operator: Id,
}

impl core::ops::Deref for VerifiedCiting {
    type Target = VerifiedEntry;

    fn deref(&self) -> &VerifiedEntry {
        &self.entry
    }
}

/// One row of the citation table, over the citing entries of kind `C`: verified entries ([`RELATIONS`]) or stated
/// ones ([`STATED_RELATIONS`]).
pub struct Relation<C: 'static = VerifiedCiting> {
    /// The row's name, which its fixtures are named after.
    pub name: &'static str,
    /// The `SPEC.md` ids that demand the relation.
    pub spec: &'static [&'static str],
    /// Exists or none other.
    pub quantifier: Quantifier,
    /// The refusal when no earlier entry answers (exists) or one does (none other).
    pub refusal: &'static str,
    /// The fields that must match, each with the refusal when the answering entry is found but that field differs.
    pub fields: &'static [(&'static str, &'static str)],
    /// The citations `entry` makes under this row, given the history; none where the row doesn't apply to it.
    pub cites: Cites<C>,
    /// How `target`, an earlier entry, answers `cite` made by `entry`, given the whole history.
    pub answers: Answers<C>,
}

fn verified(entry: &Entry) -> Option<&VerifiedEntry> {
    match entry {
        Entry::Verified(v) => Some(v),
        Entry::Stated(_) => None,
    }
}

fn permitted(entry: &Entry) -> Option<&VerifiedEntry> {
    verified(entry).filter(|v| v.outcome == (Outcome::Permitted {}))
}

fn stated(entry: &Entry) -> Option<&StatedEntry> {
    match entry {
        Entry::Stated(s) => Some(s),
        Entry::Verified(_) => None,
    }
}

/// `Holds` where `target` and every field hold, the first failing field's `Mismatch` where only fields fail.
fn judge(target: bool, fields: &[bool]) -> Match {
    if !target {
        return Match::NotTarget;
    }
    match fields.iter().position(|ok| !ok) {
        Some(i) => Match::Mismatch(i),
        None => Match::Holds,
    }
}

fn by_receipt(receipt: &Receipt, index: usize) -> Cite {
    Cite {
        receipt: Some(receipt.clone()),
        index,
    }
}

fn by_fields(index: usize) -> Vec<Cite> {
    alloc::vec![Cite {
        receipt: None,
        index,
    }]
}

fn none() -> Vec<Cite> {
    Vec::new()
}

/// The requester of `order` as its first acceptance in `history` records it.
fn requester_of<'a>(order: &str, history: &'a [(Receipt, Entry)]) -> Option<&'a Id> {
    history.iter().find_map(|(_, e)| match &verified(e)?.what {
        What::Acceptance {
            order: o, parties, ..
        } if o.as_str() == order => Some(&parties.requester),
        _ => None,
    })
}

/// The citing entry's effect action, if it records an effect.
fn effect_action(entry: &VerifiedEntry) -> Option<(&Action, &Permit)> {
    match &entry.what {
        What::Effect { action, decision } => Some((action, decision)),
        _ => None,
    }
}

/// The commitment the effect at `receipt` in `history` erases, where it is an `erase_statement` effect.
fn erased_by<'a>(
    receipt: &Receipt,
    history: &'a History,
) -> Option<&'a crate::module_interface::Digest> {
    history.iter().find_map(|(r, e)| match &verified(e)?.what {
        What::Effect {
            action: Action::EraseStatement { commitment, .. },
            ..
        } if r == receipt => Some(commitment),
        _ => None,
    })
}

/// The commitment `entry` erases: an `erasure` entry's, or an `erase_statement` decision's or effect's.
fn erasing(entry: &VerifiedEntry) -> Option<&crate::module_interface::Digest> {
    match &entry.what {
        What::Erasure { evidence } => Some(evidence),
        What::Effect {
            action: Action::EraseStatement { commitment, .. },
            ..
        } => Some(commitment),
        What::Decide { request, .. } => match &request.action {
            Action::EraseStatement { commitment, .. } => Some(commitment),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `entry` is one that can rest on a statement: anything but the erasure's own path (its decision, effect
/// and record) and a read of a statement or a permission over one, which rest on nothing they name.
fn rests_on_statements(entry: &VerifiedEntry) -> bool {
    !matches!(
        &entry.what,
        What::Erasure { .. }
            | What::CheckRead { .. }
            | What::GrantPermission { .. }
            | What::RevokePermission { .. }
            | What::Result { .. }
            | What::Effect {
                action: Action::EraseStatement { .. },
                ..
            }
    ) && !matches!(&entry.what, What::Decide { request, .. }
        if matches!(request.action, Action::EraseStatement { .. }))
}

/// Whether `value` holds `needle` anywhere.
fn holds_value(value: &serde_json::Value, needle: &serde_json::Value) -> bool {
    value == needle
        || match value {
            serde_json::Value::Array(items) => items.iter().any(|v| holds_value(v, needle)),
            serde_json::Value::Object(map) => map.values().any(|v| holds_value(v, needle)),
            _ => false,
        }
}

/// Whether `entry` cites the statement kept under `commitment`: the receipt of a stated entry of `history` naming
/// it, or the commitment itself, anywhere in what it records.
fn cites_statement(
    entry: &VerifiedEntry,
    commitment: &crate::module_interface::Digest,
    history: &History,
) -> bool {
    let Ok(what) = serde_json::to_value(&entry.what) else {
        return false;
    };
    let mut needles: Vec<serde_json::Value> = history
        .iter()
        .filter(|(_, e)| stated(e).is_some_and(|s| &s.commitment == commitment))
        .filter_map(|(r, _)| serde_json::to_value(r).ok())
        .collect();
    needles.extend(serde_json::to_value(commitment).ok());
    needles.iter().any(|n| holds_value(&what, n))
}

/// The provider of `segment` of `order`: the one this log's `move_value` effect for it names.
fn segment_provider<'a>(order: &Id, segment: &Id, history: &'a History) -> Option<&'a Id> {
    history
        .iter()
        .find_map(|(_, e)| match effect_action(verified(e)?)? {
            (
                Action::MoveValue {
                    order: o,
                    segment: s,
                    provider,
                    ..
                },
                _,
            ) if o == order && s == segment => Some(provider),
            _ => None,
        })
}

/// The citing entry's recorded input, if it records one.
fn input(entry: &VerifiedEntry) -> Option<&crate::state_machine::RecordedInput> {
    match &entry.what {
        What::Input { input } => Some(input),
        _ => None,
    }
}

/// The input's own order.
fn input_order(entry: &VerifiedEntry) -> &str {
    input(entry).map_or("", |i| i.subject.order.as_str())
}

/// The provider, operator or module a stated entry names as making it: the actor whose key signs, attributed or
/// claimed. Every row comparing a statement's maker reads this, never the party it states for, which a sibling under
/// the same publisher shares.
fn maker_provider(statement: &StatedEntry) -> &Id {
    &statement.attribution.maker().acts
}

/// The order `transaction` belongs to, and the segment, where it names one: those the first stated entry naming it
/// under an order names (see `transaction-belongs-to-one-order`).
fn binding_of<'a>(transaction: &Id, history: &'a History) -> Option<(&'a Id, Option<&'a Id>)> {
    history.iter().find_map(|(_, e)| {
        let s = stated(e)?;
        let (order, t) = order_transaction(s)?;
        (t == transaction).then(|| (order, segment_of(s)))
    })
}

/// The segment a stated entry is about, where it names one.
fn segment_of(statement: &StatedEntry) -> Option<&Id> {
    statement.about.as_ref()?.segment.as_ref()
}

/// Whether `statement` is about what the entry citing it is about: the order its `about` names is `order`, the
/// segment it names is `segment` where the citing entry names one, and the transaction it names belongs to `order`
/// and, where the citing entry names a segment, to that segment. A statement about nothing is citable on its body;
/// the log verifies none whose body names a transaction ([`attribute`](super::attribute)).
fn about_agrees(
    statement: &StatedEntry,
    order: Option<&Id>,
    segment: Option<&Id>,
    history: &History,
) -> bool {
    let Some(about) = &statement.about else {
        return true;
    };
    about.order.as_ref().is_none_or(|o| Some(o) == order)
        && about
            .segment
            .as_ref()
            .is_none_or(|s| segment.is_none_or(|c| c == s))
        && about
            .transaction
            .as_ref()
            .is_none_or(|t| match binding_of(t, history) {
                Some((o, s)) => Some(o) == order && segment.is_none_or(|c| s == Some(c)),
                None => order.is_none() && segment.is_none(),
            })
}

/// A stated entry made by `party`, about what its citing entry is about: its signature verified, `party` the actor
/// whose key signs it ([`maker_provider`]), and its `about` the citing entry's ([`about_agrees`]). An unverified
/// statement is attributed to no one (`REQ-AUD-2`).
fn statement_by(
    target: &Entry,
    party: &Id,
    order: Option<&Id>,
    segment: Option<&Id>,
    history: &History,
) -> Match {
    match stated(target) {
        Some(s) => judge(
            true,
            &[
                matches!(s.attribution, Attribution::Verified { .. }),
                maker_provider(s) == party,
                about_agrees(s, order, segment, history),
            ],
        ),
        None => Match::NotTarget,
    }
}

/// A stated entry about what its citing entry is about ([`about_agrees`]).
fn statement_about(
    target: &Entry,
    order: Option<&Id>,
    segment: Option<&Id>,
    history: &History,
) -> Match {
    match stated(target) {
        Some(s) => judge(true, &[about_agrees(s, order, segment, history)]),
        None => Match::NotTarget,
    }
}

/// The citation table, in the order the engine reads it. `contracts/core-internal/README.md` lists it.
pub static RELATIONS: [Relation; 63] = [
    Relation {
        name: "effect-permits-a-decided-action",
        spec: &["REQ-AUZ-1"],
        quantifier: Quantifier::Exists,
        refusal: "an effect's permit names a permitted decide entry of this log",
        fields: &[
            (
                "request",
                "an effect's permit names its decide entry's request",
            ),
            (
                "decision",
                "an effect's permit names its decide entry's decision",
            ),
            (
                "action",
                "an effect's action is the one its decide entry decided",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::ResolveStuck { .. }, _)) | None => none(),
            Some((_, permit)) => alloc::vec![by_receipt(permit.receipt(), 0)],
        },
        answers: |e, _, t, _| {
            let (Some((action, permit)), Some(t)) = (effect_action(e), permitted(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Decide { request, decision } => judge(
                    true,
                    &[
                        &request.request == permit.request(),
                        decision == permit.decision(),
                        &request.action == action,
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "stuck-effect-permits-a-stuck-move",
        spec: &["REQ-AUZ-4"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck order's effect names a permitted stuck move of this log, never a decide",
        fields: &[
            (
                "request",
                "a stuck order's effect names its stuck move's request",
            ),
            (
                "decision",
                "a stuck order's effect names its stuck move's decision",
            ),
            (
                "order",
                "a stuck order's effect is for its stuck move's order",
            ),
            (
                "carries_out",
                "a stuck order's effect carries out its stuck move's decide step",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::ResolveStuck { .. }, permit)) => {
                alloc::vec![by_receipt(permit.receipt(), 0)]
            }
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                Some((
                    Action::ResolveStuck {
                        order, carries_out, ..
                    },
                    permit,
                )),
                Some(t),
            ) = (effect_action(e), permitted(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::StuckMove { request, decision } => judge(
                    true,
                    &[
                        &request.request == permit.request(),
                        decision == permit.decision(),
                        &request.order == order,
                        &request.carries_out == carries_out,
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "permit-backs-one-effect",
        spec: &["INV-8"],
        quantifier: Quantifier::NoneOther,
        refusal: "a permit backs one effect only",
        fields: &[],
        cites: |e, _| effect_action(e).map_or_else(none, |_| by_fields(0)),
        answers: |e, _, t, _| {
            let (Some((_, permit)), Some(t)) = (effect_action(e), verified(t)) else {
                return Match::NotTarget;
            };
            judge(
                t != &e.entry
                    && matches!(&t.what, What::Effect { decision, .. } if decision == permit),
                &[],
            )
        },
    },
    Relation {
        name: "move-rests-on-its-authorisation",
        spec: &["REQ-AUZ-3"],
        quantifier: Quantifier::Exists,
        refusal: "a value-moving effect rests on a permitted authorisation of this log",
        fields: &[
            (
                "order",
                "a value-moving effect's authorisation is of its order",
            ),
            (
                "destination",
                "a value-moving effect's route ends at its authorisation's destination",
            ),
            (
                "to_asset",
                "a value-moving effect's route delivers its authorisation's to asset",
            ),
            (
                "floor",
                "a value-moving effect's route delivers at least its authorisation's floor",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                Some((
                    Action::MoveValue {
                        order,
                        authorisation,
                        route,
                        ..
                    },
                    _,
                )),
                Some(t),
            ) = (effect_action(e), permitted(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Authorise {
                    authorisation: a, ..
                } => judge(
                    &a.authorisation == authorisation,
                    &[
                        &a.order == order,
                        a.destination == route.destination,
                        a.to_asset == route.to_asset,
                        a.floor
                            .as_ref()
                            .is_none_or(|f| &route.delivers >= f.amount()),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "owner-change-rests-on-its-destination-check",
        spec: &["REQ-ORD-24"],
        quantifier: Quantifier::Exists,
        refusal: "an order waiting on a new owner rests on a destination check of this log that found one",
        fields: &[],
        cites: |e, _| match &e.what {
            What::OrderRefusal {
                order: Some(_),
                class: OrderRefusalClass::DestinationOwnerChanged,
                ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, verified(t).map(|t| &t.what)) {
            (
                What::OrderRefusal {
                    order: Some(order), ..
                },
                Some(What::DestinationCheck {
                    order: o, outcome, ..
                }),
            ) => judge(
                o == order && *outcome == DestinationOutcome::OwnerChanged,
                &[],
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "move-rests-on-its-orders-acceptance",
        spec: &["REQ-ORD-34"],
        quantifier: Quantifier::Exists,
        refusal: "a value-moving effect rests on an acceptance of its order in this log",
        fields: &[(
            "authorisation",
            "a value-moving effect's acceptance names its authorisation",
        )],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                Some((
                    Action::MoveValue {
                        order,
                        authorisation,
                        ..
                    },
                    _,
                )),
                Some(t),
            ) = (effect_action(e), verified(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Acceptance {
                    order: o,
                    authorisation: a,
                    ..
                } => judge(o == order, &[a == authorisation]),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "automatic-deposit-rests-on-its-arrival",
        spec: &["REQ-ORD-15", "REQ-ORD-32"],
        quantifier: Quantifier::Exists,
        refusal: "an acceptance on an automatic deposit rests on an arrival of its order in this log",
        fields: &[(
            "reserve",
            "an automatic deposit's arrival is at the acceptance's origin reserve",
        )],
        cites: |e, _| match &e.what {
            What::Acceptance {
                basis: AcceptanceBasis::AutomaticDeposit,
                ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, verified(t).map(|t| &t.what)) {
            (
                What::Acceptance {
                    order,
                    origin_reserve,
                    ..
                },
                Some(What::Arrival {
                    reserve,
                    order: Some(arrived_for),
                    ..
                }),
            ) => judge(arrived_for == order, &[reserve == origin_reserve]),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "order-holds-one-acceptance-basis",
        spec: &["REQ-ORD-32", "REQ-ORD-34"],
        quantifier: Quantifier::NoneOther,
        refusal: "an order is accepted on one basis: given, or an automatic deposit, never both",
        fields: &[],
        cites: |e, _| match &e.what {
            What::Acceptance { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, verified(t).map(|t| &t.what)) {
            (
                What::Acceptance { order, basis, .. },
                Some(What::Acceptance {
                    order: o, basis: b, ..
                }),
            ) => judge(o == order && b != basis, &[]),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "first-move-rests-on-its-parts-yes",
        spec: &["REQ-ORD-32"],
        quantifier: Quantifier::Exists,
        refusal: "a part's first move rests on an acceptance of its order naming that part's yes",
        fields: &[
            (
                "origin_reserve",
                "a part's first move is from the origin reserve its part's yes names",
            ),
            (
                "provider",
                "a part's first move, its withdrawal, is made by its origin reserve",
            ),
        ],
        cites: |e, h| match first_segment_part(e, h) {
            Some(_) => by_fields(0),
            None => none(),
        },
        answers: |e, _, t, h| {
            let (
                Some((
                    Action::MoveValue {
                        order,
                        origin_reserve,
                        provider,
                        ..
                    },
                    _,
                )),
                Some(part),
                Some(t),
            ) = (effect_action(e), first_segment_part(e, h), verified(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                // An automatic deposit's order moves on no yes (`REQ-ORD-32`).
                What::Acceptance {
                    order: o,
                    basis: AcceptanceBasis::AutomaticDeposit,
                    ..
                } if o == order => judge(true, &[true, provider == origin_reserve]),
                What::Acceptance {
                    order: o,
                    basis: AcceptanceBasis::Given,
                    yeses,
                    ..
                } if o == order => match yeses.iter().find(|y| y.part.as_str() == part) {
                    Some(yes) => judge(
                        true,
                        &[
                            &yes.origin_reserve == origin_reserve,
                            provider == origin_reserve,
                        ],
                    ),
                    None => Match::NotTarget,
                },
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "stuck-effect-moves-the-decided-resolution",
        spec: &["REQ-AUZ-4"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck order's effect carries out a permitted decide step of this log",
        fields: &[
            (
                "order",
                "a stuck order's effect is for its decide step's order",
            ),
            (
                "resolution",
                "a stuck order's effect moves exactly the resolution its decide step named",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::ResolveStuck { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                Some((
                    Action::ResolveStuck {
                        order,
                        carries_out,
                        resolution,
                    },
                    _,
                )),
                Some(t),
            ) = (effect_action(e), permitted(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::StuckDecide { request, decision } => judge(
                    request.request == carries_out.request && decision == &carries_out.decision,
                    &[&request.order == order, &request.resolution == resolution],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "hand-on-rests-on-the-linked-orders-acceptance",
        spec: &["REQ-ORD-13", "REQ-ORD-43"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck order's value is handed on only to an order accepted in this log",
        fields: &[(
            "requester",
            "a linked order keeps the requester of the order handed on",
        )],
        cites: |e, _| match effect_action(e) {
            Some((
                Action::ResolveStuck {
                    resolution: Resolution::HandOn { .. },
                    ..
                },
                _,
            )) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (
                Some((
                    Action::ResolveStuck {
                        order,
                        resolution: Resolution::HandOn { linked_order, .. },
                        ..
                    },
                    _,
                )),
                Some(t),
            ) = (effect_action(e), verified(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Acceptance {
                    order: o, parties, ..
                } => judge(
                    o == linked_order,
                    &[requester_of(order.as_str(), h) == Some(&parties.requester)],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "result-ends-an-effect",
        spec: &["REQ-ORD-1", "INV-7"],
        quantifier: Quantifier::Exists,
        refusal: "a result ends an effect entry of this log",
        fields: &[],
        cites: |e, _| match &e.what {
            What::Result { effect } => alloc::vec![by_receipt(effect, 0)],
            _ => none(),
        },
        answers: |_, _, t, _| {
            judge(
                verified(t).is_some_and(|t| matches!(t.what, What::Effect { .. })),
                &[],
            )
        },
    },
    Relation {
        name: "effect-has-one-result",
        spec: &["REQ-ORD-1", "INV-7"],
        quantifier: Quantifier::NoneOther,
        refusal: "an effect has one result",
        fields: &[],
        cites: |e, _| match &e.what {
            What::Result { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::Result { effect }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            judge(
                t != &e.entry
                    && matches!(&t.what, What::Result { effect: other } if other == effect),
                &[],
            )
        },
    },
    Relation {
        name: "stuck-move-carries-out-its-decide-step",
        spec: &["REQ-AUZ-4"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck move carries out a permitted decide step of this log",
        fields: &[
            ("order", "a stuck move is for its decide step's order"),
            (
                "mover",
                "a stuck order's value is moved by another accountable party than the one that decided",
            ),
        ],
        cites: |e, _| match &e.what {
            What::StuckMove { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::StuckMove { request: mv, .. }, Some(t)) = (&e.what, permitted(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::StuckDecide { request, decision } => judge(
                    request.request == mv.carries_out.request
                        && decision == &mv.carries_out.decision,
                    &[
                        request.order == mv.order,
                        request.decider.party != mv.mover.party,
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "stuck-decision-is-of-a-stuck-order",
        spec: &["REQ-ORD-28", "REQ-AUZ-4"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck order's decision is of an order this log has an input for",
        fields: &[(
            "observed",
            "a stuck order's decision follows the order's input declaring it stuck or leaving it no route",
        )],
        cites: |e, _| match &e.what {
            What::StuckDecide { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::StuckDecide { request, .. }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            match input(t) {
                Some(i) => judge(
                    i.subject.order.as_str() == request.order.as_str(),
                    &[matches!(
                        i.observed,
                        Observed::StuckDeclared { .. } | Observed::NoRoute { .. }
                    )],
                ),
                None => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "only-a-declared-stuck-part-is-resolved",
        spec: &["REQ-ORD-28", "REQ-AUZ-4", "REQ-ORD-43"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck resolution is of a part this log declared stuck or left no route",
        fields: &[
            (
                "stuck",
                "a stuck resolution is of a part that stands stuck: a no-route that left it waiting for the customer is the customer's",
            ),
            (
                "unresolved",
                "a stuck part is resolved once: its resolution's move is done",
            ),
        ],
        // Its decision, its move and its effect, each for the part the decision names; the part's latest
        // declaration or no-route is the target, and the part must stand stuck after it, unresolved (`standing`).
        cites: |e, _| match &e.what {
            What::StuckDecide { .. } | What::StuckMove { .. } => by_fields(0),
            What::Effect {
                action: Action::ResolveStuck { .. },
                ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (order, part) = match &e.what {
                What::StuckDecide { request, .. } => {
                    (request.order.as_str(), Some(request.part.as_str()))
                }
                What::StuckMove { request, .. } => (
                    request.order.as_str(),
                    decided_part(request.order.as_str(), &request.carries_out, h),
                ),
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order, carries_out, ..
                        },
                    ..
                } => (order.as_str(), decided_part(order.as_str(), carries_out, h)),
                _ => return Match::NotTarget,
            };
            let (Some(part), Some(i), Some(at)) =
                (part, verified(t).and_then(input), position(t, h))
            else {
                return Match::NotTarget;
            };
            let declares = |i: &crate::state_machine::RecordedInput| {
                i.subject.order.as_str() == order
                    && i.subject.part.as_ref().is_some_and(|p| p.as_str() == part)
                    && matches!(
                        i.observed,
                        Observed::StuckDeclared { .. } | Observed::NoRoute { .. }
                    )
            };
            if !declares(i)
                || h[at + 1..]
                    .iter()
                    .any(|(_, later)| verified(later).and_then(input).is_some_and(declares))
            {
                return Match::NotTarget;
            }
            // A part whose plan this log doesn't hold stands where its declaration says: stuck if declared so,
            // unplaced after a no-route the published rule can't read without it.
            let (stuck, resolved) = match standing(order, part, h) {
                Some(s) => (
                    s.part.state == crate::state_machine::OrderState::Stuck,
                    s.resolved,
                ),
                None => (matches!(i.observed, Observed::StuckDeclared { .. }), false),
            };
            judge(true, &[stuck, !resolved])
        },
    },
    Relation {
        name: "input-cites-its-decision-record",
        spec: &["REQ-AUD-4"],
        quantifier: Quantifier::Exists,
        refusal: "a recorded input's decision record is an entry of this log",
        fields: &[(
            "order",
            "a recorded input's decision record is of its order",
        )],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(
                Observed::RouteChosen { .. }
                | Observed::RouteRecalculated { .. }
                | Observed::DestinationChanged { .. }
                | Observed::NoRoute { .. },
            ) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (Some(i), Some(t)) = (input(e), verified(t)) else {
                return Match::NotTarget;
            };
            let cited = match &i.observed {
                Observed::RouteChosen {
                    decision_record, ..
                }
                | Observed::RouteRecalculated {
                    decision_record, ..
                }
                | Observed::DestinationChanged {
                    decision_record, ..
                }
                | Observed::NoRoute { decision_record } => decision_record.as_str(),
                _ => return Match::NotTarget,
            };
            match &t.what {
                What::DecisionRecord {
                    decision_record,
                    order,
                    ..
                } => judge(
                    decision_record.as_str() == cited,
                    &[order.as_str() == input_order(e)],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "chosen-route-rests-on-its-orders-acceptance",
        spec: &["REQ-ORD-34"],
        quantifier: Quantifier::Exists,
        refusal: "a chosen route rests on an acceptance of its order in this log",
        fields: &[
            (
                "parts",
                "a chosen route's acceptance names one yes per part, in the plan's order",
            ),
            (
                "origin_reserve",
                "a part's yes is its first segment's provider's: the reserve it withdraws from",
            ),
        ],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::RouteChosen { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let Some(Observed::RouteChosen { parts, .. }) = input(e).map(|i| &i.observed) else {
                return Match::NotTarget;
            };
            match verified(t).map(|t| &t.what) {
                Some(What::Acceptance {
                    order,
                    basis,
                    yeses,
                    ..
                }) => judge(
                    order.as_str() == input_order(e),
                    // A given acceptance names a yes for each part of the plan, in its order, and no other; an
                    // automatic deposit's names none (`REQ-ORD-32`).
                    &[
                        *basis == AcceptanceBasis::AutomaticDeposit
                            || (yeses.len() == parts.len()
                                && yeses
                                    .iter()
                                    .zip(parts)
                                    .all(|(y, p)| y.part.as_str() == p.part_id.as_str())),
                        yeses.iter().zip(parts).all(|(y, p)| {
                            p.segments
                                .first()
                                .is_some_and(|s| s.provider.as_str() == y.origin_reserve.as_str())
                        }),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "changed-destination-rests-on-its-acceptance",
        spec: &["REQ-ORD-43", "REQ-ORD-34"],
        quantifier: Quantifier::Exists,
        refusal: "a changed destination's acceptance is an entry of this log",
        fields: &[(
            "order",
            "a changed destination's acceptance is of its order",
        )],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::DestinationChanged { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let Some(Observed::DestinationChanged { acceptance, .. }) =
                input(e).map(|i| &i.observed)
            else {
                return Match::NotTarget;
            };
            match verified(t).map(|t| &t.what) {
                Some(What::Acceptance {
                    acceptance: a,
                    order,
                    ..
                }) => judge(
                    a.as_str() == acceptance.as_str(),
                    &[order.as_str() == input_order(e)],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "hand-on-input-rests-on-the-linked-orders-acceptance",
        spec: &["REQ-ORD-43", "REQ-ORD-13"],
        quantifier: Quantifier::Exists,
        refusal: "a hand-on's acceptance is an entry of this log",
        fields: &[
            ("order", "a hand-on's acceptance is of the linked order"),
            (
                "requester",
                "a linked order keeps the requester of the order handed on",
            ),
        ],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::HandedOn { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let Some(Observed::HandedOn {
                linked_order,
                acceptance,
            }) = input(e).map(|i| &i.observed)
            else {
                return Match::NotTarget;
            };
            match verified(t).map(|t| &t.what) {
                Some(What::Acceptance {
                    acceptance: a,
                    order,
                    parties,
                    ..
                }) => judge(
                    a.as_str() == acceptance.as_str(),
                    &[
                        order.as_str() == linked_order.as_str(),
                        requester_of(input_order(e), h) == Some(&parties.requester),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "handed-on-route-points-back",
        spec: &["REQ-ORD-43"],
        quantifier: Quantifier::Exists,
        refusal: "a route handed on follows the hand-on of the order it came from in this log",
        fields: &[(
            "linked_order",
            "the order a route was handed on from names it as its linked order",
        )],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::RouteChosen {
                handed_on_from: Some(_),
                ..
            }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let Some(Observed::RouteChosen {
                handed_on_from: Some(from),
                ..
            }) = input(e).map(|i| &i.observed)
            else {
                return Match::NotTarget;
            };
            match verified(t).and_then(input) {
                Some(earlier) => match &earlier.observed {
                    Observed::HandedOn { linked_order, .. } => judge(
                        earlier.subject.order.as_str() == from.as_str(),
                        &[linked_order.as_str() == input_order(e)],
                    ),
                    _ => Match::NotTarget,
                },
                None => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "settlement-rests-on-its-evidence",
        spec: &["REQ-ORD-33", "REQ-ORD-39"],
        quantifier: Quantifier::Exists,
        refusal: "a settled segment's evidence is an entry of this log",
        fields: &[
            ("order", "a settled segment's evidence is of its order"),
            ("segment", "a settled segment's evidence is of its segment"),
        ],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::SegmentSettled { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| evidence_answers(e, t),
    },
    Relation {
        name: "reversal-rests-on-its-evidence",
        spec: &["REQ-ORD-3"],
        quantifier: Quantifier::Exists,
        refusal: "a completed reversal's evidence is an entry of this log",
        fields: &[
            ("order", "a completed reversal's evidence is of its order"),
            (
                "segment",
                "a completed reversal's evidence is of its segment",
            ),
            (
                "whose",
                "a completed reversal's evidence is another party's than the failed provider's, or the origin reserve's labelled its word",
            ),
        ],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::ReversalCompleted { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| reversal_answers(e, t, h),
    },
    Relation {
        name: "decision-record-chains-to-its-orders-previous",
        spec: &["REQ-AUD-4"],
        quantifier: Quantifier::Exists,
        refusal: "a decision record's previous is a decision record of this log",
        fields: &[("order", "a decision record's previous is of its order")],
        cites: |e, _| match &e.what {
            What::DecisionRecord {
                previous: Some(previous),
                ..
            } => alloc::vec![by_receipt(previous, 0)],
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::DecisionRecord { order, .. }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::DecisionRecord { order: earlier, .. } => judge(true, &[earlier == order]),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "posted-route-cites-its-decision-record",
        spec: &["REQ-AUD-4"],
        quantifier: Quantifier::Exists,
        refusal: "a posted route's decision record is an entry of this log",
        fields: &[("order", "a posted route's decision record is of its order")],
        cites: |e, _| match &e.what {
            What::Decide { request, .. } if matches!(request.action, Action::PostRoute { .. }) => {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                What::Decide {
                    request:
                        DecisionRequest {
                            action:
                                Action::PostRoute {
                                    order,
                                    decision_record,
                                    ..
                                },
                            ..
                        },
                    ..
                },
                Some(t),
            ) = (&e.what, verified(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::DecisionRecord {
                    record, order: o, ..
                } => judge(record == decision_record, &[o == order]),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "sent-report-is-its-partys-statement",
        spec: &["REQ-ORD-9", "REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "a sent report is a stated entry of this log",
        fields: &[
            ("verified", "a sent report is a verified statement"),
            ("maker", "a sent report is its sending party's statement"),
            (
                "about",
                "a sent report is about its pair's order and segment",
            ),
        ],
        cites: |e, _| match &e.what {
            What::ReportPair { pair } => alloc::vec![by_receipt(&pair.sent.statement, 0)],
            _ => none(),
        },
        answers: |e, _, t, h| match &e.what {
            What::ReportPair { pair } => statement_by(
                t,
                &pair.sent.party,
                Some(&pair.order),
                Some(&pair.segment),
                h,
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "received-report-is-its-partys-statement",
        spec: &["REQ-ORD-9", "REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "a received report is a stated entry of this log",
        fields: &[
            ("verified", "a received report is a verified statement"),
            (
                "maker",
                "a received report is its receiving party's statement",
            ),
            (
                "about",
                "a received report is about its pair's order and segment",
            ),
        ],
        cites: |e, _| match &e.what {
            What::ReportPair { pair } => alloc::vec![by_receipt(&pair.received.statement, 0)],
            _ => none(),
        },
        answers: |e, _, t, h| match &e.what {
            What::ReportPair { pair } => statement_by(
                t,
                &pair.received.party,
                Some(&pair.order),
                Some(&pair.segment),
                h,
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "arrival-rests-on-the-reserves-report",
        spec: &["REQ-ORD-23"],
        quantifier: Quantifier::Exists,
        refusal: "an arrival's report is a stated entry of this log",
        fields: &[
            ("verified", "an arrival's report is a verified statement"),
            ("maker", "an arrival's report is its reserve's statement"),
            (
                "about",
                "an arrival's report is about the order it arrived for",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Arrival { report, .. } => alloc::vec![by_receipt(report, 0)],
            _ => none(),
        },
        answers: |e, _, t, h| match &e.what {
            What::Arrival { reserve, order, .. } => {
                statement_by(t, reserve, order.as_ref(), None, h)
            }
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "refiling-files-an-arrival-anew",
        spec: &["REQ-ORD-23"],
        quantifier: Quantifier::Exists,
        refusal: "a refiling names an arrival of this log",
        fields: &[
            (
                "order",
                "a refiling files the arrival against the order it arrived for",
            ),
            (
                "filer",
                "an arrival is filed anew by the operator or by the reserve it arrived at",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Refiling { arrival, .. } => alloc::vec![by_receipt(arrival, 0)],
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::Refiling { order, .. }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Arrival {
                    reserve, order: o, ..
                } => judge(
                    true,
                    &[
                        o.as_ref() == Some(order),
                        e.who.role == Role::Operator
                            || (e.who.role == Role::Provider && &e.who.party == reserve),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "evidence-rests-on-statements",
        spec: &["REQ-ORD-39", "REQ-ORD-38", "REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "graded evidence rests on stated entries of this log",
        fields: &[
            (
                "verified",
                "evidence graded as signed, or as its provider's word, rests on verified statements",
            ),
            (
                "provider",
                "evidence graded as its provider's word rests on its segment's provider's statement",
            ),
            (
                "bound",
                "an unverified statement under evidence names a transaction of its order and segment",
            ),
            (
                "about",
                "graded evidence rests on statements about its order and segment",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Evidence { rests_on, .. } => rests_on
                .iter()
                .enumerate()
                .map(|(i, r)| by_receipt(r, i))
                .collect(),
            _ => none(),
        },
        answers: |e, _, t, h| match (&e.what, stated(t)) {
            (
                What::Evidence {
                    order,
                    segment,
                    grade,
                    ..
                },
                Some(s),
            ) => {
                // Grades 2 and 3 claim a signature, and a bare statement (4) is its provider's word, which an
                // unverified statement is no one's (`REQ-AUD-2`, `REQ-ORD-38`): each rests on a statement the log
                // verified, a bare statement on the segment's provider's. A public record (1) rests on the network,
                // so its statement may be unverified, but like any it is bound to the order and segment through a
                // transaction it names.
                let verified = matches!(s.attribution, Attribution::Verified { .. });
                let word = *grade == EvidenceGrade::Statement;
                let provider = segment_provider(order, segment, h);
                judge(
                    true,
                    &[
                        verified || *grade == EvidenceGrade::PublicRecord,
                        !word || provider == Some(maker_provider(s)),
                        verified
                            || s.about
                                .as_ref()
                                .and_then(|a| a.transaction.as_ref())
                                .and_then(|t| binding_of(t, h))
                                == Some((order, Some(segment))),
                        about_agrees(s, Some(order), Some(segment), h),
                    ],
                )
            }
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "afterwards-rests-on-a-statement",
        spec: &["REQ-ORD-25", "REQ-ORD-31"],
        quantifier: Quantifier::Exists,
        refusal: "what came after an order ended rests on a stated entry of this log",
        fields: &[(
            "about",
            "what came after an order ended rests on a statement about that order",
        )],
        cites: |e, _| match &e.what {
            What::Afterwards { statement, .. } => alloc::vec![by_receipt(statement, 0)],
            _ => none(),
        },
        answers: |e, _, t, h| match &e.what {
            What::Afterwards { order, .. } => statement_about(t, Some(order), None, h),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "reliance-names-its-judges-statement",
        spec: &["REQ-CPL-12"],
        quantifier: Quantifier::Exists,
        refusal: "a reliance names a stated judgement of this log",
        fields: &[
            ("verified", "a reliance's judgement is a verified statement"),
            ("maker", "a reliance's judgement is its judge's statement"),
            (
                "about",
                "a reliance's judgement is about nothing but the order relying on it",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Reliance { judgement, .. } => alloc::vec![by_receipt(judgement, 0)],
            _ => none(),
        },
        answers: |e, _, t, h| match &e.what {
            What::Reliance { judge, order, .. } => statement_by(t, judge, Some(order), None, h),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "measure-cites-entries-of-the-log",
        spec: &["REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "a measure record cites entries of this log",
        fields: &[],
        cites: |e, _| match &e.what {
            What::Measure { record } => record
                .cites
                .iter()
                .enumerate()
                .map(|(i, r)| by_receipt(r, i))
                .collect(),
            _ => none(),
        },
        answers: |_, _, _, _| Match::Holds,
    },
    Relation {
        name: "raise-cites-entries-of-the-log",
        spec: &["REQ-ORD-9", "REQ-AUD-15"],
        quantifier: Quantifier::Exists,
        refusal: "a raise cites entries of this log",
        fields: &[],
        cites: |e, _| match &e.what {
            What::Raise { raise } => raise
                .cites
                .iter()
                .enumerate()
                .map(|(i, r)| by_receipt(r, i))
                .collect(),
            _ => none(),
        },
        answers: |_, _, _, _| Match::Holds,
    },
    Relation {
        name: "erasure-is-carried-out-on-its-decision",
        spec: &["REQ-AUD-13", "REQ-AUZ-1"],
        quantifier: Quantifier::Exists,
        refusal: "an erasure follows a result of an erase effect of this log for its commitment",
        fields: &[
            (
                "done",
                "an erasure follows its erase effect's done result: the store erases only then",
            ),
            (
                "outcome",
                "an erasure after its erase effect's done result is done: the bytes are gone",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Erasure { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (What::Erasure { evidence }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            let What::Result { effect } = &t.what else {
                return Match::NotTarget;
            };
            let done = t.outcome == (Outcome::Done {});
            judge(
                erased_by(effect, h) == Some(evidence),
                &[done, !done || e.outcome == (Outcome::Done {})],
            )
        },
    },
    Relation {
        name: "erasure-waits-while-anything-rests-on-the-statement",
        spec: &["REQ-AUD-13"],
        quantifier: Quantifier::NoneOther,
        refusal: "no statement is erased while an entry of this log rests on it",
        fields: &[],
        // Refused while cited (the owner's default): its decision, its effect and its record, so the log holds no
        // done erasure the store must refuse, and no erase effect a citation made since its decision would block.
        cites: |e, _| match erasing(e) {
            Some(_) => by_fields(0),
            None => none(),
        },
        answers: |e, _, t, h| match (erasing(e), verified(t)) {
            (Some(commitment), Some(t)) => judge(
                rests_on_statements(t) && cites_statement(t, commitment, h),
                &[],
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "nothing-cites-an-erased-statement",
        spec: &["REQ-AUD-13"],
        quantifier: Quantifier::NoneOther,
        refusal: "no entry cites a statement whose erasure has begun or is done",
        fields: &[],
        cites: |e, _| {
            if rests_on_statements(e) {
                by_fields(0)
            } else {
                none()
            }
        },
        // The store erases on the erase effect's done result, so the statement counts as erased from there, before
        // the erasure entry records it.
        answers: |e, _, t, h| match verified(t).map(|t| (&t.what, &t.outcome)) {
            Some((What::Erasure { evidence }, Outcome::Done {})) => {
                judge(cites_statement(e, evidence, h), &[])
            }
            Some((What::Result { effect }, Outcome::Done {})) => match erased_by(effect, h) {
                Some(commitment) => judge(cites_statement(e, commitment, h), &[]),
                None => Match::NotTarget,
            },
            // Erasure starts with the erase effect: nothing cites the statement between it and its result, unless
            // that result failed and the bytes stay.
            Some((
                What::Effect {
                    action: Action::EraseStatement { commitment, .. },
                    ..
                },
                _,
            )) => judge(
                !verified(t).is_some_and(|t| result_failed(t, h))
                    && cites_statement(e, commitment, h),
                &[],
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "erasure-names-a-kept-statement",
        spec: &["REQ-AUD-13"],
        quantifier: Quantifier::Exists,
        refusal: "an erasure names a commitment a stated entry of this log holds",
        fields: &[],
        cites: |e, _| match &e.what {
            What::Erasure { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, stated(t)) {
            (What::Erasure { evidence }, Some(s)) => judge(&s.commitment == evidence, &[]),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "acceptance-is-of-the-quote-shown",
        spec: &["REQ-ORD-34", "REQ-QTE-7"],
        quantifier: Quantifier::Exists,
        refusal: "an acceptance's quote was shown, as this log records",
        fields: &[
            ("order", "an acceptance's quote was shown for its order"),
            (
                "shown_as",
                "an acceptance's quote was shown as a quote, not a price",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Acceptance {
                basis: AcceptanceBasis::Given,
                ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::Acceptance { order, quote, .. }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Shown {
                    order: o,
                    shown,
                    shown_as,
                } => judge(shown == quote, &[o == order, *shown_as != ShownAs::Price]),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "shortlisted-authority-cites-its-shortlist",
        spec: &["REQ-CPL-2", "REQ-CPL-6", "REQ-CPL-5"],
        quantifier: Quantifier::Exists,
        refusal: "a shortlisted route's authority names a decision record of this log",
        fields: &[
            (
                "order",
                "a shortlisted route's authority is a shortlist of its order",
            ),
            (
                "round",
                "a shortlisted route's authority is a shortlist round",
            ),
            (
                "providers",
                "a shortlisted route's authority names only providers its shortlist shortlisted",
            ),
            (
                "verifiers",
                "a shortlisted route's authority names only verifiers its shortlist's tuples need",
            ),
        ],
        cites: |e, _| match &e.what {
            What::AskAboutPerson { question }
                if matches!(question.authority, Authority::ShortlistedRoute { .. }) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::AskAboutPerson { question }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            let Authority::ShortlistedRoute {
                shortlist,
                providers,
                verifiers,
            } = &question.authority
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::DecisionRecord {
                    record,
                    order,
                    round,
                    shortlisted,
                    verifiers: needed,
                    ..
                } => judge(
                    record == shortlist,
                    &[
                        order == &question.order,
                        *round == RoutingRound::Shortlist,
                        shortlisted
                            .as_ref()
                            .is_some_and(|s| providers.iter().all(|p| s.contains(p))),
                        verifiers.iter().all(|v| needed.contains(v)),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "provider-asked-before-was-asked",
        spec: &["REQ-CPL-5"],
        quantifier: Quantifier::Exists,
        refusal: "each provider a question says was asked before was asked in a permitted question of this log",
        fields: &[(
            "order",
            "a provider asked before was asked about the same order",
        )],
        cites: |e, _| match &e.what {
            What::AskAboutPerson { question } => (0..question.asked_before.len())
                .flat_map(by_fields)
                .collect(),
            _ => none(),
        },
        answers: |e, c, t, _| {
            let (What::AskAboutPerson { question }, Some(t)) = (&e.what, permitted(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::AskAboutPerson { question: earlier } => judge(
                    earlier.ask.contains(&question.asked_before[c.index]),
                    &[earlier.order == question.order],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "asked-before-is-complete",
        spec: &["REQ-CPL-5"],
        quantifier: Quantifier::NoneOther,
        refusal: "a question names every provider already asked about its order",
        fields: &[],
        cites: |e, _| match &e.what {
            What::AskAboutPerson { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::AskAboutPerson { question }, Some(t)) = (&e.what, permitted(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::AskAboutPerson { question: earlier } => judge(
                    t != &e.entry
                        && earlier.order == question.order
                        && earlier
                            .ask
                            .iter()
                            .any(|p| !question.asked_before.contains(p)),
                    &[],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "revocation-revokes-a-grant",
        spec: &["REQ-AUZ-7"],
        quantifier: Quantifier::Exists,
        refusal: "a revocation names a permission granted in this log",
        fields: &[],
        // Only a permitted revocation revokes; a refused one of a permission never granted is recorded as refused.
        cites: |e, _| match &e.what {
            What::RevokePermission { .. } if e.outcome == (Outcome::Permitted {}) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, permitted(t).map(|t| &t.what)) {
            (
                What::RevokePermission { change, .. },
                Some(What::GrantPermission { change: g, .. }),
            ) => judge(g.permission == change.permission, &[]),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "permitted-read-rests-on-its-grant",
        spec: &["REQ-AUZ-7", "REQ-AUD-12"],
        quantifier: Quantifier::Exists,
        refusal: "a permitted read's permission was granted in this log",
        fields: &[
            (
                "grantee",
                "a permitted read's permission was granted to its reader's party",
            ),
            (
                "scope",
                "a permitted read's permission covers what it reads: the trail covers every order and segment, an order its segments",
            ),
        ],
        cites: |e, _| match &e.what {
            What::CheckRead { check }
                if check.permission.is_some() && e.outcome == (Outcome::Permitted {}) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, permitted(t).map(|t| &t.what)) {
            (What::CheckRead { check }, Some(What::GrantPermission { change, .. })) => judge(
                Some(&change.permission) == check.permission.as_ref(),
                &[
                    change.grantee.party == check.reader.party,
                    covers(&change.scope, &check.reads),
                ],
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "permitted-read-is-not-revoked",
        spec: &["REQ-AUZ-7"],
        quantifier: Quantifier::NoneOther,
        refusal: "a permitted read's permission is not revoked in this log",
        fields: &[],
        cites: |e, _| match &e.what {
            What::CheckRead { check }
                if check.permission.is_some() && e.outcome == (Outcome::Permitted {}) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, permitted(t).map(|t| &t.what)) {
            (What::CheckRead { check }, Some(What::RevokePermission { change, .. })) => {
                judge(Some(&change.permission) == check.permission.as_ref(), &[])
            }
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "own-evidence-read-is-of-the-readers-statement",
        spec: &["REQ-AUD-5", "REQ-AUZ-7"],
        quantifier: Quantifier::Exists,
        refusal: "a permitted evidence read without a permission is of a statement this log holds",
        fields: &[
            (
                "verified",
                "a permitted evidence read without a permission is of a statement the log verified",
            ),
            (
                "maker",
                "a permitted evidence read without a permission is of a statement its reader made, or the operator's own for an operator of its party",
            ),
        ],
        // A party reads its own evidence without a permission (`REQ-AUD-5`); any other's rests on a permission,
        // whose scope `covers` the read.
        cites: |e, _| match &e.what {
            What::CheckRead {
                check:
                    ReadCheck {
                        reads: Reads::Evidence { .. },
                        permission: None,
                        ..
                    },
            } if e.outcome == (Outcome::Permitted {}) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                What::CheckRead {
                    check:
                        ReadCheck {
                            reader,
                            reads: Reads::Evidence { commitment },
                            ..
                        },
                },
                Some(s),
            ) = (&e.what, stated(t))
            else {
                return Match::NotTarget;
            };
            // The operator's own statements are made as the operator; a person logged in as the operator reads
            // them as its party, which policy's `ActsFor` binds the login to before any read is permitted.
            let operators_own = reader.role == Role::Operator
                && maker_provider(s) == &e.operator
                && s.attribution.maker().party == reader.party;
            judge(
                &s.commitment == commitment,
                &[
                    matches!(s.attribution, Attribution::Verified { .. }),
                    maker_provider(s) == &reader.acts || operators_own,
                ],
            )
        },
    },
    Relation {
        name: "customer-read-is-of-its-own-order",
        spec: &["REQ-AUZ-7"],
        quantifier: Quantifier::Exists,
        refusal: "a customer's permitted read is of an order accepted in this log",
        fields: &[
            (
                "party",
                "a customer's permitted read is of an order whose requester, beneficiary or payer it is",
            ),
            (
                "segment",
                "a customer's permitted read of a segment is of a segment its order's plan lists",
            ),
        ],
        cites: |e, _| match (&e.what, read_of_order(e)) {
            (What::CheckRead { check }, Some(_)) if check.reader.role == Role::Customer => {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (What::CheckRead { check }, Some(read)) = (&e.what, read_of_order(e)) else {
                return Match::NotTarget;
            };
            // A segment read names its order; the segment must be that order's, by the plans this log holds.
            let of_its_order = match &check.reads {
                Reads::Segment { order, segment, .. } => {
                    planned_end(order.as_str(), segment.as_str(), h).is_some()
                }
                _ => true,
            };
            match verified(t).map(|t| &t.what) {
                Some(What::Acceptance { order, parties, .. }) => {
                    let party = &check.reader.party;
                    judge(
                        order == read,
                        &[
                            [&parties.requester, &parties.beneficiary, &parties.payer]
                                .contains(&party),
                            of_its_order,
                        ],
                    )
                }
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "provider-read-is-of-its-own-segment",
        spec: &["REQ-AUZ-7"],
        quantifier: Quantifier::Exists,
        refusal: "a provider's permitted read is of a segment this log moved value on",
        fields: &[(
            "provider",
            "a provider's permitted read is of a segment it is the provider of",
        )],
        cites: |e, _| match &e.what {
            What::CheckRead {
                check:
                    ReadCheck {
                        reader,
                        reads: Reads::Segment { .. },
                        ..
                    },
            } if reader.role == Role::Provider && e.outcome == (Outcome::Permitted {}) => {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, _| {
            let What::CheckRead {
                check:
                    ReadCheck {
                        reader,
                        reads:
                            Reads::Segment {
                                order,
                                segment,
                                provider: read_provider,
                            },
                        ..
                    },
            } = &e.what
            else {
                return Match::NotTarget;
            };
            match verified(t).and_then(effect_action) {
                Some((
                    Action::MoveValue {
                        order: o,
                        segment: s,
                        provider,
                        ..
                    },
                    _,
                )) => judge(
                    o == order && s == segment,
                    &[provider == &reader.party && read_provider == &reader.party],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "segment-belongs-to-one-order",
        spec: &["REQ-ORD-38"],
        quantifier: Quantifier::NoneOther,
        refusal: "a segment belongs to one order: no value moved on it under another",
        fields: &[],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (Some((Action::MoveValue { order, segment, .. }, _)), Some(t)) =
                (effect_action(e), verified(t))
            else {
                return Match::NotTarget;
            };
            match effect_action(t) {
                Some((
                    Action::MoveValue {
                        order: o,
                        segment: s,
                        ..
                    },
                    _,
                )) => judge(s == segment && o != order, &[]),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "route-starts-on-the-withdrawal-proof",
        spec: &["REQ-ORD-40"],
        quantifier: Quantifier::Exists,
        refusal: "a route's later segment moves only once its part's first segment, the withdrawal, has settled in this log",
        fields: &[],
        cites: |e, h| match first_segment_before(e, h) {
            Some(_) => by_fields(0),
            None => none(),
        },
        answers: |e, _, t, h| {
            let (Some(first), Some((Action::MoveValue { order, .. }, _))) =
                (first_segment_before(e, h), effect_action(e))
            else {
                return Match::NotTarget;
            };
            match verified(t).and_then(input) {
                Some(i) => judge(
                    matches!(i.observed, Observed::SegmentSettled { .. })
                        && i.subject.order.as_str() == order.as_str()
                        && i.subject.segment.as_ref().map(|s| s.as_str()) == Some(first),
                    &[],
                ),
                None => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "fee-rests-on-a-settled-segment",
        spec: &["REQ-AUD-11"],
        quantifier: Quantifier::Exists,
        refusal: "a fee is recorded only for a segment settled in this log",
        fields: &[("order", "a fee's settled segment is of its order")],
        cites: |e, _| match &e.what {
            What::Fee { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let What::Fee { order, segment, .. } = &e.what else {
                return Match::NotTarget;
            };
            match verified(t).and_then(input) {
                Some(i) => judge(
                    matches!(i.observed, Observed::SegmentSettled { .. })
                        && i.subject.segment.as_ref().map(|s| s.as_str()) == Some(segment.as_str()),
                    &[i.subject.order.as_str() == order.as_str()],
                ),
                None => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "acceptance-rests-on-the-reserves-yes",
        spec: &["REQ-ORD-32"],
        quantifier: Quantifier::Exists,
        refusal: "an acceptance given rests on its reserve's yes, a stated entry of this log",
        fields: &[
            (
                "verified",
                "an acceptance's reserve's yes is a verified statement",
            ),
            (
                "maker",
                "an acceptance's reserve's yes is its origin reserve's statement",
            ),
            ("order", "an acceptance's reserve's yes is about its order"),
            (
                "transaction",
                "an acceptance's yes names the transaction it was asked under, which the reserve signed",
            ),
        ],
        // Each part's yes is cited on its own, and held to that part's origin reserve.
        cites: |e, _| match &e.what {
            What::Acceptance { yeses, .. } => yeses
                .iter()
                .enumerate()
                .map(|(i, y)| by_receipt(&y.reserve_yes, i))
                .collect(),
            _ => none(),
        },
        answers: |e, c, t, _| {
            let (What::Acceptance { order, yeses, .. }, Some(statement)) = (&e.what, stated(t))
            else {
                return Match::NotTarget;
            };
            let Some(yes) = yeses.get(c.index) else {
                return Match::NotTarget;
            };
            judge(
                true,
                &[
                    matches!(statement.attribution, Attribution::Verified { .. }),
                    maker_provider(statement) == &yes.origin_reserve,
                    statement.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order),
                    statement
                        .about
                        .as_ref()
                        .is_some_and(|a| a.transaction.is_some()),
                ],
            )
        },
    },
    Relation {
        name: "acceptance-is-asked-in-the-users-session",
        spec: &["REQ-ORD-32"],
        quantifier: Quantifier::Exists,
        refusal: "an acceptance given is asked in a session this log opened",
        fields: &[
            ("holder", "an acceptance's session is its requester's"),
            (
                "expires_at",
                "an acceptance's session is still open when it is accepted",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Acceptance {
                session: Some(_), ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                What::Acceptance {
                    session: Some(session),
                    parties,
                    ..
                },
                Some(t),
            ) = (&e.what, permitted(t))
            else {
                return Match::NotTarget;
            };
            match &t.what {
                What::OpenSession {
                    request,
                    opened: Some(opened),
                } => judge(
                    &opened.session == session,
                    &[
                        request.holder == parties.requester,
                        opened.expires_at.unix_nanos() >= e.when.at.unix_nanos(),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "move-segment-is-in-its-orders-plan",
        spec: &["REQ-ORD-38", "REQ-ORD-40"],
        quantifier: Quantifier::Exists,
        refusal: "a value-moving effect's segment is in a route of its order this log recorded",
        fields: &[
            (
                "order",
                "a value-moving effect's segment is in a route of its own order",
            ),
            (
                "provider",
                "a value-moving effect is made by the provider its plan names for the segment",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (
                Some((
                    Action::MoveValue {
                        order,
                        segment,
                        provider,
                        ..
                    },
                    _,
                )),
                Some(i),
            ) = (effect_action(e), verified(t).and_then(input))
            else {
                return Match::NotTarget;
            };
            let planned = |plan: &'_ [crate::state_machine::SegmentPlan]| {
                plan.iter()
                    .find(|s| s.segment_id.as_str() == segment.as_str())
                    .map(|s| s.provider.as_str() == provider.as_str())
            };
            let listed = match &i.observed {
                Observed::RouteChosen { parts, .. } => {
                    parts.iter().find_map(|p| planned(&p.segments))
                }
                Observed::RouteRecalculated { segments, .. }
                | Observed::DestinationChanged { segments, .. } => planned(segments),
                _ => return Match::NotTarget,
            };
            judge(
                listed.is_some(),
                &[
                    i.subject.order.as_str() == order.as_str(),
                    listed == Some(true),
                ],
            )
        },
    },
    Relation {
        name: "recalculation-starts-from-evidenced-value",
        spec: &["REQ-ORD-2", "INV-4"],
        quantifier: Quantifier::Exists,
        refusal: "a recalculation starts from its part's latest segment settled in this log, where its value is evidenced",
        fields: &[
            (
                "part",
                "a recalculation starts from a settled segment of its own part",
            ),
            (
                "node",
                "a recalculation starts where its settled segment ended: that segment's end is the new route's start",
            ),
        ],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::RouteRecalculated { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (Some(recalculated), Some(i)) = (input(e), verified(t).and_then(input)) else {
                return Match::NotTarget;
            };
            let Observed::RouteRecalculated { segments, .. } = &recalculated.observed else {
                return Match::NotTarget;
            };
            let order = recalculated.subject.order.as_str();
            let ended = i
                .subject
                .segment
                .as_ref()
                .and_then(|s| planned_end(order, s.as_str(), h));
            // The part's latest settled segment: its end is where the value is evidenced now. An earlier one's end
            // the value has left.
            let latest = position(t, h).is_some_and(|at| {
                !h[at + 1..].iter().any(|(_, later)| {
                    verified(later).and_then(input).is_some_and(|l| {
                        matches!(l.observed, Observed::SegmentSettled { .. })
                            && l.subject.order == i.subject.order
                            && l.subject.part == i.subject.part
                    })
                })
            });
            judge(
                matches!(i.observed, Observed::SegmentSettled { .. })
                    && i.subject.order.as_str() == order
                    && latest,
                &[
                    i.subject.part.as_ref().map(|p| p.as_str())
                        == recalculated.subject.part.as_ref().map(|p| p.as_str()),
                    ended.is_some() && ended == segments.first().map(|s| &s.from),
                ],
            )
        },
    },
    Relation {
        name: "recalculation-waits-for-moves-in-flight",
        spec: &["REQ-ORD-2", "INV-4"],
        quantifier: Quantifier::NoneOther,
        refusal: "a part's route restarts only once no move of it is in flight",
        fields: &[],
        // A move in flight may still deliver: restarting the route around it, by a recalculation or a change of
        // destination, moves the same value twice. Only the value's own evidence releases a move: its segment
        // settled, its reversal completed, or a deposit that failed, which leaves the value where it was; or a
        // failed result, so it never moved. A done result alone releases nothing (`move_in_flight`).
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::RouteRecalculated { .. } | Observed::DestinationChanged { .. }) => {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, h| match input(e) {
            Some(i) => move_in_flight(i, t, h),
            None => Match::NotTarget,
        },
    },
    Relation {
        name: "floor-wait-waits-for-moves-in-flight",
        spec: &["REQ-ORD-27", "INV-4"],
        quantifier: Quantifier::NoneOther,
        refusal: "a part waits at its floor only once no move of it is in flight",
        fields: &[],
        // A floor is judged on value that sits somewhere: a move in flight may still deliver.
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::WaitingAtFloor {}) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| match input(e) {
            Some(i) => move_in_flight(i, t, h),
            None => Match::NotTarget,
        },
    },
    Relation {
        name: "retired-segment-takes-no-move",
        spec: &["INV-4", "REQ-ORD-2"],
        quantifier: Quantifier::NoneOther,
        refusal: "a segment a recalculation or a change of destination retired takes no further move",
        fields: &[],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (Some((Action::MoveValue { order, segment, .. }, _)), Some(i)) =
                (effect_action(e), verified(t).and_then(input))
            else {
                return Match::NotTarget;
            };
            let (Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. }) = &i.observed
            else {
                return Match::NotTarget;
            };
            let (order, segment) = (order.as_str(), segment.as_str());
            let Some(at) = h.iter().position(|(_, e)| e == t) else {
                return Match::NotTarget;
            };
            let before = &h[..at];
            judge(
                i.subject.order.as_str() == order
                    && part_of_segment(order, segment, before)
                        .is_some_and(|p| i.subject.part.as_ref().is_some_and(|q| q.as_str() == p))
                    && !segments.iter().any(|s| s.segment_id.as_str() == segment),
                &[],
            )
        },
    },
    Relation {
        name: "segment-is-moved-once",
        spec: &["INV-4", "INV-8"],
        quantifier: Quantifier::NoneOther,
        refusal: "a segment is moved once: no other move of it stands in this log",
        fields: &[],
        // Another move on the segment stands unless its result failed, so it never moved; the same effect sent
        // again is a retry, not another move.
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (Some((Action::MoveValue { order, segment, .. }, _)), Some(t)) =
                (effect_action(e), verified(t))
            else {
                return Match::NotTarget;
            };
            match effect_action(t) {
                Some((
                    Action::MoveValue {
                        order: o,
                        segment: s,
                        ..
                    },
                    _,
                )) => judge(
                    o == order && s == segment && t.what != e.what && !result_failed(t, h),
                    &[],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "ended-part-takes-no-more",
        spec: &["REQ-STM-6", "REQ-ORD-43", "INV-4"],
        quantifier: Quantifier::NoneOther,
        refusal: "a part that has ended takes no new route, wait, move or no-route",
        fields: &[],
        // Handed on, cancelled, ended on request, complete, failed, or resolved while stuck: the part's value is
        // gone or delivered, and nothing routes or moves it again (`standing`).
        cites: |e, _| match part_cited(e) {
            Some(_) => by_fields(0),
            None => none(),
        },
        answers: |e, _, t, h| {
            let Some((order, part)) = part_cited(e).and_then(|c| c.resolve(h)) else {
                return Match::NotTarget;
            };
            standing_at(order, part, t, h, |s| s.ended())
        },
    },
    Relation {
        name: "held-part-takes-no-move",
        spec: &["REQ-ORD-27", "REQ-ORD-43", "REQ-ORD-28"],
        quantifier: Quantifier::NoneOther,
        refusal: "no move starts on a part waiting for the customer or stuck",
        fields: &[],
        // The customer's choice resolves a waiting part (a change of destination with its acceptance, a
        // recalculation within a new authorisation, a hand-on), and the operator's, under policy, a stuck one; only
        // then does the part move again (`standing`).
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let Some((order, part)) = part_cited(e).and_then(|c| c.resolve(h)) else {
                return Match::NotTarget;
            };
            standing_at(order, part, t, h, |s| s.held() && !s.resolved)
        },
    },
    Relation {
        name: "waiting-at-floor-rests-on-the-customers-floor",
        spec: &["REQ-ORD-27", "REQ-AUZ-3"],
        quantifier: Quantifier::Exists,
        refusal: "a part waits at its floor only on an authorisation of this log that sets one",
        fields: &[
            (
                "order",
                "a part waits at its floor on its own order's authorisation",
            ),
            (
                "floor",
                "a part waits at its floor only where the customer set one",
            ),
        ],
        // Core judges the floor (REQ-ORD-27); the floor it judges by is the customer's, in its authorisation.
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::WaitingAtFloor {}) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (Some(i), Some(t)) = (input(e), permitted(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::Authorise {
                    authorisation: a, ..
                } => judge(
                    true,
                    &[
                        a.order.as_str() == i.subject.order.as_str(),
                        a.floor.is_some(),
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "order-is-opened-once",
        spec: &["REQ-ORD-19"],
        quantifier: Quantifier::NoneOther,
        refusal: "an order is opened once: no other chosen route of it is in this log",
        fields: &[],
        cites: |e, _| match input(e).map(|i| &i.observed) {
            Some(Observed::RouteChosen { .. }) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match verified(t) {
            Some(earlier) => match input(earlier) {
                Some(i) => judge(
                    earlier != &e.entry
                        && matches!(i.observed, Observed::RouteChosen { .. })
                        && i.subject.order.as_str() == input_order(e),
                    &[],
                ),
                None => Match::NotTarget,
            },
            None => Match::NotTarget,
        },
    },
];

/// How an evidence entry answers a settled segment's or a completed reversal's citation.
fn evidence_answers(entry: &VerifiedEntry, target: &Entry) -> Match {
    let Some(i) = input(entry) else {
        return Match::NotTarget;
    };
    let cited = match &i.observed {
        Observed::SegmentSettled { evidence } | Observed::ReversalCompleted { evidence } => {
            evidence.as_str()
        }
        _ => return Match::NotTarget,
    };
    match verified(target).map(|t| &t.what) {
        Some(What::Evidence {
            evidence,
            order,
            segment,
            ..
        }) => judge(
            evidence.as_str() == cited,
            &[
                order.as_str() == i.subject.order.as_str(),
                i.subject.segment.as_ref().map(|s| s.as_str()) == Some(segment.as_str()),
            ],
        ),
        _ => Match::NotTarget,
    }
}

/// The order a permitted read of an order or one of its segments is of.
fn read_of_order(entry: &VerifiedEntry) -> Option<&Id> {
    match &entry.what {
        What::CheckRead { check } if entry.outcome == (Outcome::Permitted {}) => match &check.reads
        {
            Reads::Order { order } | Reads::OrderState { order } | Reads::Segment { order, .. } => {
                Some(order)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Where `entry` moves value on the first segment of a part's route, by the `route_chosen` plan `history` holds: the
/// part's id.
fn first_segment_part<'a>(entry: &VerifiedEntry, history: &'a History) -> Option<&'a str> {
    let Some((Action::MoveValue { order, segment, .. }, _)) = effect_action(entry) else {
        return None;
    };
    history.iter().find_map(|(_, e)| {
        let i = verified(e).and_then(input)?;
        let Observed::RouteChosen { parts, .. } = &i.observed else {
            return None;
        };
        if i.subject.order.as_str() != order.as_str() {
            return None;
        }
        parts.iter().find_map(|part| {
            let first = part.segments.first()?.segment_id.as_str();
            (first == segment.as_str()).then_some(part.part_id.as_str())
        })
    })
}

/// Where `entry` moves value on a segment of a part's route, by the `route_chosen` plan `history` holds, and that
/// segment isn't the part's first: the first segment's id.
fn first_segment_before<'a>(entry: &VerifiedEntry, history: &'a History) -> Option<&'a str> {
    let Some((Action::MoveValue { order, segment, .. }, _)) = effect_action(entry) else {
        return None;
    };
    history.iter().find_map(|(_, e)| {
        let i = verified(e).and_then(input)?;
        let Observed::RouteChosen { parts, .. } = &i.observed else {
            return None;
        };
        if i.subject.order.as_str() != order.as_str() {
            return None;
        }
        parts.iter().find_map(|part| {
            let first = part.segments.first()?.segment_id.as_str();
            let later = part
                .segments
                .iter()
                .skip(1)
                .any(|s| s.segment_id.as_str() == segment.as_str());
            later.then_some(first)
        })
    })
}

/// The part `segment` of `order` belongs to, by the plans `history` holds: a `route_chosen` part listing it, or the
/// part a `route_recalculated` or `destination_changed` gave it to.
fn part_of_segment<'a>(order: &str, segment: &str, history: &'a History) -> Option<&'a str> {
    history.iter().find_map(|(_, e)| {
        let i = verified(e).and_then(input)?;
        if i.subject.order.as_str() != order {
            return None;
        }
        match &i.observed {
            Observed::RouteChosen { parts, .. } => parts
                .iter()
                .find(|p| p.segments.iter().any(|s| s.segment_id.as_str() == segment))
                .map(|p| p.part_id.as_str()),
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. } => segments
                .iter()
                .any(|s| s.segment_id.as_str() == segment)
                .then(|| i.subject.part.as_ref().map(|p| p.as_str()))
                .flatten(),
            _ => None,
        }
    })
}

/// The index of `entry` in `history`: the candidates a row answers are `history`'s own entries.
fn position(entry: &Entry, history: &History) -> Option<usize> {
    history.iter().position(|(_, e)| core::ptr::eq(e, entry))
}

/// What a part row cites a part by: an input about the part, or a move on one of its segments.
enum PartCited<'a> {
    Input(&'a str, &'a str),
    Move(&'a str, &'a str),
}

impl<'a> PartCited<'a> {
    /// The order and part, a move's by the plans `history` holds.
    fn resolve(self, history: &'a History) -> Option<(&'a str, &'a str)> {
        match self {
            PartCited::Input(order, part) => Some((order, part)),
            PartCited::Move(order, segment) => {
                part_of_segment(order, segment, history).map(|part| (order, part))
            }
        }
    }
}

/// The part `entry` acts on, where it routes, waits, finds no route or moves value.
fn part_cited(entry: &VerifiedEntry) -> Option<PartCited<'_>> {
    if let Some((Action::MoveValue { order, segment, .. }, _)) = effect_action(entry) {
        return Some(PartCited::Move(order.as_str(), segment.as_str()));
    }
    let i = input(entry)?;
    match i.observed {
        Observed::RouteRecalculated { .. }
        | Observed::DestinationChanged { .. }
        | Observed::WaitingAtFloor {}
        | Observed::NoRoute { .. } => Some(PartCited::Input(
            i.subject.order.as_str(),
            i.subject.part.as_ref()?.as_str(),
        )),
        _ => None,
    }
}

/// `Holds` where `target` is the entry that put `part` of `order` where it stands, and `stands` accepts it.
fn standing_at(
    order: &str,
    part: &str,
    target: &Entry,
    history: &History,
    stands: impl Fn(&super::standing::Standing) -> bool,
) -> Match {
    let Some(at) = position(target, history) else {
        return Match::NotTarget;
    };
    let candidate = verified(target).is_some_and(|t| {
        input(t).is_some_and(|i| i.subject.order.as_str() == order)
            || matches!(t.what, What::Result { .. })
    });
    if !candidate {
        return Match::NotTarget;
    }
    match standing(order, part, history) {
        Some(s) => judge(s.since == at && stands(&s), &[]),
        None => Match::NotTarget,
    }
}

/// Whether the move `target` is on a segment of the part `restart` is about, and in flight: its segment hasn't
/// settled, been reversed, or failed as a deposit, and its result didn't fail (`INV-4`, `REQ-ORD-2`). A `done`
/// result alone releases nothing: the value's own evidence does.
fn move_in_flight(
    restart: &crate::state_machine::RecordedInput,
    target: &Entry,
    history: &History,
) -> Match {
    let (Some(part), Some(t)) = (restart.subject.part.as_ref(), verified(target)) else {
        return Match::NotTarget;
    };
    let Some((Action::MoveValue { order, segment, .. }, _)) = effect_action(t) else {
        return Match::NotTarget;
    };
    let (order, part) = (order.as_str(), part.as_str());
    if order != restart.subject.order.as_str()
        || part_of_segment(order, segment.as_str(), history) != Some(part)
    {
        return Match::NotTarget;
    }
    match standing(order, part, history) {
        Some(s) => judge(
            !s.released(segment.as_str()) && !result_failed(t, history),
            &[],
        ),
        None => Match::NotTarget,
    }
}

/// Whether the effect `effect` has a `failed` result in `history`.
fn result_failed(effect: &VerifiedEntry, history: &History) -> bool {
    let Some((at, _)) = history.iter().find(|(_, e)| verified(e) == Some(effect)) else {
        return false;
    };
    history.iter().any(|(_, e)| {
        verified(e).is_some_and(|r| {
            matches!(&r.what, What::Result { effect } if effect == at)
                && r.outcome == (Outcome::Failed {})
        })
    })
}

/// Where `segment` of `order` ends, by the first plan in `history` that lists it: a `route_chosen`, a
/// `route_recalculated` or a `destination_changed`.
fn planned_end<'a>(
    order: &str,
    segment: &str,
    history: &'a History,
) -> Option<&'a crate::state_machine::Endpoint> {
    history.iter().find_map(|(_, e)| {
        let i = verified(e).and_then(input)?;
        if i.subject.order.as_str() != order {
            return None;
        }
        let plan: alloc::vec::Vec<&crate::state_machine::SegmentPlan> = match &i.observed {
            Observed::RouteChosen { parts, .. } => parts.iter().flat_map(|p| &p.segments).collect(),
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. } => segments.iter().collect(),
            _ => return None,
        };
        plan.into_iter()
            .find(|s| s.segment_id.as_str() == segment)
            .map(|s| &s.to)
    })
}

/// Whether a permission's `scope` covers `reads`: the trail covers every order, order state and segment, an order
/// its state and its segments, and anything else only itself.
pub fn covers(scope: &Reads, reads: &Reads) -> bool {
    match (scope, reads) {
        (
            Reads::Trail {},
            Reads::Trail {}
            | Reads::Order { .. }
            | Reads::OrderState { .. }
            | Reads::Segment { .. },
        ) => true,
        (Reads::Order { order }, Reads::Order { order: o })
        | (Reads::Order { order }, Reads::OrderState { order: o })
        | (Reads::Order { order }, Reads::Segment { order: o, .. }) => order == o,
        _ => scope == reads,
    }
}

/// How an evidence entry answers a completed reversal: as [`evidence_answers`], and its `whose` another party's than
/// the failed segment's provider, unless it is the origin reserve's, labelled its word (`REQ-ORD-3`, ADR-0008).
fn reversal_answers(entry: &VerifiedEntry, target: &Entry, history: &History) -> Match {
    let found = evidence_answers(entry, target);
    if found == Match::NotTarget {
        return found;
    }
    let (Some(i), Some(What::Evidence { whose, grade, .. })) =
        (input(entry), verified(target).map(|t| &t.what))
    else {
        return Match::NotTarget;
    };
    let moved = history
        .iter()
        .find_map(|(_, e)| match verified(e).and_then(effect_action) {
            Some((
                Action::MoveValue {
                    order,
                    segment,
                    provider,
                    origin_reserve,
                    ..
                },
                _,
            )) if order.as_str() == i.subject.order.as_str()
                && i.subject.segment.as_ref().map(|s| s.as_str()) == Some(segment.as_str()) =>
            {
                Some((provider, origin_reserve))
            }
            _ => None,
        });
    let another = moved.is_some_and(|(provider, origin)| {
        whose != provider || (whose == origin && *grade == EvidenceGrade::Statement)
    });
    match found {
        Match::Mismatch(i) => Match::Mismatch(i),
        _ if another => Match::Holds,
        _ => Match::Mismatch(2),
    }
}

/// Which of the log's handles an entry came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Through {
    /// The general handle, [`calls::Log`](super::calls::Log): orchestration's and the checks'.
    General,
    /// Policy's handle, [`calls::PolicyLog`](super::calls::PolicyLog).
    Policy,
}

/// The transaction a stated entry is about, under the order it is about, where it names both.
fn order_transaction(statement: &StatedEntry) -> Option<(&Id, &Id)> {
    let about = statement.about.as_ref()?;
    Some((about.order.as_ref()?, about.transaction.as_ref()?))
}

/// The transaction a provider's result is about, where it names one: the answer to a question Core asked under it.
fn answered_transaction(statement: &StatedEntry) -> Option<&Id> {
    (statement.what.statement.as_str() == crate::provider_role::RESULT_SHAPE)
        .then(|| statement.about.as_ref()?.transaction.as_ref())
        .flatten()
}

/// A stated entry as the rows of [`STATED_RELATIONS`] read it: with the maker this deployment's operator signs its
/// own statements as, Core's questions among them, which a question's verified `acts` must be: a module the operator
/// publishes states for the operator's party, but its operation is no question of Core's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatedCiting {
    /// The stated entry.
    pub entry: StatedEntry,
    /// The maker the deployment's operator signs as.
    pub operator: Id,
}

/// The rows a stated entry is held to, read as [`RELATIONS`] are. `contracts/core-internal/README.md` lists them in
/// the same table.
pub static STATED_RELATIONS: [Relation<StatedCiting>; 2] = [
    Relation {
        name: "answer-follows-its-question",
        spec: &["REQ-ORD-32", "REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "a provider's answer under a transaction follows the question Core asked under it",
        fields: &[
            ("verified", "a question is a verified statement"),
            (
                "maker",
                "a question is the operator's own statement, signed as the operator",
            ),
            (
                "order",
                "an answer is about the order its question asked for",
            ),
        ],
        cites: |e, _| answered_transaction(&e.entry).map_or_else(none, |_| by_fields(0)),
        answers: |citing, _, t, _| {
            let e = &citing.entry;
            let (Some(transaction), Some(t)) = (answered_transaction(e), stated(t)) else {
                return Match::NotTarget;
            };
            let Some(asked) = t.about.as_ref().filter(|a| {
                t.what.statement.as_str() == crate::provider_role::OPERATION_SHAPE
                    && a.transaction.as_ref() == Some(transaction)
            }) else {
                return Match::NotTarget;
            };
            let order = e.about.as_ref().and_then(|a| a.order.as_ref());
            judge(
                true,
                &[
                    matches!(t.attribution, Attribution::Verified { .. }),
                    maker_provider(t) == &citing.operator,
                    asked.order.is_some() && order.is_none_or(|o| asked.order.as_ref() == Some(o)),
                ],
            )
        },
    },
    Relation {
        name: "transaction-belongs-to-one-order",
        spec: &["REQ-ORD-32", "REQ-ORD-38"],
        quantifier: Quantifier::NoneOther,
        refusal: "a transaction belongs to one order and one segment: the first entry that names it fixes them",
        fields: &[],
        cites: |e, _| order_transaction(&e.entry).map_or_else(none, |_| by_fields(0)),
        answers: |citing, _, t, _| {
            let e = &citing.entry;
            let (Some((order, transaction)), Some(t)) = (order_transaction(e), stated(t)) else {
                return Match::NotTarget;
            };
            judge(
                order_transaction(t).is_some_and(|(o, x)| {
                    x == transaction && (o != order || segment_of(t) != segment_of(e))
                }),
                &[],
            )
        },
    },
];

/// Whether the log admits `entry`, given its `history`: each entry it holds with its receipt, in order. The log runs
/// it on every append ([`calls::Log::append`](super::calls::Log::append)), since a permit, a receipt or an id can be
/// built or read from anywhere. It reads [`RELATIONS`] row by row, and refuses on the first row the entry breaks:
///
/// - an `exists` row breaks when no entry of `history` (at the cited receipt, where the citation is by receipt)
///   answers a citation; its refusal names the field that failed on the entry that came closest, or the row's own
///   refusal where none was even the kind asked for;
/// - a `none other` row breaks when an entry of `history`, other than this same entry retried, answers.
///
/// A stated entry is held to [`STATED_RELATIONS`] alike, with `operator`, the maker this deployment's operator signs
/// its own statements as ([`StatementKeys::operator`](super::calls::StatementKeys::operator)). Before any row, the
/// entry's handle is held to its kind: a decision kind ([`calls::is_decision`](super::calls::is_decision)) only on
/// the policy path, and any other entry only on the general path, so a decision written by anyone but policy is
/// refused even where its type was got round.
pub fn admit_entry(
    history: &[(Receipt, Entry)],
    entry: &Entry,
    through: Through,
    operator: &Id,
) -> Result<(), Violation> {
    let decision = matches!(entry, Entry::Verified(v) if super::calls::is_decision(&v.what));
    match (through, decision) {
        (Through::General, true) => {
            return Err(Violation(super::calls::DECISION_ON_THE_GENERAL_HANDLE));
        }
        (Through::Policy, false) => {
            return Err(Violation(super::calls::RECORD_ON_THE_POLICY_HANDLE));
        }
        _ => {}
    }
    match entry {
        Entry::Verified(entry) => {
            let citing = VerifiedCiting {
                entry: entry.clone(),
                operator: operator.clone(),
            };
            admit_under(&RELATIONS, &citing, history)
        }
        Entry::Stated(entry) => {
            let citing = StatedCiting {
                entry: entry.clone(),
                operator: operator.clone(),
            };
            admit_under(&STATED_RELATIONS, &citing, history)
        }
    }
}

/// Holds `entry` to every row of `rows` over `history`, refusing on the first it breaks.
fn admit_under<C>(
    rows: &[Relation<C>],
    entry: &C,
    history: &[(Receipt, Entry)],
) -> Result<(), Violation> {
    for row in rows {
        for cite in (row.cites)(entry, history) {
            let candidates = history
                .iter()
                .filter(|(r, _)| cite.receipt.as_ref().is_none_or(|c| c == r))
                .map(|(_, e)| (row.answers)(entry, &cite, e, history));
            match row.quantifier {
                Quantifier::Exists => {
                    let mut closest: Option<usize> = None;
                    let mut holds = false;
                    for answer in candidates {
                        match answer {
                            Match::Holds => {
                                holds = true;
                                break;
                            }
                            Match::Mismatch(i) => closest = closest.max(Some(i)),
                            Match::NotTarget => {}
                        }
                    }
                    if !holds {
                        return Err(Violation(closest.map_or(row.refusal, |i| row.fields[i].1)));
                    }
                }
                Quantifier::NoneOther => {
                    let mut candidates = candidates;
                    if candidates.any(|a| a == Match::Holds) {
                        return Err(Violation(row.refusal));
                    }
                }
            }
        }
    }
    Ok(())
}
