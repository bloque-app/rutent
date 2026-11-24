//! The plain wallet's record and the way it answers the provider-role contract's reserve operations: the part of the
//! sandboxed module that holds no host binding, so the platform's tests can run it natively beside the Wasm
//! build (`tests/holding_wallet_record.rs`).

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::provider_role::{
    BalanceAnswer, BalanceOutcome, BalanceVerification, BalanceVerificationResult, Deposit,
    EvidenceGrade, EvidencePiece, Fee, HoldingBalance, HoldingBalanceAnswer, HoldingBalanceOutcome,
    HoldingBalanceResult, MovementSettlement, NoDetail, Refusal, Refused, ReserveMovementOutcome,
    ReserveMovementResult, ResultBody, Unknown, Withdrawal,
};

/// What rests in the account.
#[derive(Default)]
pub struct Wallet {
    /// The node and asset the account was fixed to by its first operation.
    fixed: Option<(String, String)>,
    /// What each order holds, in whole units of the asset.
    held: BTreeMap<String, u128>,
    /// Every order this instance took a landing for, held still or released in full. An order not here is one the
    /// wallet has no record of: its instance began after the value landed (a restart, `REQ-ORD-43`).
    known: BTreeSet<String>,
    /// The landings it carried out, by their transaction: for which order and how much. A landing repeated under
    /// the same transaction is the one it was, answered done and adding nothing (`INV-1`).
    landings: BTreeMap<String, (String, u128)>,
}

/// A whole number of units: digits only, no sign and no leading zero.
fn amount(text: &str) -> Option<u128> {
    let plain = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if plain { text.parse().ok() } else { None }
}

fn refused() -> ReserveMovementOutcome {
    ReserveMovementOutcome::Refused(Refused {
        refusal: Refusal::Final(NoDetail {}),
    })
}

fn settled(
    provider: &str,
    kind: &str,
    transaction: &str,
    units: u128,
    asset: &str,
) -> ReserveMovementOutcome {
    ReserveMovementOutcome::Succeeded(MovementSettlement {
        amount_in: units.to_string(),
        amount_out: units.to_string(),
        fee: Fee {
            asset: asset.to_owned(),
            amount: "0".to_owned(),
            billed_separately: None,
        },
        evidence: vec![EvidencePiece {
            grade: EvidenceGrade::SignedByProvider,
            reference: format!("{provider}-{kind}-{transaction}"),
            by: provider.to_owned(),
        }],
    })
}

/// One movement asked of the account, whichever way it goes.
struct Movement<'a> {
    deposit: bool,
    provider: &'a str,
    transaction: &'a str,
    node: &'a str,
    asset: &'a str,
    order: Option<&'a String>,
    amount: &'a str,
    dry_run: bool,
}

impl Wallet {
    /// Whether `node` and `asset` are the account's: the ones its first successful operation fixed, or any while
    /// none has.
    fn is_for(&self, node: &str, asset: &str) -> bool {
        match &self.fixed {
            Some((n, a)) => n == node && a == asset,
            None => true,
        }
    }

    fn movement(&mut self, m: Movement) -> ReserveMovementOutcome {
        let (Some(order), Some(units)) = (m.order, amount(m.amount)) else {
            return refused();
        };
        if m.dry_run || units == 0 || !self.is_for(m.node, m.asset) {
            return refused();
        }
        if m.deposit {
            if let Some((landed_for, landed)) = self.landings.get(m.transaction) {
                // A repeat of a landing it carried out: the same landing, answered as it was, and nothing added. The
                // same transaction for another order or amount is no repeat.
                return if landed_for == order && *landed == units {
                    settled(m.provider, "deposit", m.transaction, units, m.asset)
                } else {
                    refused()
                };
            }
        } else if !self.known.contains(order) {
            // No record of the order: not a refusal, since the value may be in the account and only the record is
            // lost. The order is not known, and the host lands it again from its own log before it releases.
            return ReserveMovementOutcome::Unknown(Unknown {});
        }
        let held = self.held.get(order).copied().unwrap_or(0);
        if m.deposit {
            let total: u128 = self.held.values().sum();
            if total.checked_add(units).is_none() {
                return refused();
            }
            self.held.insert(order.clone(), held + units);
            self.known.insert(order.clone());
            self.landings
                .insert(m.transaction.to_owned(), (order.clone(), units));
        } else if units > held {
            return refused();
        } else if units == held {
            self.held.remove(order);
        } else {
            self.held.insert(order.clone(), held - units);
        }
        // Only a movement that went through fixes the account to its node and asset: a refused first operation
        // leaves it free.
        self.fixed
            .get_or_insert_with(|| (m.node.to_owned(), m.asset.to_owned()));
        let kind = if m.deposit { "deposit" } else { "withdrawal" };
        settled(m.provider, kind, m.transaction, units, m.asset)
    }

    pub fn deposit(&mut self, d: &Deposit) -> ResultBody {
        let outcome = self.movement(Movement {
            deposit: true,
            provider: &d.provider,
            transaction: &d.transaction,
            node: &d.origin,
            asset: &d.asset,
            order: d.inputs.get("order"),
            amount: &d.amount,
            dry_run: d.dry_run,
        });
        ResultBody::Deposit(ReserveMovementResult {
            transaction: d.transaction.clone(),
            outcome,
        })
    }

    pub fn withdrawal(&mut self, w: &Withdrawal) -> ResultBody {
        let outcome = self.movement(Movement {
            deposit: false,
            provider: &w.provider,
            transaction: &w.transaction,
            node: &w.destination,
            asset: &w.asset,
            order: w.inputs.get("order"),
            amount: &w.amount,
            dry_run: w.dry_run,
        });
        ResultBody::Withdrawal(ReserveMovementResult {
            transaction: w.transaction.clone(),
            outcome,
        })
    }

    pub fn balance(&self, h: &HoldingBalance) -> ResultBody {
        let outcome = if self.is_for(&h.node, &h.asset) {
            HoldingBalanceOutcome::Answered(HoldingBalanceAnswer {
                balance: self.held.values().sum::<u128>().to_string(),
            })
        } else {
            HoldingBalanceOutcome::Refused(Refused {
                refusal: Refusal::Final(NoDetail {}),
            })
        };
        ResultBody::HoldingBalance(HoldingBalanceResult {
            transaction: h.transaction.clone(),
            outcome,
        })
    }

    pub fn verification(&self, v: &BalanceVerification) -> ResultBody {
        let outcome = match (amount(&v.amount), &self.fixed) {
            (Some(units), Some((_, asset))) if *asset == v.asset => {
                BalanceOutcome::Answered(BalanceAnswer {
                    sufficient: self.held.get(&v.account).copied().unwrap_or(0) >= units,
                })
            }
            // Nothing has landed yet, so no asset is fixed and nothing is held: only a stated nothing is covered.
            (Some(units), None) => BalanceOutcome::Answered(BalanceAnswer {
                sufficient: units == 0,
            }),
            _ => BalanceOutcome::Refused(rutent_contracts::provider_role::BalanceRefused {
                refusal: rutent_contracts::provider_role::BalanceRefusal::Final(NoDetail {}),
            }),
        };
        ResultBody::BalanceVerification(BalanceVerificationResult {
            transaction: v.transaction.clone(),
            outcome,
        })
    }
}
