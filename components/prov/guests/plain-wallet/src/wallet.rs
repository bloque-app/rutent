//! The plain wallet's record and the way it answers the provider-role contract's reserve operations: the part of the
//! sandboxed module that holds no host binding, so the platform's tests can run it natively beside the Wasm
//! build (`tests/holding_wallet_record.rs`).

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::provider_role::{
    BalanceAnswer, BalanceOutcome, BalanceVerification, BalanceVerificationResult, Confirmed,
    Deposit, EvidenceGrade, EvidencePiece, Fee, HoldingBalance, HoldingBalanceAnswer,
    HoldingBalanceOutcome, HoldingBalanceResult, MovementSettlement, NoDetail, Operation,
    REVERSAL_CONFIRMATION_SHAPE, Refusal, Refused, ReserveMovementOutcome, ReserveMovementResult,
    ResultBody, ReversalConfirmation, ReversalConfirmationOutcome, ReversalConfirmationRequest,
    ReversalConfirmationResult, Signature, SignatureScheme, Statement, Unknown, Withdrawal,
};

/// The prefix of the provider's own state keys under which the account's statement of credits is kept, one per
/// reversed segment: `credit/{segment's transaction}` holds the JSON `{"asset": …, "amount": …}` of the credit that
/// came back into the account for that segment's reversal (`T-020-57`). The wallet's outside part reads it; the
/// sandboxed part reads no state. The outside part, built without this file, keeps the same text.
#[allow(dead_code)]
pub const CREDIT_PREFIX: &str = "credit/";

/// The outside operation the sandboxed part asks its outside part for the account's statement of one credit.
pub const CREDIT: &str = "wallet.credit";

/// A credit the account's statement says came back for a reversed segment: in which asset, and how much.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    /// The asset.
    pub asset: String,
    /// The amount, in whole units.
    pub amount: String,
}

impl Credit {
    /// The credit an entry of the account's statement states, as the outside part read it: `None` where it states
    /// nothing readable.
    pub fn read(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        Some(Credit {
            asset: value.get("asset")?.as_str()?.to_owned(),
            amount: value.get("amount")?.as_str()?.to_owned(),
        })
    }
}

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

    /// The answer to Core's question whether the credit of a reversed segment came back into the account
    /// (`reversal_confirmation`, `T-020-57`): confirmed only where the account's statement holds a credit for that very
    /// segment, in the asset asked and of the amount asked, in full; otherwise not confirmed, which says nothing else.
    /// A credit short of what is asked is not confirmed, so less returned never completes the reversal (`REQ-ORD-3`).
    /// A malformed or zero amount, or an asset the account isn't fixed to, is refused as final. The confirmation is
    /// nested in the account's own name, in the deployment and protocol version of `asked`, for the platform to sign.
    pub fn confirmation(
        &self,
        asked: &Statement<Operation>,
        c: &ReversalConfirmationRequest,
        credit: Option<&Credit>,
    ) -> ResultBody {
        let units = amount(&c.amount).filter(|u| *u > 0);
        let fixed_elsewhere = self.fixed.as_ref().is_some_and(|(_, a)| *a != c.asset);
        let outcome = match units {
            None => ReversalConfirmationOutcome::Refused(Refused {
                refusal: Refusal::Final(NoDetail {}),
            }),
            Some(_) if fixed_elsewhere => ReversalConfirmationOutcome::Refused(Refused {
                refusal: Refusal::Final(NoDetail {}),
            }),
            Some(units) => {
                let arrived = credit.is_some_and(|credit| {
                    credit.asset == c.asset && amount(&credit.amount) == Some(units)
                });
                if arrived {
                    ReversalConfirmationOutcome::Confirmed(Confirmed {
                        confirmation: Box::new(Statement {
                            deployment: asked.deployment.clone(),
                            protocol_version: asked.protocol_version.clone(),
                            shape: REVERSAL_CONFIRMATION_SHAPE.to_owned(),
                            maker: c.provider.clone(),
                            made_at: asked.made_at.clone(),
                            // The platform signs it in the account's name; the module holds no key.
                            signature: Signature {
                                scheme: SignatureScheme::Ed25519,
                                key: asked.signature.key.clone(),
                                value: asked.signature.value.clone(),
                            },
                            statement: ReversalConfirmation {
                                segment: c.segment.clone(),
                                asset: c.asset.clone(),
                                amount: c.amount.clone(),
                            },
                        }),
                    })
                } else {
                    ReversalConfirmationOutcome::NotConfirmed(NoDetail {})
                }
            }
        };
        ResultBody::ReversalConfirmation(ReversalConfirmationResult {
            transaction: c.transaction.clone(),
            outcome,
        })
    }
}
