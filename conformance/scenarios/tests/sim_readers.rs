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
use rutent_contracts::reader::{Answered, Asked, Standing};
use rutent_contracts::router_api::Id;
use rutent_evidence::{Movement, MovementId};
use rutent_reader_host::config::Protocol;
use rutent_reader_host::records::{CallRecord, ConnectionOutcome};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{AMOUNT, RESERVE};
use rutent_scenarios::sim::readers::{
    self, EndpointFault, PEER_PORT, Rule, peer_of, pool_host, rule_of,
};

const EXCHANGE: &str = "sim-exchange";

const RAMP: &str = "sim-ramp";

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
    let mut net = core.world().network_mut(network).expect("a network");
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

/// Each simulated network's reader, read through Core's own adapter over the deployment's reader host, answers as the
/// simulated network itself does: its rules, where each movement stands and the movement it ties to a transaction,
/// each compared with the world's network directly, never with a second reading of the same conversion (the review of
/// `T-024-25` at `64309ac3`, Low 1), on every listed network, both rules.
// verifies: REQ-ORD-39, DEC-28
#[test]
fn simulated_reader_answers_as_its_simulated_network_does() {
    use rutent_core_orch::closing::PublicRecords as _;
    use rutent_evidence::{PublicNetwork as _, Status};
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
    // Every listed reader passed Core's start-up check: the listing's identity, trust rule and version.
    assert!(
        core.reader_raises().is_empty(),
        "{:?}",
        core.reader_raises()
    );
    let mut asked: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for network in &listed {
        let network = network.network.as_str().to_owned();
        let mut ids: Vec<String> = drive(&mut core, &network).into();
        ids.extend(delivered_to(&mut core, &network).into_iter().map(|m| m.id));
        asked.insert(network, ids);
    }

    let mut standings = BTreeSet::new();
    for (network, ids) in &asked {
        let net = core.world().network(network).expect("a network");
        let pass = core.public_records();
        assert_eq!(
            pass.rules(network).expect("readable"),
            Some(net.rules()),
            "{network}'s rules"
        );
        for movement in ids {
            let movement_id = MovementId::new(movement.clone()).unwrap();
            let truth = net.status(&movement_id);
            assert_eq!(
                pass.status(network, &movement_id).expect("readable"),
                truth,
                "{network}: {movement}"
            );
            standings.insert(match &truth {
                Status::Unknown => "unknown",
                Status::Pending(_) => "pending",
                Status::Final(_) => "final",
                Status::ReorgedOut => "reorged_out",
            });
            // The movement tied to it as a transaction: the movement that transaction names, where the network holds
            // it, gone where it doesn't, as the simulated providers give each movement its transaction.
            let tied = pass
                .movement(network, &id(movement))
                .expect("readable")
                .map(|moved| {
                    assert_eq!(moved.read_by.reader.as_str(), readers::reader_name(network));
                    (moved.id.as_str().to_owned(), moved.destination)
                });
            let expected = match truth {
                Status::Pending(m) | Status::Final(m) => Some((movement.clone(), m.destination)),
                Status::Unknown | Status::ReorgedOut => None,
            };
            assert_eq!(tied, expected, "{network}: the movement of {movement}");
        }
    }
    assert_eq!(
        standings.len(),
        4,
        "unknown, pending, final and reorged out are each answered"
    );

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

/// On every EVM network listed, and for both questions about a movement (where it stands, and which movement a
/// transaction is tied to), one lying or silent RPC of three leaves the reader answering what the network says, and
/// two leave it unreadable (the review of `T-024-25` at `64309ac3`, Low 3: not the first network or `status_of` alone).
// verifies: REQ-ORD-39, DEC-28
#[test]
fn simulated_evm_network_with_one_lying_rpc_of_three_still_answers_and_with_two_is_unreadable() {
    let names = [RESERVE, EXCHANGE, RAMP];
    let (mut core, _) = core_with("AC-1", &names);
    let listed = listed_as_routed(&core, &names);
    let evm: Vec<_> = listed
        .iter()
        .filter(|n| rule_of(n.network.as_str()) == Rule::RpcPool)
        .cloned()
        .collect();
    assert!(!evm.is_empty(), "an evm network");
    let mut paid = BTreeMap::new();
    for network in &evm {
        let network = network.network.as_str().to_owned();
        let [final_one, ..] = drive(&mut core, &network);
        paid.insert(network, final_one);
    }
    let endpoints = core.world().reader_endpoints();
    let readers = Readers::start(&evm, endpoints.clone(), &Plan::default());
    for (network, paid) in &paid {
        for asked in [status_of(paid), movement_of(paid)] {
            let truth = readers.ask(network, asked.clone()).1.answered().cloned();
            assert!(
                matches!(
                    truth,
                    Some(Answered::MovementStatus {
                        standing: Standing::Final(_)
                    }) | Some(Answered::MovementOf { movement: Some(_) })
                ),
                "{network}: {truth:?}"
            );
            let truth = truth.as_ref();
            let [a, b, _] = readers::POOL.map(|label| pool_host(network, label));
            let with = |faults: &[(&str, EndpointFault)]| {
                for host in [&a, &b] {
                    endpoints.plant(host, None);
                }
                for (host, fault) in faults {
                    endpoints.plant(host, Some(*fault));
                }
                readers.ask(network, asked.clone()).1
            };
            let answered = |read: Read| read.answered().cloned();
            // One endpoint of three lies, or is silent: the other two agree, and the reader answers what the network
            // says.
            assert_eq!(
                answered(with(&[(&a, EndpointFault::Lies)])).as_ref(),
                truth,
                "{network}: one lying rpc of three"
            );
            assert_eq!(
                answered(with(&[(&a, EndpointFault::Silent)])).as_ref(),
                truth,
                "{network}: one silent rpc of three"
            );
            // Two do: no two agree, or too few answer, and the reader can't stand behind an answer.
            for (faults, why) in [
                (
                    [(&a, EndpointFault::Lies), (&b, EndpointFault::Lies)],
                    "rpc_answers_disagree",
                ),
                (
                    [(&a, EndpointFault::Silent), (&b, EndpointFault::Silent)],
                    "too_few_rpc_answers",
                ),
                (
                    [(&a, EndpointFault::Lies), (&b, EndpointFault::Silent)],
                    "rpc_answers_disagree",
                ),
            ] {
                let faults = faults.map(|(h, f)| (h.as_str(), f));
                let read = with(&faults);
                assert_eq!(read.why(), Some(why), "{network}: {faults:?}: {read:?}");
            }
            // And with the pool whole again, it answers once more.
            assert_eq!(answered(with(&[])).as_ref(), truth);
        }
    }
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
        // As Core reads it, through its adapter, from the deployment's reader host planted the same way.
        let moved = rutent_core_orch::closing::PublicRecords::movement(
            &core.public_records(),
            &bank,
            &id(&movement.id),
        )
        .expect("readable")
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

/// A reader planted hostile, as the harness plants one: it crashes, answers malformed, names another movement, lies
/// about its network's identity at start-up, or its host signs with a key the listing doesn't name. Core reads nothing
/// it can settle on from any of them, each for its own reason (the review of `T-024-25` at `64309ac3`, Low 2), and
/// Core's own start-up check keeps every such network from starting while the others run on (Low 3: a start-up liar).
// verifies: REQ-ORD-39, DEC-28
#[test]
fn hostile_simulated_reader_leaves_core_nothing_to_settle_on() {
    use rutent_core_orch::closing::Unreadable;
    use rutent_core_orch::closing::readers::{NotStarted, Standing as Stood};
    let names = [RESERVE, EXCHANGE];
    let (mut core, _) = core_with("AC-1", &names);
    let listed = listed_as_routed(&core, &names);
    for network in &listed {
        drive(&mut core, network.network.as_str());
    }
    let endpoints = core.world().reader_endpoints();
    for act in [
        "crash",
        "malformed",
        "other-movement",
        "other-identity",
        "unlisted-key",
    ] {
        let readers = Readers::start(&listed, endpoints.clone(), &Plan::everywhere(&listed, act));
        for network in &listed {
            let network = network.network.as_str();
            let (_, read) = readers.ask(network, status_of(&format!("{network}-final")));
            // A start-up liar answers its movements as they stand; it lies about its rules alone.
            assert_eq!(
                read.is_unreadable(),
                act != "other-identity",
                "{act} on {network}: {read:?}"
            );
            match act {
                "crash" => assert_eq!(read.why(), Some("reader_crashed"), "{read:?}"),
                "malformed" => assert_eq!(read.why(), Some("answer_breaks_its_shape"), "{read:?}"),
                "other-movement" => {
                    assert_eq!(read.why(), Some("answer_to_another_question"), "{read:?}");
                }
                "other-identity" => {
                    // Its movements it answers as they stand; its rules it lies about, which the host holds to its
                    // configuration and signs as unreadable.
                    let (_, rules) = readers.ask(network, Asked::NetworkRules(NoDetail {}));
                    assert_eq!(rules.why(), Some("rules_not_as_configured"), "{rules:?}");
                }
                _ => assert_eq!(read, Read::Unreadable(Unreadable::NotSigned), "{read:?}"),
            }
        }
    }
    // The same faults planted in the deployment's reader host beside Core, one network at a time: that network's
    // reader fails Core's start-up check and doesn't start, and every other listed network still does.
    let first = listed[0].network.as_str().to_owned();
    for act in ["crash", "other-identity", "unlisted-key"] {
        core.plant_readers(&Plan::default().with(&first, act));
        assert!(
            matches!(
                core.readers().standing(&first),
                Stood::NotStarted(NotStarted::Unread(_))
            ),
            "{act}: {:?}",
            core.readers().standing(&first)
        );
        for other in listed.iter().skip(1) {
            assert_eq!(
                core.readers().standing(other.network.as_str()),
                Stood::Started,
                "{act}: {} runs on",
                other.network
            );
        }
    }
    core.plant_readers(&Plan::default());
    assert_eq!(core.readers().standing(&first), Stood::Started);
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
    // Under an id of the network's own, never one drawn from Core's source (#353's review, round 2).
    assert!(
        core.log().entries().iter().any(|(_, e)| matches!(e,
            rutent_contracts::core_internal::Entry::Verified(v)
                if matches!(&v.what, rutent_contracts::core_internal::What::Effect { decision, .. }
                    if decision.request().as_str() == format!("delist-{evm}")))),
        "asked as delist-{evm}"
    );
    // Off Core's adapter and the catalogue's public list.
    assert_eq!(
        core.readers().standing(evm.as_str()),
        rutent_core_orch::closing::readers::Standing::NotListed
    );
    assert!(!core.catalogue().nodes().is_started_network(evm.as_str()));
    // A network no listing names has nothing to delist: refused, and the log takes nothing (#343's review, L7).
    let entries = core.log().entries().len();
    assert!(core.delist_public_network(&evm).is_err(), "delisted twice");
    assert!(core.delist_public_network(&id("never-listed")).is_err());
    assert_eq!(core.log().entries().len(), entries);
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
