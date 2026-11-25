//! The plain wallet as a node's holding account (`T-016-02`; `REQ-CAT-23`, `INV-5`, `REQ-MDL-1`).
//!
//! A public node's holding account is, by default, a plain wallet: a Wasm component (`guests/plain-wallet/`) the
//! operator publishes, admitted through [`Platform::admit`] on the operator's own signed release assertion, with no
//! provider advertisement behind it, and registered for the node by the operator. This file is the operator's side of it:
//!
//! - [`Holdings::register`] records a wallet as a node's holding account, and refuses unless the wallet is a module
//!   the operator publishes (`T-016-04`: admitted on the operator's own release, never a provider's module), the
//!   node is on the operator's public list and the platform runs that wallet (a release that waits for its fresh
//!   acceptance, or one never admitted, does not run);
//! - [`WalletHolding`] is the account as the host keeps it: it binds the account to one node and one asset, writes
//!   each landing and release as the provider-role contract's own `deposit` and `withdrawal`, and records only what
//!   the wallet confirmed in its result, so its [`HoldingReading`] is the wallet's word, per order;
//! - [`Holdings::land`] is where value rests between segments: in a registered node's account, or nowhere. A node
//!   with no holding account takes none, and rutent keeps none itself (`INV-5`).
//!
//! A reading has two halves that come from two places, which is what makes `reconcile` mean something
//! (`REQ-AUD-15`, `INV-1`): the **balance** is the wallet's own figure, asked of it as a `holding_balance`
//! (provider-role 3.0.0) and never computed from what the host sent it; the **attribution**, the value held for each
//! order, is the host's own record of what the wallet confirmed. A wallet that lost value, a provider process
//! restarted among other ways, shows as an account short of its attribution.

use std::collections::BTreeMap;

use rutent_contracts::provider_role::{
    BalanceOutcome, BalanceVerification, Deposit, HoldingBalance, HoldingBalanceOutcome,
    MovementSettlement, Operation, OuterNode, ReserveMovementOutcome, ResultBody, Statement,
    Withdrawal,
};

mod handed_on;

pub use handed_on::{HandedOnStart, StartError, WithdrawalRecord, proof_of_withdrawal};

use crate::catalogue::{Catalogue, NodeError};
use crate::holding::{
    BalanceSource, HeldForOrder, HoldingAccount as _, HoldingError, HoldingReading, OrderLedger,
};
use crate::module::platform::{Platform, World};
use crate::module::types::{Handled, PlatformRefusal, ProcessId};

/// The name of the input that carries the order a movement is for.
pub const ORDER_INPUT: &str = "order";

/// The operator, as far as the wallet needs it: the statement an operation is sent in, made and signed by the
/// operator.
pub trait Operator {
    /// The operator's statement of `operation`.
    fn statement(&self, operation: Operation) -> Statement<Operation>;
}

/// Why a wallet could not be registered as a node's holding account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// The platform runs no module of this provider: it was never admitted, or its release waits for a fresh
    /// acceptance (`REQ-MDL-1`).
    NotRunning(String),
    /// The node, or the way to its account, is refused by the operator's node list (`REQ-CAT-22`).
    Node(NodeError),
    /// The asset is blank.
    BlankAsset,
    /// The catalogue doesn't record the operator as the module's publisher: a provider's module, admitted on its
    /// provider's advertisement, is never a node's holding account (`REQ-CAT-23`).
    NotOperatorPublished(String),
    /// The node already has a holding account.
    NodeTaken(String),
    /// This wallet is already the account of another node: a wallet is one node's.
    WalletTaken {
        /// The wallet's provider.
        provider: String,
        /// The node it is the account of.
        node: String,
    },
}

/// Why value could not be landed in, or released from, a holding account. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletError {
    /// The node has no holding account registered, so value has nowhere to rest there (`REQ-CAT-23`).
    NoHoldingAccount(String),
    /// The movement breaks the account's own rules: its asset, its node, an amount, or what an order holds.
    Holding(HoldingError),
    /// The platform did not pass the wallet's answer on.
    NotAnswered(PlatformRefusal),
    /// The wallet answered, and did not say the movement succeeded.
    NotSucceeded,
    /// The wallet answered that it has no record of the order, which is not a refusal: the value may rest in the
    /// account and only the wallet's record be lost, as after its module restarted (`REQ-ORD-43`). The host lands
    /// what its own record holds for the order again, and then releases.
    NotKnown,
    /// The wallet's answer is of another operation than the one asked.
    WrongResult,
    /// The wallet says it moved another amount than it was asked to.
    Disagrees {
        /// What was asked.
        asked: u128,
        /// What the wallet stated.
        stated: String,
    },
}

impl From<HoldingError> for WalletError {
    fn from(error: HoldingError) -> Self {
        Self::Holding(error)
    }
}

/// A whole number of units as the contracts write one: digits only, no sign, no leading zero.
fn parse_amount(text: &str) -> Result<u128, HoldingError> {
    let plain = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if !plain {
        return Err(HoldingError::BadAmount(text.to_owned()));
    }
    text.parse()
        .map_err(|_| HoldingError::BadAmount(text.to_owned()))
}

/// A node's holding account as the host keeps it: a plain wallet's, bound to its node and asset.
#[derive(Debug, Clone)]
pub struct WalletHolding {
    provider: String,
    ledger: OrderLedger,
    sent: u64,
    /// The process the wallet's module ran in when it last confirmed a movement: another one since means its
    /// module restarted, and with it the wallet's own record (`REQ-ORD-43`, `T-020-46`).
    process: Option<ProcessId>,
}

impl WalletHolding {
    /// The account of `node` held in `asset` by the wallet of `provider`, with nothing in it.
    pub fn new(
        node: impl Into<String>,
        asset: impl Into<String>,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            ledger: OrderLedger::new(node, asset),
            sent: 0,
            process: None,
        }
    }

    /// The provider whose module is the account.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The node the account is at.
    pub fn node(&self) -> &str {
        self.ledger.node()
    }

    /// The asset the account holds.
    pub fn asset(&self) -> &str {
        self.ledger.asset()
    }

    /// Value arrives at the node for an order: the wallet is asked to place it in the account, and the host records
    /// it only if the wallet confirms the same amount. Anything else leaves the account as it was.
    pub fn land<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        node: &str,
        asset: &str,
        order: &str,
        amount: &str,
    ) -> Result<(), WalletError> {
        let (next, ..) =
            self.movement(platform, operator, true, node, asset, order, amount, None)?;
        self.ledger = next;
        self.process = platform.process(&self.provider);
        Ok(())
    }

    /// Whether the wallet's module restarted since it last confirmed a movement, as the platform reports its process:
    /// its own record is then gone, and what the host attributes may no longer rest in the wallet's record
    /// (`REQ-ORD-43`, `T-020-46`). False where it confirmed none yet.
    pub fn restarted<W: World>(&self, platform: &Platform<W>) -> bool {
        self.process
            .is_some_and(|seen| platform.process(&self.provider) != Some(seen))
    }

    /// What the wallet itself holds for `order`, by its own answers, up to `amount`: the most it says the order's
    /// account holds at least of (`balance_verification`, asked per order, each under a transaction of `transaction`'s
    /// own, `{transaction}-v{n}`). Never the host's attribution, which outlives the wallet's record, and never a
    /// repeat landing's `succeeded`, which adds nothing (`T-016-05`).
    pub fn held_by_wallet<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        order: &str,
        amount: u128,
        transaction: &str,
    ) -> Result<u128, WalletError> {
        let mut asked = 0u32;
        let mut verify = |units: u128| -> Result<bool, WalletError> {
            asked += 1;
            let operation = Operation::BalanceVerification(BalanceVerification {
                transaction: format!("{transaction}-v{asked}"),
                provider: self.provider.clone(),
                session: transaction.to_owned(),
                account: order.to_owned(),
                asset: self.ledger.asset().to_owned(),
                amount: units.to_string(),
            });
            match platform.handle(&self.provider, &operator.statement(operation)) {
                Handled::Answered(result) => match result.statement {
                    ResultBody::BalanceVerification(r) => match r.outcome {
                        BalanceOutcome::Answered(answer) => Ok(answer.sufficient),
                        BalanceOutcome::Refused(_) => Err(WalletError::NotSucceeded),
                    },
                    _ => Err(WalletError::WrongResult),
                },
                Handled::Refused(why) => Err(WalletError::NotAnswered(why)),
            }
        };
        if amount == 0 || verify(amount)? {
            return Ok(amount);
        }
        // The wallet holds less than `amount`: the most it holds at least of, between none and `amount`.
        let (mut low, mut high) = (0u128, amount);
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if verify(middle)? {
                low = middle;
            } else {
                high = middle;
            }
        }
        Ok(low)
    }

    /// Restores in the wallet what the log holds for `order` at the node, `amount`, before a release (`REQ-ORD-43`,
    /// `INV-1`, `T-020-46`): what the wallet itself holds for the order is asked first ([`Self::held_by_wallet`]),
    /// and only what it lacks is landed again, under `transaction`, a transaction of Core's own: the host's counter
    /// would mint one the platform's durable record holds already. The host's attribution ends at `amount` for the
    /// order, not above it, so a re-landing never attributes value twice. Answers what it landed again.
    pub fn restore<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        order: &str,
        amount: &str,
        transaction: &str,
    ) -> Result<u128, WalletError> {
        let units = parse_amount(amount)?;
        let held = self.held_by_wallet(platform, operator, order, units, transaction)?;
        if held >= units {
            self.process = platform.process(&self.provider);
            return Ok(0);
        }
        let short = units - held;
        let (node, asset) = (
            self.ledger.node().to_owned(),
            self.ledger.asset().to_owned(),
        );
        let (mut next, ..) = self.movement(
            platform,
            operator,
            true,
            &node,
            &asset,
            order,
            &short.to_string(),
            Some(transaction.to_owned()),
        )?;
        let attributed = next.held_for(order);
        if attributed > units {
            next.release(order, attributed - units)?;
        }
        self.ledger = next;
        self.process = platform.process(&self.provider);
        Ok(short)
    }

    /// Value leaves the account for an order, up to what the order holds.
    pub fn release<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        node: &str,
        asset: &str,
        order: &str,
        amount: &str,
    ) -> Result<(), WalletError> {
        let (next, ..) =
            self.movement(platform, operator, false, node, asset, order, amount, None)?;
        self.ledger = next;
        self.process = platform.process(&self.provider);
        Ok(())
    }

    /// What the host recorded the wallet confirmed for each order: the attribution half of a reading, in order name
    /// order. It is the host's own record, not the wallet's word, and its sum is not the account's balance.
    pub fn attribution(&self) -> Vec<HeldForOrder> {
        self.ledger
            .reading()
            .map(|reading| reading.held)
            .unwrap_or_default()
    }

    /// The account as `CMP-CORE-AUDIT` reads it (`REQ-AUD-15`): the balance is the wallet's own figure, asked of it
    /// as a `holding_balance`, and the value held for each order is the host's attribution, so a wallet that no
    /// longer holds what the host attributed reads short of it. Nothing is changed.
    pub fn read<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
    ) -> Result<HoldingReading, WalletError> {
        let transaction = format!("{}-balance-{}", self.provider, self.sent + 1);
        self.sent += 1;
        let operation = Operation::HoldingBalance(HoldingBalance {
            transaction: transaction.clone(),
            provider: self.provider.clone(),
            node: self.ledger.node().to_owned(),
            asset: self.ledger.asset().to_owned(),
        });
        let answered = match platform.handle(&self.provider, &operator.statement(operation)) {
            Handled::Answered(result) => result.statement,
            Handled::Refused(why) => return Err(WalletError::NotAnswered(why)),
        };
        let ResultBody::HoldingBalance(result) = answered else {
            return Err(WalletError::WrongResult);
        };
        if result.transaction != transaction {
            return Err(WalletError::WrongResult);
        }
        let HoldingBalanceOutcome::Answered(answer) = result.outcome else {
            return Err(WalletError::NotSucceeded);
        };
        Ok(HoldingReading {
            node: self.ledger.node().to_owned(),
            asset: self.ledger.asset().to_owned(),
            balance: parse_amount(&answer.balance)?,
            source: BalanceSource::Module,
            held: self.attribution(),
        })
    }

    /// Asks the wallet for a movement and returns the account as it would then stand, with the settlement the wallet
    /// stated. The account itself is not changed: the caller replaces it once it has what it needs.
    #[allow(clippy::too_many_arguments)]
    fn movement<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        landing: bool,
        node: &str,
        asset: &str,
        order: &str,
        amount: &str,
        transaction: Option<String>,
    ) -> Result<
        (
            OrderLedger,
            MovementSettlement,
            String,
            Statement<ResultBody>,
        ),
        WalletError,
    > {
        // The account's own rules, before the wallet is asked anything: its node and asset (the asset check
        // `T-016-01` left to the contract types), the amount, and what the order holds. The record is changed on a
        // copy, which replaces the account's only once the wallet has confirmed.
        if node != self.ledger.node() {
            return Err(HoldingError::WrongNode {
                expected: self.ledger.node().to_owned(),
                found: node.to_owned(),
            }
            .into());
        }
        if asset != self.ledger.asset() {
            return Err(HoldingError::WrongAsset {
                expected: self.ledger.asset().to_owned(),
                found: asset.to_owned(),
            }
            .into());
        }
        let units = parse_amount(amount)?;
        let mut next = self.ledger.clone();
        if landing {
            next.hold(order, units)?;
        } else {
            next.release(order, units)?;
        }

        let kind = if landing { "deposit" } else { "withdrawal" };
        let transaction = transaction.unwrap_or_else(|| {
            self.sent += 1;
            format!("{}-{kind}-{}", self.provider, self.sent)
        });
        let network = OuterNode {
            network: self.provider.clone(),
        };
        let inputs = BTreeMap::from([(ORDER_INPUT.to_owned(), order.to_owned())]);
        let operation = if landing {
            Operation::Deposit(Deposit {
                transaction: transaction.clone(),
                provider: self.provider.clone(),
                origin: node.to_owned(),
                destination: network,
                asset: asset.to_owned(),
                amount: units.to_string(),
                dry_run: false,
                inputs,
            })
        } else {
            Operation::Withdrawal(Withdrawal {
                transaction: transaction.clone(),
                provider: self.provider.clone(),
                session: None,
                origin: network,
                destination: node.to_owned(),
                asset: asset.to_owned(),
                amount: units.to_string(),
                dry_run: false,
                inputs,
            })
        };
        // The wallet's signed answer, as the platform passed it on: a route handed on keeps it as the holding
        // account's own statement of the withdrawal (`T-020-26`).
        let (answered, signed) =
            match platform.handle(&self.provider, &operator.statement(operation)) {
                Handled::Answered(result) => {
                    let signed = (*result).clone();
                    (result.statement, signed)
                }
                Handled::Refused(why) => return Err(WalletError::NotAnswered(why)),
            };
        let result = match (landing, answered) {
            (true, ResultBody::Deposit(r)) | (false, ResultBody::Withdrawal(r))
                if r.transaction == transaction =>
            {
                r
            }
            _ => return Err(WalletError::WrongResult),
        };
        let settled = match result.outcome {
            ReserveMovementOutcome::Succeeded(settled) => settled,
            ReserveMovementOutcome::Unknown(_) if !landing => return Err(WalletError::NotKnown),
            _ => return Err(WalletError::NotSucceeded),
        };
        let stated = units.to_string();
        if settled.amount_in != stated || settled.amount_out != stated {
            return Err(WalletError::Disagrees {
                asked: units,
                stated: settled.amount_in,
            });
        }
        Ok((next, settled, transaction, signed))
    }
}

/// The holding accounts of the operator's public nodes: one plain wallet each.
#[derive(Debug, Clone, Default)]
pub struct Holdings {
    accounts: BTreeMap<String, WalletHolding>,
}

impl Holdings {
    /// Registers the wallet of `provider`, holding `asset`, as the holding account of the public node `node`
    /// (`REQ-CAT-23`). The wallet must be a module the operator publishes, as the catalogue records
    /// ([`Catalogue::publish_operator_module`]), never a provider's (`T-016-04`); the platform must run it, admitted
    /// on the operator's own signed release and accepted; and the node must be on the operator's public list. The
    /// way to the account is recorded in the catalogue. Nothing is registered otherwise.
    pub fn register<W: World>(
        &mut self,
        catalogue: &mut Catalogue,
        platform: &Platform<W>,
        node: &str,
        provider: &str,
        asset: &str,
    ) -> Result<(), RegisterError> {
        if asset.trim().is_empty() {
            return Err(RegisterError::BlankAsset);
        }
        if platform.active_version(provider).is_none() {
            return Err(RegisterError::NotRunning(provider.to_owned()));
        }
        if !catalogue.publishers().published_by_operator(provider) {
            return Err(RegisterError::NotOperatorPublished(provider.to_owned()));
        }
        if self.accounts.contains_key(node) {
            return Err(RegisterError::NodeTaken(node.to_owned()));
        }
        if let Some(taken) = self.accounts.values().find(|a| a.provider() == provider) {
            return Err(RegisterError::WalletTaken {
                provider: provider.to_owned(),
                node: taken.node().to_owned(),
            });
        }
        catalogue
            .register_public_holding(node, provider)
            .map_err(RegisterError::Node)?;
        self.accounts
            .insert(node.to_owned(), WalletHolding::new(node, asset, provider));
        Ok(())
    }

    /// The holding account of a node, if one is registered.
    pub fn account(&self, node: &str) -> Option<&WalletHolding> {
        self.accounts.get(node)
    }

    /// Value arrives at `node` for an order. It rests in the node's holding account or it is refused: a node with
    /// no account takes none (`REQ-CAT-23`), and rutent holds nothing itself (`INV-5`).
    pub fn land<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        node: &str,
        asset: &str,
        order: &str,
        amount: &str,
    ) -> Result<(), WalletError> {
        self.accounts
            .get_mut(node)
            .ok_or_else(|| WalletError::NoHoldingAccount(node.to_owned()))?
            .land(platform, operator, node, asset, order, amount)
    }

    /// [`WalletHolding::restore`] at `node`'s holding account: what the log holds for `order` there, `amount`,
    /// restored in the wallet before a release, under `transaction` (`T-020-46`).
    pub fn restore<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        node: &str,
        order: &str,
        amount: &str,
        transaction: &str,
    ) -> Result<u128, WalletError> {
        self.accounts
            .get_mut(node)
            .ok_or_else(|| WalletError::NoHoldingAccount(node.to_owned()))?
            .restore(platform, operator, order, amount, transaction)
    }

    /// [`WalletHolding::held_by_wallet`] at `node`'s holding account: what its wallet itself holds for `order`, up to
    /// `amount`, by its own per-order answers (`T-020-46`).
    pub fn held_by_wallet<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        node: &str,
        order: &str,
        amount: u128,
        transaction: &str,
    ) -> Result<u128, WalletError> {
        self.accounts
            .get_mut(node)
            .ok_or_else(|| WalletError::NoHoldingAccount(node.to_owned()))?
            .held_by_wallet(platform, operator, order, amount, transaction)
    }

    /// Whether `node`'s holding account's module restarted since it last confirmed a movement
    /// ([`WalletHolding::restarted`]).
    pub fn restarted<W: World>(&self, platform: &Platform<W>, node: &str) -> bool {
        self.accounts
            .get(node)
            .is_some_and(|account| account.restarted(platform))
    }

    /// Value leaves `node`'s holding account for an order.
    pub fn release<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
        node: &str,
        asset: &str,
        order: &str,
        amount: &str,
    ) -> Result<(), WalletError> {
        self.accounts
            .get_mut(node)
            .ok_or_else(|| WalletError::NoHoldingAccount(node.to_owned()))?
            .release(platform, operator, node, asset, order, amount)
    }

    /// The attribution every registered account holds, by node, in node order: the host's own record (see
    /// [`WalletHolding::attribution`]).
    pub fn attributions(&self) -> Vec<(String, Vec<HeldForOrder>)> {
        self.accounts
            .iter()
            .map(|(node, account)| (node.clone(), account.attribution()))
            .collect()
    }

    /// The reading of every registered account, by node in node order, for `CMP-CORE-AUDIT`: each account's own
    /// result, so one whose wallet could not be read shows as the failure it is and is never left out
    /// (`REQ-AUD-15`).
    pub fn readings<W: World>(
        &mut self,
        platform: &mut Platform<W>,
        operator: &impl Operator,
    ) -> Vec<(String, Result<HoldingReading, WalletError>)> {
        self.accounts
            .iter_mut()
            .map(|(node, account)| (node.clone(), account.read(platform, operator)))
            .collect()
    }
}
