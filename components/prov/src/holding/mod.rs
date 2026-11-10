//! Holding-account modules at public nodes (`F-016`).
//!
//! A node where value rests between segments has a holding account (`REQ-CAT-23`). This module is the interface
//! `CMP-CORE-AUDIT` reads: the account's balance, and the value held for each order at that node. It also holds
//! the check of `REQ-AUD-15`: the balance must equal the sum held for the orders, and a difference is raised to
//! the operator and nothing more (the check only detects, as `REQ-ORD-9` does).
//!
//! The interface names no provider and imports nothing of ours (`DEC-2`): nodes, orders and assets are plain
//! identifiers, amounts are whole units in the node's asset. [`wallet`] binds it to the contracts' types and to the
//! module runtime: the plain wallet, a Wasm component registered as a public node's holding account (`T-016-02`).

pub mod wallet;

use std::collections::BTreeMap;
use std::fmt;

/// Whose word a balance is. At a public node it is the operator's own module's reading; at a private node it is
/// its publisher's statement, labelled as such (`REQ-AUD-15`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalanceSource {
    /// Read from the holding-account module the operator registered at a public node.
    Module,
    /// Stated by the publisher of a private node; rutent cannot check it beyond comparing it.
    Publisher(String),
}

/// What one order has resting in a holding account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldForOrder {
    /// The order the value is attributed to.
    pub order: String,
    /// The amount held for it, in the account's asset.
    pub amount: u128,
}

/// A holding account as `CMP-CORE-AUDIT` reads it at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingReading {
    /// The node whose holding account this is.
    pub node: String,
    /// The asset it holds; one account, one asset.
    pub asset: String,
    /// The account's balance, as the account reports it.
    pub balance: u128,
    /// Whose word the balance is.
    pub source: BalanceSource,
    /// The value held for each order, one entry per order.
    pub held: Vec<HeldForOrder>,
}

/// Why a holding account could not be read, changed or checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldingError {
    /// The amounts held add up past what a balance can state.
    SumOverflow,
    /// A reading names one order twice.
    DuplicateOrder(String),
    /// A hold or release of nothing.
    ZeroAmount,
    /// An amount that is not a whole number of units, written as the contracts write one: digits only, no sign and
    /// no leading zero, and no larger than a balance can state.
    BadAmount(String),
    /// A hold or release in another asset than the account's: one account, one asset.
    WrongAsset {
        /// The account's asset.
        expected: String,
        /// The asset asked.
        found: String,
    },
    /// A hold or release at another node than the account's.
    WrongNode {
        /// The account's node.
        expected: String,
        /// The node asked.
        found: String,
    },
    /// A release asks for more than the order has held.
    ReleaseExceedsHeld {
        /// The order.
        order: String,
        /// What it holds.
        held: u128,
        /// What the release asked for.
        asked: u128,
    },
}

impl fmt::Display for HoldingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SumOverflow => f.write_str("the amounts held add up past the largest balance"),
            Self::DuplicateOrder(o) => write!(f, "order {o} appears twice in one reading"),
            Self::ZeroAmount => f.write_str("an amount of zero"),
            Self::BadAmount(a) => write!(f, "{a:?} is not a whole number of units"),
            Self::WrongAsset { expected, found } => {
                write!(f, "the account holds {expected}, not {found}")
            }
            Self::WrongNode { expected, found } => {
                write!(f, "the account is at {expected}, not {found}")
            }
            Self::ReleaseExceedsHeld { order, held, asked } => {
                write!(f, "order {order} holds {held} and {asked} was asked")
            }
        }
    }
}

impl std::error::Error for HoldingError {}

/// The interface of a holding account: what `CMP-CORE-AUDIT` reads (`REQ-AUD-15`). The module behind it is the
/// operator's at a public node and the publisher's at a private one.
pub trait HoldingAccount {
    /// The balance and the per-order attribution, read together so they are of one moment.
    fn reading(&self) -> Result<HoldingReading, HoldingError>;
}

/// A difference between an account's balance and the sum held for its orders, to be raised to the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// The balance is below the sum held.
    Short {
        /// The node.
        node: String,
        /// How far below.
        by: u128,
        /// Whose word the balance is.
        source: BalanceSource,
    },
    /// The balance is above the sum held.
    Over {
        /// The node.
        node: String,
        /// How far above.
        by: u128,
        /// Whose word the balance is.
        source: BalanceSource,
    },
}

impl Difference {
    /// The node it was found at.
    pub fn node(&self) -> &str {
        match self {
            Self::Short { node, .. } | Self::Over { node, .. } => node,
        }
    }

    /// Whose word the balance was, so a private node's figure is shown as its publisher's.
    pub fn source(&self) -> &BalanceSource {
        match self {
            Self::Short { source, .. } | Self::Over { source, .. } => source,
        }
    }
}

/// The outcome of checking one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciliation {
    /// The balance equals the sum held.
    Matches,
    /// It does not; the operator is told.
    Differs(Difference),
}

/// Checks that an account's balance equals the sum of the value held for each order (`REQ-AUD-15`, `INV-1`).
/// It reads and reports; it changes nothing.
pub fn reconcile(reading: &HoldingReading) -> Result<Reconciliation, HoldingError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut sum: u128 = 0;
    for entry in &reading.held {
        if entry.amount == 0 {
            return Err(HoldingError::ZeroAmount);
        }
        if !seen.insert(entry.order.as_str()) {
            return Err(HoldingError::DuplicateOrder(entry.order.clone()));
        }
        sum = sum
            .checked_add(entry.amount)
            .ok_or(HoldingError::SumOverflow)?;
    }
    let node = reading.node.clone();
    let source = reading.source.clone();
    Ok(match reading.balance.cmp(&sum) {
        std::cmp::Ordering::Equal => Reconciliation::Matches,
        std::cmp::Ordering::Less => Reconciliation::Differs(Difference::Short {
            node,
            by: sum - reading.balance,
            source,
        }),
        std::cmp::Ordering::Greater => Reconciliation::Differs(Difference::Over {
            node,
            by: reading.balance - sum,
            source,
        }),
    })
}

/// A holding account that keeps the attribution itself, so its balance is the sum held by construction. A module
/// can use it as its record of what each order has resting; a drift between that record and the real funds is
/// what the module's own report would show to [`reconcile`].
#[derive(Debug, Clone)]
pub struct OrderLedger {
    node: String,
    asset: String,
    held: BTreeMap<String, u128>,
}

impl OrderLedger {
    /// An empty ledger for one node and asset.
    pub fn new(node: impl Into<String>, asset: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            asset: asset.into(),
            held: BTreeMap::new(),
        }
    }

    /// Records value arriving for an order.
    pub fn hold(&mut self, order: &str, amount: u128) -> Result<(), HoldingError> {
        if amount == 0 {
            return Err(HoldingError::ZeroAmount);
        }
        let total = self.total()?;
        total.checked_add(amount).ok_or(HoldingError::SumOverflow)?;
        *self.held.entry(order.to_owned()).or_default() += amount;
        Ok(())
    }

    /// Records value leaving for an order. More than the order holds is refused, and nothing changes.
    pub fn release(&mut self, order: &str, amount: u128) -> Result<(), HoldingError> {
        if amount == 0 {
            return Err(HoldingError::ZeroAmount);
        }
        let held = self.held_for(order);
        if amount > held {
            return Err(HoldingError::ReleaseExceedsHeld {
                order: order.to_owned(),
                held,
                asked: amount,
            });
        }
        if amount == held {
            self.held.remove(order);
        } else {
            self.held.insert(order.to_owned(), held - amount);
        }
        Ok(())
    }

    /// The node the ledger is for.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The asset the ledger holds.
    pub fn asset(&self) -> &str {
        &self.asset
    }

    /// What one order has resting here; zero for an order that holds nothing.
    pub fn held_for(&self, order: &str) -> u128 {
        self.held.get(order).copied().unwrap_or(0)
    }

    fn total(&self) -> Result<u128, HoldingError> {
        self.held
            .values()
            .try_fold(0u128, |sum, a| sum.checked_add(*a))
            .ok_or(HoldingError::SumOverflow)
    }
}

impl HoldingAccount for OrderLedger {
    fn reading(&self) -> Result<HoldingReading, HoldingError> {
        Ok(HoldingReading {
            node: self.node.clone(),
            asset: self.asset.clone(),
            balance: self.total()?,
            source: BalanceSource::Module,
            held: self
                .held
                .iter()
                .map(|(order, amount)| HeldForOrder {
                    order: order.clone(),
                    amount: *amount,
                })
                .collect(),
        })
    }
}
