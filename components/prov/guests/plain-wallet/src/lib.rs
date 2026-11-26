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
//! The record lives in the instance, so a restart of the account's module empties it (a module's state is read-only
//! through the host, and the wallet holds none outside its instance). It is rebuilt from the log, by Core, which reads
//! what the wallet holds for each order (`balance_verification`, asked per order) and lands again what it lacks
//! (`REQ-ORD-43`). The wallet's half of that:
//!
//! - a landing it carried out, asked again under the same transaction, is answered as it was and adds nothing, so a
//!   re-landing can't double (`INV-1`); the same transaction for another order or amount is refused;
//! - a release for an order it holds no record of, as a fresh instance doesn't, is answered `unknown`, never refused:
//!   the value may still rest in the account, and Core lands it again before it releases.
//!
//! The record itself, and how it answers, is `wallet.rs`, which the platform's tests also run natively.
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
mod wallet;

#[cfg(feature = "sandboxed")]
mod sandboxed {
    use std::cell::RefCell;

    use rutent_contracts::provider_role::{Operation, Statement};

    use super::wallet::Wallet;

    wit_bindgen::generate!({ world: "sandboxed", path: "../../src/module/wit" });

    thread_local! {
        static WALLET: RefCell<Wallet> = RefCell::new(Wallet::default());
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
