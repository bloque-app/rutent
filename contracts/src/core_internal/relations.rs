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

mod closing;
pub use closing::CLOSING_RELATIONS;

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

/// The deployment's operator, as the log knows it: the maker it signs its own statements as
/// ([`StatementKeys::operator`](super::calls::StatementKeys::operator)), and the accountable party it is, which that
/// maker states for ([`StatementKeys::publisher`](super::calls::StatementKeys::publisher)). The two differ: a
/// deployment's operator may sign as `conformance-operator` and be the party `operator`, and a row reads each where
/// it means it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentOperator {
    /// The maker the deployment's operator signs its statements as: a statement's `acts`.
    pub maker: Id,
    /// The operator's accountable party: the `party` its people act for.
    pub party: Id,
}

/// A verified entry as the rows of [`RELATIONS`] read it: with the deployment's operator, by the maker it signs its
/// own statements as, so a row can tell the operator's own statements from a module's it publishes, and by its
/// party, so a row can tell the operator's people from anyone stating the operator's role. It reads as the entry
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCiting {
    /// The verified entry.
    pub entry: VerifiedEntry,
    /// The deployment's operator.
    pub operator: DeploymentOperator,
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
pub static RELATIONS: [Relation; 81] = [
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
            (
                "part",
                "a value-moving effect's authorisation covers the part its segment is of",
            ),
            (
                "in_force",
                "a value-moving effect's authorisation is in force for its part: its terms are those its part's latest setting authorisation set",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        // Destinations change per part, so an authorisation may cover only some parts, and a replacement replaces
        // the earlier one for the parts it names alone (`T-021-04`).
        answers: |e, _, t, h| {
            let (
                Some((
                    Action::MoveValue {
                        order,
                        segment,
                        authorisation,
                        route,
                        ..
                    },
                    _,
                )),
                Some(v),
            ) = (effect_action(e), permitted(t))
            else {
                return Match::NotTarget;
            };
            match &v.what {
                What::Authorise {
                    authorisation: a, ..
                } => {
                    let part = part_of_segment(order.as_str(), segment.as_str(), h);
                    // In force: the terms of the part's latest authorisation that set them; a restatement by
                    // anyone but the customer sets nothing (`only-the-customer-sets-new-terms`).
                    let replaced = part.is_some_and(|p| {
                        in_force(order.as_str(), Some(p), h)
                            .is_some_and(|current| terms(current) != terms(a))
                    });
                    judge(
                        &a.authorisation == authorisation,
                        &[
                            &a.order == order,
                            a.destination == route.destination,
                            a.to_asset == route.to_asset,
                            a.floor
                                .as_ref()
                                .is_none_or(|f| &route.delivers >= f.amount()),
                            // An unplanned segment is `move-segment-is-in-its-orders-plan`'s.
                            part.is_none_or(|p| covers_part(a, p)),
                            !replaced,
                        ],
                    )
                }
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "only-the-customer-sets-new-terms",
        spec: &["REQ-AUZ-3", "REQ-ORD-34", "INV-10"],
        quantifier: Quantifier::NoneOther,
        refusal: "an authorisation with terms other than those in force is the customer's: anyone else's restates them",
        fields: &[],
        // Two engines may each read the same terms in force and permit one authorisation each; whichever comes
        // last, an operator's restatement can't bring the old destination back (`T-021-04`'s review). The first
        // authorisation of a part sets its terms, whoever gives it (`REQ-AUZ-9`).
        cites: |e, _| match (&e.what, &e.outcome) {
            (What::Authorise { .. }, Outcome::Permitted {}) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (
                What::Authorise {
                    authorisation: a, ..
                },
                Some(What::Authorise {
                    authorisation: t_a, ..
                }),
            ) = (&e.what, permitted(t).map(|v| &v.what))
            else {
                return Match::NotTarget;
            };
            if by_the_customer(a, h) {
                return Match::NotTarget;
            }
            judge(
                covered_parts(a, h).into_iter().any(|p| {
                    in_force(a.order.as_str(), p, h).is_some_and(|current| {
                        core::ptr::eq(current, t_a) && terms(current) != terms(a)
                    })
                }),
                &[],
            )
        },
    },
    Relation {
        name: "only-the-payer-moves-the-origin-account",
        spec: &["REQ-AUZ-3", "REQ-AUZ-4", "REQ-ORD-17"],
        quantifier: Quantifier::Exists,
        refusal: "a part's origin account is set by its first authorisation, and moved only by the order's payer",
        fields: &[],
        // A delivery back to the origin is a refund of the payer's value, so neither the requester nor the operator
        // redirects it (the owner's form default). None to point to: the authorisation is refused where it names
        // another origin account than the one in force for a part it covers, and isn't the payer's.
        cites: |e, h| match (&e.what, &e.outcome) {
            (
                What::Authorise {
                    authorisation: a, ..
                },
                Outcome::Permitted {},
            ) if !by_the_payer(a, h)
                && covered_parts(a, h).into_iter().any(|p| {
                    origin_account_in_force(a.order.as_str(), p, h)
                        .is_some_and(|current| current != a.origin_account.as_ref())
                }) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |_, _, _, _| Match::NotTarget,
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
        fields: &[
            (
                "authorisation",
                "a value-moving effect's acceptance names its authorisation",
            ),
            (
                "part",
                "a value-moving effect's acceptance has a yes for the part its segment is of",
            ),
        ],
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (
                Some((
                    Action::MoveValue {
                        order,
                        segment,
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
                    basis,
                    yeses,
                    ..
                } => {
                    // A given acceptance has a yes per part it accepts; an automatic deposit's has none, for the
                    // order's one part. A part's first move is `first-move-rests-on-its-parts-yes`'s, and an
                    // unplanned segment `move-segment-is-in-its-orders-plan`'s.
                    let part = part_of_segment(order.as_str(), segment.as_str(), h);
                    let first = first_segment_part(e, h).is_some();
                    let covered = first
                        || *basis == AcceptanceBasis::AutomaticDeposit
                        || part.is_none_or(|p| yeses.iter().any(|y| y.part.as_str() == p));
                    judge(o == order, &[a == authorisation, covered])
                }
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
        name: "split-deposit-arrival-names-its-part",
        spec: &["REQ-ORD-15", "REQ-ORD-42"],
        quantifier: Quantifier::Exists,
        refusal: "a split route of an order accepted on an automatic deposit rests on an arrival naming each part",
        fields: &[],
        // An automatic deposit's order of more than one part: each part's value arrived, and its arrival says which
        // part it is for. One of one part needs none.
        cites: |e, h| match input(e).map(|i| (i, &i.observed)) {
            Some((i, Observed::RouteChosen { parts, .. }))
                if parts.len() > 1 && deposited(i.subject.order.as_str(), h) =>
            {
                (0..parts.len()).flat_map(by_fields).collect()
            }
            _ => none(),
        },
        answers: |e, c, t, _| {
            let (Some(i), Some(What::Arrival { order, part, .. })) =
                (input(e), verified(t).map(|t| &t.what))
            else {
                return Match::NotTarget;
            };
            let Observed::RouteChosen { parts, .. } = &i.observed else {
                return Match::NotTarget;
            };
            judge(
                order
                    .as_ref()
                    .is_some_and(|o| o.as_str() == i.subject.order.as_str())
                    && parts.get(c.index).is_some_and(|p| {
                        part.as_ref()
                            .is_some_and(|q| q.as_str() == p.part_id.as_str())
                    }),
                &[],
            )
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
            (
                "transaction",
                "a stuck order's effect goes under the transaction its decide step named",
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
                        transaction,
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
                    &[
                        &request.order == order,
                        &request.resolution == resolution,
                        &request.transaction == transaction,
                    ],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "stuck-resolution-transaction-is-its-own",
        spec: &["REQ-AUZ-4", "REQ-ORD-38"],
        quantifier: Quantifier::NoneOther,
        refusal: "a stuck resolution's transaction is its own: no other resolution, statement, move, yes or deposit notice uses it",
        fields: &[],
        // Its outcome's evidence binds through it, so a transaction another entry of the log already uses would let
        // that entry's statements settle this resolution. Its own decide step and effect, and either retried, aside,
        // and, after its decision, its own question and the answers to it.
        cites: |e, _| match resolution_transaction(e) {
            Some(_) => by_fields(0),
            None => none(),
        },
        answers: |e, _, t, h| {
            let Some((_, transaction, decided)) = resolution_transaction(e) else {
                return Match::NotTarget;
            };
            let uses = match t {
                // A statement naming it: any, before the resolution's own decision, which then takes over another
                // question's transaction; after it, only the resolution's own, Core's question and the answers to it,
                // never one binding it to a segment (a move's).
                Entry::Stated(s) => s.about.as_ref().is_some_and(|a| {
                    a.transaction.as_ref() == Some(transaction)
                        && (position(t, h) < decided_at(decided, h)
                            || a.segment.is_some()
                            || !(s.what.statement.as_str() == crate::provider_role::RESULT_SHAPE
                                || (s.what.statement.as_str()
                                    == crate::provider_role::OPERATION_SHAPE
                                    && maker_provider(s) == &e.operator.maker)))
                }),
                Entry::Verified(v) => match &v.what {
                    What::Acceptance { yeses, .. } => yeses.iter().any(|y| {
                        entry_at(&y.reserve_yes, h)
                            .and_then(stated)
                            .and_then(|s| s.about.as_ref())
                            .and_then(|a| a.transaction.as_ref())
                            == Some(transaction)
                    }),
                    What::Arrival { .. } => deposit_of(&v.what, h) == Some(transaction),
                    _ => resolution_transaction(v)
                        .is_some_and(|(_, other, by)| other == transaction && by != decided),
                },
            };
            judge(uses, &[])
        },
    },
    Relation {
        name: "stuck-resolution-transaction-is-unbound-or-its-orders",
        spec: &["REQ-AUZ-4", "REQ-ORD-38"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck resolution's transaction is unbound, or bound at order level to its own order",
        fields: &[],
        // None to point to: the first statement naming the transaction (`binding_of`), where there is one, names
        // this order and no segment.
        cites: |e, h| match resolution_transaction(e) {
            Some((order, transaction, _)) => match binding_of(transaction, h) {
                Some(bound) if bound != (order, None) => by_fields(0),
                _ => none(),
            },
            None => none(),
        },
        answers: |_, _, _, _| Match::NotTarget,
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
        // One exception: a move whose outcome was unknown, `unanswered`, takes one final result, which settles only
        // on its outcome's confirmation (`unknown-outcome-settles-on-its-confirmation`).
        answers: |e, _, t, _| {
            let (What::Result { effect }, Some(t)) = (&e.what, verified(t)) else {
                return Match::NotTarget;
            };
            let settles = t.outcome == (Outcome::Unanswered {})
                && matches!(e.outcome, Outcome::Done {} | Outcome::Failed {});
            judge(
                t != &e.entry
                    && !settles
                    && matches!(&t.what, What::Result { effect: other } if other == effect),
                &[],
            )
        },
    },
    Relation {
        name: "unknown-outcome-settles-on-its-confirmation",
        spec: &["REQ-AUZ-4", "REQ-ORD-1"],
        quantifier: Quantifier::Exists,
        refusal: "a move whose outcome was unknown settles only on a permitted confirmation of its outcome in this log",
        fields: &[(
            "outcome",
            "a move whose outcome was unknown settles on the outcome its confirmation names",
        )],
        // Its first result `unanswered`, its final one follows policy's permitted confirmation, on two actors'
        // word (`outcome-confirmation-rests-on-its-move`).
        cites: |e, h| match (&e.what, &e.outcome) {
            (What::Result { effect }, Outcome::Done {} | Outcome::Failed {})
                if h.iter().any(|(_, x)| {
                    verified(x).is_some_and(|r| {
                        matches!(&r.what, What::Result { effect: other } if other == effect)
                            && r.outcome == (Outcome::Unanswered {})
                    })
                }) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::Result { effect }, Some(t)) = (&e.what, permitted(t)) else {
                return Match::NotTarget;
            };
            match &t.what {
                What::ConfirmOutcome { request, .. } => judge(
                    &request.effect == effect,
                    &[match request.outcome {
                        MoveOutcome::Done => e.outcome == (Outcome::Done {}),
                        MoveOutcome::Failed => e.outcome == (Outcome::Failed {}),
                    }],
                ),
                _ => Match::NotTarget,
            }
        },
    },
    Relation {
        name: "outcome-report-rests-on-its-move",
        spec: &["REQ-AUZ-4", "REQ-ORD-1", "ADR-0011"],
        quantifier: Quantifier::Exists,
        refusal: "an outcome's report is of a move of this log whose outcome is unknown",
        fields: &[
            (
                "unknown",
                "an outcome is reported only where its move's result is unanswered",
            ),
            (
                "open",
                "an outcome is reported only while its move is unsettled: no final result and no confirmation",
            ),
            (
                "mover",
                "a move's outcome is reported by its own mover: a move_value's decided-for actor or its accountable party, a stuck resolution's mover",
            ),
        ],
        cites: |e, _| match &e.what {
            What::ReportOutcome { request, .. } => alloc::vec![by_receipt(&request.effect, 0)],
            _ => none(),
        },
        answers: |e, _, entry, h| {
            let (What::ReportOutcome { request, .. }, Some(t)) = (&e.what, verified(entry)) else {
                return Match::NotTarget;
            };
            let Some((order, _)) = moved_part(t, h) else {
                return Match::NotTarget;
            };
            judge(
                order == &request.order,
                &[
                    unanswered(entry, h),
                    !settled(entry, h),
                    moved_by(t, &request.reporter, h),
                ],
            )
        },
    },
    Relation {
        name: "one-report-per-effect",
        spec: &["REQ-AUZ-4", "ADR-0011"],
        quantifier: Quantifier::NoneOther,
        refusal: "a move's outcome is reported once: no other permitted report of its effect",
        fields: &[],
        // An identical retry aside, a second report, of the same outcome or the other, is refused: the confirmer
        // confirms the one report there is.
        cites: |e, _| match &e.what {
            What::ReportOutcome { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, permitted(t).map(|t| &t.what)) {
            (
                What::ReportOutcome { request, .. },
                Some(
                    other @ What::ReportOutcome {
                        request: earlier, ..
                    },
                ),
            ) => judge(earlier.effect == request.effect && other != &e.what, &[]),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "outcome-confirmation-rests-on-its-move",
        spec: &["REQ-AUZ-4", "REQ-ORD-1", "ADR-0011"],
        quantifier: Quantifier::Exists,
        refusal: "an outcome's confirmation is of a move of this log whose outcome is unknown",
        fields: &[(
            "unknown",
            "an outcome is confirmed only where its move's result is unanswered",
        )],
        cites: |e, _| match &e.what {
            What::ConfirmOutcome { request, .. } => alloc::vec![by_receipt(&request.effect, 0)],
            _ => none(),
        },
        answers: |e, _, entry, h| {
            let (What::ConfirmOutcome { request, .. }, Some(t)) = (&e.what, verified(entry)) else {
                return Match::NotTarget;
            };
            let Some((order, _)) = moved_part(t, h) else {
                return Match::NotTarget;
            };
            judge(order == &request.order, &[unanswered(entry, h)])
        },
    },
    Relation {
        name: "outcome-confirmation-rests-on-its-report",
        spec: &["REQ-AUZ-4", "ADR-0011"],
        quantifier: Quantifier::Exists,
        refusal: "an outcome's confirmation cites its mover's permitted report of the same move and outcome",
        fields: &[
            (
                "party",
                "an outcome is confirmed by another accountable party than its reporter's",
            ),
            (
                "login",
                "an outcome is confirmed by another login than its reporter's",
            ),
            (
                "evidence",
                "an outcome's confirmation cites another statement than its report, by another maker",
            ),
        ],
        // Two actors' word: the report is the mover's own decision, the confirmation another actor's, and each rests
        // on its own statement, made by a different maker.
        cites: |e, _| match &e.what {
            What::ConfirmOutcome { request, .. } => alloc::vec![by_receipt(&request.report, 0)],
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (What::ConfirmOutcome { request, .. }, Some(t)) = (&e.what, permitted(t)) else {
                return Match::NotTarget;
            };
            let What::ReportOutcome {
                request: report, ..
            } = &t.what
            else {
                return Match::NotTarget;
            };
            let makers = (
                entry_at(&report.evidence, h).and_then(stated),
                entry_at(&request.confirmed_with, h).and_then(stated),
            );
            judge(
                report.effect == request.effect
                    && report.order == request.order
                    && report.outcome == request.outcome,
                &[
                    request.confirmer.party != report.reporter.party,
                    request.confirmer.acts != report.reporter.acts,
                    report.evidence != request.confirmed_with
                        && match makers {
                            (Some(a), Some(b)) => {
                                a != b && a.attribution.maker().party != b.attribution.maker().party
                            }
                            _ => false,
                        },
                ],
            )
        },
    },
    Relation {
        name: "outcome-confirmation-cites-verified-evidence",
        spec: &["REQ-AUZ-4", "REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "an outcome's report and its confirmation each cite a stated entry of this log",
        fields: &[
            (
                "verified",
                "an outcome's report and its confirmation each cite a verified statement",
            ),
            (
                "result",
                "an outcome's evidence is a provider's result, not a question or another shape",
            ),
            (
                "maker",
                "an outcome's evidence is not the deployment operator's own statement",
            ),
            (
                "about",
                "an outcome's evidence is about its move's own order and part: a move_value's own segment, a stuck resolution's own transaction",
            ),
            ("after", "an outcome's evidence follows its move in the log"),
            (
                "made",
                "an outcome's evidence was made no earlier than its move",
            ),
        ],
        cites: |e, _| match &e.what {
            What::ReportOutcome { request, .. } => alloc::vec![by_receipt(&request.evidence, 0)],
            What::ConfirmOutcome { request, .. } => {
                alloc::vec![by_receipt(&request.confirmed_with, 0)]
            }
            _ => none(),
        },
        answers: |e, _, t, h| {
            let effect = match &e.what {
                What::ReportOutcome { request, .. } => &request.effect,
                What::ConfirmOutcome { request, .. } => &request.effect,
                _ => return Match::NotTarget,
            };
            let Some(s) = stated(t) else {
                return Match::NotTarget;
            };
            let moved = entry_at(effect, h);
            let about = moved
                .and_then(verified)
                .is_some_and(|m| evidence_about(s, m, h));
            // Evidence older than the move can't say what the move came to: neither one appended before it, nor one its
            // maker made before it, appended after.
            let after = match (moved.and_then(|m| position(m, h)), position(t, h)) {
                (Some(m), Some(at)) => at > m,
                _ => false,
            };
            let made = moved
                .and_then(verified)
                .is_some_and(|m| s.when.at.unix_nanos() >= m.when.at.unix_nanos());
            judge(
                true,
                &[
                    matches!(s.attribution, Attribution::Verified { .. }),
                    // What a provider answered, never Core's own question re-appended, nor the operator's word.
                    s.what.statement.as_str() == crate::provider_role::RESULT_SHAPE,
                    maker_provider(s) != &e.operator.maker,
                    about,
                    after,
                    made,
                ],
            )
        },
    },
    Relation {
        name: "one-confirmation-per-effect",
        spec: &["REQ-AUZ-4", "REQ-ORD-1"],
        quantifier: Quantifier::NoneOther,
        refusal: "a move's outcome is confirmed once: no other permitted confirmation of its effect",
        fields: &[],
        // An identical retry aside, a second confirmation, of the same outcome or the other, is refused: the first
        // settled the move.
        cites: |e, _| match &e.what {
            What::ConfirmOutcome { .. } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, permitted(t).map(|t| &t.what)) {
            (
                What::ConfirmOutcome { request, .. },
                Some(
                    other @ What::ConfirmOutcome {
                        request: earlier, ..
                    },
                ),
            ) => judge(earlier.effect == request.effect && other != &e.what, &[]),
            _ => Match::NotTarget,
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
            (
                "login",
                "a stuck order's value is moved by another login than the one that decided",
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
                        request.decider.acts != mv.mover.acts,
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
                // Without a plan, a declaration counts only for a part the order's acceptance names.
                None => (
                    matches!(i.observed, Observed::StuckDeclared { .. })
                        && accepted_part(order, part, h),
                    false,
                ),
            };
            judge(true, &[stuck, !resolved])
        },
    },
    Relation {
        name: "delivery-goes-to-the-parts-ends",
        spec: &["REQ-AUZ-4", "REQ-AUZ-3", "INV-10"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck part's delivery rests on the authorisation in force for the part",
        fields: &[(
            "to",
            "a stuck part's delivery goes to the part's authorised destination, whole, or back to its origin",
        )],
        // Checked where the value is decided and where it is moved, not only once it arrived (`stuck_resolved`).
        cites: |e, _| match &e.what {
            What::StuckDecide { request, .. }
                if matches!(request.resolution, Resolution::Deliver { .. }) =>
            {
                by_fields(0)
            }
            What::Effect {
                action:
                    Action::ResolveStuck {
                        resolution: Resolution::Deliver { .. },
                        ..
                    },
                ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| {
            let (order, part, resolution) = match &e.what {
                What::StuckDecide { request, .. } => (
                    request.order.as_str(),
                    Some(request.part.as_str()),
                    &request.resolution,
                ),
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order,
                            carries_out,
                            resolution,
                            ..
                        },
                    ..
                } => (
                    order.as_str(),
                    decided_part(order.as_str(), carries_out, h),
                    resolution,
                ),
                _ => return Match::NotTarget,
            };
            let (
                Some(part),
                Some(What::Authorise {
                    authorisation: a, ..
                }),
            ) = (part, permitted(t).map(|v| &v.what))
            else {
                return Match::NotTarget;
            };
            judge(
                in_force(order, Some(part), h).is_some_and(|current| core::ptr::eq(current, a)),
                &[resolved_to(order, part, resolution, h).is_some()],
            )
        },
    },
    Relation {
        name: "stuck-resolved-follows-a-done-resolution",
        spec: &["REQ-AUZ-4", "REQ-ORD-28"],
        quantifier: Quantifier::Exists,
        refusal: "a stuck part is resolved on a result of this log's resolution of that part",
        fields: &[
            (
                "done",
                "a stuck part is resolved once its resolution's move is done",
            ),
            (
                "to",
                "a stuck part is resolved where its decision sent the value: the order's destination or its origin",
            ),
        ],
        // The target is a result of a `resolve_stuck` effect whose decision names the part. A `stuck_resolved`
        // follows a `deliver` to where it sent the value; a stuck part's `handed_on` follows a `hand_on` to its
        // linked order (a waiting part's is the customer's, and needs none).
        cites: |e, h| match input(e) {
            Some(i) if matches!(i.observed, Observed::StuckResolved { .. }) => by_fields(0),
            Some(i)
                if matches!(i.observed, Observed::HandedOn { .. })
                    && i.subject.part.as_ref().is_some_and(|p| {
                        standing(i.subject.order.as_str(), p.as_str(), h).is_some_and(|s| {
                            s.part.state == crate::state_machine::OrderState::Stuck
                        })
                    }) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, h| {
            let Some(i) = input(e) else {
                return Match::NotTarget;
            };
            let Some(part) = i.subject.part.as_ref() else {
                return Match::NotTarget;
            };
            let order = i.subject.order.as_str();
            let Some(result) = verified(t) else {
                return Match::NotTarget;
            };
            let What::Result { effect } = &result.what else {
                return Match::NotTarget;
            };
            let resolution = h.iter().find_map(|(r, x)| match &verified(x)?.what {
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order: o,
                            carries_out,
                            resolution,
                            ..
                        },
                    ..
                } if r == effect
                    && o.as_str() == order
                    && decided_part(order, carries_out, h) == Some(part.as_str()) =>
                {
                    Some(resolution)
                }
                _ => None,
            });
            let Some(resolution) = resolution else {
                return Match::NotTarget;
            };
            let to = match (&i.observed, resolution) {
                (Observed::StuckResolved { to, .. }, _) => {
                    resolved_to(order, part.as_str(), resolution, h) == Some(*to)
                }
                (
                    Observed::HandedOn { linked_order, .. },
                    Resolution::HandOn {
                        linked_order: to, ..
                    },
                ) => to.as_str() == linked_order.as_str(),
                _ => false,
            };
            judge(true, &[result.outcome == (Outcome::Done {}), to])
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
        name: "arrival-names-its-notice",
        spec: &["REQ-ORD-23", "REQ-ORD-14"],
        quantifier: Quantifier::Exists,
        refusal: "an arrival's notice is a stated entry of this log",
        fields: &[
            ("verified", "an arrival's notice is a verified statement"),
            ("maker", "an arrival's notice is its reserve's statement"),
            (
                "transaction",
                "an arrival's notice names the deposit's signed transaction",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Arrival {
                notice: Some(_), ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, _| match (&e.what, stated(t)) {
            (
                What::Arrival {
                    notice: Some(notice),
                    reserve,
                    ..
                },
                Some(s),
            ) => judge(
                &s.commitment == notice,
                &[
                    // An unverified statement is attributed to no one, so the transaction it claims keys nothing.
                    matches!(s.attribution, Attribution::Verified { .. }),
                    maker_provider(s) == reserve,
                    s.about.as_ref().is_some_and(|a| a.transaction.is_some()),
                ],
            ),
            _ => Match::NotTarget,
        },
    },
    Relation {
        name: "one-order-per-deposit",
        spec: &["REQ-ORD-23", "REQ-ORD-14", "INV-8"],
        quantifier: Quantifier::NoneOther,
        refusal: "one deposit opens one order: no other arrival of its deposit names an order",
        fields: &[],
        // Keyed by the deposit's signed transaction, the one its notice names (`deposit_of`), not the notice's salted
        // commitment, which the same bytes kept twice would change: a second order on it, or a second filing with
        // other figures, is refused; the same arrival sent again is a retry.
        cites: |e, _| match &e.what {
            What::Arrival {
                order: Some(_),
                notice: Some(_),
                ..
            } => by_fields(0),
            _ => none(),
        },
        answers: |e, _, t, h| match (&e.what, verified(t).map(|t| &t.what)) {
            (
                arrival @ What::Arrival { .. },
                Some(other @ What::Arrival { order: Some(_), .. }),
            ) => judge(
                deposit_of(arrival, h).is_some_and(|d| deposit_of(other, h) == Some(d))
                    && other != &e.what,
                &[],
            ),
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
                "an arrival is filed anew by the deployment's operator or by the reserve it arrived at",
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
                        // The deployment's operator, by the party the log knows it as, not any actor stating the
                        // operator's role; or the arrival's own reserve acting for itself.
                        (e.who.role == Role::Operator && e.who.party == e.operator.party)
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
                && maker_provider(s) == &e.operator.maker
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
        name: "part-waits-for-value-in-motion",
        spec: &["REQ-ORD-2", "INV-4", "REQ-ORD-27", "REQ-AUZ-4"],
        quantifier: Quantifier::NoneOther,
        refusal: "a part's route restarts, it waits at its floor, is handed on or is resolved only while none of its value is in motion",
        fields: &[],
        // One predicate (`in_motion`) for every input and decision that would move a part's value from where it
        // sits: a value-moving effect in motion may still deliver, and moving around it moves the same value twice.
        cites: |e, h| match motion_subject(e, h) {
            Some(_) => by_fields(0),
            None => none(),
        },
        answers: |e, _, t, h| {
            let Some((order, part)) = motion_subject(e, h) else {
                return Match::NotTarget;
            };
            let Some(v) = verified(t) else {
                return Match::NotTarget;
            };
            // The same effect sent again is a retry; a hand-on's own done resolution is the one it completes.
            if v.what == e.what {
                return Match::NotTarget;
            }
            if let (
                Some(Observed::HandedOn { linked_order, .. }),
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            resolution:
                                Resolution::HandOn {
                                    linked_order: to, ..
                                },
                            ..
                        },
                    ..
                },
            ) = (input(e).map(|i| &i.observed), &v.what)
                && to.as_str() == linked_order.as_str()
                && result_done(v, h)
            {
                return Match::NotTarget;
            }
            in_motion(order, part, t, h)
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
        refusal: "a part that has ended takes no move",
        fields: &[],
        // Handed on, cancelled, ended on request, complete, failed, or resolved while stuck: the part's value is
        // gone or delivered, and nothing moves it again (`standing`). An input on it the state machine refuses
        // (`input-is-one-the-state-machine-takes`).
        cites: |e, _| match effect_action(e) {
            Some((Action::MoveValue { .. }, _)) => by_fields(0),
            _ => none(),
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
    Relation {
        name: "operator-command-is-the-deployment-operators",
        spec: &["REQ-AUD-10", "REQ-AUD-1"],
        quantifier: Quantifier::Exists,
        refusal: "an operator command is the deployment operator's: an operator acting for its party",
        fields: &[],
        // Read by party, as a refiling's filer is: not any actor stating the operator's role, nor anyone of another
        // role. None to point to: the command is refused where its actor isn't the deployment's operator.
        cites: |e, _| match &e.what {
            What::Command { .. }
                if !(e.who.role == Role::Operator && e.who.party == e.operator.party) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |_, _, _, _| Match::NotTarget,
    },
    Relation {
        name: "input-not-taken-is-one-refused",
        spec: &["REQ-AUD-1", "REQ-STM-3", "REQ-STM-4"],
        quantifier: Quantifier::Exists,
        refusal: "an input recorded as not taken is one the state machine or the host refuses, for the reason it names",
        fields: &[],
        // The trace of a refused input, which no order reads; one the machine would take, or another reason than
        // its own, would be a false trace.
        cites: |e, h| match &e.what {
            What::InputNotTaken { input, reason }
                if super::standing::not_taken(h, input) != Some(*reason) =>
            {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |_, _, _, _| Match::NotTarget,
    },
    Relation {
        name: "input-is-one-the-state-machine-takes",
        spec: &["REQ-STM-3", "INV-4", "REQ-AUD-1"],
        quantifier: Quantifier::Exists,
        refusal: "a recorded input is one the state machine takes from where its order stands",
        fields: &[],
        // The log reads each order's inputs through the state machine's published rules (`standing`); one those
        // rules don't take, which `CMP-STM` would answer not applicable, would put the log and the order apart. The
        // differential oracle in `CMP-STM`'s tests holds the two to each other. Last in the table, so every other
        // row's refusal comes first.
        cites: |e, h| match input(e) {
            Some(i) if !super::standing::state_machine_takes(h, i) => by_fields(0),
            _ => none(),
        },
        answers: |_, _, _, _| Match::NotTarget,
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

/// Where a stuck resolution sends the value of `part` of `order`, by the authorisation in force for the part
/// (`in_force`), whole: its destination (reserve, node and account), or its origin, the in-force origin reserve at
/// the node the part's first segment withdrew from. `None` for a hand-on, or where neither holds. No entry records
/// the origin's account, so a delivery back to the origin isn't held to one: a shape gap, named in the README.
fn resolved_to(
    order: &str,
    part: &str,
    resolution: &Resolution,
    history: &History,
) -> Option<crate::state_machine::ResolvedTo> {
    let Resolution::Deliver { to, .. } = resolution else {
        return None;
    };
    let a = in_force(order, Some(part), history)?;
    if &a.destination == to {
        return Some(crate::state_machine::ResolvedTo::Destination);
    }
    let origin_node = history
        .iter()
        .filter_map(|(_, e)| verified(e).and_then(input))
        .filter(|i| i.subject.order.as_str() == order)
        .find_map(|i| match &i.observed {
            Observed::RouteChosen { parts, .. } => parts
                .iter()
                .find(|p| p.part_id.as_str() == part)
                .and_then(|p| p.segments.first())
                .map(|s| &s.from.node),
            _ => None,
        });
    let from_origin = to.reserve == a.origin_reserve
        && origin_account_in_force(order, Some(part), history).flatten() == Some(&to.account)
        && matches!(origin_node, Some(crate::provider_role::NodeRef::Outer(n)) if n == &to.node);
    from_origin.then_some(crate::state_machine::ResolvedTo::Origin)
}

/// Whether `order` was accepted on an automatic deposit in `history`.
fn deposited(order: &str, history: &History) -> bool {
    history.iter().any(|(_, e)| {
        matches!(&verified(e).map(|v| &v.what), Some(What::Acceptance {
            order: o,
            basis: AcceptanceBasis::AutomaticDeposit,
            ..
        }) if o.as_str() == order)
    })
}

/// Whether an acceptance of `order` in `history` has a yes for `part`.
fn accepted_part(order: &str, part: &str, history: &History) -> bool {
    history.iter().any(|(_, e)| {
        matches!(&verified(e).map(|v| &v.what), Some(What::Acceptance { order: o, yeses, .. })
            if o.as_str() == order && yeses.iter().any(|y| y.part.as_str() == part))
    })
}

/// The terms an authorisation sets: where the value goes, in what, at least what, and from which origin reserve.
fn terms(
    a: &Authorisation,
) -> (
    &Destination,
    &crate::provider_role::Denomination,
    Option<&crate::router_api::PositiveAmount>,
    &Id,
) {
    (
        &a.destination,
        &a.to_asset,
        a.floor.as_ref(),
        &a.origin_reserve,
    )
}

/// Whether the order's customer gave `a`: its requester, or one acting for it as its party, in the customer's role,
/// as the engine and Core read it.
fn by_the_customer(a: &Authorisation, history: &History) -> bool {
    a.given_by.role == Role::Customer
        && requester_of(a.order.as_str(), history).is_some_and(|r| &a.given_by.party == r)
}

/// Whether the order's payer gave `a`: one acting for the party its acceptance names as payer, in the customer's
/// role. A refund back to the origin is the payer's, so only the payer moves it (`origin_account_in_force`).
fn by_the_payer(a: &Authorisation, history: &History) -> bool {
    a.given_by.role == Role::Customer
        && history.iter().any(|(_, e)| {
            matches!(verified(e).map(|v| &v.what), Some(What::Acceptance { order, parties, .. })
                if order == &a.order && parties.payer == a.given_by.party)
        })
}

/// The origin account in force for `part` of `order` after `history`: the one the first permitted authorisation
/// covering the part names, then each later one the payer gave. Anyone else's names the one in force or is refused
/// (`only-the-payer-moves-the-origin-account`); which accounts the payer holds is policy's register, not the log's.
fn origin_account_in_force<'a>(
    order: &str,
    part: Option<&str>,
    history: &'a History,
) -> Option<Option<&'a Id>> {
    let mut current: Option<Option<&Id>> = None;
    for (_, e) in history {
        let Some(What::Authorise {
            authorisation: a, ..
        }) = permitted(e).map(|v| &v.what)
        else {
            continue;
        };
        let covers = match part {
            Some(p) => covers_part(a, p),
            None => a.parts.is_none(),
        };
        if a.order.as_str() == order && covers && (current.is_none() || by_the_payer(a, history)) {
            current = Some(a.origin_account.as_ref());
        }
    }
    current
}

/// The authorisation whose terms are in force for `part` of `order` after `history` (`None`: one naming no part,
/// for the order as a whole): the first permitted one covering it, then each later one the customer gave. Anyone
/// else's restates the terms in force and sets nothing.
fn in_force<'a>(
    order: &str,
    part: Option<&str>,
    history: &'a History,
) -> Option<&'a Authorisation> {
    let mut current: Option<&Authorisation> = None;
    for (_, e) in history {
        let Some(What::Authorise {
            authorisation: a, ..
        }) = permitted(e).map(|v| &v.what)
        else {
            continue;
        };
        let covers = match part {
            Some(p) => covers_part(a, p),
            None => a.parts.is_none(),
        };
        if a.order.as_str() == order && covers && (current.is_none() || by_the_customer(a, history))
        {
            current = Some(a);
        }
    }
    current
}

/// The parts `a` covers, as far as `history` can name them: those it names, or else every part the order's plans
/// list, or else the order as a whole.
fn covered_parts<'a>(a: &'a Authorisation, history: &'a History) -> Vec<Option<&'a str>> {
    if let Some(parts) = &a.parts {
        return parts.iter().map(|p| Some(p.as_str())).collect();
    }
    let planned: Vec<Option<&str>> = history
        .iter()
        .filter_map(|(_, e)| verified(e).and_then(input))
        .filter(|i| i.subject.order.as_str() == a.order.as_str())
        .find_map(|i| match &i.observed {
            Observed::RouteChosen { parts, .. } => {
                Some(parts.iter().map(|p| Some(p.part_id.as_str())).collect())
            }
            _ => None,
        })
        .unwrap_or_default();
    if planned.is_empty() {
        alloc::vec![None]
    } else {
        planned
    }
}

/// Whether `authorisation` covers `part`: every part where it names none.
fn covers_part(authorisation: &Authorisation, part: &str) -> bool {
    authorisation
        .parts
        .as_ref()
        .is_none_or(|parts| parts.iter().any(|p| p.as_str() == part))
}

/// The permitted confirmation of the outcome of the effect at `effect` in `history`, where one is.
fn confirmation_of<'a>(effect: &Receipt, history: &'a History) -> Option<&'a OutcomeConfirmation> {
    history.iter().find_map(|(_, e)| match &permitted(e)?.what {
        What::ConfirmOutcome { request, .. } if &request.effect == effect => Some(request),
        _ => None,
    })
}

/// The deposit an arrival is of: the signed transaction its notice names, the notice being its reserve's verified
/// stated entry of `history` under that commitment (`arrival-names-its-notice`).
fn deposit_of<'a>(arrival: &What, history: &'a History) -> Option<&'a Id> {
    let What::Arrival {
        notice: Some(notice),
        reserve,
        ..
    } = arrival
    else {
        return None;
    };
    history.iter().find_map(|(_, e)| {
        let s = stated(e).filter(|s| {
            matches!(s.attribution, Attribution::Verified { .. })
                && &s.commitment == notice
                && maker_provider(s) == reserve
        })?;
        s.about.as_ref()?.transaction.as_ref()
    })
}

/// Where in `history` the permitted `stuck_decide` with `decided`'s request and decision sits, or past its end where
/// none does yet (the decision being admitted).
fn decided_at(decided: (&Id, &Id), history: &History) -> Option<usize> {
    Some(
        history
            .iter()
            .position(|(_, x)| {
                permitted(x).is_some_and(|v| {
                    matches!(&v.what, What::StuckDecide { request, decision }
                        if (&request.request, decision) == decided)
                })
            })
            .unwrap_or(history.len()),
    )
}

/// The transaction a question of Core's names, where `statement` is one: an `operation` its operator, `operator`,
/// made.
fn operator_question<'a>(statement: &'a StatedEntry, operator: &Id) -> Option<&'a Id> {
    (statement.what.statement.as_str() == crate::provider_role::OPERATION_SHAPE
        && maker_provider(statement) == operator)
        .then(|| statement.about.as_ref()?.transaction.as_ref())
        .flatten()
}

/// Whether `transaction` is a stuck resolution's in `history`: a `stuck_decide` names it.
fn resolution_transaction_in(transaction: &Id, history: &History) -> bool {
    history.iter().any(|(_, e)| {
        matches!(verified(e).map(|v| &v.what), Some(What::StuckDecide { request, .. })
            if &request.transaction == transaction)
    })
}

/// A stuck resolution's order, transaction and decision, where `entry` is one: a `stuck_decide`, or a
/// `resolve_stuck` effect, by the decision it carries out.
fn resolution_transaction(entry: &VerifiedEntry) -> Option<(&Id, &Id, (&Id, &Id))> {
    match &entry.what {
        What::StuckDecide { request, decision } => Some((
            &request.order,
            &request.transaction,
            (&request.request, decision),
        )),
        What::Effect {
            action:
                Action::ResolveStuck {
                    order,
                    carries_out,
                    transaction,
                    ..
                },
            ..
        } => Some((
            order,
            transaction,
            (&carries_out.request, &carries_out.decision),
        )),
        _ => None,
    }
}

/// The entry of `history` at `receipt`, where one is.
fn entry_at<'a>(receipt: &Receipt, history: &'a History) -> Option<&'a Entry> {
    history.iter().find(|(r, _)| r == receipt).map(|(_, e)| e)
}

/// Whether the effect `entry` of `history` has an `unanswered` result: its outcome unknown.
fn unanswered(entry: &Entry, history: &History) -> bool {
    position(entry, history).is_some_and(|at| {
        let at = &history[at].0;
        history.iter().any(|(_, x)| {
            verified(x).is_some_and(|r| {
                matches!(&r.what, What::Result { effect } if effect == at)
                    && r.outcome == (Outcome::Unanswered {})
            })
        })
    })
}

/// Whether the effect `entry` of `history` is settled: it has a final result, `done` or `failed`, or a permitted
/// confirmation of its outcome.
fn settled(entry: &Entry, history: &History) -> bool {
    position(entry, history).is_some_and(|at| {
        let at = &history[at].0;
        confirmation_of(at, history).is_some()
            || history.iter().any(|(_, x)| {
                verified(x).is_some_and(|r| {
                    matches!(&r.what, What::Result { effect } if effect == at)
                        && matches!(r.outcome, Outcome::Done {} | Outcome::Failed {})
                })
            })
    })
}

/// The order a move of value moves, and its part, where the log names it: a `move_value`'s by its segment, a
/// `resolve_stuck`'s by the decide step it carries out.
fn moved_part<'a>(
    effect: &'a VerifiedEntry,
    history: &'a History,
) -> Option<(&'a Id, Option<&'a str>)> {
    match effect_action(effect)?.0 {
        Action::MoveValue { order, segment, .. } => Some((
            order,
            part_of_segment(order.as_str(), segment.as_str(), history),
        )),
        Action::ResolveStuck {
            order, carries_out, ..
        } => Some((order, decided_part(order.as_str(), carries_out, history))),
        _ => None,
    }
}

/// Whether `reporter` is the move `effect`'s own mover: for a `move_value`, the actor its permitted `decide` step
/// was for, or one of that actor's accountable party; for a `resolve_stuck`, its permitted `stuck_move`'s mover.
fn moved_by(effect: &VerifiedEntry, reporter: &Actor, history: &History) -> bool {
    let Some((action, permit)) = effect_action(effect) else {
        return false;
    };
    match action {
        Action::MoveValue { .. } => entry_at(permit.receipt(), history)
            .and_then(permitted)
            .is_some_and(|d| match &d.what {
                What::Decide { request, .. } => {
                    &request.caller == reporter || request.caller.party == reporter.party
                }
                _ => false,
            }),
        Action::ResolveStuck { carries_out, .. } => history.iter().any(|(_, x)| {
            permitted(x).is_some_and(|m| {
                matches!(&m.what, What::StuckMove { request: m, .. }
                    if m.carries_out == *carries_out && &m.mover == reporter)
            })
        }),
        _ => false,
    }
}

/// Whether `statement` is about the move `effect` moved: a `move_value`'s segment itself, through that segment's
/// transaction; a stuck resolution's own transaction, the one its decide step named and its effect goes under, a
/// planless part's too.
fn evidence_about(statement: &StatedEntry, effect: &VerifiedEntry, history: &History) -> bool {
    match effect_action(effect).map(|(a, _)| a) {
        Some(Action::MoveValue { order, segment, .. }) => {
            about_the_segment(statement, order, segment, history)
        }
        Some(Action::ResolveStuck {
            order, transaction, ..
        }) => about_the_resolution(statement, order, transaction, history),
        _ => false,
    }
}

/// Whether `statement` is about a stuck resolution of `order` through its own `transaction`: it names the order and
/// that transaction, no segment, and the transaction is first bound to this order and to no segment
/// (`binding_of`).
fn about_the_resolution(
    statement: &StatedEntry,
    order: &Id,
    transaction: &Id,
    history: &History,
) -> bool {
    let Some(about) = &statement.about else {
        return false;
    };
    about.order.as_ref() == Some(order)
        && about.transaction.as_ref() == Some(transaction)
        && about.segment.is_none()
        && binding_of(transaction, history) == Some((order, None))
}

/// Whether `statement` is about `segment` of `order` itself: it names the order and a transaction first bound to that
/// segment of that order (`binding_of`), and, where it names a segment, that one.
fn about_the_segment(statement: &StatedEntry, order: &Id, segment: &Id, history: &History) -> bool {
    let Some(about) = &statement.about else {
        return false;
    };
    about.order.as_ref() == Some(order)
        && about.segment.as_ref().is_none_or(|named| named == segment)
        && about
            .transaction
            .as_ref()
            .and_then(|t| binding_of(t, history))
            .is_some_and(|(o, s)| o == order && s == Some(segment))
}

/// The index of `entry` in `history`: the candidates a row answers are `history`'s own entries.
fn position(entry: &Entry, history: &History) -> Option<usize> {
    history.iter().position(|(_, e)| core::ptr::eq(e, entry))
}

/// What a part row cites a part by: an input about the part, or a move on one of its segments.
enum PartCited<'a> {
    Move(&'a str, &'a str),
}

impl<'a> PartCited<'a> {
    /// The order and part, a move's by the plans `history` holds.
    fn resolve(self, history: &'a History) -> Option<(&'a str, &'a str)> {
        match self {
            PartCited::Move(order, segment) => {
                part_of_segment(order, segment, history).map(|part| (order, part))
            }
        }
    }
}

/// The part `entry` moves value on, where it is a value-moving effect.
fn part_cited(entry: &VerifiedEntry) -> Option<PartCited<'_>> {
    match effect_action(entry) {
        Some((Action::MoveValue { order, segment, .. }, _)) => {
            Some(PartCited::Move(order.as_str(), segment.as_str()))
        }
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

/// The part `entry` would move value from where it sits, where it is one that waits for value in motion: a
/// `route_recalculated`, `destination_changed`, `waiting_at_floor` or `handed_on` of the part, a stuck decision
/// for it, or its resolution's effect.
fn motion_subject<'a>(
    entry: &'a VerifiedEntry,
    history: &'a History,
) -> Option<(&'a str, &'a str)> {
    match &entry.what {
        What::StuckDecide { request, .. } => Some((request.order.as_str(), request.part.as_str())),
        What::Effect {
            action: Action::ResolveStuck {
                order, carries_out, ..
            },
            ..
        } => decided_part(order.as_str(), carries_out, history).map(|p| (order.as_str(), p)),
        _ => {
            let i = input(entry)?;
            matches!(
                i.observed,
                Observed::RouteRecalculated { .. }
                    | Observed::DestinationChanged { .. }
                    | Observed::WaitingAtFloor {}
                    | Observed::HandedOn { .. }
            )
            .then_some(())?;
            Some((i.subject.order.as_str(), i.subject.part.as_ref()?.as_str()))
        }
    }
}

/// Whether `target` moves value of `part` of `order` and is still in motion (`INV-4`, `REQ-ORD-2`): a
/// `move_value` on a segment of the part until its segment settles, is reversed or fails as a deposit; a
/// `resolve_stuck` for the part until its `stuck_resolved` (a delivery) or `handed_on` (a hand-on) is recorded.
/// Either stops with a `failed` result: it never moved. A `done` result alone releases nothing.
fn in_motion(order: &str, part: &str, target: &Entry, history: &History) -> Match {
    let Some(t) = verified(target) else {
        return Match::NotTarget;
    };
    match &t.what {
        What::Effect {
            action: Action::MoveValue {
                order: o, segment, ..
            },
            ..
        } if o.as_str() == order
            && part_of_segment(order, segment.as_str(), history) == Some(part) =>
        {
            let settled = position(target, history)
                .is_some_and(|at| confirmation_of(&history[at].0, history).is_some());
            match standing(order, part, history) {
                Some(s) => judge(
                    !s.released(segment.as_str()) && !result_failed(t, history) && !settled,
                    &[],
                ),
                None => Match::NotTarget,
            }
        }
        What::Effect {
            action:
                Action::ResolveStuck {
                    order: o,
                    carries_out,
                    resolution,
                    ..
                },
            ..
        } if o.as_str() == order && decided_part(order, carries_out, history) == Some(part) => {
            let ended = position(target, history).is_some_and(|at| {
                history[at + 1..].iter().any(|(_, later)| {
                    verified(later).and_then(input).is_some_and(|i| {
                        i.subject.order.as_str() == order
                            && i.subject.part.as_ref().is_some_and(|p| p.as_str() == part)
                            && match resolution {
                                Resolution::Deliver { .. } => {
                                    matches!(i.observed, Observed::StuckResolved { .. })
                                }
                                Resolution::HandOn { .. } => {
                                    matches!(i.observed, Observed::HandedOn { .. })
                                }
                            }
                    })
                })
            });
            let settled = position(target, history)
                .is_some_and(|at| confirmation_of(&history[at].0, history).is_some());
            judge(!ended && !result_failed(t, history) && !settled, &[])
        }
        _ => Match::NotTarget,
    }
}

/// Whether the effect `effect` has a `done` result in `history`.
fn result_done(effect: &VerifiedEntry, history: &History) -> bool {
    let Some((at, _)) = history.iter().find(|(_, e)| verified(e) == Some(effect)) else {
        return false;
    };
    history.iter().any(|(_, e)| {
        verified(e).is_some_and(|r| {
            matches!(&r.what, What::Result { effect } if effect == at)
                && r.outcome == (Outcome::Done {})
        })
    })
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
pub static STATED_RELATIONS: [Relation<StatedCiting>; 3] = [
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
    Relation {
        name: "one-question-per-resolution-transaction",
        spec: &["REQ-AUZ-4", "REQ-ORD-38"],
        quantifier: Quantifier::NoneOther,
        refusal: "Core asks one question under a stuck resolution's transaction: no other of its questions names it",
        fields: &[],
        // A stuck resolution's outcome rests on the answers to Core's question under its own transaction, so a
        // second question would let a second set of answers settle it. The same question sent again is a retry.
        cites: |e, h| match operator_question(&e.entry, &e.operator) {
            Some(transaction) if resolution_transaction_in(transaction, h) => by_fields(0),
            _ => none(),
        },
        answers: |citing, _, t, _| {
            let (Some(transaction), Some(t)) = (
                operator_question(&citing.entry, &citing.operator),
                stated(t),
            ) else {
                return Match::NotTarget;
            };
            judge(
                operator_question(t, &citing.operator) == Some(transaction) && t != &citing.entry,
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
/// A verified entry is read with `operator`, the deployment's operator by its maker and its party
/// ([`DeploymentOperator`]); a stated entry is held to [`STATED_RELATIONS`] alike, with the operator's maker. Before any row, the
/// entry's handle is held to its kind: a decision kind ([`calls::is_decision`](super::calls::is_decision)) only on
/// the policy path, and any other entry only on the general path, so a decision written by anyone but policy is
/// refused even where its type was got round. Before that, the entry is held to its own shape's rules
/// ([`Entry::check`]): one built in memory field by field, which no deserialisation checked, is refused as one read
/// from the wire would be.
pub fn admit_entry(
    history: &[(Receipt, Entry)],
    entry: &Entry,
    through: Through,
    operator: &DeploymentOperator,
) -> Result<(), Violation> {
    // An entry built in memory is held to its shape's rules, as one read from the wire is when it deserialises.
    entry.check()?;
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
            admit_under(&RELATIONS, &citing, history)?;
            admit_under(&CLOSING_RELATIONS, &citing, history)
        }
        Entry::Stated(entry) => {
            let citing = StatedCiting {
                entry: entry.clone(),
                operator: operator.maker.clone(),
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
