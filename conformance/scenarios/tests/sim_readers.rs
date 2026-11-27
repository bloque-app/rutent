//! The simulated networks' readers as reader services (`T-024-25`, `DEC-28`, ADR-0007 as amended): every network a
//! scenario routes through is listed by a decided `list_public_network`, and its reader, run by the reader host inside
//! the harness over mTLS, answers as the simulated network does, reaches only what it declares, holds to its trust
//! rule and carries the harness's planted faults (`readers_support`).

mod execution_support;
mod readers_support;

use std::collections::{BTreeMap, BTreeSet};

use execution_support::*;
use readers_support::{Plan, Read, Readers, TestCa};
use rutent_contracts::module_interface::{CallOutcome, RefusalReason, Refused};
use rutent_contracts::provider_role::NoDetail;
use rutent_contracts::reader::{Answered, Asked, ReaderMismatch, Standing, Tied};
use rutent_contracts::router_api::Id;
use rutent_evidence::{Movement, MovementId};
use rutent_reader_host::config::Protocol;
use rutent_reader_host::records::{CallRecord, ConnectionOutcome};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{AMOUNT, RESERVE};
use rutent_scenarios::sim::readers::{
    self, EndpointFault, PEER_PORT, Rule, peer_of, pool_host, rule_of, standing_of,
};

const EXCHANGE: &str = "sim-exchange";

fn id(text: &str) -> Id {
    Id::new(text).expect("an id")
}

fn status_of(movement: &str) -> Asked {
    Asked::MovementStatus {
        movement: id(movement),
    }
}

fn movement_of(transaction: &str) -> Asked {
    Asked::MovementOf {
        transaction: id(transaction),
    }
}

/// The networks the providers `names` installed in `core` have their tuples on.
fn networks_of(core: &Core, names: &[&str]) -> BTreeSet<String> {
    names
        .iter()
        .flat_map(|name| {
            core.fleet()
                .provider(name)
                .expect("installed")
                .spec
                .networks()
        })
        .collect()
}

/// The movements delivered to `network`, in order: what the providers recorded on it.
fn delivered_to(core: &mut Core, network: &str) -> Vec<rutent_conformance::sim::Movement> {
    core.world()
        .delivered()
        .iter()
        .filter_map(|d| serde_json::from_str::<rutent_conformance::sim::Movement>(&d.body).ok())
        .filter(|m| m.network == network)
        .collect()
}

/// Records a movement of `network`'s own on it, by hand: final, then one reorged out where the rules let a reorg undo
/// it, then one pending, beside one it never recorded. The ids, in that order.
fn drive(core: &mut Core, network: &str) -> [String; 4] {
    let ids = ["final", "gone", "pending", "unknown"].map(|what| format!("{network}-{what}"));
    let movement = |n: u32| Movement {
        destination: format!("{network}-dest-{n}"),
        asset: "USDC".to_owned(),
        amount: format!("{}", 100 * n),
    };
    let net = core.world().network_mut(network).expect("a network");
    net.record(MovementId::new(ids[0].clone()).unwrap(), movement(1))
        .unwrap();
    net.advance(3);
    net.record(MovementId::new(ids[1].clone()).unwrap(), movement(2))
        .unwrap();
    net.reorg(1);
    net.record(MovementId::new(ids[2].clone()).unwrap(), movement(3))
        .unwrap();
    ids
}

/// Every network a scenario routes through is listed, each as its reader's listing names it.
fn listed_as_routed(
    core: &Core,
    names: &[&str],
) -> Vec<rutent_contracts::core_internal::PublicNetwork> {
    let listed = core.public_networks();
    let names_listed: BTreeSet<String> = listed
        .iter()
        .map(|n| n.network.as_str().to_owned())
        .collect();
    assert_eq!(
        names_listed,
        networks_of(core, names),
        "every network a provider's tuples are on is listed, and no other"
    );
    for network in &listed {
        assert_eq!(network, &readers::listing_of(network.network.as_str()));
    }
    listed
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn simulated_reader_answers_as_its_simulated_network_does() {
    let names = [RESERVE, EXCHANGE];
    let (mut core, session) = core_with("AC-1", &names);
    // An order run first, so the scenario's own movements are on its networks.
    let _ = run(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    let listed = listed_as_routed(&core, &names);
    let rules: BTreeSet<Rule> = listed.iter().map(|n| rule_of(n.network.as_str())).collect();
    assert_eq!(
        rules,
        BTreeSet::from([Rule::LightClient, Rule::RpcPool]),
        "one network follows the Substrate rule, another the EVM rule"
    );
    let mut asked: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for network in &listed {
        let network = network.network.as_str().to_owned();
        let mut ids: Vec<String> = drive(&mut core, &network).into();
        ids.extend(delivered_to(&mut core, &network).into_iter().map(|m| m.id));
        asked.insert(network, ids);
    }
    // What Core reads now, published to the readers' endpoints too.
    let _ = core.public_records();
    let readers = Readers::start(&listed, core.world().reader_endpoints(), &Plan::default());

    let mut standings = BTreeSet::new();
    for (network, ids) in &asked {
        let net = core.world().network(network).expect("a network").clone();
        // Its rules: the listing's identity and trust rule, and the network's own finality and reorg rule.
        let (_, read) = readers.ask(network, Asked::NetworkRules(NoDetail {}));
        let Some(Answered::NetworkRules { rules }) = read.answered() else {
            panic!("{network}'s rules: {read:?}");
        };
        let listing = readers::listing_of(network).listing.unwrap();
        assert_eq!(rules.identity, listing.identity);
        assert_eq!(rules.trust, listing.trust);
        assert_eq!(
            (rules.finality, rules.reorg),
            readers::rules_of(rutent_evidence::PublicNetwork::rules(&net))
        );
        for movement in ids {
            // Where it stands: as the simulated network says.
            let expected = standing_of(&rutent_evidence::PublicNetwork::status(
                &net,
                &MovementId::new(movement.clone()).unwrap(),
            ))
            .expect("a standing");
            let (_, read) = readers.ask(network, status_of(movement));
            assert_eq!(
                read.answered(),
                Some(&Answered::MovementStatus {
                    standing: expected.clone()
                }),
                "{network}: {movement}"
            );
            standings.insert(match &expected {
                Standing::Unknown(_) => "unknown",
                Standing::Pending(_) => "pending",
                Standing::Final(_) => "final",
                Standing::ReorgedOut(_) => "reorged_out",
            });
            // The movement tied to it as a transaction: as Core's public records tie it.
            let tied = core
                .public_records()
                .movement(network, &id(movement))
                .map(|moved| Tied {
                    id: id(moved.id.as_str()),
                    destination: id(&moved.destination),
                });
            let (_, read) = readers.ask(network, movement_of(movement));
            assert_eq!(
                read.answered(),
                Some(&Answered::MovementOf { movement: tied }),
                "{network}: the movement of {movement}"
            );
        }
    }
    assert_eq!(
        standings.len(),
        4,
        "unknown, pending, final and reorged out are each answered"
    );
    drop(readers);

    // Core opened again on its log lists the same networks, and lists none anew.
    let entries = core.log().entries().len();
    let core = rutent_scenarios::harness::Scenario::new("execution", "AC-1", given(&names)).reopen(
        core,
        1,
        &mut rutent_scenarios::assembly::machine::Reference,
    );
    assert_eq!(core.public_networks(), listed);
    assert_eq!(core.log().entries().len(), entries, "nothing listed again");
}

// verifies: REQ-MDL-7
#[test]
fn simulated_reader_reaches_only_the_endpoints_it_declares() {
    let names = [RESERVE, EXCHANGE];
    let (mut core, _) = core_with("AC-1", &names);
    let listed = listed_as_routed(&core, &names);
    for network in &listed {
        drive(&mut core, network.network.as_str());
    }
    let _ = core.public_records();
    let endpoints = core.world().reader_endpoints();

    // Readers that keep to what they declare: every call the host made for them is to a declared destination.
    let keeping = Readers::start(&listed, endpoints.clone(), &Plan::default());
    for network in &listed {
        let network = network.network.as_str();
        let (_, read) = keeping.ask(network, status_of(&format!("{network}-final")));
        assert!(!read.is_unreadable(), "{network}: {read:?}");
    }
    let declared = |network: &str| -> BTreeSet<String> {
        match rule_of(network) {
            Rule::RpcPool => readers::POOL
                .iter()
                .map(|label| pool_host(network, label))
                .collect(),
            Rule::LightClient => BTreeSet::from([peer_of(network)]),
        }
    };
    let records = keeping.records();
    assert!(!records.is_empty(), "the host recorded the readers' calls");
    for record in &records {
        let network = record.reader().trim_end_matches("-reader");
        match record {
            CallRecord::Request(r) => {
                assert_eq!(rule_of(network), Rule::RpcPool, "{r:?}");
                for hop in &r.hops {
                    assert!(declared(network).contains(hop.host.as_str()), "{hop:?}");
                    assert!(hop.destination.is_some(), "{hop:?}");
                }
                assert!(matches!(r.outcome, CallOutcome::Completed(_)), "{r:?}");
            }
            CallRecord::Connection(c) => {
                assert_eq!(rule_of(network), Rule::LightClient, "{c:?}");
                assert_eq!((c.protocol, c.port), (Protocol::P2p, PEER_PORT), "{c:?}");
                assert!(declared(network).contains(&c.host), "{c:?}");
                assert_eq!(c.outcome, ConnectionOutcome::Connected);
            }
        }
    }
    let all_declared: BTreeSet<String> = listed
        .iter()
        .flat_map(|n| declared(n.network.as_str()))
        .collect();
    assert!(
        keeping
            .connector
            .dialled()
            .iter()
            .all(|host| all_declared.contains(host)),
        "{:?}",
        keeping.connector.dialled()
    );
    drop(keeping);

    // Readers that stray, each also asking the world's own host for its network, which it doesn't declare, and a light
    // client a port its rule doesn't name: the host refuses and records each, and the answer is unreadable.
    let straying = Readers::start(&listed, endpoints, &Plan::everywhere(&listed, "undeclared"));
    for network in &listed {
        let network = network.network.as_str();
        let (question, read) = straying.ask(network, status_of(&format!("{network}-final")));
        assert!(read.is_unreadable(), "{network}: {read:?}");
        let world_host = format!("{network}.chain.example");
        let refused: Vec<_> = straying
            .records()
            .into_iter()
            .filter_map(|r| match r {
                CallRecord::Request(r) if r.hops.iter().any(|h| h.host.as_str() == world_host) => {
                    Some(r)
                }
                _ => None,
            })
            .collect();
        assert!(!refused.is_empty(), "{network}: the stray call is recorded");
        for record in &refused {
            assert_eq!(record.provider.as_str(), readers::reader_name(network));
            assert_eq!(record.module_version.as_str(), readers::READER_VERSION);
            assert_eq!(record.transaction.as_str(), question.request.as_str());
            assert_eq!(record.hops[0].destination, None, "no declared destination");
            assert_eq!(
                record.outcome,
                CallOutcome::Refused(Refused {
                    reason: RefusalReason::UndeclaredDestination
                })
            );
        }
        if rule_of(network) == Rule::LightClient {
            let port: Vec<_> = straying
                .records()
                .into_iter()
                .filter_map(|r| match r {
                    CallRecord::Connection(c) if c.port == 8080 => Some(c),
                    _ => None,
                })
                .collect();
            assert!(!port.is_empty(), "the undeclared port is recorded");
            assert!(
                port.iter()
                    .all(|c| c.outcome == ConnectionOutcome::UndeclaredDestination)
            );
        }
        assert!(
            !straying.connector.dialled().contains(&world_host),
            "nothing reached {world_host}"
        );
    }
    assert!(
        straying
            .dialer
            .dialled()
            .iter()
            .all(|to| to.port() == PEER_PORT),
        "no undeclared port was dialled: {:?}",
        straying.dialer.dialled()
    );
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn simulated_evm_network_with_one_lying_rpc_of_three_still_answers_and_with_two_is_unreadable() {
    let names = [RESERVE, EXCHANGE];
    let (mut core, _) = core_with("AC-1", &names);
    let listed = listed_as_routed(&core, &names);
    let evm: Vec<_> = listed
        .iter()
        .filter(|n| rule_of(n.network.as_str()) == Rule::RpcPool)
        .cloned()
        .collect();
    let network = evm
        .first()
        .expect("an evm network")
        .network
        .as_str()
        .to_owned();
    let [paid, ..] = drive(&mut core, &network);
    let _ = core.public_records();
    let endpoints = core.world().reader_endpoints();
    let readers = Readers::start(&evm, endpoints.clone(), &Plan::default());
    let truth = readers
        .ask(&network, status_of(&paid))
        .1
        .answered()
        .cloned();
    assert!(
        matches!(
            truth,
            Some(Answered::MovementStatus {
                standing: Standing::Final(_)
            })
        ),
        "{truth:?}"
    );
    let truth = truth.as_ref();
    let [a, b, _] = readers::POOL.map(|label| pool_host(&network, label));
    let with = |faults: &[(&str, EndpointFault)]| {
        for host in [&a, &b] {
            endpoints.plant(host, None);
        }
        for (host, fault) in faults {
            endpoints.plant(host, Some(*fault));
        }
        readers.ask(&network, status_of(&paid)).1
    };
    let answered = |read: Read| read.answered().cloned();
    // One endpoint of three lies, or is silent: the other two agree, and the reader answers what the network says.
    assert_eq!(
        answered(with(&[(&a, EndpointFault::Lies)])).as_ref(),
        truth,
        "one lying rpc of three"
    );
    assert_eq!(
        answered(with(&[(&a, EndpointFault::Silent)])).as_ref(),
        truth,
        "one silent rpc of three"
    );
    // Two do: no two agree, or too few answer, and the reader can't stand behind an answer.
    for faults in [
        [(&a, EndpointFault::Lies), (&b, EndpointFault::Lies)],
        [(&a, EndpointFault::Silent), (&b, EndpointFault::Silent)],
        [(&a, EndpointFault::Lies), (&b, EndpointFault::Silent)],
    ] {
        let faults = faults.map(|(h, f)| (h.as_str(), f));
        let read = with(&faults);
        assert!(read.is_unreadable(), "{faults:?}: {read:?}");
    }
    // And with the pool whole again, it answers once more.
    assert_eq!(answered(with(&[])).as_ref(), truth);
}

// verifies: REQ-ORD-38
#[test]
fn simulated_reader_that_ties_a_deposit_by_destination_and_amount_plants_the_shared_movement_fault()
{
    let names = [RESERVE];
    let (mut core, session) = core_with("AC-31", &names);
    // The reserve pays two orders to one account of the same amount with one payment (`AC-31`'s scenario): the
    // network ties each to the first movement it made to that destination for that amount.
    core.tie_payments_by_destination();
    let _ = run(&mut core, within("req-1", &session, AMOUNT, RESERVE, OTHER));
    let _ = run(&mut core, within("req-2", &session, AMOUNT, RESERVE, OTHER));
    let listed = listed_as_routed(&core, &names);
    let bank = format!("{RESERVE}-bank");
    let moved = delivered_to(&mut core, &bank);
    let shared: Vec<_> = moved
        .iter()
        .filter(|m| {
            moved
                .iter()
                .filter(|o| o.destination == m.destination && o.amount == m.amount)
                .count()
                > 1
        })
        .collect();
    assert!(
        shared.len() >= 2,
        "two movements to one destination of one amount: {moved:?}"
    );
    let _ = core.public_records();
    let endpoints = core.world().reader_endpoints();

    // The reader planted as the scenario planted its network: it matches by destination and amount.
    assert!(core.ties_by_destination());
    let tying = Readers::start(
        &listed,
        endpoints.clone(),
        &Plan {
            ties: core.ties_by_destination(),
            ..Plan::default()
        },
    );
    let first_of = |movement: &rutent_conformance::sim::Movement| {
        moved
            .iter()
            .find(|o| o.destination == movement.destination && o.amount == movement.amount)
            .expect("itself at least")
            .id
            .clone()
    };
    assert!(
        shared.iter().any(|m| first_of(m) != m.id),
        "a deposit whose movement is another's"
    );
    for movement in &shared {
        let first = first_of(movement);
        let (_, read) = tying.ask(&bank, movement_of(&movement.id));
        let Some(Answered::MovementOf {
            movement: Some(tied),
        }) = read.answered()
        else {
            panic!("{}: {read:?}", movement.id);
        };
        // The shared movement: every deposit of that destination and amount tied to the first.
        assert_eq!(
            tied.id.as_str(),
            first,
            "{} is tied to the first",
            movement.id
        );
        // As Core's public records tie it under the same fault.
        let moved = core
            .public_records()
            .movement(&bank, &id(&movement.id))
            .expect("a movement");
        assert_eq!(
            (tied.id.as_str(), tied.destination.as_str()),
            (moved.id.as_str(), moved.destination.as_str())
        );
    }

    // The fault is the reader's: one that matches by transaction ties each deposit to its own movement.
    let keeping = Readers::start(&listed, endpoints, &Plan::default());
    for movement in &shared {
        let (_, read) = keeping.ask(&bank, movement_of(&movement.id));
        let Some(Answered::MovementOf {
            movement: Some(tied),
        }) = read.answered()
        else {
            panic!("{}: {read:?}", movement.id);
        };
        assert_eq!(tied.id.as_str(), movement.id);
    }
}

/// A reader planted hostile, as the harness plants one: it crashes, answers malformed, names another movement, or its
/// host signs with a key the listing doesn't name. Core reads nothing it can settle on from any of them.
// verifies: REQ-ORD-39, DEC-28
#[test]
fn hostile_simulated_reader_leaves_core_nothing_to_settle_on() {
    let names = [RESERVE, EXCHANGE];
    let (mut core, _) = core_with("AC-1", &names);
    let listed = listed_as_routed(&core, &names);
    for network in &listed {
        drive(&mut core, network.network.as_str());
    }
    let _ = core.public_records();
    let endpoints = core.world().reader_endpoints();
    for act in ["crash", "malformed", "other-movement", "unlisted-key"] {
        let readers = Readers::start(&listed, endpoints.clone(), &Plan::everywhere(&listed, act));
        for network in &listed {
            let network = network.network.as_str();
            let (_, read) = readers.ask(network, status_of(&format!("{network}-final")));
            assert!(read.is_unreadable(), "{act} on {network}: {read:?}");
            match act {
                "crash" => assert_eq!(read.why(), Some("reader_crashed"), "{read:?}"),
                "unlisted-key" => assert!(
                    matches!(&read, Read::Refused(m) if m.contains(&ReaderMismatch::SignedWithAnotherKey)),
                    "{read:?}"
                ),
                _ => assert!(
                    read.why().is_some(),
                    "{act}: signed as unreadable: {read:?}"
                ),
            }
        }
    }
}

/// The test CA the harness makes is the one every listing names, by digest, and the same at every run.
// verifies: DEC-28
#[test]
fn harness_ca_is_the_one_the_listings_name() {
    let ca = TestCa::new();
    assert_eq!(ca.der(), TestCa::new().der(), "deterministic");
    assert_eq!(
        rutent_prov::module::wire::sha256(&ca.der()),
        readers::CA_SHA256
    );
}

/// A network delisted by a decided `delist_public_network` is no longer among those the log lists, and stays off once
/// Core opens again (#353's review, Low 2).
// verifies: DEC-28
#[test]
fn delisted_simulated_network_is_no_longer_listed() {
    let names = [RESERVE, EXCHANGE];
    let (mut core, _) = core_with("AC-1", &names);
    let listed = listed_as_routed(&core, &names);
    let evm = listed
        .iter()
        .find(|n| rule_of(n.network.as_str()) == Rule::RpcPool)
        .expect("an evm network")
        .network
        .clone();
    core.delist_public_network(&evm)
        .expect("the delisting is decided");
    let kept: Vec<_> = listed
        .iter()
        .filter(|n| n.network != evm)
        .cloned()
        .collect();
    assert_eq!(core.public_networks(), kept);
    let core = rutent_scenarios::harness::Scenario::new("execution", "AC-1", given(&names)).reopen(
        core,
        1,
        &mut rutent_scenarios::assembly::machine::Reference,
    );
    assert_eq!(core.public_networks(), kept);
}
