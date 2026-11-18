//! The plain wallet (`T-016-02`, `F-016`): a public node's holding account, by default (`REQ-CAT-23`), as a module
//! outside the core. Its sandboxed part keeps the record of what rests in the account for each order, in the
//! instance, for as long as the provider's process lives, and speaks the provider-role contract's reserve
//! operations, in the contract's own types:
//!
//! - `deposit`: value resting at the account's node is placed in it, for the order named in the operation's
//!   `order` input. The operation's `origin` is the node; its `destination` network is the account's own name.
//! - `withdrawal`: value leaves the account to its node, for an order, up to what the order holds. The operation's
//!   `destination` is the node.
//! - `balance_verification`: whether the account of an order, named by `account`, holds at least the stated amount.
//!
//! - `holding_balance`: the balance the account holds at its node, in whole units of its asset, which is the sum of
//!   what its orders hold. The operator asks it so the balance is the wallet's own word (`REQ-AUD-15`).
//!
//! The account is one node's and one asset's: the first movement that succeeds fixes both, and an operation naming
//! another is refused. A refused operation fixes nothing. A request that is not well formed, a dry run, a zero amount or a release above what an order
//! holds is refused as final, and changes nothing. Its outside part has nothing to do: a plain wallet declares no
//! outside operation, destination or secret.
//!
//! The parts are chosen by a feature (see `Cargo.toml`).

#[cfg(all(feature = "sandboxed", feature = "outside"))]
compile_error!("choose one part: `sandboxed` or `outside`");

#[cfg(feature = "sandboxed")]
mod sandboxed {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use rutent_contracts::provider_role::{
        BalanceAnswer, BalanceOutcome, BalanceVerification, BalanceVerificationResult, Deposit,
        EvidenceGrade, EvidencePiece, Fee, HoldingBalance, HoldingBalanceAnswer,
        HoldingBalanceOutcome, HoldingBalanceResult, MovementSettlement, NoDetail, Operation,
        Refusal, Refused, ReserveMovementOutcome, ReserveMovementResult, ResultBody, Statement,
        Withdrawal,
    };

    wit_bindgen::generate!({ world: "sandboxed", path: "../../src/module/wit" });

    /// What rests in the account.
    #[derive(Default)]
    struct Wallet {
        /// The node and asset the account was fixed to by its first operation.
        fixed: Option<(String, String)>,
        /// What each order holds, in whole units of the asset.
        held: BTreeMap<String, u128>,
    }

    thread_local! {
        static WALLET: RefCell<Wallet> = RefCell::new(Wallet::default());
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
            let held = self.held.get(order).copied().unwrap_or(0);
            if m.deposit {
                let total: u128 = self.held.values().sum();
                if total.checked_add(units).is_none() {
                    return refused();
                }
                self.held.insert(order.clone(), held + units);
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

        fn deposit(&mut self, d: &Deposit) -> ResultBody {
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

        fn withdrawal(&mut self, w: &Withdrawal) -> ResultBody {
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

        fn balance(&self, h: &HoldingBalance) -> ResultBody {
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

        fn verification(&self, v: &BalanceVerification) -> ResultBody {
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

    struct Part;

    impl Guest for Part {
        fn start() -> Result<(), String> {
            Ok(())
        }

        fn handle(operation: String) -> Vec<u8> {
            let Ok(operation) = serde_json::from_str::<Statement<Operation>>(&operation) else {
                return b"not an operation".to_vec();
            };
            let body = WALLET.with(|w| {
                let mut wallet = w.borrow_mut();
                match &operation.statement {
                    Operation::Deposit(d) => Some(wallet.deposit(d)),
                    Operation::Withdrawal(w) => Some(wallet.withdrawal(w)),
                    Operation::BalanceVerification(v) => Some(wallet.verification(v)),
                    Operation::HoldingBalance(h) => Some(wallet.balance(h)),
                    _ => None,
                }
            });
            match body {
                Some(body) => serde_json::to_vec(&body).unwrap_or_default(),
                None => b"not handled".to_vec(),
            }
        }
    }

    export!(Part);
}

#[cfg(feature = "outside")]
mod outside {
    wit_bindgen::generate!({ world: "outside", path: "../../src/module/wit" });

    struct Part;

    impl Guest for Part {
        fn start() -> Result<(), String> {
            Ok(())
        }

        /// A plain wallet's sandboxed part asks for nothing outside, so nothing is performed.
        fn perform(_outside_request: String) -> String {
            "{}".to_owned()
        }
    }

    export!(Part);
}
