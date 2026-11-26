//! The comparison of the value evidenced at nodes, across orders, with the holding accounts' balances (`INV-1`):
//! a pure function of the readings and of what is evidenced.
//!
//! Like [`super::report`], this module imports data types of the contracts and nothing else, so the check reaches
//! nothing that moves value; a test holds that.

use rutent_contracts::core_internal::{HoldingIncomparable, HoldingReading, Receipt};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id};

use super::holding::{HoldingVerdict, judge, not_comparable};

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
    /// The receipt of the log's entry that evidences it, which a raise about the account cites. The comparison
    /// reads no receipt: it is a value, and reaches no log.
    pub evidence: Receipt,
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
/// account with value evidenced and no reading, since its balance went unread; each that differs or can't be
/// compared carries the difference to raise. A reading's findings come in the readings' order, then the unread
/// accounts in the order the evidence first names them.
pub fn value_evidenced_against_balances(
    readings: &[HoldingReading],
    evidenced: &[Evidenced],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for reading in readings {
        let here = || evidenced.iter().filter(|e| e.account == reading.account);
        let verdict = if here().any(|e| e.asset != reading.asset) {
            not_comparable(
                &reading.account,
                Some((reading.word, &reading.asset)),
                HoldingIncomparable::EvidencedInAnotherAsset,
            )
        } else {
            judge(
                &reading.account,
                &reading.asset,
                reading.word,
                &reading.balance,
                here().map(|e| &e.amount),
            )
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
        verdict: not_comparable(account, None, HoldingIncomparable::NoReading),
    }));
    findings
}
