//! Types mirroring the `urn:rutent:router-api:*` schemas. Field names match the schemas, and every struct and
//! tagged kind denies unknown fields, as the schemas set `additionalProperties: false`.

use alloc::{boxed::Box, vec::Vec};
use core::num::NonZeroU8;

use serde::{Deserialize, Serialize};

use super::atoms::{non_empty_list, some};
use super::{
    Amount, Decimal, Distinct, Id, NonEmpty, PositiveAmount, ProtocolVersion, Timestamp, Version,
};
use crate::provider_role::{
    BasisPoints, Denomination, Deployment, ExchangeTuple, Fee, Judgement, NodeRef, RampTuple,
    ReserveTuple, ResultBody, SafeInteger, ShapeMismatch, Slippage, Statement, StatementShape,
    VoidedBy,
};

/// The typed operation of `REQ-ORD-37`: withdrawal, route, exchange or deposit, and no other. A kind beyond these
/// four needs a protocol change (`REQ-PRT-5`). Schema: `urn:rutent:router-api:operation#/$defs/kind`; the router
/// contract owns it, and the state machine refers to it (ruled by the orchestrator, 2026-11-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Out of a reserve, from an outer node into an inner one.
    Withdrawal,
    /// Between two inner nodes, keeping the asset.
    Route,
    /// From one asset to another, at one inner node.
    Exchange,
    /// Into a reserve, from an inner node out to an outer one.
    Deposit,
}

/// A node and an asset, with the unit the asset is counted in there. Schema:
/// `urn:rutent:router-api:common#/$defs/endpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    /// An inner node by identifier, or an outer node by its network (`REQ-CAT-17`).
    pub node: NodeRef,
    /// The asset, and its unit: one whole asset is `10^decimals` units (`REQ-CAT-11`).
    pub asset: Denomination,
}

/// A rational number, `numerator / denominator`, in whole numbers so that no figure is rounded on the way in.
/// Schema: `urn:rutent:router-api:common#/$defs/ratio`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    /// The numerator.
    pub numerator: Amount,
    /// The denominator; above zero.
    pub denominator: PositiveAmount,
}

/// What the customer asks the router for, in one round. Schema: `urn:rutent:router-api:request`.
///
/// Its `request_id` is also the id of the decision record that answers it: one routing per request, so each
/// round has its own request id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// The request's id, and so its decision record's.
    pub request_id: Id,
    /// Which round this routing is (`REQ-CAT-5`, `DEC-8`).
    pub round: Round,
    /// Where the value starts.
    pub from: Endpoint,
    /// Where the value must arrive: the one destination and to asset (`REQ-AUZ-3`).
    pub to: Endpoint,
    /// The amount that leaves `from`, in its unit. The direction is `from` to `to` (`REQ-QTE-12`).
    pub amount: PositiveAmount,
    /// What the router optimises for (`REQ-QTE-11`).
    pub objective: Objective,
    /// The worst acceptable outcome: the least the customer accepts at `to`, in its unit (`REQ-ORD-27`).
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub floor: Option<Amount>,
    /// The choice-round request whose decision record this one follows for the same order, at a recalculation
    /// (SAD §3).
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub follows: Option<Id>,
}

/// The two rounds of one routing (ruled by the orchestrator, 2026-11-06). `route` is the same pure function in
/// both; Core calls it once on the advertised snapshot, then again on a refreshed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Round {
    /// Over advertised figures only. Its record chooses nothing; the candidates it doesn't drop are the shortlist,
    /// and only their providers are asked anything (`REQ-CAT-5`).
    Shortlist {},
    /// Over a snapshot that adds fresh quotes, dry runs and eligibility judgements for the shortlisted
    /// providers. It chooses at most one candidate, and only one whose every operation is confirmed: it cites an
    /// accepted dry-run answer of its provider for its tuple, at its amount, within its validity and not void
    /// (the fresh quote for a route or exchange, the reserve's accepted dry run for a withdrawal or deposit).
    Choice {
        /// The shortlist round it refines, so both rounds are recorded (`REQ-AUD-4`), with the time its snapshot
        /// was taken: a confirming answer is made at or after it.
        refines: Refines,
    },
}

/// The shortlist round a choice round refines (`REQ-CAT-5`, ruled by the orchestrator's pre-freeze audit,
/// 2026-11-07).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refines {
    /// The shortlist round's request id.
    pub request_id: Id,
    /// When the shortlist round's snapshot was taken, at or before the choice round's own. In the choice round an
    /// accepted answer made before it is not usable: it confirms nothing, needn't be cited and covers nothing, as
    /// one past its validity. A refusal or a judgement binds whenever it was made. The contract checks against
    /// this stated value; that it is the shortlist's real time is Core's to keep (`REQ-AUD-4`).
    pub taken_at: Timestamp,
}

/// The customer's objective (`REQ-QTE-11`). Whoever builds the request writes the default, lowest total cost,
/// when the customer chose none, so the record always names the objective it was computed for (`REQ-AUD-4`).
///
/// Among candidates that are not dropped, the best by the objective is chosen; candidates equal by it are ordered
/// by `candidate_id` ascending, comparing bytes, and the first is chosen, so an independent recompute agrees
/// (`THR-10`). Adding an objective is a contract change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Objective {
    /// The most value at the destination: the largest sum, over the candidate's parts, of the last operation's
    /// `amount_out`. The default.
    LowestTotalCost {},
    /// The shortest settlement: a part's time is the sum of its operations' declared settlement seconds, each
    /// from the usable fresh quote the operation cites for its amount in, or else its price; a split's is its
    /// slowest part's. The smallest wins.
    FastestSettlement {},
}

/// The catalogue as the router reads it, at one time: every figure it may use, each with its age at that time and
/// its origin (`REQ-QTE-4`). Schema: `urn:rutent:router-api:catalogue-snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct CatalogueSnapshot {
    /// The snapshot's id.
    pub snapshot_id: Id,
    /// When it was taken: every figure's age is counted to this time.
    pub taken_at: Timestamp,
    /// The figures, each with an id unique in the snapshot.
    pub figures: Vec<Figure>,
}

/// One figure the router may use (`REQ-QTE-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Figure {
    /// Unique in its snapshot; operations and drop reasons name a figure by this id.
    pub figure_id: Id,
    /// Who stated the figure: a provider, or, for in-flight exposure, the operator's own records.
    pub origin: Origin,
    /// How old the figure is at the snapshot's `taken_at`, in seconds since its origin stated it, rounded up. For
    /// a statement it is counted from the envelope's `made_at`.
    pub age_seconds: SafeInteger,
    /// The figure's stated validity, in seconds since its origin stated it, rounded down; absent when the origin
    /// states none, as for an advertisement, which holds until replaced. For a statement it is counted from its
    /// `made_at` to its `valid_until`, and is always present.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub valid_for_seconds: Option<SafeInteger>,
    /// The figure itself.
    pub value: FigureValue,
}

impl Figure {
    /// Whether the figure may be used: its age is at most its validity (`REQ-QTE-4`). With age rounded up and
    /// validity rounded down, a figure past its `valid_until` is never within it.
    pub fn is_within_validity(&self) -> bool {
        self.valid_for_seconds
            .is_none_or(|valid| self.age_seconds <= valid)
    }
}

/// Where a figure comes from (`REQ-AUD-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    /// A provider: every figure but exposure.
    Provider {
        /// The provider.
        provider: Id,
        /// Its publisher, for ceilings across one publisher's providers (`REQ-ORD-8`).
        publisher: Id,
        /// The provider version the figure belongs to.
        provider_version: Version,
        /// The version of the admitted module that gave the figure: its admission artefact's version (`F-003`,
        /// ruled by the orchestrator, 2026-11-06).
        module_version: Version,
    },
    /// The operator's own records: in-flight exposure.
    Operator {
        /// Which record or ledger it was read from.
        source: Id,
    },
}

impl Origin {
    /// The provider, for a provider's figure.
    pub fn provider(&self) -> Option<&Id> {
        match self {
            Origin::Provider { provider, .. } => Some(provider),
            Origin::Operator { .. } => None,
        }
    }

    /// The publisher, for a provider's figure.
    pub fn publisher(&self) -> Option<&Id> {
        match self {
            Origin::Provider { publisher, .. } => Some(publisher),
            Origin::Operator { .. } => None,
        }
    }
}

/// What a figure is. Tuples are the provider-role contract's shapes, by reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FigureValue {
    /// A reserve tuple as advertised (`urn:rutent:provider-role:advertisement#/$defs/reserve_tuple`).
    ReserveTuple {
        /// The tuple.
        tuple: ReserveTuple,
    },
    /// An exchange tuple as advertised (`urn:rutent:provider-role:advertisement#/$defs/exchange_tuple`).
    ExchangeTuple {
        /// The tuple.
        tuple: ExchangeTuple,
    },
    /// A ramp tuple as advertised (`urn:rutent:provider-role:advertisement#/$defs/ramp_tuple`).
    RampTuple {
        /// The tuple.
        tuple: RampTuple,
    },
    /// A tuple's advertised price, from the tuple's provider: its fee formula (`REQ-QTE-5`), its rate where its
    /// category sets one, and its declared settlement time (`REQ-QTE-3`).
    Price {
        /// The tuple figure it prices, by id.
        tuple: Id,
        /// The fee formula.
        fee: FeeFormula,
        /// The rate; present exactly when the tuple is an exchange's.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        rate: Option<Pricing>,
        /// The declared settlement time.
        settlement_seconds: SafeInteger,
    },
    /// A shortlisted provider's detailed quote: its signed result for a dry-run transfer or conversion, whose
    /// outcome is a `transfer_quote` or `conversion_quote` (`urn:rutent:provider-role:result`, `REQ-QTE-1`).
    /// Carried whole and unchanged, so its signature still verifies.
    FreshQuote {
        /// The tuple figure it quotes, by id: a ramp's for a transfer, an exchange's for a conversion.
        tuple: Id,
        /// The provider's result, in its envelope.
        quote: Box<Statement<ResultBody>>,
    },
    /// A shortlisted provider's signed result for a dry-run withdrawal, deposit, transfer or conversion:
    /// `dry_run_accepted` or `refused` (`urn:rutent:provider-role:result`, `REQ-CAT-14`). Carried whole.
    DryRun {
        /// The tuple figure it answers for, by id.
        tuple: Id,
        /// The amount the dry run asked about, in the tuple's input unit. A refusal doesn't restate it, so the
        /// snapshot records it; for an accepted answer it is the answer's own `amount_in`.
        amount_in: PositiveAmount,
        /// The provider's result, in its envelope.
        result: Box<Statement<ResultBody>>,
    },
    /// A verifier's signed judgement of the customer for a class a shortlisted tuple needs
    /// (`urn:rutent:provider-role:judgement`). Carried whole; its maker is the figure's provider, the verifier.
    Eligibility {
        /// The tuple figure it is for, by id.
        tuple: Id,
        /// The verifier's judgement, in its envelope.
        judgement: Box<Statement<Judgement>>,
    },
    /// The cross-module calls a provider's admitted module declares, as its release's `outside.calls` lists them
    /// (module-interface, `REQ-MDL-21`): which other providers' modules it may call, and for which provider-role
    /// operation kinds. From the provider: the origin's `module_version` names the release that declares them.
    /// It states no validity, as an advertisement doesn't: it holds until the release is replaced. At most one per
    /// provider in a snapshot, each callee listed once and none the provider itself (router-api 3.0.0,
    /// `T-011-06`). A route segment names only calls its provider declares here ([`Operation::calls`]).
    Calls {
        /// The declared calls, each callee once.
        calls: NonEmpty<DeclaredCall>,
    },
    /// Value already in flight at one provider, in one asset, from the operator's records (`REQ-ORD-8`).
    Exposure {
        /// The provider.
        provider: Id,
        /// Its publisher.
        publisher: Id,
        /// The asset and its unit.
        asset: Denomination,
        /// The value in flight.
        in_flight: Amount,
    },
}

/// The kind of a cross-module call: a provider-role operation kind, as `urn:rutent:provider-role:operation` names
/// it, in the module interface's terms (`validate_call_against`, `REQ-MDL-21`). Not the typed operation's
/// [`OperationKind`]: a call asks a module for one provider-role operation, such as a `transfer` or a
/// `balance_verification`.
pub use crate::module_interface::OperationKind as CallKind;

/// One cross-module call a provider's module declares (module-interface `outside.calls`, `REQ-MDL-21`): the provider
/// whose module it may call, and the kinds it may ask of it, each once. Schema:
/// `urn:rutent:router-api:common#/$defs/declared_call`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredCall {
    /// The provider whose module may be called.
    pub callee: Id,
    /// The kinds that may be asked of it, each once.
    pub kinds: NonEmpty<CallKind>,
}

/// One cross-module call a route segment names (`REQ-MDL-21`, `THR-86`): in that segment, the operation's provider's
/// module may call `callee`'s for `kind`. The platform's call rule (`judge_call`, `T-014-04`) lets a module call
/// another only where its release declares the call and a segment of its route names it. Schema:
/// `urn:rutent:router-api:common#/$defs/route_call`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCall {
    /// The provider whose module may be called: the provider of another operation of the same part.
    pub callee: Id,
    /// The provider-role operation kind that may be asked of it.
    pub kind: CallKind,
}

/// The shape of a decision record statement.
pub const DECISION_RECORD_SHAPE: &str = "urn:rutent:router-api:decision-record";

/// A fee formula: flat and proportional parts, depending only on amount and tuple (`REQ-QTE-5`).
///
/// The fee on an amount `a` entering the tuple is `flat + ceil(a × numerator / denominator)`, in the tuple's
/// input unit: the proportion rounds up, so a provider rounding its charge either way stays within its formula
/// and a route is never ranked on a fee lower than its provider may charge. Where a `maximum` is given, the
/// charge is known only after execution and may come to it, so route evaluation counts the fee at `maximum`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeFormula {
    /// The flat part.
    pub flat: Amount,
    /// The proportional part.
    pub proportional: Ratio,
    /// The most the charge may come to, where it is known only after execution.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub maximum: Option<Amount>,
}

/// How an amount converts, in whole to-assets per whole from-asset, after the fee is taken out (`REQ-EXT-2`,
/// `REQ-QTE-12`).
///
/// The net amount `n = a − fee` fills the levels in order: level `i` takes the part of `n` above the previous
/// level's `up_to` (zero for the first) and at most its own. The output, in the to unit, is
/// `floor(Σ take_i × rate_i × 10^to.decimals / 10^from.decimals)`, rounded down once, at the end. A fixed rate is
/// one level with no bound. A net amount above the last level's `up_to` isn't served: the price gives no output,
/// and a candidate that needs it is dropped as `outside_band`. A tuple with no rate (a reserve or ramp) outputs
/// `floor(n × 10^destination.decimals / 10^origin.decimals)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pricing {
    /// One rate for every amount.
    Fixed {
        /// The rate (`urn:rutent:provider-role:result#/$defs/rate`).
        rate: Decimal,
    },
    /// Rates by level, as an order book's depth.
    Levels {
        /// The levels, their bounds strictly increasing.
        levels: NonEmpty<Level>,
    },
}

/// One level of a [`Pricing::Levels`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    /// The cumulative net amount this level fills up to, inclusive, in the from unit.
    pub up_to: PositiveAmount,
    /// The rate for the net amount that falls in this level.
    pub rate: Decimal,
}

/// What the operator allows (`REQ-QTE-11`, `REQ-ORD-8`, ADR-0006), and the context every statement is judged in
/// (`REQ-PRT-1`). Schema: `urn:rutent:router-api:policy-bounds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct PolicyBounds {
    /// The deployment routing belongs to. A statement in the snapshot that names another is void here.
    pub deployment: Deployment,
    /// The protocol version routing belongs to. A statement that names another is void here.
    pub protocol_version: ProtocolVersion,
    /// The policy's version (`REQ-AUD-4`).
    pub policy_version: Version,
    /// The most operations a part may have: the search's hop limit.
    pub max_hops: NonZeroU8,
    /// The most parts a split may have; `1` routes without splitting, as v0 does (`DEC-8`).
    pub max_parts: NonZeroU8,
    /// Providers never chosen.
    pub excluded_providers: Distinct<Id>,
    /// Tuples never chosen.
    pub excluded_tuples: Distinct<TupleKey>,
    /// Ceilings on value in flight at one provider, at most one per provider and asset, id and decimals
    /// (`REQ-ORD-8`).
    pub provider_ceilings: Vec<ProviderCeiling>,
    /// Ceilings on value in flight across one publisher's providers, at most one per publisher and asset, id and
    /// decimals.
    pub publisher_ceilings: Vec<PublisherCeiling>,
    /// The operator's own fee (`REQ-QTE-2`, `REQ-QTE-5`), taken at the destination, in the request's `to` unit,
    /// from what the providers' terms deliver there: `flat + ceil(a × numerator / denominator)` on that amount
    /// `a`, capped by its `maximum` where it gives one and never raised to it. Its proportion is at most one.
    ///
    /// The floor is judged on what is left after it (`REQ-ORD-27`): a candidate that would leave the customer less
    /// than the floor is dropped as `below_floor`. Ranking isn't: it compares the providers' amounts before this
    /// fee ([`Ranking`]).
    pub operator_fee: FeeFormula,
    /// The slippage the operator sets per tuple (`REQ-QTE-9`): the most an exchange's rate may move against the
    /// customer on a segment settling within three days, at most one per tuple. Every exchange tuple a snapshot
    /// routed under these bounds declares settling within [`THREE_DAYS_SECONDS`] has one
    /// ([`PolicyBounds::check_against`]). Empty if no such tuple is routed.
    pub slippage_limits: Vec<SlippageLimit>,
}

/// Three calendar days, in seconds from segment start: the line `REQ-QTE-9` and `REQ-QTE-10` draw. A provider
/// declares settlement in seconds from segment start, and the start's date isn't known when a route is quoted, so
/// three calendar days are read as `3 × 86 400` seconds, inclusive: a segment declaring at most this settles
/// within three days.
pub const THREE_DAYS_SECONDS: u64 = 3 * 86_400;

/// The slippage the operator sets for one tuple (`REQ-QTE-9`): the largest move of its rate against the customer
/// on a segment that settles within three days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlippageLimit {
    /// The tuple, by its provider and its two ends, as [`PolicyBounds::excluded_tuples`] names one, so the limit
    /// holds across snapshots.
    pub tuple: TupleKey,
    /// The largest move of the rate against the customer, in basis points.
    pub max_bps: BasisPoints,
}

/// A tuple, by its provider and its two ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleKey {
    /// The provider.
    pub provider: Id,
    /// Where the tuple starts.
    pub from: Endpoint,
    /// Where the tuple ends.
    pub to: Endpoint,
}

/// A ceiling on value in flight at one provider, in one asset (`REQ-ORD-8`).
///
/// A candidate's value at the provider is the sum, over every operation of every part at that provider, of its
/// `amount_in` where its `from` asset is this one (id and decimals), or else its `amount_out` where its `to` asset
/// is, so a split's parts count together and value an exchange produces counts too; to it is added the snapshot's
/// exposure for the provider and asset, or zero. Above `amount`, the candidate is dropped as `over_ceiling`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCeiling {
    /// The provider.
    pub provider: Id,
    /// The asset and its unit.
    pub asset: Denomination,
    /// The most in flight.
    pub amount: Amount,
}

/// A ceiling on value in flight across one publisher's providers, in one asset (`REQ-ORD-8`): as
/// [`ProviderCeiling`], summed over every provider whose figures name this publisher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherCeiling {
    /// The publisher.
    pub publisher: Id,
    /// The asset and its unit.
    pub asset: Denomination,
    /// The most in flight.
    pub amount: Amount,
}

/// One typed operation of a route, priced (`REQ-ORD-37`): one segment at one provider. Schema:
/// `urn:rutent:router-api:operation`, which extends the typed operation `#/$defs/operation` (kind, from, to).
///
/// A withdrawal goes from an outer node into an inner one, a deposit the other way, both keeping the asset; a route
/// keeps the asset between two different inner nodes; an exchange stays at one inner node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Operation {
    /// The kind; one of the four, never another (`REQ-PRT-5`).
    pub kind: OperationKind,
    /// The provider the operation runs at.
    pub provider: Id,
    /// Where the value starts, in that unit.
    pub from: Endpoint,
    /// Where the value ends, in that unit.
    pub to: Endpoint,
    /// What enters, in `from`'s unit, evaluated at the actual amount after the fees before it (`REQ-QTE-12`).
    pub amount_in: PositiveAmount,
    /// The provider's fee, in `from`'s asset and unit, out of `amount_in`
    /// (`urn:rutent:provider-role:result#/$defs/fee`).
    pub fee: Fee,
    /// What arrives, in `to`'s unit.
    pub amount_out: Amount,
    /// The snapshot figures the operation was evaluated with, by id, each once (`REQ-QTE-4`).
    pub figures: NonEmpty<Id>,
    /// The cross-module calls this segment makes, each once (router-api 3.0.0, `T-011-06`, `REQ-MDL-21`): each is a
    /// call its provider's module declares, in the one [`FigureValue::Calls`] figure the operation cites, of the
    /// release its tuple comes from, to the provider of another operation of the same part. Empty, and absent on
    /// the wire, when it names none; a present list is never empty, so an operation has one form.
    #[serde(
        default,
        deserialize_with = "non_empty_list",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub calls: Vec<RouteCall>,
}

/// One part of a split: a route, an ordered list of typed operations from the request's origin to its destination
/// (`REQ-ORD-37`, `REQ-ORD-42`). Each operation starts where the one before ended, with what it delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Part {
    /// The operations, in order.
    pub operations: NonEmpty<Operation>,
}

/// One candidate the router considered: a split of one or more parts (`REQ-QTE-16`), and why it was dropped unless
/// it was kept (`REQ-AUD-4`). Schema: `urn:rutent:router-api:candidate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Candidate {
    /// Unique in its record.
    pub candidate_id: Id,
    /// The parts; their amounts add up to the request's.
    pub parts: NonEmpty<Part>,
    /// Why it was dropped; absent on a shortlisted candidate, or on the chosen one.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub dropped: Option<DropReason>,
}

/// Which ceiling a candidate passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeilingScope {
    /// A [`ProviderCeiling`].
    Provider,
    /// A [`PublisherCeiling`].
    Publisher,
}

/// Why a candidate was dropped (`REQ-AUD-4`).
///
/// Non-exhaustive since 3.0.0 (`DEC-4`, `T-011-05`): a match outside this crate needs a wildcard arm, which spares
/// Rust callers when a reason is added. The contract is language-neutral, and a reader on an older schema rejects a
/// reason it doesn't know, so a new wire value is still a major under `DEC-4`; the router suite pins the reasons
/// it knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum DropReason {
    /// It used a figure past its stated validity (`REQ-QTE-4`).
    StaleFigure {
        /// The figure, by id.
        figure: Id,
    },
    /// It used a statement whose envelope names another deployment or protocol version than the bounds'
    /// (`REQ-PRT-1`).
    VoidStatement {
        /// The figure, by id.
        figure: Id,
    },
    /// A condition that voids a fresh quote it used holds (`REQ-QTE-1`).
    QuoteVoided {
        /// The quote figure, by id.
        figure: Id,
        /// The condition, in the provider-role vocabulary.
        condition: VoidedBy,
    },
    /// A provider's dry run refused one of its operations.
    DryRunRefused {
        /// The dry-run figure, by id.
        figure: Id,
    },
    /// A verifier judged the customer `no` for a class one of its operations needs.
    Ineligible {
        /// The eligibility figure, by id.
        figure: Id,
    },
    /// An operation's amount falls outside its provider's band, after the fees before it, or above its price's
    /// last level (`REQ-QTE-12`).
    OutsideBand {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index.
        operation: u32,
    },
    /// An operation's net amount is above its price's last level, so the price serves it nothing (`REQ-QTE-12`,
    /// ADR-0006; ruled by the orchestrator's pre-freeze audit, 2026-11-07). The part is listed only up to and
    /// including that operation, which arrives with `0`; the part needn't reach the destination, since nothing
    /// after it can be stated. So an exhaustive search records every candidate it considered (`REQ-AUD-4`).
    BeyondLevels {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index: the part's last.
        operation: u32,
    },
    /// An operation runs through a tuple the snapshot holds no price for, so its terms can't be stated
    /// (`REQ-AUD-4`; ruled by the owner, 2026-11-09, item `aud4-unstateable-candidates`). As with
    /// [`DropReason::BeyondLevels`], the part is listed only up to and including that operation, which states a fee
    /// of `0` and arrives with `0`, and needn't reach the destination. That operation cites its tuple and no price,
    /// and the snapshot holds no price for that tuple.
    NoPrice {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index: the part's last.
        operation: u32,
    },
    /// An operation's fee, by its price's formula (counted at its maximum where one is given), is above the amount
    /// that reaches it, so it can't be stated (`REQ-QTE-5`, `REQ-AUD-4`; ruled by the owner, 2026-11-09, item
    /// `aud4-unstateable-candidates`). As with [`DropReason::BeyondLevels`], the part is listed only up to and
    /// including that operation, which states a fee equal to its amount in and arrives with `0`, and needn't reach
    /// the destination. It cites no usable accepted answer at its amount in, since a provider's answer binds over
    /// its price and its fee is at most what enters. That the formula's fee is above the amount in is the router
    /// suite's to recompute.
    FeeAboveAmount {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index: the part's last.
        operation: u32,
    },
    /// An operation short of the destination delivers nothing, so nothing after it can be stated (`REQ-QTE-13`,
    /// `REQ-AUD-4`; ruled by the owner, 2026-11-09, item `aud4-unstateable-candidates`): its fee takes all that
    /// enters, or its output rounds to `0`. As with [`DropReason::BeyondLevels`], the part is listed only up to and
    /// including that operation, its last, which arrives with `0` and doesn't end at the destination; at the
    /// destination such a part is `no_positive_outcome`. Its terms are stated as its figures give them, and the
    /// router suite recomputes them like any other's.
    NothingDelivered {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index: the part's last.
        operation: u32,
    },
    /// Its value in flight, its parts counted together, would pass a ceiling (`REQ-ORD-8`).
    OverCeiling {
        /// Which kind of ceiling.
        scope: CeilingScope,
        /// The provider or publisher.
        party: Id,
        /// The ceiling's asset and unit: a ceiling matches on both.
        asset: Denomination,
    },
    /// In a choice round, one of its operations isn't confirmed: it cites no accepted dry-run answer of its
    /// provider for its tuple, at its amount, within its validity and not void.
    Unconfirmed {
        /// The part, by index.
        part: u32,
        /// The operation within the part, by index.
        operation: u32,
    },
    /// It uses a provider or tuple the policy excludes (`REQ-QTE-11`).
    ExcludedByPolicy {},
    /// What it would leave the customer at the destination, after the operator's fee
    /// ([`PolicyBounds::operator_fee`]), is below the customer's floor (`REQ-ORD-27`; since 4.0.0, `T-020-02`).
    BelowFloor {},
    /// One of its parts arrives with nothing (`REQ-QTE-13`).
    NoPositiveOutcome {},
    /// It follows a rate cycle that implies free profit, possibly stale or false (ADR-0006).
    SuspectRateCycle {},
    /// Another candidate comes first by the objective and its tie-break (`REQ-QTE-11`).
    Outranked {
        /// That candidate, by id.
        by: Id,
    },
}

/// The router's record of one routing round (`REQ-AUD-4`, `REQ-QTE-16`): its inputs verbatim, every candidate and
/// why each was dropped, and the chosen one. Its id is its request's. Schema:
/// `urn:rutent:router-api:decision-record`, inside the statement envelope ([`RecordedDecision`]): the router
/// returns it, and the deployment signs it when it records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct DecisionRecord {
    /// The router implementation's version.
    pub router_version: Version,
    /// The request, as received.
    pub request: Request,
    /// The snapshot, as read.
    pub snapshot: CatalogueSnapshot,
    /// The policy bounds, as applied.
    pub bounds: PolicyBounds,
    /// What the ranking compared: always the providers' amounts before the operator's fee.
    pub ranking: Ranking,
    /// Every candidate considered.
    pub candidates: Vec<Candidate>,
    /// The chosen candidate, by id; never in a shortlist round, and absent in a choice round with no route.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub chosen: Option<Id>,
}

impl DecisionRecord {
    /// The record's id: its request's.
    pub fn record_id(&self) -> &Id {
        &self.request.request_id
    }

    /// Every candidate considered.
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// The chosen candidate, whose parts are the chosen split; `None` in a shortlist round or with no route.
    pub fn chosen(&self) -> Option<&Candidate> {
        let id = self.chosen.as_ref()?;
        self.candidates.iter().find(|c| &c.candidate_id == id)
    }

    /// In a shortlist round, the candidates kept: those not dropped.
    pub fn shortlist(&self) -> Vec<&Candidate> {
        self.candidates
            .iter()
            .filter(|c| c.dropped.is_none())
            .collect()
    }
}

impl StatementShape for DecisionRecord {
    const SHAPE: &'static str = DECISION_RECORD_SHAPE;

    fn check_nested_shapes(&self) -> Result<(), ShapeMismatch> {
        for figure in &self.snapshot.figures {
            match &figure.value {
                FigureValue::FreshQuote { quote, .. } => quote.check_shape()?,
                FigureValue::DryRun { result, .. } => result.check_shape()?,
                FigureValue::Eligibility { judgement, .. } => judgement.check_shape()?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// A decision record in its envelope, as the deployment records it (`REQ-PRT-1`), signed with the deployment's
/// `operator_key`. Nothing in the record ties a carried statement's `signature.key` to its maker; Core does that
/// before the record is formed (`contracts/router-api/README.md`, "Unchecked by either").
pub type RecordedDecision = Statement<DecisionRecord>;

/// What a decision record's ranking compares (`REQ-QTE-11`, `REQ-AUD-4`).
///
/// One value today: the providers' amounts and declared times, before the operator's fee
/// ([`PolicyBounds::operator_fee`]). Where one formula is taken at the destination from every candidate and its
/// proportion is at most one, what the customer receives after it never falls as what arrives grows, so ranking
/// after it could differ only where it turns a difference into a tie, which goes to the lowest `candidate_id`.
/// Ranking net of the fee, which a formula depending on the tuple (`REQ-QTE-5`) would need for `REQ-QTE-11`'s
/// lowest total cost, would be a new value here, so a record always says which it used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ranking {
    /// On the providers' amounts before the operator's fee.
    BeforeOperatorFee,
}

/// A provider's terms for one tuple as stated at one time (`REQ-QTE-6`): its fee formula and, where its tuple sets
/// a rate, its slippage regime. Schema: `urn:rutent:router-api:stated-terms`.
///
/// Core keeps each tuple's history of these, keyed by [`StatedTerms::tuple`], each with the time it was stated; the
/// terms that bind a segment are the last stated at or before it started, and before that the route's total is a
/// quote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatedTerms {
    /// The tuple they are for, by its provider and its two ends, so a history is keyed across snapshots.
    pub tuple: TupleKey,
    /// When the provider stated them.
    pub stated_at: Timestamp,
    /// The fee formula.
    pub fee: FeeFormula,
    /// The slippage regime, where the tuple sets a rate.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub slippage: Option<Slippage>,
}

/// One segment's declared settlement time and its actual one (`REQ-QTE-3`). Schema:
/// `urn:rutent:router-api:settlement-times`.
///
/// The declared time is what the segment's quote or price declared, in seconds from segment start; the actual one
/// runs from `started_at` to `settled_at`, never backwards. Core keeps one per segment it starts, and the
/// `actual_seconds` of core-internal's `Settlement` log entry is `rutent_router::quote::measure` of it, rounded up to
/// whole seconds, so the log and this pair agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct SettlementTimes {
    /// The declared settlement time, in seconds from segment start.
    pub declared_seconds: SafeInteger,
    /// When the segment started.
    pub started_at: Timestamp,
    /// When it settled: at or after `started_at`.
    pub settled_at: Timestamp,
}

/// A quote a reserve holds for a new route before it starts (`REQ-QTE-15`): a choice round's record, whose chosen
/// candidate is the quote, with the validity stated for the hold. Schema: `urn:rutent:router-api:held-quote`.
///
/// It stays a quote until each segment starts: it binds no provider, whose terms bind only from segment start
/// (`REQ-QTE-6`), so dropping it, or its quote failing, has no effect beyond the hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct HeldQuote {
    /// The record the quote is from: it chose a candidate.
    pub record: DecisionRecord,
    /// The hold's stated validity, at or after the record's snapshot was taken.
    pub valid_until: Timestamp,
}
