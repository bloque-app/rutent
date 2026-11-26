//! Simulated providers (`T-024-01`, `REQ-MOD-4`, `DEC-23`): the stand-ins of `role::stand_in`, made scriptable, and
//! able to keep their books where the platform keeps shared state.
//!
//! The simulated providers are Wasm components (`components/prov/module-guests/sim/`) admitted through the platform
//! like any module. This module is what they are made of, in a form that runs in process too, so a twin of a module
//! can be held beside it and compared:
//!
//! - [`Spec`]: which role a provider plays and under which name. Everything else about its identity, its
//!   publisher, its keys and, for a reserve, its bank and its private node, follows from the two ([`Spec::of`]),
//!   so a module, its twin and the kit that lists its advertisement agree without being told.
//! - [`Behaviour`]: how it answers one operation. A script is a queue of them, kept by the platform's world, and the
//!   module asks for the next one through its outside part, as an operation's first step. An operation with none
//!   queued is answered as the role asks.
//! - [`Simulated`]: the stand-in under a spec, which answers an operation as a behaviour says, and reports the
//!   book entry ([`Entry`]) the operation made, if it made one.
//!
//! **Where the books live.** A provider's process dies at its deadline, and again when it crashes, and a module that
//! kept its books in its own memory would lose them there. The WIT world has no state write (`F-013`'s notes), so a
//! simulated provider's books are a journal of [`Entry`] the platform's world keeps under [`BOOK_KEY`], one per
//! operation that changed them. The module's outside part writes each entry by calling the provider's own backend,
//! the rail it declares, which is an outbound call the platform records (`REQ-MDL-8`), and reads the journal back
//! through the platform's state read. A module that finds the journal longer than what it has applied replays it
//! into a fresh stand-in ([`Simulated::restore`]), which is deterministic, so a restarted process holds the books
//! the one before it did.
//!
//! **What a script can make a provider do.** Each [`Behaviour`] field is one of the ways the row names:
//! answer ([`Act::Answer`]), refuse ([`Act::Refuse`] and its two lifting variants), stay silent
//! ([`Behaviour::silent`]), answer late ([`Behaviour::late`]), crash mid-operation ([`Behaviour::crash`]), report
//! outcome unknown ([`Act::Unknown`]), deliver short or in pieces ([`Delivery`]), and, for a reserve, confirm a
//! reversal or not ([`Behaviour::reversal_credited`]). What a behaviour does to the effects, which a module alone
//! can do (spin, trap), the module does; [`Simulated::answer`] only decides the answer and the entry.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::role::stand_in::{Exchange, Identity, Ramp, Reserve, Verifier, refusal_to};

pub use crate::role::stand_in::Shape;
use crate::role::{Received, RoleHarness, RoleProvider, SuiteContext};
#[cfg(feature = "signing")]
use crate::role::{placeholder_key, placeholder_signature};
use rutent_contracts::provider_role::{Advertisement, Operation, Statement};
#[cfg(feature = "signing")]
use rutent_contracts::provider_role::{
    INNER_SEGMENTS_SHAPE, InnerSegment as RecordedInner, InnerSegments, Signature, SignatureScheme,
    Standing,
};

/// The state key under which the platform's world keeps a simulated provider's journal: the JSON array of its
/// [`Entry`]. It is a shared key, readable by the provider's outside part for any transaction.
pub const BOOK_KEY: &str = "book";

/// The state key under which the platform's world names the deployment a simulated provider serves, as the JSON of
/// provider-role's `Deployment`: the provider holds every operation to it. Where the world names none, it serves the
/// suite's default context (`T-024-02`: the assembly's deployment names the operator's real key).
pub const DEPLOYMENT_KEY: &str = "deployment";

/// The state key a scenario that sets a reserve's opening balance seeds it under, as text (`T-024-22`): the sandboxed
/// part builds its stand-in with that figure in the account, in place of [`Reserve::OPENING`]. Where none is seeded,
/// it keeps the stand-in's own.
pub const OPENING_KEY: &str = "opening";

/// The prefix of the state keys a script's next behaviour is read under: `script/next/{kind}/{dry|real}`. Reading
/// one takes the behaviour out of the queue, which is how a script is consumed.
pub const SCRIPT_PREFIX: &str = "script/next";

/// The secret a simulated provider's rail takes as a bearer header: declared on [`SETTLE`], resolved by the outside part
/// through the credential proxy (`T-025-03`).
pub const CREDENTIAL: &str = "api-key";

/// The state key of the provider's private notes, which its outside part reads on each operation and never hands
/// back (`T-025-03`).
pub const PRIVATE_NOTES_KEY: &str = "private/notes";

/// The state key of the journal's length, so a module can tell whether it holds the books without reading them.
pub const BOOK_LENGTH_KEY: &str = "book/length";

/// The outside operation a simulated provider's sandboxed part asks first: the next behaviour, and the journal's
/// length.
pub const BEGIN: &str = "sim.begin";

/// The outside operation that reads the journal back, when a module's books and the world's differ.
pub const BOOK: &str = "sim.book";

/// The outside operation that writes one entry to the provider's rail, the backend that keeps its books.
pub const SETTLE: &str = "sim.settle";

/// The outside operation that records one movement on a simulated public network.
pub const CHAIN: &str = "sim.chain";

/// The host of provider `provider`'s rail, behind the destination a simulated provider's declaration names `rail`.
pub fn rail_host(provider: &str) -> String {
    format!("rail.{provider}.example")
}

/// The host of the simulated public network `network`, behind the destination named `chain-{network}`.
pub fn chain_host(network: &str) -> String {
    format!("{network}.chain.example")
}

/// The state key a behaviour for an operation of `kind` is read under, for a dry run or a real operation.
pub fn script_key(kind: &str, dry_run: bool) -> String {
    format!(
        "{SCRIPT_PREFIX}/{kind}/{}",
        if dry_run { "dry" } else { "real" }
    )
}

/// The role a simulated provider plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A reserve, between a bank and its own private node.
    Reserve,
    /// An exchange, within one public node.
    Exchange,
    /// A ramp, between two public nodes.
    Ramp,
    /// A verifier, for the role only (`DEC-16`): it takes no script and has no book.
    Verifier,
}

impl Role {
    /// The role named `name`: `reserve`, `exchange`, `ramp` or `verifier`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "reserve" => Some(Self::Reserve),
            "exchange" => Some(Self::Exchange),
            "ramp" => Some(Self::Ramp),
            "verifier" => Some(Self::Verifier),
            _ => None,
        }
    }

    /// Whether providers of this role move value, and so have a book and a script.
    pub fn moves_value(self) -> bool {
        !matches!(self, Self::Verifier)
    }
}

/// A simulated provider's identity: its role and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    /// The role it plays.
    pub role: Role,
    /// The provider identifier it advertises and signs as.
    pub provider: String,
    /// Whether a reserve declares that it starts routes on automatic deposits (`REQ-ORD-13`).
    #[serde(default)]
    pub automatic_deposits: bool,
    /// What a reserve's account holds when its books are empty, where the scenario sets it; the stand-in's own
    /// ([`Reserve::OPENING`]) where none (`T-024-22`).
    #[serde(default)]
    pub opening: Option<u128>,
    /// The customer's standing instruction for a reserve's automatic deposits, as an integrator or the operator set it:
    /// the account they go to, who set it, for whom (`T-020-50`); the reserve's notices of that holder's credits name it.
    #[serde(default)]
    pub deposit_instruction: Option<(String, String, String)>,
}

impl Spec {
    /// The simulated provider of `role` named `provider`.
    pub fn of(role: Role, provider: &str) -> Self {
        Self {
            role,
            provider: provider.to_owned(),
            automatic_deposits: false,
            opening: None,
            deposit_instruction: None,
        }
    }

    /// This provider, a reserve holding the customer's standing instruction for its automatic deposits: `set_by` set
    /// them for `set_for`, sending that holder's deposits to `account` (`T-020-50`).
    #[must_use]
    pub fn with_deposit_instruction(mut self, account: &str, set_by: &str, set_for: &str) -> Self {
        self.deposit_instruction =
            Some((account.to_owned(), set_by.to_owned(), set_for.to_owned()));
        self
    }

    /// This provider, a reserve whose account holds `opening` when its books are empty: a run that scans for balances
    /// gives each a figure no other figure of the run takes (`T-024-22`).
    #[must_use]
    pub fn opening_with(mut self, opening: u128) -> Self {
        self.opening = Some(opening);
        self
    }

    /// This provider, a reserve declaring that it starts routes on automatic deposits.
    #[must_use]
    pub fn starting_routes_on_automatic_deposits(mut self) -> Self {
        self.automatic_deposits = true;
        self
    }

    /// Its publisher: one per provider, so two reserves on two private nodes have two (`DEC-24`), unless its shape
    /// names the publisher it shares with its group (`T-024-04`).
    pub fn publisher(&self) -> String {
        shape_of(&self.provider)
            .publisher
            .unwrap_or_else(|| format!("{}-publisher", self.beside()))
    }

    /// The provider whose books and nodes this one sits beside: itself, unless its shape says another
    /// ([`Shape::beside`]).
    fn beside(&self) -> String {
        shape_of(&self.provider)
            .beside
            .unwrap_or_else(|| self.provider.clone())
    }

    /// The bank network of a reserve, its outer node: its shape's where that names one (an on-chain network).
    pub fn bank(&self) -> String {
        shape_of(&self.provider)
            .outer
            .unwrap_or_else(|| format!("{}-bank", self.beside()))
    }

    /// The inner node of a reserve, its ledger: a private node its publisher declares, unless its shape names the
    /// node, one its group shares or one on the operator's public list.
    pub fn ledger(&self) -> String {
        shape_of(&self.provider)
            .node
            .unwrap_or_else(|| format!("{}-ledger", self.beside()))
    }

    /// The nodes of the operator's public list this provider's tuples reach beyond the default stand-ins' own: a
    /// reserve's or an exchange's node where its shape says it is public, a ramp's destination, which is one, and its
    /// origin where its shape says so.
    pub fn public_nodes(&self) -> Vec<String> {
        let shape = shape_of(&self.provider);
        let mut nodes = Vec::new();
        match self.role {
            Role::Reserve if shape.public_node => nodes.push(self.ledger()),
            Role::Exchange if shape.public_node => nodes.extend(shape.node.clone()),
            Role::Ramp => {
                if !shape.outer_private {
                    nodes.extend(shape.outer.clone());
                }
                if shape.public_node {
                    nodes.extend(shape.node.clone());
                }
            }
            _ => {}
        }
        nodes
    }

    /// The private nodes this provider's publisher declares through it: its node unless the operator lists it public
    /// (`REQ-CAT-22`). The segments it serves inside them are the ones its publisher records (`REQ-ORD-41`).
    pub fn private_nodes(&self) -> Vec<String> {
        let shape = shape_of(&self.provider);
        let mut nodes = match self.role {
            Role::Reserve if !shape.public_node && !shape.undeclared_node => vec![self.ledger()],
            Role::Exchange | Role::Ramp if !shape.public_node => {
                shape.node.clone().into_iter().collect()
            }
            _ => Vec::new(),
        };
        // A ramp's destination its publisher declares private, and a reserve's bank rail its publisher declares so
        // (`T-024-05`).
        if self.role == Role::Ramp && shape.outer_private {
            nodes.extend(shape.outer.clone());
        }
        if self.role == Role::Reserve && shape.rail_private {
            nodes.push(self.bank());
        }
        nodes
    }

    /// The simulated public networks this provider's movements land on, which its module's declaration names: a
    /// reserve's bank; the public node an exchange converts at or a ramp takes value to. A conversion or a transfer
    /// into a private node lands on none.
    pub fn networks(&self) -> Vec<String> {
        let shape = shape_of(&self.provider);
        match self.role {
            // A rail with no public record is served by no network; the movement into one its publisher declares private
            // is on the public node the deposit leaves (`AC-34`, `T-024-05`).
            Role::Reserve if shape.rail_private => vec![self.ledger()],
            Role::Reserve if shape.rail_unserved => Vec::new(),
            Role::Reserve => vec![self.bank()],
            Role::Exchange => match &shape.node {
                None => vec!["polygon".to_owned()],
                Some(node) if shape.public_node => vec![node.clone()],
                Some(_) => Vec::new(),
            },
            Role::Ramp if shape.outer_private => Vec::new(),
            Role::Ramp => vec![shape.outer.unwrap_or_else(|| "bsc".to_owned())],
            Role::Verifier => Vec::new(),
        }
    }

    /// The seed of its placeholder keys: 40 to 59, from its name. A kit that lists several providers checks that
    /// their seeds differ ([`Spec::seed`]).
    pub fn seed(&self) -> u8 {
        let hash = self
            .provider
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            });
        40 + u8::try_from(hash % 20).expect("below twenty")
    }

    /// The stand-in's identity.
    pub fn identity(&self) -> Identity {
        Identity::new(&self.provider, &self.publisher(), self.seed())
    }

    /// The stand-in of this identity, in `context`.
    pub fn make(&self, context: &SuiteContext) -> Box<dyn Standin> {
        let identity = self.identity();
        match self.role {
            Role::Reserve => {
                let reserve = Reserve::declaring(
                    context,
                    &identity,
                    &self.bank(),
                    &self.ledger(),
                    self.automatic_deposits,
                )
                .shaped(&shape_of(&self.provider));
                let reserve = match self.opening {
                    Some(opening) => reserve.opening_with(opening),
                    None => reserve,
                };
                Box::new(match &self.deposit_instruction {
                    Some((account, set_by, set_for)) => {
                        reserve.with_deposit_instruction(account, set_by, set_for)
                    }
                    None => reserve,
                })
            }
            Role::Exchange => {
                Box::new(Exchange::named(context, &identity).shaped(&shape_of(&self.provider)))
            }
            Role::Ramp => {
                Box::new(Ramp::named(context, &identity).shaped(&shape_of(&self.provider)))
            }
            Role::Verifier => {
                let verifier = Verifier::named(context, &identity);
                // `sim-verifier-lax` says yes without checking (`T-024-12`, `THR-91`): the name chooses it, as a
                // reserve's shape is chosen.
                if self.provider.ends_with("-lax") {
                    Box::new(verifier.saying_yes_without_checking())
                } else {
                    Box::new(verifier)
                }
            }
        }
    }
}

/// How long a reserve declares, in seconds, when it is slow: more than the three days past which a quote has no firm
/// rate (`REQ-QTE-10`).
pub const SLOW_SECONDS: u64 = 4 * 86_400;

/// What the gross reserves charge on each movement, quoted ([`shape_of`]).
pub const GROSS_FEE: u128 = 5_000;

/// The shape the simulated reserve named `provider` takes (`T-024-03`): the default for any name but these, which a
/// scenario that needs reserves alike in what they serve and unlike in what they charge, how fast they settle and
/// how well they evidence it, installs by name. The guests build a provider from its name alone, as they seed its keys
/// from it, so the name is where a shape is kept.
///
/// - `sim-reserve-a-quick` sits beside `sim-reserve-a`, serving its two tuples: it charges 5 000 on each movement,
///   declares thirty seconds to settle (Core prices a plain reserve at sixty) and declares a public record.
/// - `sim-reserve-slow` declares four days to settle ([`SLOW_SECONDS`]).
/// - `sim-reserve-charging` charges 7 on each movement beyond what it quoted.
/// - `sim-reserve-holding` declares that it answers the operator's hold requests, and holds the quote asked
///   (provider-role 4.0.0, `REQ-QTE-15`, `T-020-56`).
/// - `sim-reserve-dear` charges 6 000 000 on each movement, quoted.
/// - `sim-reserve-gross` charges [`GROSS_FEE`] on each movement, quoted, and reports gross (`T-024-13`): it states
///   giving out what it took in, its charge within it, so what arrives is that less the charge.
/// - `sim-reserve-gross-charging` is that, charging 7 beyond what it quoted.
///
/// Two groups of providers that reach one another's nodes (`T-024-04`), each of one publisher where it has a private
/// node (a private node is its publisher's alone) and its public nodes on the operator's list. The first is a route
/// across a reserve, an exchange and a ramp, and a second reserve (`AC-1`, `AC-2`, `AC-30`):
///
/// - `sim-join-in`, a reserve between its bank and the private node `sim-join-ledger`, in COP, its tuples declaring
///   a fill increment of 10 (`T-020-35`);
/// - `sim-join-exchange`, at that node, from COP to USD, a statement by its publisher;
/// - `sim-join-ramp`, from that node to the public node `sim-join-chain`, in USD, charging nothing and declaring
///   600 seconds; `sim-join-ramp-quick`, the same tuple at 5 000 a transfer and 30 seconds; `sim-join-ramp-steps`,
///   the first with internal steps (`REQ-CAT-21`);
/// - `sim-join-out`, a reserve in USD between that public node and its own bank.
///
/// The second is on-chain: no exchange, no local money (`AC-26`, `AC-27`), all in USDC at two decimals:
///
/// - `sim-chain-in`, a reserve between the network `sim-chain-a` and its private node `sim-chain-ledger`;
/// - `sim-chain-ramp-b`, from that node to the public node `sim-chain-b`; `sim-chain-ramp-c`, from `sim-chain-b` to
///   `sim-chain-c`;
/// - `sim-chain-out-c`, a reserve between `sim-chain-c` and the network `sim-chain-d`, and `sim-chain-out-b`, one
///   between `sim-chain-b` and `sim-chain-e`; each deposits to the address the requester provided
///   (`beneficiary.address`).
pub fn shape_of(provider: &str) -> Shape {
    use rutent_contracts::provider_role::EvidenceGrade;
    let join = |node: &str| Shape {
        publisher: Some("sim-join-publisher".to_owned()),
        node: Some(node.to_owned()),
        ..Shape::default()
    };
    let pair_a = || Shape {
        publisher: Some("sim-pair-a-publisher".to_owned()),
        node: Some("sim-pair-a-ledger".to_owned()),
        asset: Some("COP".to_owned()),
        ..Shape::default()
    };
    let chain = |node: &str| Shape {
        publisher: Some("sim-chain-publisher".to_owned()),
        node: Some(node.to_owned()),
        ..Shape::default()
    };
    match provider {
        "sim-join-in" => Shape {
            increment: Some(10),
            ..join("sim-join-ledger")
        },
        "sim-join-exchange" => Shape {
            asset: Some("COP".to_owned()),
            to_asset: Some("USD".to_owned()),
            grade: Some(EvidenceGrade::SignedByProvider),
            ..join("sim-join-ledger")
        },
        "sim-join-ramp" | "sim-join-ramp-steps" => Shape {
            asset: Some("USD".to_owned()),
            outer: Some("sim-join-chain".to_owned()),
            settles_in: Some(600),
            internal_steps: provider.ends_with("-steps"),
            ..join("sim-join-ledger")
        },
        "sim-join-ramp-quick" => Shape {
            asset: Some("USD".to_owned()),
            outer: Some("sim-join-chain".to_owned()),
            fee: 5_000,
            settles_in: Some(30),
            ..join("sim-join-ledger")
        },
        "sim-join-out" => Shape {
            asset: Some("USD".to_owned()),
            node: Some("sim-join-chain".to_owned()),
            public_node: true,
            ..Shape::default()
        },
        // A route across two private nodes (`T-024-05`, `DEC-24`), in COP, two decimals, in four segments (the router
        // takes at most four): a withdrawal into A's node, a ramp from it onto the public node between the two, a ramp
        // from that into B's node, and a deposit out of B's node. Each publisher shares its node among its providers;
        // a private node is its publisher's alone.
        //
        // - `sim-pair-in` (publisher A): a reserve between its bank and the private node `sim-pair-a-ledger`;
        //   `sim-pair-ramp-a` (A): from that node to the public node `sim-pair-x`;
        // - `sim-pair-ramp-b` (B): from `sim-pair-x` into B's private node `sim-pair-b-ledger`;
        //   `sim-pair-out` (B): a reserve between that node and its own bank.
        "sim-pair-in" => pair_a(),
        "sim-pair-ramp-a" => Shape {
            outer: Some("sim-pair-x".to_owned()),
            asset: Some("COP".to_owned()),
            ..pair_a()
        },
        "sim-pair-ramp-b" => Shape {
            node: Some("sim-pair-x".to_owned()),
            public_node: true,
            outer: Some("sim-pair-b-ledger".to_owned()),
            outer_private: true,
            asset: Some("COP".to_owned()),
            publisher: Some("sim-pair-b-publisher".to_owned()),
            ..Shape::default()
        },
        "sim-pair-out" => Shape {
            node: Some("sim-pair-b-ledger".to_owned()),
            publisher: Some("sim-pair-b-publisher".to_owned()),
            ..Shape::default()
        },
        // A reserve out of the joined route's public node whose bank rail its publisher declares private, with no
        // public record of its own (`sim-join-out-private`); and one whose rail has no public record and is declared
        // by no one (`sim-join-out-silent`), which no route can end in (`AC-34`).
        "sim-join-out-private" => Shape {
            asset: Some("USD".to_owned()),
            node: Some("sim-join-chain".to_owned()),
            public_node: true,
            rail_private: true,
            rail_unserved: true,
            ..Shape::default()
        },
        // Two reserves of two publishers on one public ledger (`T-020-39`): `sim-duo-in` between its bank and the
        // ledger `sim-duo-in-ledger`, which the operator lists public, and `sim-duo-out` between that ledger and its own
        // bank, so a route withdraws at the one and deposits at the other and its order involves two parties.
        "sim-duo-in" => Shape {
            public_node: true,
            ..Shape::default()
        },
        "sim-duo-out" => Shape {
            node: Some("sim-duo-in-ledger".to_owned()),
            public_node: true,
            ..Shape::default()
        },
        // An exchange of a third publisher at the public ledger `sim-duo-in-ledger`, from COP to USD, and a reserve of a
        // fourth, in USD, between that ledger and its own bank (`T-024-23`): a route that withdraws at `sim-duo-in`,
        // converts at the exchange and deposits at the reserve has a failing segment whose provider is not its origin
        // reserve's publisher, so the origin reserve's word that the value is back counts (`AC-4`).
        "sim-duo-exchange" => Shape {
            node: Some("sim-duo-in-ledger".to_owned()),
            public_node: true,
            asset: Some("COP".to_owned()),
            to_asset: Some("USD".to_owned()),
            ..Shape::default()
        },
        "sim-duo-out-usd" => Shape {
            asset: Some("USD".to_owned()),
            node: Some("sim-duo-in-ledger".to_owned()),
            public_node: true,
            ..Shape::default()
        },
        // A reserve whose node no publisher declares and the operator doesn't list (`REQ-CAT-22`).
        "sim-ghost" => Shape {
            node: Some("sim-ghost-ledger".to_owned()),
            undeclared_node: true,
            ..Shape::default()
        },
        "sim-join-out-silent" => Shape {
            asset: Some("USD".to_owned()),
            node: Some("sim-join-chain".to_owned()),
            public_node: true,
            rail_unserved: true,
            ..Shape::default()
        },
        "sim-chain-in" => Shape {
            asset: Some("USDC".to_owned()),
            outer: Some("sim-chain-a".to_owned()),
            ..chain("sim-chain-ledger")
        },
        "sim-chain-ramp-b" => Shape {
            asset: Some("USDC".to_owned()),
            outer: Some("sim-chain-b".to_owned()),
            ..chain("sim-chain-ledger")
        },
        "sim-chain-ramp-c" => Shape {
            asset: Some("USDC".to_owned()),
            node: Some("sim-chain-b".to_owned()),
            public_node: true,
            outer: Some("sim-chain-c".to_owned()),
            ..Shape::default()
        },
        "sim-chain-out-c" | "sim-chain-out-b" => {
            let (node, bank) = if provider.ends_with('c') {
                ("sim-chain-c", "sim-chain-d")
            } else {
                ("sim-chain-b", "sim-chain-e")
            };
            Shape {
                asset: Some("USDC".to_owned()),
                node: Some(node.to_owned()),
                public_node: true,
                outer: Some(bank.to_owned()),
                deposit_input: Some("beneficiary.address".to_owned()),
                ..Shape::default()
            }
        }
        "sim-reserve-a-quick" => Shape {
            fee: 5_000,
            settles_in: Some(30),
            grade: Some(rutent_contracts::provider_role::EvidenceGrade::PublicRecord),
            beside: Some("sim-reserve-a".to_owned()),
            ..Shape::default()
        },
        "sim-reserve-slow" => Shape {
            settles_in: Some(SLOW_SECONDS),
            ..Shape::default()
        },
        "sim-reserve-charging" => Shape {
            extra_charge: 7,
            ..Shape::default()
        },
        "sim-reserve-holding" => Shape {
            holds: true,
            ..Shape::default()
        },
        "sim-reserve-holding-briefly" => Shape {
            holds: true,
            holds_for: Some(BRIEF_HOLD_SECONDS),
            ..Shape::default()
        },
        "sim-reserve-dear" => Shape {
            fee: 6_000_000,
            ..Shape::default()
        },
        "sim-reserve-gross" => Shape {
            fee: GROSS_FEE,
            gross: true,
            ..Shape::default()
        },
        "sim-reserve-gross-charging" => Shape {
            fee: GROSS_FEE,
            extra_charge: 7,
            gross: true,
            ..Shape::default()
        },
        _ => Shape::default(),
    }
}

/// A stand-in the simulation can drive: a provider with the harness's hooks.
pub trait Standin: RoleProvider + RoleHarness {}

impl<T: RoleProvider + RoleHarness> Standin for T {}

/// What a provider does with an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    /// Answers as its role asks.
    #[default]
    Answer,
    /// Refuses finally, whatever the operation, and does nothing.
    Refuse,
    /// Refuses, saying when it lifts.
    RefuseLifting,
    /// Refuses, saying it waits on the person.
    RefuseWaitingOnThePerson,
    /// Does the operation and reports outcome unknown: the provider's own result, which its status query settles
    /// later (`REQ-ORD-1`). It is not the platform's `OutcomeUnknown`, which
    /// refuses an operation that passed something out and then ended with no answer.
    Unknown,
    /// Fails the operation, with evidence, having moved nothing on.
    Fail,
}

/// How much a provider delivers of what it took in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// All of it.
    #[default]
    Full,
    /// Short: `bps` basis points less than it priced, in a result that still states what it took in (`REQ-ORD-21`,
    /// `THR-109`).
    Short {
        /// The share not delivered, in basis points.
        bps: u16,
    },
    /// In pieces: `count` movements and as many evidence pieces, one each, which add up to what it delivered
    /// (`REQ-ORD-30`).
    Pieces {
        /// How many, from two to eight.
        count: u8,
    },
}

/// Where in an operation a module stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crash {
    /// Before it has written anything: the operation took no effect.
    BeforeSettle,
    /// After it has written its entry and sent its movements: the operation took effect, and nothing says so.
    AfterSettle,
}

/// How a simulated provider answers one operation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behaviour {
    /// What it does with the operation.
    pub act: Act,
    /// How much it delivers.
    pub delivery: Delivery,
    /// Never answers, and does nothing: the module doesn't get as far as the operation.
    pub silent: bool,
    /// Does the operation, and answers too late: the module takes effect, and then doesn't answer within its
    /// deadline. A repeat of the operation later is answered as it was.
    pub late: bool,
    /// Crashes mid-operation.
    pub crash: Option<Crash>,
    /// For a reserve asked to confirm a reversal: the credit arrived, so it confirms. Without it a reserve says no
    /// credit has arrived (ADR-0008).
    pub reversal_credited: bool,
    /// Moves value on no public network, though it says it did: its movements are never sent, so the network
    /// records none.
    pub unrecorded: bool,
    /// Records, in its publisher's books, this amount out for the segment, though it delivered another: the
    /// publisher's record disagrees with what the segment did, and a replay finds the difference (`REQ-ORD-41`,
    /// `AC-35`). The result and the movement are as the segment did them.
    pub misrecorded_out: Option<String>,
    /// Moves value on a public network with its charge on the wrong side (`T-024-13`): a withdrawal's movement is
    /// recorded at what reached the ledger, net of the charge, instead of what entered; a deposit's or a transfer's at
    /// the `amount_out` it states, its charge within, instead of what arrives. The result it gives is unchanged.
    pub charge_misplaced: bool,
    /// Whose fault it shows a failure was, on a result that fails (`T-024-23`, `REQ-ORD-11`): the customer's or the
    /// operator's. The result it signs says so in its `fault`; without it a failed result shows none, as the provider
    /// carrying the reversal does. It changes nothing in the books, and a result that doesn't fail carries none.
    pub fault: Option<rutent_contracts::provider_role::Fault>,
    /// With [`Act::Unknown`]: the operation, left unknown, is one that failed; the provider's answer to a status query
    /// of it says so (`REQ-ORD-1`, `T-020-47`). Only a ramp's transfer can be driven so; another's outcome is its
    /// own.
    pub unknown_then_fails: bool,
    /// On a dry run it accepts, the terms it hands back for the tuple asked: its own statement, nested in its answer,
    /// which the platform signs with its key (provider-role 4.0.0, `REQ-QTE-6`, `T-020-56`), stating this fee formula.
    /// A reserve's or a ramp's tuple alone, whose two ends hold one asset: an exchange's sets a rate, whose slippage
    /// regime the stand-in doesn't state. Nothing on a real operation, a refusal or another question.
    pub terms: Option<rutent_contracts::provider_role::FeeFormula>,
}

impl Behaviour {
    /// A behaviour of `act`.
    pub fn act(act: Act) -> Self {
        Self {
            act,
            ..Self::default()
        }
    }

    /// Outcome unknown, and failed: nothing moved on, and a status query of it later says so (`T-020-47`).
    pub fn unknown_that_failed() -> Self {
        Self {
            act: Act::Unknown,
            unknown_then_fails: true,
            ..Self::default()
        }
    }

    /// Delivering `delivery`.
    pub fn delivering(delivery: Delivery) -> Self {
        Self {
            delivery,
            ..Self::default()
        }
    }

    /// Crashing at `crash`.
    pub fn crashing(crash: Crash) -> Self {
        Self {
            crash: Some(crash),
            ..Self::default()
        }
    }

    /// Silent.
    pub fn silent() -> Self {
        Self {
            silent: true,
            ..Self::default()
        }
    }

    /// Late.
    pub fn late() -> Self {
        Self {
            late: true,
            ..Self::default()
        }
    }

    /// Recording `amount_out` as what the segment delivered, in its publisher's books.
    pub fn misrecording(amount_out: &str) -> Self {
        Self {
            misrecorded_out: Some(amount_out.to_owned()),
            ..Self::default()
        }
    }

    /// Confirming the credit of a reversal.
    pub fn crediting_a_reversal() -> Self {
        Self {
            reversal_credited: true,
            ..Self::default()
        }
    }

    /// Failing the operation, showing that `fault` was whose (`T-024-23`).
    pub fn failing_by(fault: rutent_contracts::provider_role::Fault) -> Self {
        Self {
            act: Act::Fail,
            fault: Some(fault),
            ..Self::default()
        }
    }

    /// Handing back, on each dry run it accepts, its terms for the tuple asked: a fee of at most `maximum`, with no
    /// flat or proportional part (`T-020-56`).
    pub fn stating_terms_of_at_most(maximum: &str) -> Self {
        Self {
            terms: Some(rutent_contracts::provider_role::FeeFormula {
                flat: "0".to_owned(),
                proportional: rutent_contracts::provider_role::Proportion {
                    numerator: "0".to_owned(),
                    denominator: "1".to_owned(),
                },
                maximum: Some(maximum.to_owned()),
            }),
            ..Self::default()
        }
    }

    /// Undoing a segment, and stating the value back short by `bps` basis points (`T-024-23`): the provider's own
    /// reversal says less than the segment took in, or, for a reserve asked to confirm, the credit that arrived is less.
    pub fn short_by(bps: u16) -> Self {
        Self {
            delivery: Delivery::Short { bps },
            ..Self::default()
        }
    }
}

/// What changed a provider's books: one operation, as it was asked, and how it was answered. Replaying a journal of
/// them into a fresh stand-in gives the books back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The operation, the operator's statement.
    pub operation: Statement<Operation>,
    /// The behaviour it was answered with.
    pub behaviour: Behaviour,
}

/// One movement on a simulated public network that an operation made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Movement {
    /// The network it is on.
    pub network: String,
    /// Its identifier: the operation's transaction, or that and the piece's number.
    pub id: String,
    /// Where it went.
    pub destination: String,
    /// The asset.
    pub asset: String,
    /// The amount, in the network's unit.
    pub amount: String,
}

/// How an operation was answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The result body's JSON, as the module returns it: the platform checks it, signs it and sends it on.
    pub body: Vec<u8>,
    /// The entry the operation made in the books, if it changed them.
    pub entry: Option<Entry>,
    /// The movements on public networks it made.
    pub movements: Vec<Movement>,
}

/// A stand-in under a [`Spec`], answering as scripts say.
pub struct Simulated {
    spec: Spec,
    context: SuiteContext,
    provider: Box<dyn Standin>,
    applied: usize,
    /// The transactions of the real movements it has served, so a repeat of one, which moves nothing, makes no
    /// second entry in the journal.
    seen: BTreeSet<String>,
    /// The entries its books hold, in order: what a rebuild replays.
    log: Vec<Entry>,
}

impl Simulated {
    /// The simulated provider `spec` names, with empty books.
    pub fn new(spec: Spec, context: &SuiteContext) -> Self {
        let provider = spec.make(context);
        Self {
            spec,
            context: context.clone(),
            provider,
            applied: 0,
            seen: BTreeSet::new(),
            log: Vec::new(),
        }
    }

    /// Its identity.
    pub fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Its advertisement, as the stand-in signs it.
    pub fn advertisement(&self) -> Received<Advertisement> {
        self.provider.advertisement()
    }

    /// The fingerprint of its books.
    pub fn state(&self) -> String {
        self.provider.state()
    }

    /// How many entries of the journal its books hold.
    pub fn applied(&self) -> usize {
        self.applied
    }

    /// Records that its books took one more entry of the journal.
    pub fn applied_one(&mut self) {
        self.applied += 1;
    }

    /// Forgets how much of the journal its books hold, so the next operation replays it: after an entry that
    /// couldn't be written, the books hold what the journal doesn't.
    pub fn diverged(&mut self) {
        self.applied = usize::MAX;
    }

    /// Hands the provider the hooks the role suite drives: the next real operation ends with outcome unknown.
    pub fn force_outcome_unknown(&mut self) -> bool {
        self.provider.force_outcome_unknown()
    }

    /// Hands the provider the hooks the role suite drives: the next real operation ends with outcome unknown, and failed.
    pub fn force_outcome_unknown_then_failure(&mut self) -> bool {
        self.provider.force_outcome_unknown_then_failure()
    }

    /// Hands the provider the hooks the role suite drives: the next real operation fails.
    pub fn force_failure(&mut self) -> bool {
        self.provider.force_failure()
    }

    /// The internal steps the provider took to serve what its books hold, in order (`REQ-CAT-21`): how it served its
    /// segments, which the route doesn't show.
    pub fn internal_steps(&self) -> Vec<String> {
        self.provider.internal_steps()
    }

    /// Hands the provider the hooks the role suite drives: the reversed credit arrived.
    pub fn credit_reversal(
        &mut self,
        segment: &str,
        node: &str,
        asset: &str,
        amount: &str,
    ) -> bool {
        self.provider.credit_reversal(segment, node, asset, amount)
    }

    /// Puts the books back to the journal's: a fresh stand-in, with every entry replayed into it. Replay is
    /// deterministic: the same entries give the same books.
    pub fn restore(&mut self, journal: &[Entry]) {
        self.replay(journal.to_vec());
        self.applied = journal.len();
    }

    /// Makes the books those of `entries`, replayed into a fresh stand-in.
    fn replay(&mut self, entries: Vec<Entry>) {
        self.provider = self.spec.make(&self.context);
        self.seen.clear();
        self.log.clear();
        for entry in entries {
            self.answer(&entry.operation, &entry.behaviour);
        }
    }

    /// Answers `operation` as `behaviour` says. The books change as the answer says they did: a refusal changes
    /// nothing, and the entry is returned for the journal.
    pub fn answer(&mut self, operation: &Statement<Operation>, behaviour: &Behaviour) -> Answer {
        let refusing = matches!(
            behaviour.act,
            Act::Refuse | Act::RefuseLifting | Act::RefuseWaitingOnThePerson
        );
        if refusing {
            let body = refusal_to(&operation.statement, behaviour.act_class());
            return Answer {
                body: serde_json::to_vec(&body).expect("a result serialises"),
                entry: None,
                movements: Vec::new(),
            };
        }
        // A status question moves nothing: the provider answers with its own result of the transaction asked about, as
        // its books hold it, or says it knows none (`REQ-ORD-1`, `T-013-11`'s query, `T-020-47`).
        if let Operation::StatusQuery(q) = &operation.statement {
            use serde_json::json;
            let outcome = match self.provider.status(&q.about) {
                Some(result) => json!({
                    "status": "answered",
                    "result": serde_json::to_value(result.statement()).expect("a result serialises"),
                }),
                None => json!({ "status": "not_known" }),
            };
            let body = json!({
                "kind": "status_query",
                "transaction": q.transaction,
                "about": q.about,
                "outcome": outcome,
            });
            return Answer {
                body: serde_json::to_vec(&body).expect("a result serialises"),
                entry: None,
                movements: Vec::new(),
            };
        }
        // A hold request moves nothing: a reserve that declares it answers them holds the quote asked, its own hold of
        // exactly the request's transaction, record, candidate and whom it is for, until the validity asked, nested in
        // its answer for the platform to sign; any other refuses it (`REQ-QTE-15`, `T-020-56`).
        if let Operation::Hold(h) = &operation.statement {
            let body = match self.hold(operation, h) {
                Some(hold) => serde_json::json!({
                    "kind": "hold",
                    "transaction": h.transaction,
                    "outcome": { "status": "held", "hold": hold },
                }),
                None => serde_json::to_value(refusal_to(
                    &operation.statement,
                    crate::role::stand_in::RefusalClass::Final,
                ))
                .expect("a result serialises"),
            };
            return Answer {
                body: serde_json::to_vec(&body).expect("a result serialises"),
                entry: None,
                movements: Vec::new(),
            };
        }
        let before = self.provider.state();
        let mut credited = false;
        if behaviour.reversal_credited
            && let Operation::ReversalConfirmation(c) = &operation.statement
        {
            // The reserve's node the value is shown back at: its private one, or its ledger where that is a public node.
            let node = self
                .provider
                .advertisement()
                .into_statement()
                .statement
                .private_nodes
                .first()
                .cloned()
                .unwrap_or_else(|| self.spec.ledger());
            // A credit that arrived short (`T-024-23`) is less than the segment took in, so the reserve is asked about
            // an amount that never arrived.
            let arrived = match behaviour.delivery {
                Delivery::Short { bps } => {
                    short_of(&c.amount, bps).unwrap_or_else(|| c.amount.clone())
                }
                _ => c.amount.clone(),
            };
            self.provider
                .credit_reversal(&c.segment, &node, &c.asset, &arrived);
            credited = true;
        }
        // A hook is armed for a real movement alone: the stand-in disarms it only when a movement succeeds or
        // fails, and an operation that isn't one would leave it for the next.
        let armed = real_movement(&operation.statement)
            && matches!(behaviour.act, Act::Unknown | Act::Fail);
        if armed {
            match behaviour.act {
                Act::Unknown if behaviour.unknown_then_fails => {
                    self.provider.force_outcome_unknown_then_failure();
                }
                Act::Unknown => {
                    self.provider.force_outcome_unknown();
                }
                _ => {
                    self.provider.force_failure();
                }
            }
        }
        let received = self.provider.handle(operation);
        // The books changed where an operation moved value for the first time, or where the fingerprint says they
        // did, as an exchange's repeat does, or where a credit came in.
        let first = real_movement(&operation.statement)
            && transaction_of(&operation.statement).is_some_and(|t| self.seen.insert(t));
        let changed = credited || first || self.provider.state() != before;
        let mut body =
            serde_json::to_value(&received.statement().statement).expect("a result serialises");
        // Whether the hook did what it was armed for. Where it didn't, the operation was one the stand-in answered
        // without moving, a refusal or a repeat, and the hook is still armed in it: the books are rebuilt without it,
        // and the entry says the operation was answered as the role asks.
        let consumed = match behaviour.act {
            Act::Unknown => body["outcome"]["status"] == "unknown",
            _ => body["outcome"]["status"] == "failed",
        };
        if matches!(operation.statement, Operation::Reversal(_)) {
            deliver_reversal(&mut body, behaviour.delivery);
        } else {
            deliver(&mut body, behaviour.delivery);
        }
        show_fault(&mut body, behaviour.fault);
        if let Some(fee) = &behaviour.terms {
            self.hand_back_terms(operation, &mut body, fee);
        }
        // A repeat that changed nothing sends no movement again: a network that reorged the first would take it
        // as a new one in another block.
        let mut movements = if behaviour.unrecorded || !changed {
            Vec::new()
        } else {
            movements_made(
                &operation.statement,
                &body,
                Some(&self.provider.advertisement().into_statement().statement),
            )
        };
        // A deposit out to a rail its reserve's publisher declares private leaves no movement on the rail, which no public
        // network serves: the movement into it is the public node's the deposit started at, on the network that node
        // is, under the deposit's own transaction (`AC-34`, `T-024-05`).
        if movements.is_empty()
            && !behaviour.unrecorded
            && changed
            && shape_of(&self.spec.provider).rail_private
            && let rutent_contracts::provider_role::Operation::Deposit(deposit) =
                &operation.statement
        {
            let mut into = deposit.clone();
            into.destination.network.clone_from(&deposit.origin);
            movements = movements_made(
                &rutent_contracts::provider_role::Operation::Deposit(into),
                &body,
                None,
            );
        }
        if behaviour.charge_misplaced {
            misplace_the_charge(&operation.statement, &body, &mut movements);
        }
        let leaked = armed && !consumed;
        let entry = changed.then(|| Entry {
            operation: operation.clone(),
            behaviour: if leaked {
                Behaviour {
                    act: Act::Answer,
                    ..behaviour.clone()
                }
            } else {
                behaviour.clone()
            },
        });
        if let Some(entry) = &entry {
            self.log.push(entry.clone());
        }
        if leaked {
            let log = std::mem::take(&mut self.log);
            self.replay(log);
        }
        Answer {
            body: serde_json::to_vec(&body).expect("a result serialises"),
            entry,
            movements,
        }
    }
}

impl Simulated {
    /// The statement envelope a stand-in nests in its answer in its own name, of `shape`, about `statement`: in its
    /// deployment and protocol version, made when the operation was, with a placeholder signature under its statements
    /// key, which the platform replaces with the provider's own (`T-014-05`).
    fn nested(&self, operation: &Statement<Operation>, shape: &str, statement: Value) -> Value {
        let advertised = self.provider.advertisement().into_statement().statement;
        let key = advertised
            .keys
            .iter()
            .find(|k| k.purpose == rutent_contracts::provider_role::KeyPurpose::Statements)
            .map(|k| k.public_key.as_str().to_owned())
            .unwrap_or_default();
        serde_json::json!({
            "deployment": self.context.deployment,
            "protocol_version": self.context.protocol_version,
            "shape": shape,
            "maker": self.spec.provider,
            "made_at": operation.made_at,
            "signature": {
                "scheme": "ed25519",
                "key": key,
                "value": crate::role::placeholder_signature(),
            },
            "statement": statement,
        })
    }

    /// The reserve's hold of what `request` asks, where it declares that it answers hold requests and is the reserve
    /// asked; `None` otherwise.
    fn hold(
        &self,
        operation: &Statement<Operation>,
        request: &rutent_contracts::provider_role::HoldRequest,
    ) -> Option<Value> {
        let shape = shape_of(&self.spec.provider);
        let answers = self.spec.role == Role::Reserve
            && shape.holds
            && request.provider == self.spec.provider;
        // A reserve holding briefly holds no later than its own limit from when it was asked (`T-020-57`).
        let valid_until = shape
            .holds_for
            .and_then(|seconds| later_by(&operation.made_at, seconds))
            .filter(|brief| seconds_of(brief) < seconds_of(&request.valid_until))
            .unwrap_or_else(|| request.valid_until.clone());
        answers.then(|| {
            self.nested(
                operation,
                rutent_contracts::provider_role::HOLD_SHAPE,
                serde_json::json!({
                    "transaction": request.transaction,
                    "record": request.record,
                    "candidate": request.candidate,
                    "for": request.held_for,
                    "valid_until": valid_until,
                }),
            )
        })
    }

    /// Nests in `body`, its answer to `operation`, its terms for the tuple asked, stating `fee`, where `operation` is a
    /// dry run it accepted of a tuple it advertises whose ends hold one asset (`T-020-56`).
    fn hand_back_terms(
        &self,
        operation: &Statement<Operation>,
        body: &mut Value,
        fee: &rutent_contracts::provider_role::FeeFormula,
    ) {
        if body["outcome"]["status"] != "dry_run_accepted" {
            return;
        }
        let Some(tuple) = self.tuple_asked(&operation.statement) else {
            return;
        };
        let terms = self.nested(
            operation,
            rutent_contracts::provider_role::TERMS_SHAPE,
            serde_json::json!({ "tuple": tuple, "fee": fee }),
        );
        body["outcome"]["terms"] = terms;
    }

    /// The tuple a dry run asks about, as terms name it: the provider, and each end's node with its asset in its unit
    /// there, as the provider advertises it. A reserve's withdrawal or deposit, or a ramp's transfer; `None` for any
    /// other operation, a real one, or a tuple it doesn't advertise.
    fn tuple_asked(&self, operation: &Operation) -> Option<Value> {
        use rutent_contracts::provider_role::NodeRef;
        let advertised = self.provider.advertisement().into_statement().statement;
        let end = |node: Value, asset: &str, decimals: u8| serde_json::json!({ "node": node, "asset": { "id": asset, "decimals": decimals } });
        let (origin, destination, asset) = match operation {
            Operation::Withdrawal(o) if o.dry_run => (
                NodeRef::Outer(o.origin.clone()),
                NodeRef::Inner(o.destination.clone()),
                &o.asset,
            ),
            Operation::Deposit(o) if o.dry_run => (
                NodeRef::Inner(o.origin.clone()),
                NodeRef::Outer(o.destination.clone()),
                &o.asset,
            ),
            Operation::Transfer(o) if o.dry_run => {
                let tuple = advertised.offers.ramp.as_ref()?.tuples.iter().find(|t| {
                    t.origin == o.origin && t.destination == o.destination && t.asset == o.asset
                })?;
                return Some(serde_json::json!({
                    "provider": self.spec.provider,
                    "from": end(o.origin.clone().into(), &o.asset, tuple.decimals.origin),
                    "to": end(o.destination.clone().into(), &o.asset, tuple.decimals.destination),
                }));
            }
            _ => return None,
        };
        let tuple = advertised
            .offers
            .reserve
            .as_ref()?
            .tuples
            .iter()
            .find(|t| t.origin == origin && t.destination == destination && &t.asset == asset)?;
        Some(serde_json::json!({
            "provider": self.spec.provider,
            "from": end(serde_json::to_value(&origin).ok()?, asset, tuple.decimals.origin),
            "to": end(serde_json::to_value(&destination).ok()?, asset, tuple.decimals.destination),
        }))
    }
}

impl Behaviour {
    fn act_class(&self) -> crate::role::stand_in::RefusalClass {
        use crate::role::stand_in::RefusalClass;
        match self.act {
            Act::RefuseLifting => RefusalClass::LiftsAtKnownTime,
            Act::RefuseWaitingOnThePerson => RefusalClass::WaitsOnPerson,
            _ => RefusalClass::Final,
        }
    }
}

/// The operator's statement carrying `statement`, a provider-role operation as JSON, in `context`: what the platform
/// hands a module.
pub fn operation(context: &SuiteContext, statement: Value) -> Statement<Operation> {
    serde_json::from_value(serde_json::json!({
        "deployment": context.deployment,
        "protocol_version": context.protocol_version,
        "shape": rutent_contracts::provider_role::OPERATION_SHAPE,
        "maker": context.operator,
        "made_at": "2026-11-09T00:00:00Z",
        "signature": {
            "scheme": "ed25519",
            "key": context.deployment.operator_key,
            "value": crate::role::placeholder_signature(),
        },
        "statement": statement,
    }))
    .expect("an operator's statement carrying an operation")
}

/// An operation's kind, as its statement names it, and whether it is a dry run: what a script is read under.
pub fn kind_of(operation: &Operation) -> (String, bool) {
    let value = serde_json::to_value(operation).expect("an operation serialises");
    (
        value["kind"].as_str().unwrap_or_default().to_owned(),
        value["dry_run"].as_bool().unwrap_or(false),
    )
}

/// Whether `operation` moves value for real: a withdrawal, deposit, transfer or conversion that isn't a dry run,
/// or a reversal.
fn real_movement(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Withdrawal(_)
            | Operation::Deposit(_)
            | Operation::Transfer(_)
            | Operation::Conversion(_)
    ) && !kind_of(operation).1
        || matches!(operation, Operation::Reversal(_))
}

/// Records a one-piece movement with its charge on the wrong side ([`Behaviour::charge_misplaced`]): what a
/// withdrawal took in less its charge, or what a deposit or a transfer states giving out, its charge within it.
fn misplace_the_charge(operation: &Operation, result: &Value, movements: &mut [Movement]) {
    let [moved] = movements else {
        return;
    };
    let outcome = &result["outcome"];
    let number = |name: &str| outcome[name].as_str().and_then(|a| a.parse::<u128>().ok());
    let wrong = match operation {
        Operation::Withdrawal(_) => number("amount_in")
            .zip(
                outcome["fee"]["amount"]
                    .as_str()
                    .and_then(|a| a.parse::<u128>().ok()),
            )
            .map(|(entered, fee)| entered.saturating_sub(fee)),
        Operation::Deposit(_) | Operation::Transfer(_) => number("amount_out"),
        _ => None,
    };
    if let Some(wrong) = wrong {
        moved.amount = wrong.to_string();
    }
}

/// `amount` less `bps` basis points of it, where it is a number.
/// How long `sim-reserve-holding-briefly` holds a quote, at most, from when it was asked (`T-020-57`).
pub const BRIEF_HOLD_SECONDS: u64 = 60;

/// The seconds since 1970 of `time`, an RFC 3339 time in UTC (`…Z`), its fraction dropped; `None` for any other.
fn seconds_of(time: &str) -> Option<i64> {
    let (date, clock) = time.strip_suffix('Z')?.split_once('T')?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let clock = clock.split('.').next()?;
    let mut c = clock.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (h, min, sec) = (c.next()??, c.next()??, c.next()??);
    // Days from the civil date (H. Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + min * 60 + sec)
}

/// `time`, an RFC 3339 time in UTC, `seconds` later, to the whole second, written `YYYY-MM-DDTHH:MM:SSZ`.
fn later_by(time: &str, seconds: u64) -> Option<String> {
    let total = seconds_of(time)?.checked_add(i64::try_from(seconds).ok()?)?;
    let (days, rest) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    // The civil date of a day count (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    ))
}

fn short_of(amount: &str, bps: u16) -> Option<String> {
    let whole = amount.parse::<u128>().ok()?;
    Some((whole - whole * u128::from(bps.min(10_000)) / 10_000).to_string())
}

/// Applies a delivery to a reversal's result: a settled one states the value back, and a short delivery states less of
/// it than the segment took in (`T-024-23`). A reversal moves no pieces and gives out nothing, so nothing else changes.
fn deliver_reversal(body: &mut Value, delivery: Delivery) {
    let Delivery::Short { bps } = delivery else {
        return;
    };
    let Some(outcome) = body.get_mut("outcome") else {
        return;
    };
    if outcome["status"] != "succeeded" {
        return;
    }
    if let Some(short) = outcome["amount"].as_str().and_then(|a| short_of(a, bps)) {
        outcome["amount"] = Value::from(short);
    }
}

/// Sets the fault a failed result shows (`T-024-23`): only a result that failed names one.
fn show_fault(body: &mut Value, fault: Option<rutent_contracts::provider_role::Fault>) {
    let Some(fault) = fault else {
        return;
    };
    let Some(outcome) = body.get_mut("outcome") else {
        return;
    };
    if outcome["status"] == "failed" {
        outcome["fault"] = serde_json::to_value(fault).expect("a fault serialises");
    }
}

/// Applies a delivery to a result's JSON: only a succeeded outcome delivers anything.
fn deliver(body: &mut Value, delivery: Delivery) {
    let Some(outcome) = body.get_mut("outcome") else {
        return;
    };
    if outcome["status"] != "succeeded" {
        return;
    }
    match delivery {
        Delivery::Full => {}
        Delivery::Short { bps } => {
            let Some(out) = outcome["amount_out"]
                .as_str()
                .and_then(|a| a.parse::<u128>().ok())
            else {
                return;
            };
            let kept = out - out * u128::from(bps.min(10_000)) / 10_000;
            outcome["amount_out"] = Value::from(kept.to_string());
        }
        Delivery::Pieces { count } => {
            let count = count.clamp(2, 8);
            let Some(first) = outcome["evidence"].get(0).cloned() else {
                return;
            };
            let reference = first["reference"].as_str().unwrap_or_default().to_owned();
            let pieces: Vec<Value> = (1..=count)
                .map(|i| {
                    let mut piece = first.clone();
                    piece["reference"] = Value::from(format!("{reference}.{i}"));
                    piece
                })
                .collect();
            outcome["evidence"] = Value::Array(pieces);
        }
    }
}

/// The movements on public networks an operation made, from its result: none unless it succeeded. A result with
/// several evidence pieces made as many movements, `{transaction}.{n}` each, which split what it moved. What the
/// network moved is what a public node saw: value entering a reserve from a public network is what entered, its
/// charge included (`amount_in`); value leaving onto one is what arrives, a gross segment's `amount_out` less its
/// charge. Without the advertisement's terms a tuple counts as net.
pub fn movements_of(operation: &Operation, result: &Value) -> Vec<Movement> {
    movements_made(operation, result, None)
}

/// [`movements_of`], knowing from `advertisement` whether the tuple the operation ran through reports gross.
pub fn movements_made(
    operation: &Operation,
    result: &Value,
    advertisement: Option<&rutent_contracts::provider_role::Advertisement>,
) -> Vec<Movement> {
    let outcome = &result["outcome"];
    if outcome["status"] != "succeeded" {
        return Vec::new();
    }
    let number = |name: &str| outcome[name].as_str().and_then(|a| a.parse::<u128>().ok());
    let Some(given_out) = number("amount_out") else {
        return Vec::new();
    };
    // What the network moved: what entered a reserve from a public network, or what arrives onto one.
    let out = match operation {
        // What entered with its charge, always: the network moved it before the reserve credited anything, so a result
        // rewritten to deliver short is the reserve crediting less inside its own ledger.
        Operation::Withdrawal(_) => match number("amount_in") {
            Some(entered) => entered,
            None => return Vec::new(),
        },
        Operation::Deposit(_) | Operation::Transfer(_) => {
            match arriving(operation, outcome, given_out, advertisement) {
                Some(arrives) => arrives,
                None => return Vec::new(),
            }
        }
        _ => given_out,
    };
    let operation_json = serde_json::to_value(operation).expect("an operation serialises");
    let text = |name: &str| operation_json[name].as_str().unwrap_or_default().to_owned();
    let (network, destination, asset) = match operation {
        Operation::Withdrawal(w) => (
            w.origin.network.clone(),
            format!("{}-ledger", text("provider")),
            w.asset.clone(),
        ),
        Operation::Deposit(d) => (
            d.destination.network.clone(),
            d.inputs
                .get("beneficiary.bank_account")
                .or_else(|| d.inputs.get("beneficiary.address"))
                .cloned()
                .unwrap_or_else(|| format!("{}-beneficiary", text("provider"))),
            d.asset.clone(),
        ),
        Operation::Transfer(t) => (
            t.destination.clone(),
            t.inputs
                .get("beneficiary.address")
                .cloned()
                .unwrap_or_else(|| format!("{}-beneficiary", text("provider"))),
            t.asset.clone(),
        ),
        Operation::Conversion(c) => (
            c.node.clone(),
            format!("{}-pool", text("provider")),
            c.to.clone(),
        ),
        _ => return Vec::new(),
    };
    // Value moved into or within a private node is on no public network: its publisher's book records it.
    if advertisement.is_some_and(|ad| ad.private_nodes.contains(&network)) {
        return Vec::new();
    }
    let transaction = text("transaction");
    let pieces = outcome["evidence"].as_array().map_or(1, Vec::len).max(1);
    let each = out / pieces as u128;
    (0..pieces)
        .map(|i| Movement {
            network: network.clone(),
            id: if pieces == 1 {
                transaction.clone()
            } else {
                format!("{transaction}.{}", i + 1)
            },
            destination: destination.clone(),
            asset: asset.clone(),
            amount: if i + 1 == pieces {
                out - each * (pieces as u128 - 1)
            } else {
                each
            }
            .to_string(),
        })
        .collect()
}

/// One segment a provider on a private node served inside it, as its publisher records it (`REQ-ORD-41`, `DEC-24`):
/// the asset and amount in and out, and the fee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnerSegment {
    /// The transaction the segment ran under.
    pub transaction: String,
    /// The asset that came in.
    pub asset_in: String,
    /// How much came in.
    pub amount_in: String,
    /// The asset that went out.
    pub asset_out: String,
    /// How much went out.
    pub amount_out: String,
    /// The fee.
    pub fee: String,
}

/// The inner segments `journal` records, in order: one for each entry whose result succeeded, as the books were
/// kept (a delivery short of what was taken in shows in them, since the record is of what was delivered).
pub fn recorded_segments(
    spec: &Spec,
    context: &SuiteContext,
    journal: &[Entry],
) -> Vec<InnerSegment> {
    let mut replay = Simulated::new(spec.clone(), context);
    let mut recorded = Vec::new();
    for entry in journal {
        let answer = replay.answer(&entry.operation, &entry.behaviour);
        replay.applied += 1;
        let Ok(result) = serde_json::from_slice::<Value>(&answer.body) else {
            continue;
        };
        let outcome = &result["outcome"];
        if outcome["status"] != "succeeded" {
            continue;
        }
        let operation = serde_json::to_value(&entry.operation.statement).unwrap_or(Value::Null);
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let (asset_in, asset_out) = match operation["kind"].as_str() {
            Some("conversion") => (text(&operation["from"]), text(&operation["to"])),
            _ => (text(&operation["asset"]), text(&operation["asset"])),
        };
        recorded.push(InnerSegment {
            transaction: text(&operation["transaction"]),
            asset_in,
            amount_in: text(&outcome["amount_in"]),
            asset_out,
            amount_out: entry
                .behaviour
                .misrecorded_out
                .clone()
                .unwrap_or_else(|| text(&outcome["amount_out"])),
            fee: text(&outcome["fee"]["amount"]),
        });
    }
    recorded
}

/// The record a provider's publisher keeps of the segments `journal` holds, as the publisher's own signed statement
/// (`REQ-ORD-41`, `REQ-ORD-3`; provider-role's `inner-segments`): made by `spec`'s provider, which a publisher holds
/// no key to do itself, stating for `spec.publisher()` about the private node the provider serves in, each segment
/// that ran inside it with its transaction, asset and amount in and out and fee, labelled the publisher's word. It is
/// signed with the statements key the provider's advertisement lists. `None` where the provider serves in no private
/// node, or ran no segment inside it.
///
/// The record states what the books say, whatever they say: no figure is refused here, as the contract refuses none.
#[cfg(feature = "signing")]
pub fn publishers_record(
    spec: &Spec,
    context: &SuiteContext,
    journal: &[Entry],
) -> Option<Statement<InnerSegments>> {
    let node = spec.private_nodes().into_iter().next()?;
    let segments: Vec<RecordedInner> = recorded_segments(spec, context, journal)
        .into_iter()
        .map(|s| RecordedInner {
            transaction: s.transaction,
            asset_in: s.asset_in,
            amount_in: s.amount_in,
            asset_out: s.asset_out,
            amount_out: s.amount_out,
            fee: s.fee,
        })
        .collect();
    if segments.is_empty() {
        return None;
    }
    let key = placeholder_key(spec.seed());
    let record = Statement {
        deployment: context.deployment.clone(),
        protocol_version: context.protocol_version.clone(),
        shape: INNER_SEGMENTS_SHAPE.to_owned(),
        maker: spec.provider.clone(),
        made_at: crate::role::stand_in::made_at().to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key,
            value: placeholder_signature(),
        },
        statement: InnerSegments {
            node,
            publisher: spec.publisher(),
            standing: Standing::PublishersWord,
            segments,
        },
    };
    Some(crate::signing::sign_statement(
        &record,
        &crate::signing::real(record.signature.key.as_str()),
    ))
}

/// The transaction an operation names, if it names one.
pub fn transaction_of(operation: &Operation) -> Option<String> {
    serde_json::to_value(operation).ok()?["transaction"]
        .as_str()
        .map(str::to_owned)
}

/// What arrives by a deposit or a transfer that gave out `given_out`: that less its charge, restated in the out unit,
/// where its tuple reports gross; as it stands otherwise.
fn arriving(
    operation: &Operation,
    outcome: &Value,
    given_out: u128,
    advertisement: Option<&rutent_contracts::provider_role::Advertisement>,
) -> Option<u128> {
    use rutent_contracts::provider_role::{NodeRef, Reporting, less_the_charge};
    let Some(ad) = advertisement else {
        return Some(given_out);
    };
    let (reporting, units) = match operation {
        Operation::Deposit(d) => ad.offers.reserve.as_ref()?.tuples.iter().find_map(|t| {
            (matches!(t.origin, NodeRef::Inner(_))
                && matches!(t.destination, NodeRef::Outer(_))
                && t.asset == d.asset)
                .then_some((
                    t.terms.reporting,
                    (t.decimals.origin, t.decimals.destination),
                ))
        })?,
        Operation::Transfer(x) => ad.offers.ramp.as_ref()?.tuples.iter().find_map(|t| {
            (t.origin == x.origin && t.destination == x.destination && t.asset == x.asset)
                .then_some((
                    t.terms.reporting,
                    (t.decimals.origin, t.decimals.destination),
                ))
        })?,
        _ => return Some(given_out),
    };
    if reporting != Reporting::Gross {
        return Some(given_out);
    }
    let fee = outcome["fee"]["amount"].as_str()?;
    less_the_charge(&given_out.to_string(), fee, units.0, units.1)?
        .parse()
        .ok()
}
