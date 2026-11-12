//! The citation table's rows for evidence and closing (`T-020-09`): a part's final deposit settled only on a public
//! record and its reserve's verified statement, one public movement evidencing one segment, and a replay resting on its
//! exit's result. Kept apart from the rest of the table so the rows other tasks add don't meet these; [`admit_entry`]
//! reads them after [`RELATIONS`](super::RELATIONS).
//!
//! [`admit_entry`]: super::admit_entry

use super::*;

/// Whether `segment` of `order` is a part's last, a deposit, in a plan `history` holds: a `route_chosen`, a
/// `route_recalculated` or a `destination_changed`. Where the part closes (`REQ-ORD-33`).
fn final_deposit(order: &str, segment: &str, history: &History) -> bool {
    use crate::state_machine::{OperationKind, SegmentPlan};
    let last = |plan: &[SegmentPlan]| {
        plan.last()
            .is_some_and(|s| s.segment_id.as_str() == segment && s.kind == OperationKind::Deposit)
    };
    history.iter().any(|(_, e)| {
        let Some(i) = verified(e).and_then(input) else {
            return false;
        };
        if i.subject.order.as_str() != order {
            return false;
        }
        match &i.observed {
            Observed::RouteChosen { parts, .. } => parts.iter().any(|p| last(&p.segments)),
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. } => last(segments),
            _ => false,
        }
    })
}

/// The network of the destination the `move_value` effect for `segment` of `order` names: where its route ends, as
/// policy held it against the authorisation (`move-rests-on-its-authorisation`).
fn authorised_network<'a>(order: &Id, segment: &Id, history: &'a History) -> Option<&'a str> {
    history
        .iter()
        .find_map(|(_, e)| match effect_action(verified(e)?)? {
            (
                Action::MoveValue {
                    order: o,
                    segment: s,
                    route,
                    ..
                },
                _,
            ) if o == order && s == segment => Some(route.destination.node.network.as_str()),
            _ => None,
        })
}

/// The `segment_settled` input `entry` records: the evidence it cites, its order and its segment.
type Settled<'a> = (
    &'a crate::state_machine::Id,
    &'a crate::state_machine::Id,
    &'a crate::state_machine::Id,
);

fn settled(entry: &VerifiedEntry) -> Option<Settled<'_>> {
    let i = input(entry)?;
    match &i.observed {
        Observed::SegmentSettled { evidence } => {
            Some((evidence, &i.subject.order, i.subject.segment.as_ref()?))
        }
        _ => None,
    }
}

/// The public movement graded evidence names, at whatever grade, with its order and segment.
fn relied_on(entry: &VerifiedEntry) -> Option<(&PublicMovement, &Id, &Id)> {
    match &entry.what {
        What::Evidence {
            movement: Some(movement),
            order,
            segment,
            ..
        } => Some((movement, order, segment)),
        _ => None,
    }
}

/// The rows, in the order the engine reads them. `contracts/core-internal/README.md` lists them with the rest.
pub static CLOSING_RELATIONS: [Relation; 3] = [
    Relation {
        name: "final-deposit-closes-on-a-public-record",
        spec: &["REQ-ORD-33", "REQ-ORD-39"],
        // None other: the evidence a part's final deposit settles on is found by `settlement-rests-on-its-evidence`
        // first, so this row refuses where it is found and falls short of a close.
        quantifier: Quantifier::NoneOther,
        refusal: "a part's final deposit settles only on a public record of a movement on its authorised destination's network, or into it where it is a private rail, naming the movement, resting on its reserve's verified statement: never a statement alone",
        fields: &[],
        cites: |e, h| match settled(e) {
            Some((_, order, segment)) if final_deposit(order.as_str(), segment.as_str(), h) => {
                by_fields(0)
            }
            _ => none(),
        },
        answers: |e, _, t, h| {
            let Some((cited, order, segment)) = settled(e) else {
                return Match::NotTarget;
            };
            let Some(What::Evidence {
                evidence,
                order: o,
                segment: s,
                grade,
                rests_on,
                movement,
                ..
            }) = verified(t).map(|t| &t.what)
            else {
                return Match::NotTarget;
            };
            if evidence.as_str() != cited.as_str()
                || o.as_str() != order.as_str()
                || s.as_str() != segment.as_str()
            {
                return Match::NotTarget;
            }
            // A deposit to a rail declared private closes on the movement into it, which names the rail as its
            // `node`: the log can't tell the rail is declared private, a residual for `T-025-05`.
            // Into a rail declared private, the movement names the rail, which is the destination's network.
            let authorised = authorised_network(o, s, h);
            let network = movement.as_ref().is_some_and(|m| match &m.node {
                Some(node) => authorised == Some(node.as_str()),
                None => authorised == Some(m.network.as_str()),
            });
            let provider = segment_provider(o, s, h);
            let reserve = rests_on.iter().any(|r| {
                h.iter().any(|(receipt, entry)| {
                    receipt == r
                        && stated(entry).is_some_and(|st| {
                            matches!(st.attribution, Attribution::Verified { .. })
                                && Some(maker_provider(st)) == provider
                        })
                })
            });
            let closes = *grade == crate::provider_role::EvidenceGrade::PublicRecord
                && movement.is_some()
                && network
                && reserve;
            judge(!closes, &[])
        },
    },
    Relation {
        name: "movement-evidences-one-segment",
        spec: &["REQ-ORD-38", "REQ-ORD-33"],
        quantifier: Quantifier::NoneOther,
        refusal: "one public movement evidences one segment of one order: another relied on it already",
        fields: &[],
        cites: |e, _| relied_on(e).map_or_else(none, |_| by_fields(0)),
        answers: |e, _, t, _| {
            let (Some((movement, order, segment)), Some((other, o, s))) =
                (relied_on(e), verified(t).and_then(relied_on))
            else {
                return Match::NotTarget;
            };
            // A node's movements in and out evidence each segment inside it, of its one order (`REQ-ORD-33`): only
            // another order, or another segment where neither names the node, relies on it a second time.
            let node_level = other.node.is_some() || movement.node.is_some();
            judge(
                other.network == movement.network
                    && other.id == movement.id
                    && (o != order || (s != segment && !node_level)),
                &[],
            )
        },
    },
    Relation {
        name: "replay-rests-on-its-exit",
        spec: &["REQ-ORD-41", "REQ-AUD-2"],
        quantifier: Quantifier::Exists,
        refusal: "a replay rests on its exit segment's result, a stated entry of this log",
        fields: &[
            ("order", "a replay's exit result is about its order"),
            (
                "segment",
                "a replay's exit result is about its exit segment",
            ),
        ],
        cites: |e, _| match &e.what {
            What::Replay { rests_on, .. } => alloc::vec![by_receipt(rests_on, 0)],
            _ => none(),
        },
        answers: |e, _, t, _| {
            let (What::Replay { order, segment, .. }, Some(s)) = (&e.what, stated(t)) else {
                return Match::NotTarget;
            };
            let about = s.about.as_ref();
            judge(
                true,
                &[
                    about.and_then(|a| a.order.as_ref()) == Some(order),
                    about.and_then(|a| a.segment.as_ref()) == Some(segment),
                ],
            )
        },
    },
];
