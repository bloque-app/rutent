//! The state-machine contract: the published state list, the recorded-input envelope, the order as one or more
//! parts, and the transition function's signature (`DEC-3`, `DEC-4`, ADR-0002).
//!
//! This is a library API (`DEC-4`): these types are the API, and they serialise to the schemas under
//! `contracts/state-machine/schemas/`. `CMP-STM` implements [`StateMachine`]; this module holds no transition
//! rules of its own but one, published so that every implementation applies it the same way: what a part becomes
//! when no route is left ([`after_no_route`], the owner's ruling `spec-no-route-left`).
//!
//! # The typed operation is the router's
//!
//! A segment plan is the router contract's typed operation (`urn:rutent:router-api:operation#/$defs/operation`,
//! [`OperationKind`] and [`Endpoint`]) with a segment id and a provider. [`SegmentPlan::check`] holds the rules of
//! that operation that JSON Schema can't state, the same rules as the router's own `Operation::check`.
//!
//! # No route left
//!
//! A `no_route` input moves a part by [`after_no_route`]: a part whose value can't be routed to the node where the
//! destination reserve lives becomes stuck (`REQ-ORD-28`); a part whose value is already at that node, after a
//! failed final deposit, waits for the customer (`REQ-ORD-43`). The part remembers where its final deposit failed
//! ([`Part::deposit_failed_at`]), kept by the published update rule [`deposit_failed_at_after`], so the rule holds
//! after a recalculation that started from there failed too.
//!
//! # The destination
//!
//! A part's destination ([`Destination`]) is its route's last deposit: the destination reserve (its provider), the
//! outer node it deposits to, and the asset with its decimals. `INV-10` keeps it: a recalculation must end at the
//! same destination ([`recalculation_keeps_destination`]), and only the customer's accepted `destination_changed`
//! replaces it. [`Order::check`] requires every part's route to end with a deposit at its destination.
//!
//! # Rust is as strict as the schemas
//!
//! Every text field that the schemas require non-empty is a non-empty newtype ([`Id`], [`RecordId`]), times are
//! a checked [`Timestamp`], and every struct and observed kind refuses unknown fields. The cross-field rules a
//! field type can't carry are checked by [`Order::check`] and [`RecordedInput::check`], and deserialising either
//! runs its check, so a deserialised value always serialises to a valid instance. A value built field by field in
//! Rust is checked only when `check` is called: an implementation of [`StateMachine`] returns only orders that
//! pass it.
//!
//! A few rules hold in Rust but can't be stated in JSON Schema; `contracts/state-machine/README.md` lists every
//! rule and where it is enforced:
//!
//! - segment and part ids are unique within an order for its whole life, retired segments included;
//! - an `applied` last input is the input that reached the state.
//!
//! # Repeats and record ids
//!
//! Repeats are kept from moving anything twice (`REQ-STM-4`, `INV-8`) by a split of duties:
//!
//! - **The host keeps at most one record per record id**, and applies each recorded input once. That is the host
//!   contract's guarantee `exactly-once` ([`host`], `T-005-02`, ADR-0002), not the state machine's.
//! - **One [`RecordId`] names exactly one content.** Recording a second, different input under a used id is a
//!   refusal by the record store, never a silent no-op; it is the host contract's to report.
//!
//! The state machine is a second line only: an input whose record id is the order's [`Order::last_record`] or
//! its [`LastInput::record_id`] comes back as [`Outcome::Repeat`]. It keeps no other history, so it can't tell an
//! older repeat from a fresh input, and doesn't try.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

pub use crate::provider_role::NodeRef;
pub use crate::router_api::{Endpoint, Id as NodeId, OperationKind};

pub mod host;

/// The contract's version: the first `## ` heading of `contracts/state-machine/CHANGELOG.md`.
pub const VERSION: &str = "1.0.0";

/// A rule of the contract that a value breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub &'static str);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Violation {}

macro_rules! non_empty_text {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The value, or `None` for empty text.
            pub fn new(text: impl Into<String>) -> Option<Self> {
                let text = text.into();
                (!text.is_empty()).then_some($name(text))
            }

            /// The value as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Violation;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                $name::new(text).ok_or(Violation(concat!(stringify!($name), " must not be empty")))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

non_empty_text!(
    /// A non-empty id or name: an order, part, segment, provider, node, asset, clock, party, decision record,
    /// evidence or acceptance record.
    Id
);

non_empty_text!(
    /// The id of the record entry an input was written as, before any effect (`INV-7`). Never empty: an input
    /// without its record can't be built or deserialised (`REQ-STM-3`). One record id names exactly one content.
    RecordId
);

/// An RFC 3339 date-time, `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`, with a real calendar date and no leap
/// second (seconds `00` to `59`). The schema's
/// `pattern` states the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp(String);

impl Timestamp {
    /// The timestamp, or `None` if the text isn't an RFC 3339 date-time.
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        is_rfc3339(&text).then_some(Timestamp(text))
    }

    /// The timestamp as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Timestamp {
    type Error = Violation;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Timestamp::new(text).ok_or(Violation("recorded_at must be an RFC 3339 date-time"))
    }
}

impl From<Timestamp> for String {
    fn from(value: Timestamp) -> Self {
        value.0
    }
}

/// Deserialises an optional id that, when present, is a real [`Id`]: `null` is refused, as the schemas refuse it.
/// An absent field stays `None` through `#[serde(default)]`.
fn some_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Id>, D::Error> {
    Id::deserialize(deserializer).map(Some)
}

fn is_rfc3339(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(range)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0')))
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        digits(0..4),
        digits(5..7),
        digits(8..10),
        digits(11..13),
        digits(14..16),
        digits(17..19),
    ) else {
        return false;
    };
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    let mut rest = &b[19..];
    if let Some(fraction) = rest.strip_prefix(b".") {
        let n = fraction.iter().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return false;
        }
        rest = &fraction[n..];
    }
    match rest {
        b"Z" => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            // Every byte is a digit before any arithmetic on it.
            if ![h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()) {
                return false;
            }
            let hours = u32::from(h1 - b'0') * 10 + u32::from(h2 - b'0');
            let minutes = u32::from(m1 - b'0') * 10 + u32::from(m2 - b'0');
            hours <= 23 && minutes <= 59
        }
        _ => false,
    }
}

/// The state of an order, or of one part of it (`REQ-STM-1`, `REQ-STM-2`). Schema:
/// `urn:rutent:state-machine:order-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    /// Not yet past its first segment.
    NotPastFirstSegment,
    /// Ended without completing, including cancelled by its reserve.
    EndedWithoutCompleting,
    /// Its first segment settled.
    FirstSegmentSettled,
    /// Waiting for the customer: among other causes, a part whose value is at the node where the destination
    /// reserve lives, after a failed final deposit, with no route left (`REQ-ORD-43`, [`after_no_route`]).
    WaitingForCustomer,
    /// Stuck: over time, in dispute, or with no route left to the node where the destination reserve lives
    /// (`REQ-ORD-28`, [`after_no_route`]).
    Stuck,
    /// Complete.
    Complete,
    /// Failed.
    Failed,
    /// Handed on, its value passed to a linked order (`REQ-ORD-43`).
    HandedOn,
}

impl OrderState {
    /// The published list, in the schema's order.
    pub const ALL: [OrderState; 8] = [
        OrderState::NotPastFirstSegment,
        OrderState::EndedWithoutCompleting,
        OrderState::FirstSegmentSettled,
        OrderState::WaitingForCustomer,
        OrderState::Stuck,
        OrderState::Complete,
        OrderState::Failed,
        OrderState::HandedOn,
    ];
}

/// The state of a segment (`REQ-STM-1`, `REQ-STM-2`). Schema: `urn:rutent:state-machine:segment-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    /// Part of the route, its provider not yet bound. A further state `REQ-STM-2` allows, so that every segment
    /// has a state from the moment its route is chosen (`REQ-STM-1`).
    Pending,
    /// Started, from when its provider is bound.
    Started,
    /// Settled.
    Settled,
    /// Failed, with the reversal owed.
    FailedReversalOwed,
    /// Outcome unknown.
    OutcomeUnknown,
    /// Reversal complete.
    ReversalComplete,
}

impl SegmentState {
    /// The published list, in the schema's order.
    pub const ALL: [SegmentState; 6] = [
        SegmentState::Pending,
        SegmentState::Started,
        SegmentState::Settled,
        SegmentState::FailedReversalOwed,
        SegmentState::OutcomeUnknown,
        SegmentState::ReversalComplete,
    ];
}

/// One segment of a route: the router's typed operation (`REQ-ORD-37`), a transition from one (node, asset) pair to
/// another at one provider (`REQ-STM-7`). A withdrawal, route or deposit keeps its asset, a transition that changes
/// no value. Schema: `urn:rutent:state-machine:segment-plan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct SegmentPlan {
    /// Unique within its order for the order's whole life; inputs name a segment by this id, never by position.
    pub segment_id: Id,
    /// The provider the segment runs at.
    pub provider: Id,
    /// The kind of the router's typed operation.
    pub kind: OperationKind,
    /// Where the value starts.
    pub from: Endpoint,
    /// Where the value ends.
    pub to: Endpoint,
}

impl SegmentPlan {
    /// Checks the typed operation's rules that JSON Schema can't state: a withdrawal, route or deposit keeps its
    /// asset; a route runs between two different nodes; an exchange stays at one node. The router's schema states
    /// which end of each kind is outer and which inner; this repeats those too, so a plan built in Rust is checked
    /// whole.
    pub fn check(&self) -> Result<(), Violation> {
        check_operation(self.kind, &self.from, &self.to)
    }
}

impl Serialize for SegmentPlan {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SegmentPlan::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for SegmentPlan {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let plan = SegmentPlan::deserialize(deserializer)?;
        plan.check().map_err(D::Error::custom)?;
        Ok(plan)
    }
}

/// The bare typed operation's rules, by the router contract's own [`crate::router_api::check_typed_operation`]:
/// one rule, stated once, for a priced operation and a segment plan alike.
fn check_operation(kind: OperationKind, from: &Endpoint, to: &Endpoint) -> Result<(), Violation> {
    crate::router_api::check_typed_operation(kind, from, to).map_err(|v| Violation(v.0))
}

/// Each segment of a route starts where the one before it ended, in the same unit, as the router's `Part::check`
/// requires of its operations, by the same [`crate::router_api::check_typed_chain`]; and the route ends with a
/// deposit (`REQ-ORD-16`).
fn check_chain<'a>(
    steps: impl Iterator<Item = (OperationKind, &'a Endpoint, &'a Endpoint)>,
) -> Result<(), Violation> {
    let steps: Vec<(OperationKind, &Endpoint, &Endpoint)> = steps.collect();
    crate::router_api::check_typed_chain(steps.iter().map(|(_, from, to)| (*from, *to))).map_err(
        |_| Violation("each segment starts where the one before it ended, in the same unit"),
    )?;
    match steps.last() {
        Some((OperationKind::Deposit, ..)) => Ok(()),
        _ => Err(Violation("a route ends with a deposit")),
    }
}

/// A part's destination (`INV-10`): its route's last deposit, that is the destination reserve, the outer node it
/// deposits to and the asset, decimals included. Schema: the `destination` of
/// `urn:rutent:state-machine:order#/$defs/part`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// The destination reserve: the provider of the route's last deposit.
    pub provider: Id,
    /// Where it deposits to: an outer node, and the asset with its decimals.
    pub to: Endpoint,
}

impl Destination {
    /// The destination of a route: its last segment, when that is a deposit.
    pub fn of(route: &[SegmentPlan]) -> Option<Destination> {
        let last = route.last()?;
        (last.kind == OperationKind::Deposit).then(|| Destination {
            provider: last.provider.clone(),
            to: last.to.clone(),
        })
    }

    fn of_segments(segments: &[Segment]) -> Option<Destination> {
        let last = segments.last()?;
        (last.kind == OperationKind::Deposit).then(|| Destination {
            provider: last.provider.clone(),
            to: last.to.clone(),
        })
    }
}

/// The published rule of `INV-10` for a `route_recalculated` input: whether its route ends at the part's
/// destination. One that doesn't is not applicable (`changes_destination`); another destination needs the
/// customer's accepted `destination_changed`.
pub fn recalculation_keeps_destination(part: &Part, route: &[SegmentPlan]) -> bool {
    Destination::of(route).as_ref() == Some(&part.destination)
}

/// The published rule for a `destination_changed` input (the orchestrator's ruling): whether it changes the
/// destination at all. One whose route keeps the destination ([`recalculation_keeps_destination`]) is a
/// recalculation, not a change of destination, and is not applicable (`keeps_destination`); it changes nothing,
/// `deposit_failed_at` included.
pub fn destination_change_applies(part: &Part, route: &[SegmentPlan]) -> bool {
    !recalculation_keeps_destination(part, route)
}

/// Who stated an input (`REQ-AUD-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The customer, the end user behind the order.
    Customer,
    /// The operator.
    Operator,
    /// A provider: a reserve, an exchange or a ramp.
    Provider,
    /// The router (`CMP-ROUTER`).
    Router,
    /// The evidence checker (`CMP-EVIDENCE`).
    EvidenceChecker,
    /// Core's orchestrator (`CMP-CORE-ORCH`).
    Orchestrator,
}

/// The maker of an input's statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// What kind of party it is.
    pub kind: SourceKind,
    /// Which one.
    pub id: Id,
}

/// Whether an input is what its source stated or what rutent verified (`REQ-AUD-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// What the source stated, attributed to it.
    Stated,
    /// What rutent verified.
    Verified,
}

/// What an input is about: an order, one of its parts, or one segment of a part, by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    /// The order's id.
    pub order: Id,
    /// The part's id, for an input about a part or a segment.
    #[serde(
        default,
        deserialize_with = "some_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub part: Option<Id>,
    /// The segment's id, for an input about a segment.
    #[serde(
        default,
        deserialize_with = "some_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub segment: Option<Id>,
}

/// One part of a chosen route (`REQ-ORD-42`, `DEC-8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartPlan {
    /// Unique within its order.
    pub part_id: Id,
    /// The part's route, in order; never empty.
    pub segments: Vec<SegmentPlan>,
}

/// Why a part is declared stuck. No route left is not among them: it arrives as [`Observed::NoRoute`] and
/// resolves by [`after_no_route`], to stuck or to waiting for the customer (`spec-no-route-left`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StuckReason {
    /// Over its time limit, as stated by whoever measured it.
    OverTime,
    /// A dispute over who carries a reversal (`REQ-ORD-11`).
    Dispute,
}

/// What an observed kind is about, which decides what its [`Subject`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The order as a whole: the subject names no part or segment.
    Order,
    /// One part: the subject names the part and no segment.
    Part,
    /// One segment: the subject names the part and the segment.
    Segment,
}

/// What was observed: a closed list of kinds (`REQ-STM-8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observed {
    /// The accepted route, as one or more parts; opens the order (`REQ-STM-7`).
    RouteChosen {
        /// The router's decision record, by id.
        decision_record: Id,
        /// The route's parts; never empty.
        parts: Vec<PartPlan>,
        /// The order whose value this order was handed (`REQ-ORD-43`).
        #[serde(
            default,
            deserialize_with = "some_id",
            skip_serializing_if = "Option::is_none"
        )]
        handed_on_from: Option<Id>,
    },
    /// A new route for one part, from where its value sits, to the same destination and to asset (`REQ-ORD-22`,
    /// `INV-10`). Automatic, with no acceptance: it acts within the customer's authorisation and floor
    /// (`REQ-AUZ-3`, `REQ-ORD-4`), and a route past the floor makes the part wait for the customer instead
    /// (`REQ-ORD-27`).
    RouteRecalculated {
        /// The router's decision record, by id.
        decision_record: Id,
        /// The part's new route, in order; never empty.
        segments: Vec<SegmentPlan>,
    },
    /// A new route for one part to another destination, the customer's choice after a failed final deposit
    /// (`REQ-ORD-43`). Never without the customer's acceptance (`REQ-ORD-34`, `REQ-AUZ-3`).
    DestinationChanged {
        /// The router's decision record, by id.
        decision_record: Id,
        /// The record of the customer's acceptance of the new destination, by id.
        acceptance: Id,
        /// The part's new route, in order; never empty.
        segments: Vec<SegmentPlan>,
    },
    /// No route for the part.
    NoRoute {
        /// The router's decision record, by id.
        decision_record: Id,
    },
    /// The segment's provider is bound.
    ProviderBound {},
    /// The segment settled.
    SegmentSettled {
        /// The evidence it settled on, by id.
        evidence: Id,
    },
    /// The segment failed; its reversal is owed.
    SegmentFailed {},
    /// The segment's outcome is unknown.
    SegmentOutcomeUnknown {},
    /// The segment was truly undone.
    ReversalCompleted {
        /// The evidence of the undo, by id.
        evidence: Id,
    },
    /// The part's reserve cancelled its withdrawal.
    WithdrawalCancelled {},
    /// The order is asked to end without completing.
    EndRequested {},
    /// The part is declared stuck; time over its limit arrives here, never from a clock.
    StuckDeclared {
        /// Why.
        reason: StuckReason,
    },
    /// The part's value is handed on to a new, linked order (`REQ-ORD-43`). Never without the customer's
    /// acceptance (`REQ-ORD-34`, `REQ-AUZ-3`).
    HandedOn {
        /// The order the value went to.
        linked_order: Id,
        /// The record of the customer's acceptance of the new order, by id.
        acceptance: Id,
    },
}

impl Observed {
    /// What this kind is about.
    pub fn scope(&self) -> Scope {
        match self {
            Observed::RouteChosen { .. } | Observed::EndRequested {} => Scope::Order,
            Observed::RouteRecalculated { .. }
            | Observed::DestinationChanged { .. }
            | Observed::NoRoute { .. }
            | Observed::WithdrawalCancelled {}
            | Observed::StuckDeclared { .. }
            | Observed::HandedOn { .. } => Scope::Part,
            Observed::ProviderBound {}
            | Observed::SegmentSettled { .. }
            | Observed::SegmentFailed {}
            | Observed::SegmentOutcomeUnknown {}
            | Observed::ReversalCompleted { .. } => Scope::Segment,
        }
    }
}

/// The only thing that changes a state (`REQ-STM-3`). Schema: `urn:rutent:state-machine:recorded-input`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct RecordedInput {
    /// The record entry the input was written as.
    pub record_id: RecordId,
    /// When it was recorded: the only time the state machine reads.
    pub recorded_at: Timestamp,
    /// Which clock gave `recorded_at` (`REQ-AUD-1`).
    pub time_source: Id,
    /// Who stated it.
    pub source: Source,
    /// Stated or verified.
    pub standing: Standing,
    /// What it is about.
    pub subject: Subject,
    /// What was observed.
    pub observed: Observed,
}

impl RecordedInput {
    /// Checks the rules a field type can't carry: the subject names exactly what the kind is about, and every
    /// route it carries has at least one part and segment, with unique ids.
    pub fn check(&self) -> Result<(), Violation> {
        let (part, segment) = (self.subject.part.is_some(), self.subject.segment.is_some());
        let named = match self.observed.scope() {
            Scope::Order => !part && !segment,
            Scope::Part => part && !segment,
            Scope::Segment => part && segment,
        };
        if !named {
            return Err(Violation(
                "the subject must name exactly what the observed kind is about",
            ));
        }
        let mut ids = BTreeSet::new();
        match &self.observed {
            Observed::RouteChosen { parts, .. } => {
                if parts.is_empty() {
                    return Err(Violation("a chosen route has at least one part"));
                }
                let mut part_ids = BTreeSet::new();
                for p in parts {
                    if !part_ids.insert(&p.part_id) {
                        return Err(Violation("part ids are unique within an order"));
                    }
                    route_ids(&p.segments, &mut ids)?;
                }
            }
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. } => {
                route_ids(segments, &mut ids)?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn route_ids<'a>(segments: &'a [SegmentPlan], ids: &mut BTreeSet<&'a Id>) -> Result<(), Violation> {
    if segments.is_empty() {
        return Err(Violation("a route has at least one segment"));
    }
    check_chain(segments.iter().map(|s| (s.kind, &s.from, &s.to)))?;
    for s in segments {
        s.check()?;
        if !ids.insert(&s.segment_id) {
            return Err(Violation("segment ids are unique within an order"));
        }
    }
    Ok(())
}

impl Serialize for RecordedInput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RecordedInput::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RecordedInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = RecordedInput::deserialize(deserializer)?;
        input.check().map_err(D::Error::custom)?;
        Ok(input)
    }
}

/// A segment of an order, with its one current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    /// Unique within its order for the order's whole life.
    pub segment_id: Id,
    /// The provider the segment runs at.
    pub provider: Id,
    /// The kind of the router's typed operation.
    pub kind: OperationKind,
    /// Where the value starts.
    pub from: Endpoint,
    /// Where the value ends.
    pub to: Endpoint,
    /// Its one current state.
    pub state: SegmentState,
}

impl Segment {
    /// A planned segment in the given state.
    pub fn new(plan: SegmentPlan, state: SegmentState) -> Self {
        let SegmentPlan {
            segment_id,
            provider,
            kind,
            from,
            to,
        } = plan;
        Segment {
            segment_id,
            provider,
            kind,
            from,
            to,
            state,
        }
    }

    /// The segment's plan, without its state.
    pub fn plan(&self) -> SegmentPlan {
        SegmentPlan {
            segment_id: self.segment_id.clone(),
            provider: self.provider.clone(),
            kind: self.kind,
            from: self.from.clone(),
            to: self.to.clone(),
        }
    }
}

/// One route of an order, with its own state (`REQ-ORD-42`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// Unique within its order.
    pub part_id: Id,
    /// The part's one current state.
    pub state: OrderState,
    /// The order this part's value was handed on to; present exactly when the part is handed on.
    #[serde(
        default,
        deserialize_with = "some_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub linked_order: Option<Id>,
    /// The part's current route, each segment with its state; never empty.
    pub segments: Vec<Segment>,
    /// The ids of segments a recalculation replaced. Kept so that no id is reused in the order's life, and so a
    /// late report about one is told apart from a report about nothing the order ever had.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired_segments: Vec<Id>,
    /// The part's destination (`INV-10`): its route's last deposit. Set when the route is chosen; kept by every
    /// recalculation; replaced only by the customer's accepted `destination_changed`.
    pub destination: Destination,
    /// The inner node where the part's last failed final deposit left its value: where the destination reserve
    /// lives (`REQ-ORD-43`). Kept by [`deposit_failed_at_after`]; [`after_no_route`] reads it. Absent if the part's
    /// final deposit never failed, or the customer has since changed its destination. Only an inner node: value
    /// never sits at an outer one.
    #[serde(
        default,
        deserialize_with = "some_node",
        skip_serializing_if = "Option::is_none"
    )]
    pub deposit_failed_at: Option<NodeId>,
}

fn some_node<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<NodeId>, D::Error> {
    NodeId::deserialize(deserializer).map(Some)
}

/// The published update rule for [`Part::deposit_failed_at`], applied with every input on the part: the value it
/// has after `input`, given the part before it.
///
/// - A `segment_failed` on the part's last segment, a deposit, sets it to that deposit's `from` node: each failed
///   final deposit overwrites it.
/// - A `destination_changed` clears it: the destination reserve, and so its node, is another one. One whose route
///   keeps the destination is not a change ([`destination_change_applies`]) and keeps it.
/// - Anything else, a `route_recalculated` included, keeps it.
pub fn deposit_failed_at_after(part: &Part, input: &RecordedInput) -> Option<NodeId> {
    match &input.observed {
        Observed::SegmentFailed {} => match part.segments.last() {
            Some(last)
                if last.kind == OperationKind::Deposit
                    && input.subject.segment.as_ref() == Some(&last.segment_id) =>
            {
                inner_node(&last.from.node).or_else(|| part.deposit_failed_at.clone())
            }
            _ => part.deposit_failed_at.clone(),
        },
        Observed::DestinationChanged { segments, .. }
            if destination_change_applies(part, segments) =>
        {
            None
        }
        _ => part.deposit_failed_at.clone(),
    }
}

fn inner_node(node: &NodeRef) -> Option<NodeId> {
    match node {
        NodeRef::Inner(id) => NodeId::new(id.as_str()),
        NodeRef::Outer(_) => None,
    }
}

/// Where a part's value sits, as its segments show it: at the start of its first segment that hasn't settled,
/// when that segment hasn't started, has been reversed, or is a deposit that failed (a failed deposit leaves the
/// value where it was, `REQ-ORD-43`), and that start is an inner node. `None` when the value is in flight (a
/// segment started, failed with its reversal owed, or with its outcome unknown), when it is still outside the book
/// (the withdrawal hasn't settled: nothing has moved, `REQ-ORD-12`, `REQ-ORD-40`), or when the part has delivered.
pub fn value_at(part: &Part) -> Option<&Endpoint> {
    let segment = part
        .segments
        .iter()
        .find(|s| s.state != SegmentState::Settled)?;
    let at = match (segment.state, segment.kind) {
        (SegmentState::Pending | SegmentState::ReversalComplete, _) => &segment.from,
        (SegmentState::FailedReversalOwed, OperationKind::Deposit) => &segment.from,
        _ => return None,
    };
    matches!(at.node, NodeRef::Inner(_)).then_some(at)
}

/// The published rule for a `no_route` input on a part (the owner's ruling `spec-no-route-left`): the state the
/// part goes to, or `None` when the rule can't place its value, and the input is not applicable from this state.
///
/// - A part whose value sits at the node where the destination reserve lives, after a failed final deposit,
///   waits for the customer (`REQ-ORD-43`). That node is [`Part::deposit_failed_at`], which [`Order::check`]
///   requires to be the failed deposit's `from` node while that deposit is the part's last segment.
/// - A part whose value can't be routed to that node becomes stuck (`REQ-ORD-28`).
///
/// "At that node" compares nodes only, as the ruling words it; the asset may differ after an exchange there.
pub fn after_no_route(part: &Part) -> Option<OrderState> {
    let at = value_at(part)?;
    Some(match (&part.deposit_failed_at, &at.node) {
        (Some(failed), NodeRef::Inner(here)) if failed.as_str() == here.as_str() => {
            OrderState::WaitingForCustomer
        }
        _ => OrderState::Stuck,
    })
}

/// Why an input changed nothing although it was about this order's subject or could have been.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotApplicableReason {
    /// The input is about another order.
    OtherOrder,
    /// It names a part or segment the order never had.
    UnknownSubject,
    /// The order or part has ended and doesn't change state (`REQ-STM-6`).
    Ended,
    /// The kind doesn't apply in the current state.
    NotFromThisState,
    /// A recalculation whose route ends at another destination or to asset (`INV-10`); a new destination needs
    /// [`Observed::DestinationChanged`], with the customer's acceptance.
    ChangesDestination,
    /// A `destination_changed` whose route ends at the part's destination: a recalculation, not a change of
    /// destination ([`destination_change_applies`]).
    KeepsDestination,
    /// A recalculation or change of destination whose route reuses a segment id the order has, current or
    /// retired: ids are unique for the order's whole life.
    ReusesSegmentId,
}

/// What the last input did (`REQ-AUD-1`): every input leaves a trace, including those that change nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    /// It changed the state.
    Applied {},
    /// Its record id was already applied or seen: the order's `last_record` or `last_input` (`REQ-STM-4`).
    Repeat {},
    /// It names a segment a recalculation retired: it matches no current segment.
    Late {},
    /// It changed nothing, for the reason given.
    NotApplicable {
        /// Why.
        reason: NotApplicableReason,
    },
}

/// The last input delivered to the order, and its outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastInput {
    /// The input's record id.
    pub record_id: RecordId,
    /// What it did.
    pub outcome: Outcome,
}

/// The state the transition takes and returns. Schema: `urn:rutent:state-machine:order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Order {
    /// The order's id.
    pub order_id: Id,
    /// The order's one current state.
    pub state: OrderState,
    /// The record id of the input that reached this state.
    pub last_record: RecordId,
    /// The last input delivered, applied or not, and its outcome (`REQ-AUD-1`).
    pub last_input: LastInput,
    /// The order whose value this order was handed (`REQ-ORD-43`).
    #[serde(
        default,
        deserialize_with = "some_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub handed_on_from: Option<Id>,
    /// The order's parts, one or more (`REQ-ORD-42`, `DEC-8`).
    pub parts: Vec<Part>,
}

impl Order {
    /// Checks the rules a field type can't carry. The schema states the first three; the last two only Rust
    /// can check.
    ///
    /// - The order has at least one part, and each part at least one segment.
    /// - A handed-on part names its linked order, and only a handed-on part does.
    /// - A handed-on order has a handed-on part.
    /// - Part ids, and segment ids current or retired, are unique within the order.
    /// - An `applied` last input is the input that reached the state.
    /// - Every segment meets its typed operation's rules ([`SegmentPlan::check`]), and each starts where the one
    ///   before it ended.
    /// - Every part's route ends with a deposit at the part's destination (`INV-10`).
    /// - While a part's last segment is a failed deposit, its `deposit_failed_at` is that deposit's `from` node; and
    ///   it has one only where a final deposit failed: that last segment, or one a recalculation since retired.
    pub fn check(&self) -> Result<(), Violation> {
        if self.parts.is_empty() {
            return Err(Violation("an order has at least one part"));
        }
        let mut part_ids = BTreeSet::new();
        let mut segment_ids = BTreeSet::new();
        for part in &self.parts {
            if part.segments.is_empty() {
                return Err(Violation("a part has at least one segment"));
            }
            for segment in &part.segments {
                check_operation(segment.kind, &segment.from, &segment.to)?;
            }
            check_chain(part.segments.iter().map(|s| (s.kind, &s.from, &s.to)))?;
            if NodeId::new(part.destination.provider.as_str()).is_none() {
                return Err(Violation(
                    "a destination reserve is an identifier: 1 to 128 of A-Z a-z 0-9 . _ : -, starting with a letter or digit",
                ));
            }
            if Destination::of_segments(&part.segments).as_ref() != Some(&part.destination) {
                return Err(Violation(
                    "a part's route ends with a deposit at its destination: the same reserve, outer node and asset",
                ));
            }
            let last = part.segments.last().expect("a part has a segment");
            if last.kind == OperationKind::Deposit
                && last.state == SegmentState::FailedReversalOwed
                && part.deposit_failed_at != inner_node(&last.from.node)
            {
                return Err(Violation(
                    "while a part's last segment is a failed deposit, deposit_failed_at is that deposit's from node",
                ));
            }
            let last_failed = last.kind == OperationKind::Deposit
                && last.state == SegmentState::FailedReversalOwed;
            if part.deposit_failed_at.is_some() && !last_failed && part.retired_segments.is_empty()
            {
                return Err(Violation(
                    "deposit_failed_at only where a final deposit failed: the last segment, or one a recalculation retired",
                ));
            }
            if (part.state == OrderState::HandedOn) != part.linked_order.is_some() {
                return Err(Violation(
                    "a part names a linked order exactly when it is handed on",
                ));
            }
            if !part_ids.insert(&part.part_id) {
                return Err(Violation("part ids are unique within an order"));
            }
            let ids = part
                .segments
                .iter()
                .map(|s| &s.segment_id)
                .chain(&part.retired_segments);
            for id in ids {
                if !segment_ids.insert(id) {
                    return Err(Violation(
                        "segment ids are unique within an order for its whole life, retired ones included",
                    ));
                }
            }
        }
        if self.state == OrderState::HandedOn
            && !self.parts.iter().any(|p| p.state == OrderState::HandedOn)
        {
            return Err(Violation("a handed-on order has a handed-on part"));
        }
        if self.last_input.outcome == (Outcome::Applied {})
            && self.last_input.record_id != self.last_record
        {
            return Err(Violation(
                "an applied last input is the input that reached the state",
            ));
        }
        Ok(())
    }
}

impl Serialize for Order {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Order::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Order {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let order = Order::deserialize(deserializer)?;
        order.check().map_err(D::Error::custom)?;
        Ok(order)
    }
}

/// The state machine's API: `transition(state, recordedInput) → newState` (SAD §3).
///
/// Both functions are pure and deterministic: they read no clock, draw no randomness and do no I/O, and they
/// take no `self`, so no hidden configuration can enter. Time and every other fact reach them only inside the
/// [`RecordedInput`]. The same inputs, in the same order, always reach the same states (`REQ-STM-3`, `INV-7`).
/// Every order they return passes [`Order::check`].
pub trait StateMachine {
    /// Opens an order from the input that accepted its route, or `None` if the input opens nothing.
    fn open(input: &RecordedInput) -> Option<Order>;

    /// The order's next state. Total: it always returns an order, whose [`Order::last_input`] says what this
    /// input did. An input that doesn't apply (a repeat, a late report, a report about another order or about
    /// an ended one) leaves every state as it was and is named there with its [`Outcome`] (`REQ-STM-4`,
    /// `REQ-STM-6`, `REQ-AUD-1`). The input stays recorded either way. A `no_route` input moves its part by
    /// [`after_no_route`], and is not applicable when that rule gives `None`. Every input updates
    /// [`Part::deposit_failed_at`] by [`deposit_failed_at_after`]. A `route_recalculated` that leaves the
    /// destination is not applicable (`changes_destination`, [`recalculation_keeps_destination`]); a
    /// `destination_changed` that keeps it is not applicable (`keeps_destination`,
    /// [`destination_change_applies`]).
    fn transition(state: &Order, input: &RecordedInput) -> Order;
}
