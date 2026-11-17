//! The comparison of the value evidenced at nodes, across orders, with the holding accounts' balances (`INV-1`):
//! a pure function of the readings and of what is evidenced.
//!
//! Like [`super::report`], this module imports data types of the contracts and nothing else, so the check reaches
//! nothing that moves value; a test holds that.

use rutent_contracts::core_internal::HoldingReading;
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id};

use super::Incomparable;
use super::holding::{HoldingVerdict, judge, sum};

/// Value an order's evidence puts at a holding account: what the log's records of the order show resting there,
/// read by whoever feeds the check. It is Core's own figure, apart from what the account says it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidenced {
    /// The holding account.
    pub account: Id,
    /// The order whose evidence it is.
    pub order: Id,
    /// The asset, in the unit the evidence states.
    pub asset: Denomination,
    /// The amount evidenced.
    pub amount: Amount,
}

/// What comparing one account's evidenced value with its balance finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The holding account.
    pub account: Id,
    /// What was found.
    pub verdict: HoldingVerdict,
}

/// Compares, for each reading, the balance with the sum evidenced at its account across all orders; and notes each
/// account with value evidenced and no reading, since its balance went unread. A reading's findings come in the
/// readings' order, then the unread accounts in the order the evidence first names them.
pub fn value_evidenced_against_balances(
    readings: &[HoldingReading],
    evidenced: &[Evidenced],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for reading in readings {
        let here = || evidenced.iter().filter(|e| e.account == reading.account);
        let verdict = if here().any(|e| e.asset != reading.asset) {
            HoldingVerdict::NotComparable(Incomparable::EvidencedInAnotherAsset)
        } else {
            match sum(here().map(|e| &e.amount)) {
                Some(total) => judge(
                    &reading.account,
                    &reading.asset,
                    reading.word,
                    &reading.balance,
                    &total,
                ),
                None => HoldingVerdict::NotComparable(Incomparable::SumPastTheBound),
            }
        };
        findings.push(Finding {
            account: reading.account.clone(),
            verdict,
        });
    }
    let mut unread: Vec<&Id> = Vec::new();
    for e in evidenced {
        if !readings.iter().any(|r| r.account == e.account) && !unread.contains(&&e.account) {
            unread.push(&e.account);
        }
    }
    findings.extend(unread.into_iter().map(|account| Finding {
        account: account.clone(),
        verdict: HoldingVerdict::NotComparable(Incomparable::NoReading),
    }));
    findings
}
