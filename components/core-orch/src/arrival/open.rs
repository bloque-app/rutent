//! The orders open at a reserve, Core's own (`T-020-61`, item 10; the review of #355, M3): derived from what intake started
//! and what the log took, never kept by the deployment. An order a customer requested through intake that started is open
//! at its origin reserve for each part whose first segment hasn't settled, with its payer as the holder of the account the
//! value leaves, its session, and the part's amount where its route has more than one part. The one thing only the
//! deployment knows is which account a session speaks for, at the reserve (`REQ-CAT-3`): it is asked of it.

use std::collections::BTreeSet;

use rutent_contracts::core_internal::{Entry, Receipt, VerifiedEntry, What};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::Observed;

use super::{OpenOrder, OpenOrders};
use crate::intake::Intake;

/// The orders open at a reserve as Core derives them from `intake` and the log's `history`; `account_of` answers the
/// account at the reserve a session speaks for, the deployment's register of sessions.
pub struct OpenAt<'a> {
    /// Intake, which holds what each order it opened started on.
    pub intake: &'a Intake,
    /// The log's entries.
    pub history: &'a [(Receipt, Entry)],
    /// The account a session speaks for at the reserve, as the deployment's register of sessions holds it.
    pub account_of: &'a dyn Fn(&Id) -> Option<Id>,
}

impl OpenAt<'_> {
    /// Every order open at any reserve (see the module's docs).
    pub fn all(&self) -> Vec<OpenOrder> {
        let settled: BTreeSet<String> = self
            .history
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what: What::Input { input },
                    ..
                }) if matches!(input.observed, Observed::SegmentSettled { .. }) => input
                    .subject
                    .segment
                    .as_ref()
                    .map(|s| s.as_str().to_owned()),
                _ => None,
            })
            .collect();
        let mut open = Vec::new();
        for order in self.intake.started_orders() {
            let (Some(started), Some(request)) =
                (self.intake.started(&order), self.intake.request_of(&order))
            else {
                continue;
            };
            let NodeRef::Outer(node) = &request.route.from.node else {
                continue;
            };
            let Some(account) = (self.account_of)(&request.session) else {
                continue;
            };
            let split = started.plan.len() > 1;
            let chosen = started.record.chosen();
            for (at, part) in started.plan.iter().enumerate() {
                let Some(first) = part.segments.first() else {
                    continue;
                };
                if settled.contains(first.segment_id.as_str()) {
                    continue;
                }
                let amount = if split {
                    match chosen.and_then(|c| c.parts.get(at)) {
                        Some(p) => p.operations.first().amount_in.clone(),
                        None => continue,
                    }
                } else {
                    request.route.amount.clone()
                };
                open.push(OpenOrder {
                    order: order.clone(),
                    reserve: request.origin_reserve.clone(),
                    node: node.clone(),
                    asset: request.route.from.asset.clone(),
                    account: account.clone(),
                    holder: request.parties.payer.clone(),
                    amount,
                    session: request.session.clone(),
                    part: split.then(|| Id::new(part.part_id.as_str())).flatten(),
                });
            }
        }
        open
    }
}

impl OpenOrders for OpenAt<'_> {
    fn open_at(&mut self, reserve: &Id, account: &Id) -> Vec<OpenOrder> {
        self.all()
            .into_iter()
            .filter(|o| &o.reserve == reserve && &o.account == account)
            .collect()
    }
}

/// The orders open at a reserve, as [`OpenAt::all`] derived them before the arrivals were lent: the same filter, over
/// orders already derived.
pub struct Listed(pub Vec<OpenOrder>);

impl OpenOrders for Listed {
    fn open_at(&mut self, reserve: &Id, account: &Id) -> Vec<OpenOrder> {
        self.0
            .iter()
            .filter(|o| &o.reserve == reserve && &o.account == account)
            .cloned()
            .collect()
    }
}
