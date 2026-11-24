//! The stuck orders rebuilt from the log when Core reopens (`T-020-46`, `INV-4`, `INV-8`, `REQ-AUZ-4`).
//!
//! - **The declarations**: every part a `no_route` or `stuck_declared` input left stuck or waiting in the order Core
//!   follows, with its segments and their providers, resolved where the order Core follows reads it so.
//! - **The ends**: each declared order's destination and origin, from the log's own authorisations
//!   ([`Stuck::follow`]).
//! - **What moved**: every `resolve_stuck` effect whose result is `done`, by asset, against the part it moved.
//! - **The kept decisions**: every decide step policy permitted whose decision no `resolve_stuck` effect carries out
//!   yet, kept for its move. One an effect carries out is spent, whatever came of it.
//! - **The moves in motion**: every `resolve_stuck` effect with no final result. Its effect is in the log, so it was
//!   acted on, or may have been: it is never acted on again (the segments' one register, rebuilt from the log), its
//!   mover alone takes it on, and a mover that holds no flight of it answers its outcome unknown, so it is settled by a
//!   report and its confirmation (`AC-6`). An `unanswered` result already logged stands as its first.

use rutent_contracts::core_internal::calls::GeneralEntry;
use rutent_contracts::core_internal::{Receipt, StuckDecide, VerifiedEntry};

use super::*;

impl Stuck {
    /// The stuck orders rebuilt from `history`, the log's own entries, as a Core that reopens reads them (see the
    /// module's docs), over `segments` rebuilt from the same log. The value an unresolved stuck part rests at its
    /// node is held in `exposures` there, as the declaration held it (`REQ-ORD-8`, `T-020-11`).
    pub fn reopened(
        segments: &Segments,
        clock: &dyn Clock,
        history: &[(Receipt, Entry)],
        exposures: &mut Exposures,
    ) -> Self {
        let mut stuck = Stuck::new();
        let entries: Vec<Entry> = history.iter().map(|(_, e)| e.clone()).collect();
        let mut orders: Vec<Id> = Vec::new();
        for (_, entry) in history {
            let Entry::Verified(VerifiedEntry {
                what: What::Input { input },
                ..
            }) = entry
            else {
                continue;
            };
            if !matches!(
                input.observed,
                Observed::NoRoute { .. } | Observed::StuckDeclared { .. }
            ) {
                continue;
            }
            let (Some(order), Some(part)) = (
                Id::new(input.subject.order.as_str()),
                input
                    .subject
                    .part
                    .as_ref()
                    .and_then(|p| Id::new(p.as_str())),
            ) else {
                continue;
            };
            let Some(followed) = segments.order(&order) else {
                continue;
            };
            let Some(now_part) = followed
                .parts
                .iter()
                .find(|p| p.part_id.as_str() == part.as_str())
            else {
                continue;
            };
            let parts_segments = now_part
                .segments
                .iter()
                .filter_map(|s| {
                    Some((
                        Id::new(s.segment_id.as_str())?,
                        Id::new(s.provider.as_str())?,
                    ))
                })
                .collect();
            let resolved = now_part.resolved_to.is_some() || now_part.state == OrderState::HandedOn;
            let state = if resolved {
                OrderState::Stuck
            } else {
                now_part.state
            };
            if !matches!(state, OrderState::Stuck | OrderState::WaitingForCustomer) {
                continue;
            }
            if state == OrderState::Stuck && !resolved {
                if let Some((segment, provider, asset, amount)) =
                    resting(segments, exposures, &order, &part)
                {
                    let _ = exposures.hold_resting(&order, &segment, &provider, &asset, &amount);
                }
                exposures.mark_stuck(&order);
            }
            stuck.declared.insert(
                key(&order, &part),
                Declared {
                    state,
                    since: clock.now(),
                    segments: parts_segments,
                    moved: Vec::new(),
                    in_motion: None,
                    resolved,
                },
            );
            if !orders.contains(&order) {
                orders.push(order);
            }
        }
        for order in &orders {
            let _ = stuck.follow(segments, &entries, order);
        }
        stuck.rebuild_moves(history);
        stuck
    }

    /// The kept decisions, what moved and the moves in motion, from the log's decide and move steps, effects and
    /// results.
    fn rebuild_moves(&mut self, history: &[(Receipt, Entry)]) {
        let verified = || {
            history.iter().filter_map(|(receipt, entry)| match entry {
                Entry::Verified(v) => Some((receipt, v)),
                Entry::Stated(_) => None,
            })
        };
        // Each `resolve_stuck` effect, by the decide step's request it carries out.
        let effects: Vec<(&Receipt, &Action, &Permit)> = verified()
            .filter_map(|(receipt, v)| match &v.what {
                What::Effect {
                    action: action @ Action::ResolveStuck { .. },
                    decision,
                } => Some((receipt, action, decision)),
                _ => None,
            })
            .collect();
        let results = |effect: &Receipt| -> Vec<(&Receipt, Outcome)> {
            verified()
                .filter_map(|(receipt, v)| match &v.what {
                    What::Result { effect: e } if e == effect => Some((receipt, v.outcome)),
                    _ => None,
                })
                .collect()
        };
        for (_, v) in verified() {
            let What::StuckDecide { request, decision } = &v.what else {
                continue;
            };
            if !matches!(v.outcome, Outcome::Permitted {}) {
                continue;
            }
            let spent = effects.iter().any(|(_, action, _)| {
                carried_out_of(action).is_some_and(|d| d.request == request.request)
            });
            if !spent {
                self.kept.insert(
                    request.request.as_str().to_owned(),
                    Kept {
                        request: StuckDecide::clone(request),
                        part: request.part.clone(),
                        decision: decision.clone(),
                    },
                );
            }
        }
        for (receipt, action, permit) in effects {
            let Some(decided) = carried_out_of(action) else {
                continue;
            };
            let Some(decide) = verified().find_map(|(_, v)| match &v.what {
                What::StuckDecide { request, .. } if request.request == decided.request => {
                    Some(request)
                }
                _ => None,
            }) else {
                continue;
            };
            let part = key(&decide.order, &decide.part);
            let outcomes = results(receipt);
            let done = outcomes.iter().any(|(_, o)| *o == Outcome::Done {});
            let failed = outcomes.iter().any(|(_, o)| *o == Outcome::Failed {});
            if done {
                if let (Some((asset, amount)), Some(declared)) =
                    (moved_by(action), self.declared.get_mut(&part))
                {
                    add_moved(&mut declared.moved, &asset, &amount);
                }
                continue;
            }
            if failed {
                continue;
            }
            // In motion: its effect is in the log and no final result follows it.
            let Some(mover) = verified().find_map(|(_, v)| match &v.what {
                What::StuckMove { request, .. }
                    if matches!(v.outcome, Outcome::Permitted {})
                        && request.carries_out.request == decided.request =>
                {
                    Some(request.mover.clone())
                }
                _ => None,
            }) else {
                continue;
            };
            let Some(effect_entry) = history
                .iter()
                .find(|(r, _)| r == receipt)
                .and_then(|(_, e)| GeneralEntry::try_from(e.clone()).ok())
            else {
                continue;
            };
            let unanswered = outcomes
                .iter()
                .find(|(_, o)| *o == Outcome::Unanswered {})
                .and_then(|(r, _)| {
                    let entry = history.iter().find(|(x, _)| x == *r)?.1.clone();
                    Some(Step {
                        entry: GeneralEntry::try_from(entry).ok()?,
                        receipt: Some((*r).clone()),
                    })
                });
            let at = decided.request.as_str().to_owned();
            if let Some(declared) = self.declared.get_mut(&part) {
                declared.in_motion = Some(at.clone());
            }
            self.moving.insert(
                at,
                InMotion {
                    order: decide.order.clone(),
                    part: decide.part.clone(),
                    mover,
                    reported: None,
                    reports: 0,
                    declined: Vec::new(),
                    done_on: None,
                    resolved: unanswered.as_ref().map(|_| Resolved::Unknown),
                    unanswered,
                    dropped: None,
                    effect: Step {
                        entry: effect_entry,
                        receipt: Some(receipt.clone()),
                    },
                    acted: true,
                    action: action.clone(),
                    permit: permit.clone(),
                    result: None,
                },
            );
        }
    }
}
