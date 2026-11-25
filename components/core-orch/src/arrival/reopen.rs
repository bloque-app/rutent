//! Arrivals rebuilt from the log by a Core that reopens (`T-020-51`, beside `T-020-46`'s rebuild of the rest): Core
//! keeps no store beside its log, so what it held of value arriving is read back from the entries it appended.
//!
//! - **The arrivals** (`Recorded`) and the notices taken (`Noted`): every `arrival` entry names its reserve's notice by
//!   the commitment under which the evidence store keeps it, so the notice is read back from the store, as the log's own
//!   read gives it, and read as `arrive` reads one. A notice the store no longer keeps (erased, `REQ-AUD-13`), or keeps as
//!   another statement, rebuilds nothing: fail closed, the deposit is filed on no more. A notice reported again is the
//!   same arrival, and tells the same outcome.
//! - **The deposits held**: an `arrival` naming an order that cites its `earlier` arrival holds that deposit against the
//!   order (`one-order-per-deposit`). Its deposit is filed on no more, unless the log shows the order's wait ended
//!   (`wait_ended`): that one is free again.
//! - **The waits**: an order the log shows `shown` awaiting its customer, with no acceptance, no start, no refusal and no
//!   `wait_ended` of its own, still waits. Its wait is rebuilt at the step where it waits, its lifetime counted from the
//!   first quote the log shows for it. The quote itself isn't in the log (only its digest), so the customer's
//!   acceptance of the quote the log last shows asks the route again, shown anew, as one past its validity does; the
//!   filer filing the deposit again fills in what the log doesn't hold of the filing (its request identifier, the
//!   objective, the inputs other than the destination's account, the judgements it needs and the session).
//!
//! What it doesn't rebuild: orders started or ended (a repeat of their filing is told the deposit is held, not how it
//! ended), refusals before an order opened, and the reserve's questions under way (`confirming`).

use rutent_contracts::core_internal::calls::KeptStatements;
use rutent_contracts::core_internal::{
    Entry, FILING_INPUTS_SHAPE, FilingInputs, Outcome, Receipt, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Statement;
use rutent_contracts::router_api::Id;

use super::{Arrivals, Arrived, Noted, Recorded, Report, key, read_notice, unbase64url};

impl Arrivals {
    /// The arrivals, the deposits held and the waits for a customer rebuilt from `history`, the log's own entries, as a
    /// Core that reopens reads them (see the module's docs), reading each notice back through `kept`. The lifetime of a
    /// wait is the default until [`Arrivals::set_deposit_wait`] sets it.
    pub fn reopened(history: &[(Receipt, Entry)], kept: &mut dyn KeptStatements) -> Self {
        let mut arrivals = Arrivals::new();
        let verified = || {
            history.iter().filter_map(|(receipt, entry)| match entry {
                Entry::Verified(v) => Some((receipt, v)),
                Entry::Stated(_) => None,
            })
        };
        // The arrivals and the notices, in the order the log took them: a deposit held names an earlier arrival.
        for (receipt, v) in verified() {
            let What::Arrival {
                reserve,
                order,
                matched,
                notice: Some(commitment),
                earlier,
                ..
            } = &v.what
            else {
                continue;
            };
            let Some(read) = read_back(kept, commitment, reserve) else {
                continue;
            };
            let credit = read;
            let recorded = Recorded {
                receipt: receipt.clone(),
                reserve: credit.reserve.clone(),
                node: credit.node.clone(),
                asset: credit.asset.clone(),
                amount: credit.amount.clone(),
                account: credit.account.clone(),
                holder: credit.holder.clone(),
                destination: credit.destination.clone(),
                order: order.clone(),
                matched: *matched,
                notice: commitment.clone(),
            };
            if earlier.is_none() {
                let stated = history.iter().find_map(|(r, e)| match e {
                    Entry::Stated(s) if &s.commitment == commitment => Some((r.clone(), e.clone())),
                    _ => None,
                });
                if let Some((notice_receipt, notice)) = stated {
                    let outcome = match order {
                        None => Arrived::NoOrder {
                            arrival: receipt.clone(),
                        },
                        Some(order) => Arrived::Held {
                            order: order.clone(),
                            matched: *matched,
                            arrival: receipt.clone(),
                        },
                    };
                    arrivals.notices.push(Noted {
                        transaction: credit.transaction.clone(),
                        credit,
                        commitment: commitment.clone(),
                        notice,
                        notice_receipt: Some(notice_receipt),
                        confirming: None,
                        arrival: Some((Entry::Verified(v.clone()), Some(receipt.clone()))),
                        outcome: Some(outcome),
                    });
                }
            }
            arrivals.note(recorded);
        }
        // The deposits held by an order, and the orders still waiting for their customer.
        for (_, v) in verified() {
            let What::Arrival {
                order: Some(order),
                earlier: Some(earlier),
                request,
                inputs,
                ..
            } = &v.what
            else {
                continue;
            };
            let the_log = |wanted: &dyn Fn(&What) -> bool| verified().any(|(_, v)| wanted(&v.what));
            if the_log(&|w| matches!(w, What::WaitEnded { order: o, .. } if o == order)) {
                continue;
            }
            let settled = the_log(&|w| match w {
                What::Acceptance { order: o, .. } | What::OrderRefusal { order: Some(o), .. } => {
                    o == order
                }
                What::Input { input } => input.subject.order.as_str() == order.as_str(),
                _ => false,
            });
            let waits = !settled
                && the_log(
                    &|w| matches!(w, What::Shown { order: o, awaiting: Some(_), .. } if o == order),
                );
            let held = arrivals
                .recorded
                .iter()
                .find(|r| &r.receipt == earlier)
                .cloned();
            let authorisation = verified().rev().find_map(|(_, v)| match &v.what {
                What::Authorise { authorisation, .. }
                    if &authorisation.order == order
                        && matches!(v.outcome, Outcome::Permitted {}) =>
                {
                    Some(authorisation.clone())
                }
                _ => None,
            });
            // The deposit is held against the order whatever comes of it, until a wait's end frees it.
            arrivals.spent.insert(key(earlier));
            if let (true, Some(held), Some(authorisation)) = (waits, held, authorisation) {
                let shown: Vec<(Digest, i128)> = verified()
                    .filter_map(|(_, v)| match &v.what {
                        What::Shown {
                            order: o,
                            shown,
                            awaiting: Some(_),
                            ..
                        } if o == order => Some((shown.clone(), v.when.at.unix_nanos())),
                        _ => None,
                    })
                    .collect();
                if let (Some((_, since)), Some((last, _))) = (shown.first(), shown.last())
                    && let Some(run) = super::filing::Run::rebuilt(
                        held,
                        order,
                        authorisation,
                        *since,
                        last.clone(),
                        read_filing(kept, order, request.as_ref(), inputs.as_ref()),
                    )
                {
                    arrivals.spent.remove(&key(earlier));
                    arrivals.runs.insert(key(earlier), run);
                }
            }
        }
        arrivals
    }
}

/// The filing the operator's statement kept behind `inputs` states for `order`, under the request the arrival names: its
/// inputs and objective. `None` where the arrival names no statement or request, the store keeps none (or keeps other
/// bytes than its commitment names), it isn't a statement of the filing's shape, or it is of another order: the wait is
/// then held, never accepted (`T-020-51`).
fn read_filing(
    kept: &mut dyn KeptStatements,
    order: &Id,
    request: Option<&Id>,
    inputs: Option<&Digest>,
) -> Option<(
    Id,
    std::collections::BTreeMap<String, String>,
    rutent_contracts::router_api::Objective,
)> {
    let (request, commitment) = (request?, inputs?);
    let found = kept.kept(commitment).ok()??;
    if &found.commitment != commitment {
        return None;
    }
    let bytes = unbase64url(found.bytes.as_str())?;
    let statement = serde_json::from_slice::<Statement<FilingInputs>>(&bytes).ok()?;
    (statement.shape == FILING_INPUTS_SHAPE && &statement.statement.order == order).then(|| {
        (
            request.clone(),
            statement.statement.inputs,
            statement.statement.objective,
        )
    })
}

/// The reserve's notice kept behind `commitment`, read back as `arrive` reads one reported by `reserve`.
fn read_back(
    kept: &mut dyn KeptStatements,
    commitment: &Digest,
    reserve: &Id,
) -> Option<super::Credit> {
    let kept = kept.kept(commitment).ok()??;
    if &kept.commitment != commitment {
        return None;
    }
    let bytes = unbase64url(kept.bytes.as_str())?;
    read_notice(&Report {
        reserve: reserve.clone(),
        notice: bytes,
    })
}
