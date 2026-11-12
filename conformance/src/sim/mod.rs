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
use crate::role::{Received, RoleHarness, RoleProvider, SuiteContext};
use rutent_contracts::provider_role::{Advertisement, Operation, Statement};

/// The state key under which the platform's world keeps a simulated provider's journal: the JSON array of its
/// [`Entry`]. It is a shared key, readable by the provider's outside part for any transaction.
pub const BOOK_KEY: &str = "book";

/// The state key under which the platform's world names the deployment a simulated provider serves, as the JSON of
/// provider-role's `Deployment`: the provider holds every operation to it. Where the world names none, it serves the
/// suite's default context (`T-024-02`: the assembly's deployment names the operator's real key).
pub const DEPLOYMENT_KEY: &str = "deployment";

/// The prefix of the state keys a script's next behaviour is read under: `script/next/{kind}/{dry|real}`. Reading
/// one takes the behaviour out of the queue, which is how a script is consumed.
pub const SCRIPT_PREFIX: &str = "script/next";

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
}

impl Spec {
    /// The simulated provider of `role` named `provider`.
    pub fn of(role: Role, provider: &str) -> Self {
        Self {
            role,
            provider: provider.to_owned(),
        }
    }

    /// Its publisher: one per provider, so two reserves on two private nodes have two (`DEC-24`).
    pub fn publisher(&self) -> String {
        format!("{}-publisher", self.provider)
    }

    /// The bank network of a reserve.
    pub fn bank(&self) -> String {
        format!("{}-bank", self.provider)
    }

    /// The private node of a reserve, which its publisher declares.
    pub fn ledger(&self) -> String {
        format!("{}-ledger", self.provider)
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
            Role::Reserve => Box::new(Reserve::named(
                context,
                &identity,
                &self.bank(),
                &self.ledger(),
            )),
            Role::Exchange => Box::new(Exchange::named(context, &identity)),
            Role::Ramp => Box::new(Ramp::named(context, &identity)),
            Role::Verifier => Box::new(Verifier::named(context, &identity)),
        }
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
}

impl Behaviour {
    /// A behaviour of `act`.
    pub fn act(act: Act) -> Self {
        Self {
            act,
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

    /// Confirming the credit of a reversal.
    pub fn crediting_a_reversal() -> Self {
        Self {
            reversal_credited: true,
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

    /// Hands the provider the hooks the role suite drives: the next real operation fails.
    pub fn force_failure(&mut self) -> bool {
        self.provider.force_failure()
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
        let before = self.provider.state();
        let mut credited = false;
        if behaviour.reversal_credited
            && let Operation::ReversalConfirmation(c) = &operation.statement
        {
            let node = self
                .provider
                .advertisement()
                .into_statement()
                .statement
                .private_nodes
                .first()
                .cloned()
                .unwrap_or_default();
            self.provider
                .credit_reversal(&c.segment, &node, &c.asset, &c.amount);
            credited = true;
        }
        // A hook is armed for a real movement alone: the stand-in disarms it only when a movement succeeds or
        // fails, and an operation that isn't one would leave it for the next.
        let armed = real_movement(&operation.statement)
            && matches!(behaviour.act, Act::Unknown | Act::Fail);
        if armed {
            match behaviour.act {
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
        deliver(&mut body, behaviour.delivery);
        // A repeat that changed nothing sends no movement again: a network that reorged the first would take it
        // as a new one in another block.
        let movements = if behaviour.unrecorded || !changed {
            Vec::new()
        } else {
            movements_of(&operation.statement, &body)
        };
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
/// several evidence pieces made as many movements, `{transaction}.{n}` each, which split what it gave out.
pub fn movements_of(operation: &Operation, result: &Value) -> Vec<Movement> {
    let outcome = &result["outcome"];
    if outcome["status"] != "succeeded" {
        return Vec::new();
    }
    let Some(out) = outcome["amount_out"]
        .as_str()
        .and_then(|a| a.parse::<u128>().ok())
    else {
        return Vec::new();
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
            asset_in,
            amount_in: text(&outcome["amount_in"]),
            asset_out,
            amount_out: text(&outcome["amount_out"]),
            fee: text(&outcome["fee"]["amount"]),
        });
    }
    recorded
}

/// The transaction an operation names, if it names one.
pub fn transaction_of(operation: &Operation) -> Option<String> {
    serde_json::to_value(operation).ok()?["transaction"]
        .as_str()
        .map(str::to_owned)
}
