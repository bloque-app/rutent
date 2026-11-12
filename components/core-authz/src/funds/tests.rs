//! `T-021-04`'s tests: one authorisation names one order, its destination, its to asset and any floor; a move acts
//! only inside the one in force for its order; anything else needs a new one, which, for another destination or to
//! asset, only the customer gives.

use super::*;
use proptest::prelude::*;
use rutent_contracts::core_internal::calls::{DecisionEntry, Decisions};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, DecisionRequest, Destination, Entry, NotKnownCause, Outcome, Receipt,
    RefusalClass, Role, RouteEnd, VerifiedEntry,
};
use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;

use crate::engine::{Input, Need, Reply, Ruling, Said, Source};

fn read_fixture(name: &str) -> Value {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/valid/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A valid fixture of the contract, read as whatever type the call site wants.
macro_rules! valid {
    ($name:expr) => {
        serde_json::from_value(read_fixture($name)).unwrap()
    };
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn actor(party: &str, acts: &str, role: Role, kind: ActorKind) -> Actor {
    Actor {
        party: id(party),
        acts: id(acts),
        role,
        kind,
    }
}

/// How a log handle reads: the log it appends to, whole and current, as it must; or one of the ways an assembly could
/// wire it wrong, which the test sets.
#[derive(Debug, Clone, Default)]
enum Wiring {
    #[default]
    Whole,
    /// The read fails, as a log out of reach would.
    Fails,
    /// The read is a snapshot taken once, and never moves on.
    Frozen(Vec<Authorisation>),
    /// The read drops the first authorisation the log holds as permitted.
    DropsTheFirst,
    /// The read holds every authorisation the log holds as permitted, in reverse order.
    Reversed,
    /// The read is of another log than the one appended to.
    Other(Rc<RefCell<Vec<Entry>>>),
    /// The authorisations are read whole, but no segment's part can be.
    PlanUnreadable,
}

/// A log that takes every decision entry, shared by every engine over it, as one log is across a restart or between
/// two engines; each handle on it reads as its wiring says.
#[derive(Clone)]
struct Shared {
    entries: Rc<RefCell<Vec<Entry>>>,
    wiring: Rc<RefCell<Wiring>>,
    /// The order's plan as the log holds it: each segment, by id, and the part it belongs to.
    plan: Rc<RefCell<Vec<(Id, Id)>>>,
}

impl Default for Shared {
    /// A log whose plan places the fixtures' move's segment, `seg-2`, in a part of its own, `p0`.
    fn default() -> Self {
        Shared {
            entries: Rc::default(),
            wiring: Rc::default(),
            plan: Rc::new(RefCell::new(vec![(id("seg-2"), id("p0"))])),
        }
    }
}

impl Shared {
    /// Another handle on the same log, read whole.
    fn handle(&self) -> Shared {
        Shared {
            entries: Rc::clone(&self.entries),
            wiring: Rc::default(),
            plan: Rc::clone(&self.plan),
        }
    }

    /// From now on, this handle reads as `wiring` says.
    fn wire(&self, wiring: Wiring) {
        *self.wiring.borrow_mut() = wiring;
    }

    /// What the log holds as permitted, whole.
    fn whole(&self) -> Vec<Authorisation> {
        permitted_in(self.entries.borrow().iter())
    }
}

impl PolicyLog for Shared {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.entries
            .borrow_mut()
            .push(Entry::Verified(entry.entry().clone()));
        Ok(valid!("receipt-of-an-appended-entry"))
    }
}

impl Recorded for Shared {
    fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure> {
        let wiring = self.wiring.borrow().clone();
        match wiring {
            Wiring::Whole => Ok(self.whole()),
            Wiring::Fails => Err(ReadFailure::Unavailable),
            Wiring::Frozen(snapshot) => Ok(snapshot),
            Wiring::DropsTheFirst => Ok(self.whole().into_iter().skip(1).collect()),
            Wiring::Reversed => Ok(self.whole().into_iter().rev().collect()),
            Wiring::Other(other) => Ok(permitted_in(other.borrow().iter())),
            Wiring::PlanUnreadable => Ok(self.whole()),
        }
    }

    fn charged(&mut self) -> Result<Vec<crate::parties::Counted>, ReadFailure> {
        Ok(crate::parties::charged_in(self.entries.borrow().iter()))
    }

    fn part_of(&mut self, _: &Id, segment: &Id) -> Result<Option<Id>, ReadFailure> {
        if matches!(
            *self.wiring.borrow(),
            Wiring::Fails | Wiring::PlanUnreadable
        ) {
            return Err(ReadFailure::Unavailable);
        }
        Ok(self
            .plan
            .borrow()
            .iter()
            .find(|(s, _)| s == segment)
            .map(|(_, part)| part.clone()))
    }
}

/// Sources that say yes to every input but those the test sets, so what is decided is this row's rule, the
/// deployment's ruling and the fixed rules the test steers.
#[derive(Default)]
struct Sayings(Vec<(Need, Reply<Said>)>);

impl Sources for Sayings {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        self.0
            .iter()
            .find(|(n, _)| n == need)
            .map_or(Reply::Given(Said::Yes), |(_, said)| said.clone())
    }
}

/// The deployment's policy: it needs nothing, and permits what the fixed rules let through, but for the
/// authorisations it is set to refuse, and for everything once it is set out of reach.
#[derive(Default)]
struct Deployment {
    refuses: Vec<Id>,
    unreachable: bool,
}

impl Policy for Deployment {
    fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
        if self.unreachable {
            return Reply::Unreachable;
        }
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, asked: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        match asked {
            Asked::Authorise(a) if self.refuses.contains(&a.authorisation) => {
                Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted))
            }
            _ => Reply::Given(Ruling::Permit),
        }
    }
}

type Policed = Engine<Shared, Deployment, Sayings>;

/// An engine over a log of its own.
fn engine() -> Policed {
    engine_over(&Shared::default())
}

/// An engine opened over a handle of its own on `log`: after a restart, or beside another engine over one log.
fn engine_over(log: &Shared) -> Policed {
    Engine::with_policy(log.handle(), Deployment::default(), Sayings::default())
}

/// The last entry the log took.
fn last_entry(policy: &Policed) -> VerifiedEntry {
    let Some(Entry::Verified(entry)) = policy.log.entries.borrow().last().cloned() else {
        panic!("a verified entry");
    };
    entry
}

/// The fixtures' authorisation: `auth-17`, for `ord-17`, given by its requester `cust-1`, to `acct-ref-9` at
/// `bank-co` on `co-ach`, in `COP`, with a floor of 2400000.
fn authorisation() -> Authorisation {
    valid!("authorisation-of-one-order-with-a-floor")
}

/// What `a` names as its route's end, delivering `delivers`.
fn end_of(a: &Authorisation, delivers: &str) -> RouteEnd {
    RouteEnd {
        destination: a.destination.clone(),
        to_asset: a.to_asset.clone(),
        delivers: Amount::new(delivers).unwrap(),
    }
}

/// Core's move of a segment of `order`, under `authorisation`, on a route ending at `route`, as orchestration asks
/// it once the route was recalculated. Each request has its own id.
fn move_under(n: usize, order: &Id, authorisation: &Id, route: RouteEnd) -> DecisionRequest {
    move_on(n, order, &id("seg-2"), authorisation, route)
}

/// [`move_under`], of `segment`.
fn move_on(
    n: usize,
    order: &Id,
    segment: &Id,
    authorisation: &Id,
    route: RouteEnd,
) -> DecisionRequest {
    let mut request: DecisionRequest = valid!("decision-request-to-move-value");
    request.request = id(&format!("req-move-{n}"));
    let Action::MoveValue {
        order: o,
        segment: g,
        authorisation: a,
        route: r,
        ..
    } = &mut request.action
    else {
        panic!("a move");
    };
    *o = order.clone();
    *g = segment.clone();
    *a = authorisation.clone();
    *r = route;
    request
}

const REFUSED_OUTSIDE: Answer = Answer::Refuse {
    class: RefusalClass::OutsideAuthorisation,
};

const NOT_PERMITTED: Answer = Answer::Refuse {
    class: RefusalClass::NotPermitted,
};

/// The answer to a move, which the log records as it was given.
fn decided(policy: &mut Policed, request: &DecisionRequest) -> Answer {
    let decision = policy.decide(request).unwrap();
    assert_eq!(last_entry(policy).outcome, decision.outcome.outcome());
    decision.outcome
}

// --- The property ---------------------------------------------------------------------------------------------

const RESERVES: &[&str] = &["bank-co", "bank-mx", "us-bank"];
const NETWORKS: &[&str] = &["co-ach", "mx-spei"];
const ACCOUNTS: &[&str] = &["acct-ref-9", "acct-ref-4"];
const ASSETS: &[(&str, u8)] = &[("COP", 2), ("MXN", 2), ("COP", 0)];

fn node(network: &str) -> OuterNode {
    serde_json::from_value(json!({ "network": network })).unwrap()
}

fn asset(n: usize) -> Denomination {
    let (asset, decimals) = ASSETS[n];
    serde_json::from_value(json!({ "id": asset, "decimals": decimals })).unwrap()
}

/// One authorisation of the order `ord-17`, with its destination, to asset and floor drawn.
fn an_authorisation() -> impl Strategy<Value = Authorisation> {
    (
        0..RESERVES.len(),
        0..NETWORKS.len(),
        0..ACCOUNTS.len(),
        0..ASSETS.len(),
        prop::option::of(1u64..5_000_000),
    )
        .prop_map(|(r, n, a, s, floor)| {
            let mut authorisation = authorisation();
            authorisation.destination = Destination {
                reserve: id(RESERVES[r]),
                node: node(NETWORKS[n]),
                account: id(ACCOUNTS[a]),
            };
            authorisation.to_asset = asset(s);
            authorisation.floor = floor.and_then(|f| PositiveAmount::new(f.to_string()));
            authorisation
        })
}

/// How a recalculated route's end stands to the authorisation: kept whole, or one of its parts replaced by a draw,
/// which may be the same, and what it delivers drawn around the floor.
#[derive(Debug, Clone)]
enum Recalculated {
    Kept,
    Reserve(usize),
    Node(usize),
    Account(usize),
    Asset(usize),
    Order,
}

fn a_recalculation() -> impl Strategy<Value = (Recalculated, i64)> {
    (
        prop_oneof![
            3 => Just(Recalculated::Kept),
            1 => (0..RESERVES.len()).prop_map(Recalculated::Reserve),
            1 => (0..NETWORKS.len()).prop_map(Recalculated::Node),
            1 => (0..ACCOUNTS.len()).prop_map(Recalculated::Account),
            1 => (0..ASSETS.len()).prop_map(Recalculated::Asset),
            1 => Just(Recalculated::Order),
        ],
        prop_oneof![-2i64..=2, -3_000_000i64..3_000_000],
    )
}

/// One step of a run on one order split in two parts: an authorisation given, under one of three terms, for the
/// whole order or for one of its parts, by one of four givers, under one of five ids; a move of a segment of either
/// part, or of one no plan lists, under one of those ids, on a route recalculated against the terms that id names; or
/// a restart, a fresh engine opened over the same log.
#[derive(Debug, Clone)]
enum Step {
    Give {
        terms: usize,
        parts: usize,
        by: usize,
        id: usize,
    },
    Move {
        id: usize,
        segment: usize,
        recalculated: Recalculated,
        around: i64,
    },
    Restart,
}

fn a_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0..3usize, 0..3usize, 0..4usize, 0..5usize)
            .prop_map(|(terms, parts, by, id)| Step::Give { terms, parts, by, id }),
        5 => (0..5usize, 0..3usize, a_recalculation()).prop_map(|(id, segment, (recalculated, around))| {
            Step::Move {
                id,
                segment,
                recalculated,
                around,
            }
        }),
        1 => Just(Step::Restart),
    ]
}

/// The parts a step's authorisation names: none, so the whole order; `p1`; or `p2`.
fn parts_named(parts: usize) -> Option<Vec<Id>> {
    match parts {
        0 => None,
        1 => Some(vec![id("p1")]),
        _ => Some(vec![id("p2")]),
    }
}

/// The segment a step's move is of: `s1` of `p1`, `s2` of `p2`, or `s0`, which no plan lists.
fn segment_moved(segment: usize) -> (Id, Option<Id>) {
    match segment {
        0 => (id("s1"), Some(id("p1"))),
        1 => (id("s2"), Some(id("p2"))),
        _ => (id("s0"), None),
    }
}

/// The givers a step draws: the customer, its agent, the operator acting for the user, the origin reserve. The
/// sources and the deployment let each through, so only this row's rules tell them apart.
fn giver(by: usize) -> Actor {
    match by {
        0 => actor("cust-1", "cust-1", Role::Customer, ActorKind::Person),
        1 => actor("cust-1", "agent-7", Role::Customer, ActorKind::Agent),
        2 => actor("operator", "op-login-2", Role::Operator, ActorKind::Person),
        _ => actor("us-bank", "us-bank", Role::Provider, ActorKind::Component),
    }
}

/// What the order's register should be, kept apart from the engine's: every authorisation permitted, in order, from
/// which each part's terms in force and the ids in force under them are worked out.
#[derive(Default)]
struct Model {
    given: Vec<Authorisation>,
}

impl Model {
    /// Whether `a` speaks for `part`: a part it names, or any part where it names none; for no part, the order as a
    /// whole, which only one naming no part speaks for.
    fn speaks_for(a: &Authorisation, part: Option<&Id>) -> bool {
        match (part, &a.parts) {
            (_, None) => true,
            (Some(p), Some(parts)) => parts.contains(p),
            (None, Some(_)) => false,
        }
    }

    /// The parts `a` is held to: those it names, or, naming none, the order as a whole and every part named so far.
    fn held(&self, a: &Authorisation) -> Vec<Option<Id>> {
        if let Some(parts) = &a.parts {
            return parts.iter().cloned().map(Some).collect();
        }
        let mut held = vec![None];
        for p in self.given.iter().filter_map(|g| g.parts.as_ref()).flatten() {
            if !held.contains(&Some(p.clone())) {
                held.push(Some(p.clone()));
            }
        }
        held
    }

    /// `part`'s terms in force, and the ids in force under them: the first authorisation speaking for it sets them,
    /// one of the same terms joins them, and the customer's of other terms sets them anew.
    fn standing(&self, part: Option<&Id>) -> Option<(Authorisation, Vec<Id>)> {
        let mut now: Option<(Authorisation, Vec<Id>)> = None;
        for a in self.given.iter().filter(|a| Self::speaks_for(a, part)) {
            now = match now {
                None => Some((a.clone(), vec![a.authorisation.clone()])),
                Some((terms, mut ids)) if same_terms(&terms, a) => {
                    if !ids.contains(&a.authorisation) {
                        ids.push(a.authorisation.clone());
                    }
                    Some((terms, ids))
                }
                Some(_) if a.given_by.role == Role::Customer => {
                    Some((a.clone(), vec![a.authorisation.clone()]))
                }
                kept => kept,
            };
        }
        now
    }

    /// Whether `a` should be permitted: its id names it alone, and only while it is in force for every part it is
    /// held to; a new id under other terms than those in force for any of them only from the customer.
    fn permits(&self, a: &Authorisation) -> bool {
        let held = self.held(a);
        if let Some(given) = self
            .given
            .iter()
            .find(|g| g.authorisation == a.authorisation)
        {
            return given == a
                && held.iter().all(|p| {
                    self.standing(p.as_ref())
                        .is_some_and(|(_, ids)| ids.contains(&a.authorisation))
                });
        }
        let anew = held.iter().any(|p| {
            self.standing(p.as_ref())
                .is_some_and(|(terms, _)| !same_terms(&terms, a))
        });
        !anew || a.given_by.role == Role::Customer
    }

    fn permit(&mut self, a: &Authorisation) {
        self.given.push(a.clone());
    }

    /// Whether a move of `order`'s `part` under `id`, on `route`, should be permitted: `id` in force for the part,
    /// the route inside the part's terms in force.
    fn permits_move(&self, order: &Id, part: Option<&Id>, id: &Id, route: &RouteEnd) -> bool {
        // A segment no plan lists is in no part, and moves under no authorisation.
        if part.is_none() {
            return false;
        }
        let Some((terms, ids)) = self.standing(part) else {
            return false;
        };
        &terms.order == order
            && ids.contains(id)
            && route.destination == terms.destination
            && route.to_asset == terms.to_asset
            && terms
                .floor
                .as_ref()
                .is_none_or(|f| &route.delivers >= f.amount())
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Over runs of one split order's authorisations (a first, a restatement, a replacement, a return to earlier
    /// terms, an id given again, for the whole order or for one part, each by the customer, its agent, the operator
    /// or the origin reserve), of moves of either part's segments on routes recalculated around the terms they name,
    /// and of restarts, every move permitted goes to the destination, and delivers the to asset, of the authorisation
    /// it acts under, for its order, at least its floor; and each answer, to an authorisation and to a move, is the
    /// one the register of the move's own part should give, read from the log alone across every restart.
    // verifies: REQ-AUZ-3, INV-10, REQ-ORD-43
    #[test]
    fn no_recalculation_changes_destination_or_to_asset_under_one_authorisation(
        base in an_authorisation(),
        other in an_authorisation(),
        steps in prop::collection::vec(a_step(), 1..24),
    ) {
        let mut unfloored = base.clone();
        unfloored.floor = None;
        let terms = [base.clone(), other, unfloored];
        let log = Shared::default();
        log.plan.borrow_mut().extend([(id("s1"), id("p1")), (id("s2"), id("p2"))]);
        let mut policy = engine_over(&log);
        let mut model = Model::default();
        for (n, step) in steps.into_iter().enumerate() {
            match step {
                Step::Restart => policy = engine_over(&log),
                Step::Give { terms: t, parts, by, id: k } => {
                    let mut given = terms[t].clone();
                    given.authorisation = id(&format!("auth-p{k}"));
                    given.parts = parts_named(parts);
                    given.given_by = giver(by);
                    let should = model.permits(&given);
                    let answer = policy.authorise(&given).unwrap().outcome;
                    prop_assert_eq!(
                        answer,
                        if should { Answer::Permit {} } else { NOT_PERMITTED },
                        "step {}: {:?}", n, given
                    );
                    if should {
                        model.permit(&given);
                    }
                }
                Step::Move { id: k, segment, recalculated, around } => {
                    let named = id(&format!("auth-p{k}"));
                    let (segment, part) = segment_moved(segment);
                    let around_terms = model
                        .given
                        .iter()
                        .find(|g| g.authorisation == named)
                        .unwrap_or(&base)
                        .clone();
                    let floor: i64 = around_terms
                        .floor
                        .as_ref()
                        .map_or(1_000_000, |f| f.amount().as_str().parse().unwrap());
                    let delivers = (floor + around).max(0).to_string();
                    let mut route = end_of(&around_terms, &delivers);
                    let mut order = base.order.clone();
                    match &recalculated {
                        Recalculated::Kept => {}
                        Recalculated::Reserve(r) => route.destination.reserve = id(RESERVES[*r]),
                        Recalculated::Node(k) => route.destination.node = node(NETWORKS[*k]),
                        Recalculated::Account(a) => route.destination.account = id(ACCOUNTS[*a]),
                        Recalculated::Asset(s) => route.to_asset = asset(*s),
                        Recalculated::Order => order = id("ord-18"),
                    }
                    let should = model.permits_move(&order, part.as_ref(), &named, &route);
                    let request = move_on(n, &order, &segment, &named, route.clone());
                    let answer = decided(&mut policy, &request);
                    if answer == (Answer::Permit {}) {
                        let acted_under = model.given.iter().find(|g| g.authorisation == named);
                        prop_assert!(acted_under.is_some(), "step {}: a move under no authorisation", n);
                        let acted_under = acted_under.unwrap();
                        prop_assert_eq!(&route.destination, &acted_under.destination);
                        prop_assert_eq!(&route.to_asset, &acted_under.to_asset);
                        prop_assert_eq!(&order, &acted_under.order);
                        prop_assert!(Model::speaks_for(acted_under, part.as_ref()), "step {}: the part", n);
                    }
                    prop_assert_eq!(
                        answer,
                        if should { Answer::Permit {} } else { REFUSED_OUTSIDE },
                        "step {}: {:?} of {} under {} delivering {}", n, recalculated, segment, named, delivers
                    );
                }
            }
        }
    }
}

// --- A route outside the authorisation ------------------------------------------------------------------------

/// A move whose route ends anywhere but inside the authorisation in force for its order is refused as outside it:
/// another reserve, node or account, another to asset, below the floor, another order, an authorisation never given
/// or one policy refused. A new authorisation from the customer, here a lower floor, puts the route inside again,
/// and the one it replaced covers nothing more. The register's refusal never hides a fixed rule's: a mover acting
/// for no accountable party is refused as such. It beats not known. An id already given names that authorisation
/// only.
// verifies: REQ-AUZ-3
#[test]
fn route_outside_the_authorisation_needs_a_new_one() {
    let mut policy = engine();
    let first = authorisation();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    let order = first.order.clone();
    let inside = end_of(&first, "2500000");
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &first.authorisation, inside.clone())
        ),
        Answer::Permit {}
    );

    let mut outside = Vec::new();
    let mut other = inside.clone();
    other.destination.reserve = id("bank-mx");
    outside.push((
        "another reserve",
        order.clone(),
        first.authorisation.clone(),
        other,
    ));
    let mut other = inside.clone();
    other.destination.node = node("mx-spei");
    outside.push((
        "another node",
        order.clone(),
        first.authorisation.clone(),
        other,
    ));
    let mut other = inside.clone();
    other.destination.account = id("acct-ref-4");
    outside.push((
        "another account",
        order.clone(),
        first.authorisation.clone(),
        other,
    ));
    let mut other = inside.clone();
    other.to_asset = asset(1);
    outside.push((
        "another asset",
        order.clone(),
        first.authorisation.clone(),
        other,
    ));
    let below = end_of(&first, "2399999");
    outside.push((
        "below the floor",
        order.clone(),
        first.authorisation.clone(),
        below.clone(),
    ));
    outside.push((
        "another order",
        id("ord-18"),
        first.authorisation.clone(),
        inside.clone(),
    ));
    outside.push(("never given", order.clone(), id("auth-99"), inside.clone()));
    let mut refused = first.clone();
    refused.authorisation = id("auth-refused");
    refused.floor = PositiveAmount::new("1");
    policy.policy.1.refuses.push(refused.authorisation.clone());
    assert_eq!(policy.authorise(&refused).unwrap().outcome, NOT_PERMITTED);
    outside.push((
        "refused by policy",
        order.clone(),
        refused.authorisation.clone(),
        below.clone(),
    ));
    for (n, (why, order, authorisation, route)) in outside.iter().enumerate() {
        let request = move_under(10 + n, order, authorisation, route.clone());
        assert_eq!(decided(&mut policy, &request), REFUSED_OUTSIDE, "{why}");
    }

    // A mover the register says acts for another party is refused as acting for no one, outside or not
    // (`REQ-AUZ-8`, held before any other rule).
    let mut forged = move_under(30, &order, &id("auth-99"), inside.clone());
    forged.caller = actor("rutent", "evil-orch", Role::Core, ActorKind::Component);
    let acts_for = Need {
        input: id("evil-orch"),
        from: Source::ActsFor {
            acts: id("evil-orch"),
            party: id("rutent"),
            kind: ActorKind::Component,
        },
    };
    policy.sources.0.push((acts_for, Reply::Given(Said::No)));
    assert_eq!(
        decided(&mut policy, &forged),
        Answer::Refuse {
            class: RefusalClass::NoAccountableParty
        }
    );

    // Outside is a refusal even where the deployment's policy can't be reached.
    policy.policy.1.unreachable = true;
    assert_eq!(
        decided(
            &mut policy,
            &move_under(31, &order, &first.authorisation, below.clone())
        ),
        REFUSED_OUTSIDE
    );
    assert_eq!(
        decided(
            &mut policy,
            &move_under(32, &order, &first.authorisation, inside.clone())
        ),
        Answer::NotKnown {
            cause: NotKnownCause::PolicyUnreachable
        }
    );
    policy.policy.1.unreachable = false;

    // An id already given names that authorisation only: the same again is permitted, anything else under it refused.
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    let mut reused = first.clone();
    reused.floor = PositiveAmount::new("1");
    assert_eq!(policy.authorise(&reused).unwrap().outcome, NOT_PERMITTED);

    // The customer's new authorisation, a lower floor, puts the route below the old one inside, and the one it
    // replaced covers nothing more.
    let mut lower = first.clone();
    lower.authorisation = id("auth-18");
    lower.floor = PositiveAmount::new("2000000");
    assert_eq!(policy.authorise(&lower).unwrap().outcome, Answer::Permit {});
    assert_eq!(
        decided(
            &mut policy,
            &move_under(40, &order, &lower.authorisation, below.clone())
        ),
        Answer::Permit {}
    );
    assert_eq!(
        decided(
            &mut policy,
            &move_under(41, &order, &first.authorisation, inside.clone())
        ),
        REFUSED_OUTSIDE,
        "the authorisation replaced covers nothing more"
    );
    let registered: Vec<&Id> = policy
        .authorised()
        .permitted()
        .iter()
        .map(|a| &a.authorisation)
        .collect();
    // The register is the log's: the first given twice, identically, is held twice, and the refused ones not at all.
    assert_eq!(
        registered,
        [
            &first.authorisation,
            &first.authorisation,
            &lower.authorisation
        ]
    );
}

// --- A failed final deposit and the customer's new destination ------------------------------------------------

/// After a failed final deposit, recalculation to the same destination still acts under the order's authorisation;
/// another destination, or another to asset, is a new authorisation that only the customer gives, its person or its
/// agent. The operator acting for the user and the origin reserve, each of which the roles let authorise the order
/// at its origin, give none, and nothing is kept of a refused one. Once the customer gives it, moves go to its
/// destination alone, and the old authorisation covers nothing more. Each answer is recorded, its giver with it.
///
/// This is the single-part case: the order has one part, so its one authorisation, replaced, leaves no part behind.
/// A split order whose other part still heads to the first destination waits on the authorisation naming the parts
/// it covers (the review of `d3de45a`, finding 2).
// verifies: REQ-ORD-43
#[test]
fn customers_new_destination_after_a_failed_deposit_is_authorised_anew() {
    let mut policy = engine();
    let first = authorisation();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    let order = first.order.clone();
    let to_first = end_of(&first, "2500000");

    // The final deposit at `bank-co` failed; recalculation to the same destination comes first, under the same
    // authorisation.
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &first.authorisation, to_first.clone())
        ),
        Answer::Permit {}
    );

    // The customer's choice: another reserve and account, and another asset delivered there.
    let mut anew = first.clone();
    anew.authorisation = id("auth-17-anew");
    anew.destination = Destination {
        reserve: id("bank-mx"),
        node: node("mx-spei"),
        account: id("acct-ref-4"),
    };
    anew.to_asset = asset(1);
    anew.floor = PositiveAmount::new("100000");
    let to_anew = end_of(&anew, "120000");

    // Neither the operator acting for the user nor the origin reserve chooses it.
    let operator = actor("operator", "op-login-2", Role::Operator, ActorKind::Person);
    let reserve = actor("us-bank", "us-bank", Role::Provider, ActorKind::Component);
    for by in [operator, reserve] {
        let mut given = anew.clone();
        given.given_by = by.clone();
        assert_eq!(
            policy.authorise(&given).unwrap().outcome,
            NOT_PERMITTED,
            "{by:?}"
        );
        let entry = last_entry(&policy);
        assert_eq!(entry.who, by, "the refusal names who asked");
    }
    assert_eq!(
        policy.authorised().permitted(),
        std::slice::from_ref(&first)
    );
    assert_eq!(
        decided(
            &mut policy,
            &move_under(1, &order, &anew.authorisation, to_anew.clone())
        ),
        REFUSED_OUTSIDE
    );
    let mut to_elsewhere = to_first.clone();
    to_elsewhere.destination = anew.destination.clone();
    assert_eq!(
        decided(
            &mut policy,
            &move_under(2, &order, &first.authorisation, to_elsewhere)
        ),
        REFUSED_OUTSIDE,
        "no recalculation reaches the new destination under the old authorisation"
    );

    // The customer's own choice, by its agent as by its person, is the new authorisation. The single-part case: the
    // authorisation replaced is the order's only one.
    let agent = actor("cust-1", "agent-7", Role::Customer, ActorKind::Agent);
    for (n, by) in [first.given_by.clone(), agent].into_iter().enumerate() {
        let mut policy = engine();
        assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
        let mut given = anew.clone();
        given.given_by = by.clone();
        assert_eq!(policy.authorise(&given).unwrap().outcome, Answer::Permit {});
        let entry = last_entry(&policy);
        assert_eq!(entry.who, by);
        assert_eq!(entry.outcome, Outcome::Permitted {});
        assert_eq!(
            decided(
                &mut policy,
                &move_under(10 + n, &order, &anew.authorisation, to_anew.clone())
            ),
            Answer::Permit {}
        );
        let mut to_old = to_anew.clone();
        to_old.destination = first.destination.clone();
        assert_eq!(
            decided(
                &mut policy,
                &move_under(20 + n, &order, &anew.authorisation, to_old)
            ),
            REFUSED_OUTSIDE,
            "the new authorisation names the new destination only"
        );
        assert_eq!(
            decided(
                &mut policy,
                &move_under(30 + n, &order, &first.authorisation, to_first.clone())
            ),
            REFUSED_OUTSIDE,
            "the authorisation replaced covers nothing more"
        );
    }
}

// --- The register is the log's ----------------------------------------------------------------------------------

/// The customer's first destination, its new one, and the operator's own: `auth-17` to `bank-co`, `auth-18` to
/// `bank-mx` in another asset, both the customer's, and `auth-66`, the operator's, to a third account.
fn the_reviewers_three() -> (Authorisation, Authorisation, Authorisation) {
    let first = authorisation();
    let mut anew = first.clone();
    anew.authorisation = id("auth-18");
    anew.destination = Destination {
        reserve: id("bank-mx"),
        node: node("mx-spei"),
        account: id("acct-ref-4"),
    };
    anew.to_asset = asset(1);
    anew.floor = PositiveAmount::new("100000");
    let mut operators = first.clone();
    operators.authorisation = id("auth-66");
    operators.destination.account = id("acct-ref-66");
    operators.given_by = actor("operator", "op-login-2", Role::Operator, ActorKind::Person);
    (first, anew, operators)
}

/// A restart opens a fresh engine over the same log, and the register it holds moves to is the log's: the operator's
/// own destination is still refused, and so are moves under it; the customer's replaced authorisation given again is
/// refused, and moves to its destination with it; the customer's new one still acts.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn register_is_rebuilt_from_the_log_after_a_restart() {
    let (first, anew, operators) = the_reviewers_three();
    let order = first.order.clone();
    let log = Shared::default();
    let mut before = engine_over(&log);
    assert_eq!(before.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(before.authorise(&anew).unwrap().outcome, Answer::Permit {});
    drop(before);

    let mut after = engine_over(&log);
    assert_eq!(after.authorise(&operators).unwrap().outcome, NOT_PERMITTED);
    let to_operators = end_of(&operators, "2500000");
    assert_eq!(
        decided(
            &mut after,
            &move_under(0, &order, &operators.authorisation, to_operators)
        ),
        REFUSED_OUTSIDE
    );
    assert_eq!(after.authorise(&first).unwrap().outcome, NOT_PERMITTED);
    let to_first = end_of(&first, "2500000");
    assert_eq!(
        decided(
            &mut after,
            &move_under(1, &order, &first.authorisation, to_first.clone())
        ),
        REFUSED_OUTSIDE
    );
    assert_eq!(
        decided(
            &mut after,
            &move_under(2, &order, &anew.authorisation, end_of(&anew, "120000"))
        ),
        Answer::Permit {}
    );
}

/// Two engines over one log hold one register: the customer's new destination given through one is in force in the
/// other, whose next answers are held to it.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn two_engines_over_one_log_hold_one_register() {
    let (first, anew, operators) = the_reviewers_three();
    let order = first.order.clone();
    let log = Shared::default();
    let (mut one, mut two) = (engine_over(&log), engine_over(&log));
    assert_eq!(one.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(two.authorise(&anew).unwrap().outcome, Answer::Permit {});
    assert_eq!(one.authorise(&operators).unwrap().outcome, NOT_PERMITTED);
    assert_eq!(
        decided(
            &mut one,
            &move_under(0, &order, &first.authorisation, end_of(&first, "2500000"))
        ),
        REFUSED_OUTSIDE
    );
    assert_eq!(
        decided(
            &mut one,
            &move_under(1, &order, &anew.authorisation, end_of(&anew, "120000"))
        ),
        Answer::Permit {}
    );
}

/// The floor is one of the terms: the operator acting for the user, or the origin reserve, giving the same destination
/// and to asset with no floor is giving a new authorisation, and is refused; the customer giving it is permitted.
// verifies: REQ-AUZ-3
#[test]
fn new_floor_from_anyone_but_the_customer_is_refused() {
    let first = authorisation();
    for by in [
        actor("operator", "op-login-2", Role::Operator, ActorKind::Person),
        actor("us-bank", "us-bank", Role::Provider, ActorKind::Component),
    ] {
        let mut policy = engine();
        assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
        let mut unfloored = first.clone();
        unfloored.authorisation = id("auth-17-unfloored");
        unfloored.floor = None;
        unfloored.given_by = by.clone();
        assert_eq!(
            policy.authorise(&unfloored).unwrap().outcome,
            NOT_PERMITTED,
            "{by:?}"
        );
        unfloored.given_by = first.given_by.clone();
        assert_eq!(
            policy.authorise(&unfloored).unwrap().outcome,
            Answer::Permit {}
        );
    }
}

/// Terms given again after others replaced them come back in force under their new id alone: the id they were first
/// given under covers nothing more.
// verifies: REQ-AUZ-3
#[test]
fn earlier_terms_given_again_are_in_force_under_their_new_id_alone() {
    let (first, anew, _) = the_reviewers_three();
    let order = first.order.clone();
    let mut policy = engine();
    let mut again = first.clone();
    again.authorisation = id("auth-19");
    for given in [&first, &anew, &again] {
        assert_eq!(policy.authorise(given).unwrap().outcome, Answer::Permit {});
    }
    let to_first = end_of(&first, "2500000");
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &first.authorisation, to_first.clone())
        ),
        REFUSED_OUTSIDE
    );
    assert_eq!(
        decided(
            &mut policy,
            &move_under(1, &order, &again.authorisation, to_first)
        ),
        Answer::Permit {}
    );
}

/// The same terms stated again under a new id, here by the operator acting for the user, keep the earlier id in force
/// beside it: a move under either is inside.
// verifies: REQ-AUZ-3
#[test]
fn restatement_keeps_the_earlier_id_in_force_beside_it() {
    let first = authorisation();
    let order = first.order.clone();
    let mut policy = engine();
    let mut restated = first.clone();
    restated.authorisation = id("auth-17-restated");
    restated.given_by = actor("operator", "op-login-2", Role::Operator, ActorKind::Person);
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(
        policy.authorise(&restated).unwrap().outcome,
        Answer::Permit {}
    );
    for (n, under) in [&first, &restated].into_iter().enumerate() {
        assert_eq!(
            decided(
                &mut policy,
                &move_under(n, &order, &under.authorisation, end_of(&first, "2500000"))
            ),
            Answer::Permit {},
            "{}",
            under.authorisation.as_str()
        );
    }
}

/// An authorisation replaced and then given again exactly as it was is refused, so the log never holds as permitted
/// one the register holds as replaced.
// verifies: REQ-AUZ-3
#[test]
fn replaced_authorisation_given_again_is_refused() {
    let (first, anew, _) = the_reviewers_three();
    let mut policy = engine();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&first).unwrap().outcome, NOT_PERMITTED);
    assert_eq!(
        last_entry(&policy).outcome,
        Outcome::Refused {
            class: RefusalClass::NotPermitted
        }
    );
}

/// An engine whose read of the log fails holds no register, neither an empty one nor the one it read last: it
/// refuses every authorisation and every move, the register an input it doesn't have, on its first read as on any.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn register_that_cannot_be_read_refuses_every_authorisation_and_move() {
    let (first, _, operators) = the_reviewers_three();
    let order = first.order.clone();
    let mut policy = engine();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    let inside = move_under(0, &order, &first.authorisation, end_of(&first, "2500000"));
    assert_eq!(decided(&mut policy, &inside), Answer::Permit {});

    policy.log.wire(Wiring::Fails);
    let missing = Answer::Refuse {
        class: RefusalClass::MissingInput,
    };
    assert_eq!(policy.authorise(&operators).unwrap().outcome, missing);
    let inside = move_under(1, &order, &first.authorisation, end_of(&first, "2500000"));
    assert_eq!(decided(&mut policy, &inside), missing);

    // An engine opened again whose first read fails has nothing earlier to compare it with, and refuses as well.
    let mut reopened = engine_over(&policy.log);
    reopened.log.wire(Wiring::Fails);
    assert_eq!(reopened.authorise(&operators).unwrap().outcome, missing);
}

// --- A read that isn't the log's, whole and current -------------------------------------------------------------

const MISSING: Answer = Answer::Refuse {
    class: RefusalClass::MissingInput,
};

/// A read frozen at a snapshot: the engine gives the customer's new destination, and the snapshot, which lacks it, is
/// what it next reads. It refuses, a move under the authorisation replaced and that authorisation given again alike,
/// since the read lacks its own last permit.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn snapshot_read_missing_the_engines_own_permit_is_refused() {
    let (first, anew, _) = the_reviewers_three();
    let order = first.order.clone();
    let mut policy = engine();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    policy.log.wire(Wiring::Frozen(policy.log.whole()));
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    let to_first = end_of(&first, "2500000");
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &first.authorisation, to_first)
        ),
        MISSING
    );
    assert_eq!(policy.authorise(&first).unwrap().outcome, MISSING);
}

/// A read that drops what an earlier read held doesn't extend it: the engine refuses, though the read still holds the
/// engine's own last permit.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn truncated_read_of_the_log_is_refused() {
    let (first, anew, _) = the_reviewers_three();
    let order = first.order.clone();
    let mut policy = engine();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    let to_anew = end_of(&anew, "120000");
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &anew.authorisation, to_anew.clone())
        ),
        Answer::Permit {}
    );
    policy.log.wire(Wiring::DropsTheFirst);
    assert_eq!(
        decided(
            &mut policy,
            &move_under(1, &order, &anew.authorisation, to_anew)
        ),
        MISSING
    );
    assert_eq!(policy.authorise(&first).unwrap().outcome, MISSING);
}

/// A read of another log than the one the engine appends to lacks the engine's own permit once it gives one, and the
/// engine refuses from then on: the other log's authorisation in force, which the customer's new one replaced on this
/// log, never acts again.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn read_over_another_log_is_refused() {
    let (first, anew, _) = the_reviewers_three();
    let order = first.order.clone();
    let other = Shared::default();
    let mut elsewhere = engine_over(&other);
    assert_eq!(
        elsewhere.authorise(&first).unwrap().outcome,
        Answer::Permit {}
    );
    let mut policy = engine();
    policy.log.wire(Wiring::Other(Rc::clone(&other.entries)));
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &first.authorisation, end_of(&first, "2500000"))
        ),
        MISSING
    );
    assert_eq!(
        decided(
            &mut policy,
            &move_under(1, &order, &anew.authorisation, end_of(&anew, "120000"))
        ),
        MISSING
    );
}

/// The entry an engine appends when it permits `a`, appended here straight to `log`, as an engine racing another would
/// leave it.
fn append_permitted(log: &Shared, a: &Authorisation) {
    let entry = VerifiedEntry {
        kind: rutent_contracts::core_internal::VerifiedKind::Verified,
        who: a.given_by.clone(),
        what: What::Authorise {
            authorisation: a.clone(),
            decision: a.authorisation.clone(),
        },
        when: rutent_contracts::core_internal::When {
            at: a.at.clone(),
            source: rutent_contracts::core_internal::TimeSource::CoreClock,
        },
        outcome: Outcome::Permitted {},
    };
    log.entries.borrow_mut().push(Entry::Verified(entry));
}

/// Two engines raced: both read `auth-17` in force, one permitted the customer's new destination, `auth-18`, and the
/// other a restatement of `auth-17` appended after it, by the operator acting for the user (`auth-70`) or by the origin
/// reserve (`auth-71`). Neither's authorisation sets the terms: each only joins those already in force, so the old
/// destination isn't back by either's hand. The restatement is not in force, and `auth-18` is.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn operators_restatement_raced_past_a_new_destination_is_not_in_force() {
    let (first, anew, _) = the_reviewers_three();
    let order = first.order.clone();
    let givers = [
        (
            "auth-70",
            actor("operator", "op-login-2", Role::Operator, ActorKind::Person),
        ),
        (
            "auth-71",
            actor("us-bank", "us-bank", Role::Provider, ActorKind::Component),
        ),
    ];
    for (n, (restatement, by)) in givers.into_iter().enumerate() {
        let mut restated = first.clone();
        restated.authorisation = id(restatement);
        restated.given_by = by;
        let log = Shared::default();
        for a in [&first, &anew, &restated] {
            append_permitted(&log, a);
        }
        let mut policy = engine_over(&log);
        assert_eq!(
            decided(
                &mut policy,
                &move_under(
                    10 * n,
                    &order,
                    &restated.authorisation,
                    end_of(&first, "2500000")
                )
            ),
            REFUSED_OUTSIDE,
            "{restatement}"
        );
        assert_eq!(
            decided(
                &mut policy,
                &move_under(
                    10 * n + 1,
                    &order,
                    &anew.authorisation,
                    end_of(&anew, "120000")
                )
            ),
            Answer::Permit {},
            "{restatement}"
        );
        assert_eq!(
            policy.authorise(&restated).unwrap().outcome,
            NOT_PERMITTED,
            "{restatement}"
        );
    }
}

/// A read as long as the one before it, but holding its entries in another order, doesn't extend it: the engine
/// refuses, though the read is no shorter and still holds the engine's own last permit. Read whole in that order, the
/// customer's first authorisation, coming last, would be back in force.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn reordered_read_of_the_log_is_refused() {
    let (first, anew, _) = the_reviewers_three();
    let order = first.order.clone();
    let mut policy = engine();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &anew.authorisation, end_of(&anew, "120000"))
        ),
        Answer::Permit {}
    );
    policy.log.wire(Wiring::Reversed);
    assert_eq!(
        decided(
            &mut policy,
            &move_under(1, &order, &first.authorisation, end_of(&first, "2500000"))
        ),
        MISSING
    );
    assert_eq!(policy.authorise(&first).unwrap().outcome, MISSING);
}

// --- A split order, authorised per part ------------------------------------------------------------------------

/// The split order's plan, as the log holds it: `s1` of `p1`, `s2` of `p2`.
fn split(log: &Shared) {
    log.plan
        .borrow_mut()
        .extend([(id("s1"), id("p1")), (id("s2"), id("p2"))]);
}

/// A split order's other part goes on after one part's new destination: the order is authorised whole under
/// `auth-17`; `p1`'s final deposit fails, and the customer's `auth-18`, naming `p1` alone, gives it a new
/// destination. `p2` still moves under `auth-17` to the first destination, `p1` under `auth-18` to the new one, and
/// `p1` under `auth-17` no more. A new destination for `p2` from the operator acting for the user is refused, as for
/// any part.
// verifies: REQ-ORD-43, REQ-AUZ-3, INV-10
#[test]
fn split_orders_other_part_moves_on_under_its_authorisation_after_one_parts_new_destination() {
    let (first, mut anew, _) = the_reviewers_three();
    anew.parts = Some(vec![id("p1")]);
    let order = first.order.clone();
    let log = Shared::default();
    split(&log);
    let mut policy = engine_over(&log);
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    let to_first = end_of(&first, "2500000");
    let to_anew = end_of(&anew, "120000");
    for (n, segment) in ["s1", "s2"].into_iter().enumerate() {
        let request = move_on(
            n,
            &order,
            &id(segment),
            &first.authorisation,
            to_first.clone(),
        );
        assert_eq!(
            decided(&mut policy, &request),
            Answer::Permit {},
            "{segment}"
        );
    }

    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    let on = |n, segment: &str, under: &Authorisation, route: &RouteEnd| {
        move_on(n, &order, &id(segment), &under.authorisation, route.clone())
    };
    assert_eq!(
        decided(&mut policy, &on(10, "s2", &first, &to_first)),
        Answer::Permit {},
        "p2 goes on under the order's authorisation"
    );
    assert_eq!(
        decided(&mut policy, &on(11, "s1", &anew, &to_anew)),
        Answer::Permit {},
        "p1 goes to its new destination"
    );
    assert_eq!(
        decided(&mut policy, &on(12, "s1", &first, &to_first)),
        REFUSED_OUTSIDE,
        "p1 no more under the authorisation replaced for it"
    );

    let mut operators = anew.clone();
    operators.authorisation = id("auth-19");
    operators.parts = Some(vec![id("p2")]);
    operators.given_by = actor("operator", "op-login-2", Role::Operator, ActorKind::Person);
    assert_eq!(policy.authorise(&operators).unwrap().outcome, NOT_PERMITTED);

    // After a restart, the same: the register is the log's, part by part.
    let mut reopened = engine_over(&log);
    assert_eq!(
        decided(&mut reopened, &on(20, "s2", &first, &to_first)),
        Answer::Permit {}
    );
    assert_eq!(
        decided(&mut reopened, &on(21, "s1", &first, &to_first)),
        REFUSED_OUTSIDE
    );
}

/// A replacement for one part covers that part alone: a move of `p2` under `auth-18`, which names `p1`, is refused,
/// whether to `auth-18`'s destination or to the order's, and so is a move of a segment no plan lists, which is in no
/// part.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn move_on_another_part_under_a_parts_replacement_is_refused() {
    let (first, mut anew, _) = the_reviewers_three();
    anew.parts = Some(vec![id("p1")]);
    let order = first.order.clone();
    let log = Shared::default();
    split(&log);
    let mut policy = engine_over(&log);
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    for (n, (segment, route)) in [
        ("s2", end_of(&anew, "120000")),
        ("s2", end_of(&first, "2500000")),
        ("s0", end_of(&anew, "120000")),
    ]
    .into_iter()
    .enumerate()
    {
        let request = move_on(n, &order, &id(segment), &anew.authorisation, route);
        assert_eq!(decided(&mut policy, &request), REFUSED_OUTSIDE, "{segment}");
    }
}

/// A segment's part is read from the plans the log holds, as the log reads it: the part a `route_chosen` lists it
/// under, or the part a `destination_changed` gave it to; none for a segment no plan lists, of another order, or named
/// only by an input that plans nothing.
// verifies: REQ-AUZ-3
#[test]
fn segments_part_is_read_from_the_plans_the_log_holds() {
    let entries: Vec<Entry> = [
        "verified-entry-of-a-recorded-input-route-chosen-three-parts",
        "verified-entry-of-a-recorded-input-destination-changed-with-acceptance",
        "verified-entry-of-a-recorded-input",
    ]
    .into_iter()
    .map(|name| valid!(name))
    .collect();
    let part = |order: &str, segment: &str| part_in(&entries, &id(order), &id(segment));
    assert_eq!(part("ord-1", "p2-s2"), Some(id("p2")));
    assert_eq!(part("ord-1", "p3-s3"), Some(id("p3")));
    assert_eq!(
        part("ord-1", "p1-s4"),
        Some(id("p1")),
        "a destination changed"
    );
    assert_eq!(part("ord-1", "p9-s1"), None);
    assert_eq!(part("ord-2", "p2-s2"), None, "another order");
}

/// The origin reserve is one of the terms, as the log reads them: the same destination from another origin reserve,
/// given by the operator acting for the user, is a new authorisation, and refused; given by the customer, it replaces
/// the one in force.
// verifies: REQ-AUZ-3
#[test]
fn authorisation_from_another_origin_reserve_is_a_new_one() {
    let first = authorisation();
    let order = first.order.clone();
    let mut policy = engine();
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    let mut elsewhere = first.clone();
    elsewhere.authorisation = id("auth-17-elsewhere");
    elsewhere.origin_reserve = id("bank-mx");
    elsewhere.given_by = actor("operator", "op-login-2", Role::Operator, ActorKind::Person);
    assert_eq!(policy.authorise(&elsewhere).unwrap().outcome, NOT_PERMITTED);
    elsewhere.given_by = first.given_by.clone();
    assert_eq!(
        policy.authorise(&elsewhere).unwrap().outcome,
        Answer::Permit {}
    );
    assert_eq!(
        decided(
            &mut policy,
            &move_under(0, &order, &first.authorisation, end_of(&first, "2500000"))
        ),
        REFUSED_OUTSIDE
    );
}

/// The order's authorisation, replaced for `p1` alone, still speaks for `p2`, but given again exactly as it was it is
/// refused: it is no longer in force for every part it speaks for, and permitted again, it would set `p1`'s old
/// destination anew.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn authorisation_replaced_for_one_part_given_again_is_refused() {
    let (first, mut anew, _) = the_reviewers_three();
    anew.parts = Some(vec![id("p1")]);
    let order = first.order.clone();
    let log = Shared::default();
    split(&log);
    let mut policy = engine_over(&log);
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&first).unwrap().outcome, NOT_PERMITTED);
    let to_first = end_of(&first, "2500000");
    assert_eq!(
        decided(
            &mut policy,
            &move_on(0, &order, &id("s1"), &first.authorisation, to_first)
        ),
        REFUSED_OUTSIDE
    );
}

/// A move whose segment's part can't be read is refused, the part an input the decision doesn't have, though the
/// authorisations are read whole.
// verifies: REQ-AUZ-3
#[test]
fn move_whose_part_cannot_be_read_is_refused() {
    let first = authorisation();
    let order = first.order.clone();
    let log = Shared::default();
    split(&log);
    let mut policy = engine_over(&log);
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    policy.log.wire(Wiring::PlanUnreadable);
    let request = move_on(
        0,
        &order,
        &id("s1"),
        &first.authorisation,
        end_of(&first, "2500000"),
    );
    assert_eq!(decided(&mut policy, &request), MISSING);
}

/// A move of a segment no plan of the order lists acts under no authorisation, as the log admits no such move
/// (`move-segment-is-in-its-orders-plan`): the order's `auth-17`, replaced for `p1` by `auth-18`, moves no segment
/// the plans don't place in a part, and neither does `auth-18`.
// verifies: REQ-AUZ-3, INV-10
#[test]
fn move_of_a_segment_no_plan_lists_is_refused() {
    let (first, mut anew, _) = the_reviewers_three();
    anew.parts = Some(vec![id("p1")]);
    let order = first.order.clone();
    let log = Shared::default();
    split(&log);
    let mut policy = engine_over(&log);
    assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
    assert_eq!(policy.authorise(&anew).unwrap().outcome, Answer::Permit {});
    for (n, (under, route)) in [
        (&first, end_of(&first, "2500000")),
        (&anew, end_of(&anew, "120000")),
    ]
    .into_iter()
    .enumerate()
    {
        let request = move_on(n, &order, &id("s-unplanned"), &under.authorisation, route);
        assert_eq!(
            decided(&mut policy, &request),
            REFUSED_OUTSIDE,
            "{}",
            under.authorisation.as_str()
        );
    }
}

/// A segment two plans list is read as of the part the first one places it in, as the log reads it
/// (core-internal's `part_of_segment` takes the first plan that lists it).
// verifies: REQ-AUZ-3
#[test]
fn segment_two_plans_list_is_of_the_part_the_first_places_it_in() {
    let chosen: Entry = valid!("verified-entry-of-a-recorded-input-route-chosen-three-parts");
    let mut changed: Value =
        read_fixture("verified-entry-of-a-recorded-input-destination-changed-with-acceptance");
    changed["what"]["input"]["subject"]["part"] = json!("p2");
    changed["what"]["input"]["observed"]["segments"][0]["segment_id"] = json!("p1-s1");
    let changed: Entry = serde_json::from_value(changed).unwrap();
    let segment = id("p1-s1");
    assert_eq!(
        part_in([&chosen, &changed], &id("ord-1"), &segment),
        Some(id("p1"))
    );
    assert_eq!(
        part_in([&changed, &chosen], &id("ord-1"), &segment),
        Some(id("p2"))
    );
}
