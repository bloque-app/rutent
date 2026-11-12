//! `T-021-04`'s tests: one authorisation names one order, its destination, its to asset and any floor; a move acts
//! only inside the one in force for its order; anything else needs a new one, which, for another destination or to
//! asset, only the customer gives.

use super::*;
use proptest::prelude::*;
use rutent_contracts::core_internal::calls::{DecisionEntry, Decisions};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, DecisionRequest, Destination, Entry, NotKnownCause, Outcome, Receipt,
    RefusalClass, Role, RouteEnd,
};
use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};
use serde_json::{Value, json};

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

/// A log that takes every decision entry, so what was recorded can be seen.
#[derive(Default)]
struct Taking(Vec<Entry>);

impl PolicyLog for Taking {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.0.push(Entry::Verified(entry.entry().clone()));
        Ok(valid!("receipt-of-an-appended-entry"))
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

type Policed = Engine<Taking, Deployment, Sayings>;

fn engine() -> Policed {
    Engine::with_policy(Taking::default(), Deployment::default(), Sayings::default())
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
    let mut request: DecisionRequest = valid!("decision-request-to-move-value");
    request.request = id(&format!("req-move-{n}"));
    let Action::MoveValue {
        order: o,
        authorisation: a,
        route: r,
        ..
    } = &mut request.action
    else {
        panic!("a move");
    };
    *o = order.clone();
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
    let Some(Entry::Verified(entry)) = policy.log.0.last() else {
        panic!("a verified entry");
    };
    assert_eq!(entry.outcome, decision.outcome.outcome());
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

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Over one authorisation and every run of recalculations under it, each recalculated route's moves are
    /// permitted exactly where the route ends at the authorisation's destination (reserve, node and account), in its
    /// to asset, for its order, delivering at least its floor: so no move permitted under it goes to another
    /// destination or delivers another asset, and each move that keeps inside it is still permitted.
    // verifies: REQ-AUZ-3, INV-10
    #[test]
    fn no_recalculation_changes_destination_or_to_asset_under_one_authorisation(
        authorised in an_authorisation(),
        recalculations in prop::collection::vec(a_recalculation(), 1..12),
    ) {
        let mut policy = engine();
        let decision = policy.authorise(&authorised).unwrap();
        prop_assert_eq!(decision.outcome, Answer::Permit {});
        let floor: i64 = authorised
            .floor
            .as_ref()
            .map_or(1_000_000, |f| f.amount().as_str().parse().unwrap());
        for (n, (recalculated, around)) in recalculations.into_iter().enumerate() {
            let delivers = (floor + around).max(0).to_string();
            let mut route = end_of(&authorised, &delivers);
            let mut order = authorised.order.clone();
            match &recalculated {
                Recalculated::Kept => {}
                Recalculated::Reserve(r) => route.destination.reserve = id(RESERVES[*r]),
                Recalculated::Node(k) => route.destination.node = node(NETWORKS[*k]),
                Recalculated::Account(a) => route.destination.account = id(ACCOUNTS[*a]),
                Recalculated::Asset(s) => route.to_asset = asset(*s),
                Recalculated::Order => order = id("ord-18"),
            }
            let inside = order == authorised.order
                && route.destination == authorised.destination
                && route.to_asset == authorised.to_asset
                && authorised.floor.as_ref().is_none_or(|f| &route.delivers >= f.amount());
            let request = move_under(n, &order, &authorised.authorisation, route.clone());
            let answer = decided(&mut policy, &request);
            if answer == (Answer::Permit {}) {
                prop_assert_eq!(&route.destination, &authorised.destination);
                prop_assert_eq!(&route.to_asset, &authorised.to_asset);
                prop_assert_eq!(&order, &authorised.order);
            }
            prop_assert_eq!(
                answer,
                if inside { Answer::Permit {} } else { REFUSED_OUTSIDE },
                "{:?} delivering {}", recalculated, delivers
            );
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
    assert_eq!(registered, [&first.authorisation, &lower.authorisation]);
}

// --- A failed final deposit and the customer's new destination ------------------------------------------------

/// After a failed final deposit, recalculation to the same destination still acts under the order's authorisation;
/// another destination, or another to asset, is a new authorisation that only the customer gives, its person or its
/// agent. The operator acting for the user and the origin reserve, each of which the roles let authorise the order
/// at its origin, give none, and nothing is kept of a refused one. Once the customer gives it, moves go to its
/// destination alone, and the old authorisation covers nothing more. Each answer is recorded, its giver with it.
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
        let Some(Entry::Verified(entry)) = policy.log.0.last() else {
            panic!("a verified entry");
        };
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

    // The customer's own choice, by its agent as by its person, is the new authorisation.
    let agent = actor("cust-1", "agent-7", Role::Customer, ActorKind::Agent);
    for (n, by) in [first.given_by.clone(), agent].into_iter().enumerate() {
        let mut policy = engine();
        assert_eq!(policy.authorise(&first).unwrap().outcome, Answer::Permit {});
        let mut given = anew.clone();
        given.given_by = by.clone();
        assert_eq!(policy.authorise(&given).unwrap().outcome, Answer::Permit {});
        let Some(Entry::Verified(entry)) = policy.log.0.last() else {
            panic!("a verified entry");
        };
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
