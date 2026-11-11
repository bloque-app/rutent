//! The simulated providers, admitted through the platform like any module (`T-024-01`): their books, their role
//! suites, and what a script makes them do, seen from the platform.

use std::time::Duration;

use rutent_conformance::role::{
    Received, RoleHarness, RoleProvider, SuiteContext, WithoutHooks, run_role_suite,
};
use rutent_conformance::signing::{Ed25519Verifier, Signing, signed, signed_inside};
use rutent_conformance::sim::{
    Act, Behaviour, Crash, Delivery, Role, Simulated, Spec, movements_of, operation,
};
use rutent_contracts::module_interface::{CallOutcome, Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{Advertisement, Operation, ResultBody, Statement};
use rutent_contracts::state_machine::{Observed, OrderState, SegmentState, StuckReason};
use rutent_evidence::{MovementId, PublicNetwork, Status};
use rutent_prov::module::types::Handled;
use rutent_scenarios::sim::orders::{Run, at, id};
use rutent_scenarios::sim::{Fleet, InProcess};
use serde_json::{Value, json};

/// A deadline short enough that silence is quick, and long enough that a module answering in time does.
const DEADLINE: Duration = Duration::from_secs(3);

fn fleet_of(specs: &[Spec]) -> Fleet {
    let mut fleet = Fleet::new(DEADLINE);
    for spec in specs {
        fleet.install(spec.clone());
    }
    fleet
}

fn reserve_of(provider: &str) -> Spec {
    Spec::of(Role::Reserve, provider)
}

fn withdrawal(provider: &str, transaction: &str, amount: &str) -> Statement<Operation> {
    let context = SuiteContext::default();
    operation(
        &context,
        json!({
            "kind": "withdrawal",
            "transaction": transaction,
            "provider": provider,
            "session": context.session,
            "origin": { "network": format!("{provider}-bank") },
            "destination": format!("{provider}-ledger"),
            "asset": "COP",
            "amount": amount,
            "dry_run": false,
            "inputs": { "payer.bank_account": "acct-1" },
        }),
    )
}

fn deposit(provider: &str, transaction: &str, amount: &str) -> Statement<Operation> {
    operation(
        &SuiteContext::default(),
        json!({
            "kind": "deposit",
            "transaction": transaction,
            "provider": provider,
            "origin": format!("{provider}-ledger"),
            "destination": { "network": format!("{provider}-bank") },
            "asset": "COP",
            "amount": amount,
            "dry_run": false,
            "inputs": { "beneficiary.bank_account": "acct-2" },
        }),
    )
}

fn balance(provider: &str, transaction: &str, amount: &str) -> Statement<Operation> {
    let context = SuiteContext::default();
    operation(
        &context,
        json!({
            "kind": "balance_verification",
            "transaction": transaction,
            "provider": provider,
            "session": context.session,
            "account": context.account,
            "asset": "COP",
            "amount": amount,
        }),
    )
}

/// The result an operation was answered with, as JSON.
fn answered(handled: Handled) -> Value {
    match handled {
        Handled::Answered(result) => {
            serde_json::to_value(&result.statement).expect("a result is JSON")
        }
        Handled::Refused(why) => panic!("the platform refused the operation: {why:?}"),
    }
}

// verifies: REQ-MOD-4
#[test]
fn simulated_reserve_keeps_its_book_across_operations() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    let sufficient = |fleet: &mut Fleet, transaction: &str| {
        let result = answered(fleet.handle(
            "sim-reserve",
            &balance("sim-reserve", transaction, "40000000"),
        ));
        result["outcome"]["sufficient"]
            .as_bool()
            .expect("an answer")
    };
    assert!(
        sufficient(&mut fleet, "tx-b1"),
        "the reserve holds 50 million"
    );
    let first = answered(fleet.handle(
        "sim-reserve",
        &withdrawal("sim-reserve", "tx-w1", "30000000"),
    ));
    assert_eq!(first["outcome"]["status"], "succeeded");
    assert!(
        !sufficient(&mut fleet, "tx-b2"),
        "30 million are gone from the book"
    );
    // The process is replaced: what it held in memory is gone, and the book is the platform's.
    assert!(fleet.platform.restart("sim-reserve"));
    assert!(
        !sufficient(&mut fleet, "tx-b3"),
        "the replaced process forgot the withdrawal"
    );
    let second = answered(fleet.handle(
        "sim-reserve",
        &withdrawal("sim-reserve", "tx-w2", "30000000"),
    ));
    assert_eq!(
        second["outcome"]["status"], "refused",
        "the book doesn't hold another 30 million: {second}"
    );
    // The journal holds both withdrawals: the second was refused for want of funds, and the reserve remembers that
    // answer, so a repeat of it is answered as before.
    let journal = fleet.world().journal("sim-reserve").to_vec();
    assert_eq!(journal.len(), 2);
    assert!(fleet.world().state_asked.iter().any(|k| k == "book"));
    // The book the world holds is the book a twin has when it does the same.
    let mut twin = Simulated::new(reserve_of("sim-reserve"), &SuiteContext::default());
    twin.restore(&journal);
    let mut direct = Simulated::new(reserve_of("sim-reserve"), &SuiteContext::default());
    direct.answer(
        &withdrawal("sim-reserve", "tx-w1", "30000000"),
        &Behaviour::default(),
    );
    assert_eq!(twin.state(), direct.state());
}

/// A simulated provider as a role-suite provider: its operations go through the platform, and its hooks are
/// scripts. What the suite can't ask the platform, the advertisement, the status query and the books' fingerprint,
/// it asks the stand-in twin that answers every operation the module does, and the module's answers are compared
/// with the twin's.
struct Through {
    fleet: Fleet,
    spec: Spec,
    twin: InProcess,
    advertisement: Received<Advertisement>,
}

impl Through {
    fn new(role: Role, provider: &str) -> Self {
        let spec = Spec::of(role, provider);
        let mut fleet = Fleet::new(DEADLINE);
        let installed = fleet.install(spec.clone());
        Self {
            twin: InProcess(spec.make(fleet.context())),
            advertisement: installed.advertisement,
            fleet,
            spec,
        }
    }
}

impl RoleProvider for Through {
    fn advertisement(&self) -> Received<Advertisement> {
        self.advertisement.clone()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        let expected = self.twin.handle(operation);
        let handled = self.fleet.handle(&self.spec.provider, operation);
        let Handled::Answered(answered) = handled else {
            panic!("the platform refused an operation of the role suite: {handled:?}");
        };
        // The platform signs the result, and what the module nested in it, with the provider's statements key.
        let twin_signed = Received::<ResultBody>::from_bytes(signed_inside(expected.bytes()))
            .expect("reads back");
        assert_eq!(
            serde_json::to_value(&answered.statement).expect("JSON"),
            serde_json::to_value(&twin_signed.statement().statement).expect("JSON"),
            "{}: the module answered otherwise than its twin does in process",
            self.spec.provider
        );
        // The books the platform holds are the twin's.
        let mut replay = Simulated::new(self.spec.clone(), self.fleet.context());
        replay.restore(self.fleet.world().journal(&self.spec.provider));
        assert_eq!(
            replay.state(),
            self.twin.state(),
            "{}: the journal isn't the books",
            self.spec.provider
        );
        Received::sent(&answered)
    }

    fn state(&self) -> String {
        self.twin.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        // A status query isn't an operation: the provider's own answer, signed with its statements key.
        self.twin
            .status(transaction)
            .map(|r| Received::from_bytes(signed(r.bytes())).expect("reads back"))
    }
}

impl RoleHarness for Through {
    fn force_outcome_unknown(&mut self) -> bool {
        let forced = self.twin.force_outcome_unknown();
        if forced {
            self.fleet
                .world()
                .script(&self.spec.provider, Behaviour::act(Act::Unknown));
        }
        forced
    }

    fn force_failure(&mut self) -> bool {
        let forced = self.twin.force_failure();
        if forced {
            self.fleet
                .world()
                .script(&self.spec.provider, Behaviour::act(Act::Fail));
        }
        forced
    }

    fn credit_reversal(&mut self, segment: &str, node: &str, asset: &str, amount: &str) -> bool {
        let credited = self.twin.credit_reversal(segment, node, asset, amount);
        if credited {
            self.fleet.world().script_for(
                &self.spec.provider,
                "reversal_confirmation",
                Behaviour::crediting_a_reversal(),
            );
        }
        credited
    }
}

// verifies: REQ-CAT-21, AC-20, DEC-7
#[test]
fn each_simulated_provider_passes_its_role_suite_through_the_platform() {
    for (role, provider) in [
        (Role::Reserve, "sim-reserve"),
        (Role::Exchange, "sim-exchange"),
        (Role::Ramp, "sim-ramp"),
        (Role::Verifier, "sim-verifier"),
    ] {
        let context = SuiteContext::default();
        // The first report of a certification: no hook took effect, nothing deferred, and only what the suite
        // can provoke by a hook alone is left uncovered.
        let mut plain = WithoutHooks(Through::new(role, provider));
        let first = run_role_suite(&mut plain, &context, &Ed25519Verifier);
        assert!(
            !first.hooks_used
                && first.findings.is_empty()
                && first.deferred.is_empty()
                && first.signatures_deferred == 0
                && first.uncovered.iter().all(|u| u.answer.needs_a_hook()),
            "{provider}: findings {:#?}\nuncovered {:#?}\ndeferred {:#?}",
            first.findings,
            first.uncovered,
            first.deferred
        );
        assert!(first.passed_without_hooks(), "{provider}: {first:#?}");
        // The second: the scripts are the hooks, and the answers only a hook can provoke are given and checked.
        let mut hooked = Through::new(role, provider);
        let second = run_role_suite(&mut hooked, &context, &Ed25519Verifier);
        assert!(second.passed(), "{provider}: {second:#?}");
        // The platform hid nothing and added nothing: the run is the stand-in's own, in process.
        let mut bare = Signing(InProcess(Spec::of(role, provider).make(&context)));
        let alone = run_role_suite(&mut bare, &context, &Ed25519Verifier);
        assert_eq!(first.findings, alone.findings, "{provider}: findings");
        assert_eq!(second.exercised, alone.exercised, "{provider}: exercised");
    }
}

// verifies: REQ-STM-5
#[test]
fn scripted_silence_leaves_its_segment_unresolved() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    let mut run = Run::withdrawing(
        "ord-silent",
        "sim-reserve",
        "sim-reserve-bank",
        "sim-reserve-ledger",
    );
    let transaction = Identifier::new("tx-silent").expect("an id");
    fleet
        .platform
        .begin_segment("sim-reserve", &transaction)
        .expect("a segment");
    assert_eq!(
        run.about_the_segment(Observed::ProviderBound {}),
        rutent_contracts::state_machine::Outcome::Applied {}
    );
    // The provider never answers. The platform has no result to give: it refuses at its limit, and that isn't an
    // outcome of the segment.
    fleet.world().script("sim-reserve", Behaviour::silent());
    let asked = withdrawal("sim-reserve", "tx-silent", "1000");
    let handled = fleet.handle("sim-reserve", &asked);
    assert_eq!(handled, Handled::Refused(PlatformRefusal::LimitReached));
    let events = fleet.platform.take_events();
    assert_eq!(
        events.limits.len(),
        1,
        "the limit that stopped it is recorded"
    );
    assert!(events.records.is_empty(), "it passed nothing out");
    assert!(events.unknown.is_empty());
    assert!(
        fleet.world().journal("sim-reserve").is_empty(),
        "it took no effect"
    );
    // Time passes. Declared stuck over time, the part waits, and the segment is where it was.
    let mut late = run.input(
        Some("p1"),
        None,
        Observed::StuckDeclared {
            reason: StuckReason::OverTime,
        },
    );
    late.recorded_at = at(86_400 * 30 - 1);
    run.feed(&late);
    assert_eq!(run.part_state(), OrderState::Stuck);
    assert_eq!(run.segment_state(), SegmentState::Started);
    // It answers when it is asked again, and only then is the segment settled: the answer resolves it, as time
    // didn't.
    let result = answered(fleet.handle("sim-reserve", &asked));
    assert_eq!(result["outcome"]["status"], "succeeded");
    run.about_the_segment(Observed::SegmentSettled {
        evidence: id("ev-tx-silent"),
    });
    assert_eq!(run.segment_state(), SegmentState::Settled);
}

// verifies: NFR-5
#[test]
fn provider_crashing_mid_operation_leaves_the_operations_outbound_calls_recorded() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    fleet
        .world()
        .script("sim-reserve", Behaviour::crashing(Crash::AfterSettle));
    let asked = withdrawal("sim-reserve", "tx-crash", "1000");
    let handled = fleet.handle("sim-reserve", &asked);
    // It had passed something out, so the platform doesn't say nothing happened.
    assert_eq!(handled, Handled::Refused(PlatformRefusal::OutcomeUnknown));
    let events = fleet.platform.take_events();
    assert_eq!(events.unknown.len(), 1);
    assert_eq!(events.unknown[0].transaction, "tx-crash");
    // Every outbound call it made is in the record: its entry to its rail and its movement on the network.
    let operations: Vec<&str> = events
        .records
        .iter()
        .filter(|r| r.transaction.as_str() == "tx-crash")
        .map(|r| r.operation.as_str())
        .collect();
    assert_eq!(operations, ["sim.settle", "sim.chain"]);
    assert!(
        events
            .records
            .iter()
            .all(|r| { matches!(r.outcome, CallOutcome::Completed(_)) })
    );
    // The effect is real: the rail and the network hold it, and nothing but the record says so.
    assert_eq!(fleet.world().journal("sim-reserve").len(), 1);
    let network = fleet
        .world()
        .network("sim-reserve-bank")
        .expect("its network");
    assert!(matches!(
        network.status(&MovementId::new("tx-crash").expect("an id")),
        Status::Final(_)
    ));
    // Asked again, the new process finds the book as the crashed one left it, and answers the same way.
    let repeat = answered(fleet.handle("sim-reserve", &asked));
    assert_eq!(repeat["outcome"]["status"], "succeeded");
    assert_eq!(
        fleet.world().journal("sim-reserve").len(),
        1,
        "a repeat moves nothing"
    );
}

// verifies: REQ-MOD-4, REQ-ORD-1
#[test]
fn a_crash_before_the_entry_is_written_leaves_no_effect() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    fleet
        .world()
        .script("sim-reserve", Behaviour::crashing(Crash::BeforeSettle));
    let handled = fleet.handle(
        "sim-reserve",
        &withdrawal("sim-reserve", "tx-early", "1000"),
    );
    // It passed nothing out, so nothing took effect, and the platform says so.
    assert_eq!(handled, Handled::Refused(PlatformRefusal::Unavailable));
    assert!(fleet.world().journal("sim-reserve").is_empty());
    assert!(fleet.platform.take_events().records.is_empty());
}

// verifies: REQ-MOD-4, REQ-ORD-1
#[test]
fn a_provider_that_answers_too_late_has_acted_and_answers_the_repeat_as_it_did() {
    let mut fleet = Fleet::new(Duration::from_secs(1));
    fleet.install(reserve_of("sim-reserve"));
    fleet.world().script("sim-reserve", Behaviour::late());
    let asked = withdrawal("sim-reserve", "tx-late", "1000");
    let handled = fleet.handle("sim-reserve", &asked);
    assert_eq!(handled, Handled::Refused(PlatformRefusal::OutcomeUnknown));
    assert_eq!(fleet.world().journal("sim-reserve").len(), 1, "it acted");
    // The repeat has the time a new process needs to start and replay the book.
    fleet.platform.set_deadline(DEADLINE);
    let repeat = answered(fleet.handle("sim-reserve", &asked));
    assert_eq!(repeat["outcome"]["status"], "succeeded");
    assert_eq!(fleet.world().journal("sim-reserve").len(), 1);
}

// verifies: REQ-MOD-4, REQ-ORD-1
#[test]
fn a_scripted_unknown_outcome_reaches_the_platform_as_the_providers_own_result() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    fleet
        .world()
        .script("sim-reserve", Behaviour::act(Act::Unknown));
    let result = answered(fleet.handle(
        "sim-reserve",
        &withdrawal("sim-reserve", "tx-unknown", "1000"),
    ));
    assert_eq!(result["outcome"]["status"], "unknown");
    // The platform's own refusal is a different thing: nothing was refused here.
    assert!(fleet.platform.take_events().unknown.is_empty());
    assert_eq!(fleet.world().journal("sim-reserve").len(), 1);
}

// verifies: REQ-MOD-4
#[test]
fn a_scripted_refusal_is_the_providers_and_moves_nothing() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    fleet
        .world()
        .script("sim-reserve", Behaviour::act(Act::Refuse));
    let result = answered(fleet.handle("sim-reserve", &withdrawal("sim-reserve", "tx-no", "1000")));
    assert_eq!(result["outcome"]["status"], "refused");
    assert!(fleet.world().journal("sim-reserve").is_empty());
    assert!(fleet.platform.take_events().records.is_empty());
}

// verifies: REQ-MOD-4, REQ-ORD-21
#[test]
fn a_provider_scripted_to_deliver_short_gives_less_than_it_took_in_and_the_network_records_what_it_gave()
 {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    fleet.world().script(
        "sim-reserve",
        Behaviour::delivering(Delivery::Short { bps: 500 }),
    );
    let asked = withdrawal("sim-reserve", "tx-short", "10000");
    let result = answered(fleet.handle("sim-reserve", &asked));
    assert_eq!(result["outcome"]["amount_in"], "10000");
    assert_eq!(result["outcome"]["amount_out"], "9500");
    let network = fleet
        .world()
        .network("sim-reserve-bank")
        .expect("its network");
    let Status::Final(movement) = network.status(&MovementId::new("tx-short").expect("an id"))
    else {
        panic!("the network holds the movement");
    };
    assert_eq!(movement.amount, "9500");
    // What the module sent is what a twin says it should have.
    assert_eq!(movements_of(&asked.statement, &result).len(), 1);
}

// verifies: REQ-MOD-4, REQ-ORD-30
#[test]
fn a_provider_scripted_to_deliver_in_pieces_leaves_a_movement_on_the_network_for_each_piece() {
    let mut fleet = fleet_of(&[Spec::of(Role::Exchange, "sim-exchange")]);
    fleet.world().script(
        "sim-exchange",
        Behaviour::delivering(Delivery::Pieces { count: 3 }),
    );
    let asked = operation(
        fleet.context(),
        json!({
            "kind": "conversion",
            "transaction": "tx-pieces",
            "provider": "sim-exchange",
            "node": "polygon",
            "from": "USDC",
            "to": "USDT",
            "direction": "exact_in",
            "amount": "2000000",
            "dry_run": false,
            "inputs": {},
        }),
    );
    let result = answered(fleet.handle("sim-exchange", &asked));
    assert_eq!(
        result["outcome"]["evidence"]
            .as_array()
            .expect("evidence")
            .len(),
        3
    );
    let network = fleet.world().network("polygon").expect("a network");
    let mut total = 0_u128;
    for n in 1..=3 {
        let id = MovementId::new(format!("tx-pieces.{n}")).expect("an id");
        let status = network.status(&id);
        // Two confirmations settle a movement on this network, and no block has been added yet.
        let (Status::Pending(movement) | Status::Final(movement)) = status else {
            panic!("piece {n} isn't on the network");
        };
        total += movement.amount.parse::<u128>().expect("an amount");
    }
    assert_eq!(total, 2_000_000);
}

// verifies: REQ-MOD-4, ADR-0008
#[test]
fn a_reserve_confirms_a_reversal_through_the_platform_only_when_scripted_to() {
    let mut fleet = fleet_of(&[reserve_of("sim-reserve")]);
    let ask = |transaction: &str| {
        operation(
            &SuiteContext::default(),
            json!({
                "kind": "reversal_confirmation",
                "transaction": transaction,
                "provider": "sim-reserve",
                "segment": "tx-undone",
                "asset": "COP",
                "amount": "1000",
            }),
        )
    };
    let not = answered(fleet.handle("sim-reserve", &ask("tx-c1")));
    assert_eq!(not["outcome"]["status"], "not_confirmed");
    fleet.world().script_for(
        "sim-reserve",
        "reversal_confirmation",
        Behaviour::crediting_a_reversal(),
    );
    let yes = answered(fleet.handle("sim-reserve", &ask("tx-c2")));
    assert_eq!(yes["outcome"]["status"], "confirmed");
    // It is the reserve's own signed word, and it stays on its book: the next question, with no script, is yes too.
    let again = answered(fleet.handle("sim-reserve", &ask("tx-c3")));
    assert_eq!(again["outcome"]["status"], "confirmed");
}

// verifies: DEC-24, REQ-ORD-41
#[test]
fn two_private_nodes_each_record_their_inner_segments() {
    use rutent_conformance::sim::recorded_segments;
    use rutent_evidence::replay::{
        NodeRecord, PublicExit, RecordedSegment, Replay, RouteEnd, Stated, replay_route,
    };
    let (a, b) = (reserve_of("sim-reserve-a"), reserve_of("sim-reserve-b"));
    let mut fleet = fleet_of(&[a.clone(), b.clone()]);
    // Each is a provider of its own, with its own publisher and its own private node, both in the catalogue.
    let ad = |fleet: &Fleet, spec: &Spec| {
        fleet
            .provider(&spec.provider)
            .expect("installed")
            .advertisement
            .statement()
            .statement
            .clone()
    };
    let (ad_a, ad_b) = (ad(&fleet, &a), ad(&fleet, &b));
    assert_ne!(ad_a.publisher, ad_b.publisher);
    assert_eq!(ad_a.private_nodes, [a.ledger()]);
    assert_eq!(ad_b.private_nodes, [b.ledger()]);
    // The value comes into the first node, passes to the second, and leaves it onto a public network.
    for (provider, spec_tx) in [("sim-reserve-a", "tx-a1"), ("sim-reserve-b", "tx-b1")] {
        let result = answered(fleet.handle(provider, &withdrawal(provider, spec_tx, "1000")));
        assert_eq!(result["outcome"]["status"], "succeeded");
    }
    let exit = answered(fleet.handle(
        "sim-reserve-b",
        &deposit("sim-reserve-b", "tx-b-exit", "1000"),
    ));
    assert_eq!(exit["outcome"]["status"], "succeeded");
    let context = SuiteContext::default();
    let record_of = |fleet: &mut Fleet, spec: &Spec| NodeRecord {
        node: spec.ledger(),
        segments: recorded_segments(spec, &context, fleet.world().journal(&spec.provider))
            .into_iter()
            .map(|s| RecordedSegment {
                asset_in: s.asset_in,
                amount_in: s.amount_in,
                asset_out: s.asset_out,
                amount_out: s.amount_out,
                fee: s.fee,
            })
            .collect(),
    };
    let (node_a, node_b) = (record_of(&mut fleet, &a), record_of(&mut fleet, &b));
    assert_eq!(
        node_a.segments.len(),
        1,
        "one segment inside the first node"
    );
    assert_eq!(node_b.segments.len(), 2, "two inside the second");
    assert_ne!(node_a.node, node_b.node);
    // Each record is its own provider's: the journals are two, and the segments in them are each node's.
    assert_eq!(fleet.world().journal("sim-reserve-a").len(), 1);
    assert_eq!(fleet.world().journal("sim-reserve-b").len(), 2);
    // Replayed over both nodes against the public movement out, they are consistent.
    let exit_of = |amount: &str| {
        RouteEnd::Public(PublicExit {
            movement: MovementId::new("tx-b-exit").expect("an id"),
            network: b.bank(),
            destination: "acct-2".to_owned(),
            stated: Stated {
                asset: "COP".to_owned(),
                amount: amount.to_owned(),
            },
        })
    };
    let net = fleet.world().network(&b.bank()).expect("a network").clone();
    let nodes = [node_a.clone(), node_b.clone()];
    let Replay::Consistent(consistent) = replay_route(&nodes, &exit_of("1000"), &net) else {
        panic!("the records of both nodes are consistent with the public exit");
    };
    assert_eq!(consistent.nodes, 2);
    assert!(!consistent.proves_segments_happened());
    // A record that doesn't match the movement out is a mismatch, and a route ending inside a node is its
    // publisher's word.
    let mut forged = nodes.clone();
    forged[1].segments[1].amount_out = "999".to_owned();
    assert!(matches!(
        replay_route(&forged, &exit_of("1000"), &net),
        Replay::Mismatch(_)
    ));
    assert_eq!(
        replay_route(&nodes[..1], &RouteEnd::InsideNode, &net),
        Replay::PublishersWord
    );
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn a_providers_movement_counts_on_its_network_only_when_the_network_says_so() {
    use rutent_evidence::{Expected, Unmatched, verify_movement};
    let mut fleet = fleet_of(&[Spec::of(Role::Exchange, "sim-exchange")]);
    let convert = |fleet: &mut Fleet, transaction: &str| {
        let asked = operation(
            fleet.context(),
            json!({
                "kind": "conversion",
                "transaction": transaction,
                "provider": "sim-exchange",
                "node": "polygon",
                "from": "USDC",
                "to": "USDT",
                "direction": "exact_in",
                "amount": "2000000",
                "dry_run": false,
                "inputs": {},
            }),
        );
        answered(fleet.handle("sim-exchange", &asked))
    };
    let expected = Expected {
        network: "polygon".to_owned(),
        destination: "sim-exchange-pool".to_owned(),
        asset: "USDT".to_owned(),
        amount: "2000000".to_owned(),
    };
    let id = |text: &str| MovementId::new(text).expect("an id");
    // Recorded, and not yet final: two confirmations settle a movement on this network.
    assert_eq!(
        convert(&mut fleet, "tx-net")["outcome"]["status"],
        "succeeded"
    );
    let net = |fleet: &mut Fleet| fleet.world().network("polygon").expect("a network").clone();
    assert_eq!(
        verify_movement(&net(&mut fleet), &id("tx-net"), &expected),
        Err(Unmatched::NotFinal)
    );
    fleet
        .world()
        .network_mut("polygon")
        .expect("a network")
        .advance(1);
    assert_eq!(
        verify_movement(&net(&mut fleet), &id("tx-net"), &expected),
        Ok(())
    );
    // A movement the network reorgs out before it is final evidences nothing.
    convert(&mut fleet, "tx-reorged");
    let undone = fleet
        .world()
        .network_mut("polygon")
        .expect("a network")
        .reorg(1);
    assert_eq!(undone, [id("tx-reorged")]);
    assert_eq!(
        verify_movement(&net(&mut fleet), &id("tx-reorged"), &expected),
        Err(Unmatched::ReorgedOut)
    );
    // A provider that says it moved value and sent no movement leaves nothing on the network.
    fleet.world().script(
        "sim-exchange",
        Behaviour {
            unrecorded: true,
            ..Behaviour::default()
        },
    );
    assert_eq!(
        convert(&mut fleet, "tx-hidden")["outcome"]["status"],
        "succeeded"
    );
    assert_eq!(
        verify_movement(&net(&mut fleet), &id("tx-hidden"), &expected),
        Err(Unmatched::UnknownMovement)
    );
}

// verifies: DEC-24, REQ-MOD-4
#[test]
fn every_role_and_two_private_nodes_live_in_one_fleet() {
    let specs = [
        reserve_of("sim-reserve-a"),
        reserve_of("sim-reserve-b"),
        Spec::of(Role::Exchange, "sim-exchange"),
        Spec::of(Role::Ramp, "sim-ramp"),
        Spec::of(Role::Verifier, "sim-verifier"),
    ];
    let fleet = fleet_of(&specs);
    let mut keys = Vec::new();
    for spec in &specs {
        let installed = fleet.provider(&spec.provider).expect("installed");
        assert_eq!(
            fleet.platform.active_version(&spec.provider).as_deref(),
            Some("1.0.0")
        );
        keys.extend(installed.advertisement.statement().statement.keys.clone());
    }
    let distinct: std::collections::BTreeSet<String> = keys
        .iter()
        .map(|k| k.public_key.as_str().to_owned())
        .collect();
    assert_eq!(distinct.len(), keys.len(), "no two providers share a key");
}
