//! The simulated providers' behaviours and books, in process (`T-024-01`): what a script makes a provider do, and
//! that a journal replayed into a fresh stand-in gives its books back.

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::{
    Act, Behaviour, Crash, Delivery, Entry, Role, Simulated, Spec, movements_of, operation,
    recorded_segments,
};
use rutent_contracts::provider_role::{Operation, Statement};
use serde_json::{Value, json};

fn context() -> SuiteContext {
    SuiteContext::default()
}

fn exchange() -> Simulated {
    Simulated::new(Spec::of(Role::Exchange, "sim-exchange"), &context())
}

fn reserve() -> Simulated {
    Simulated::new(Spec::of(Role::Reserve, "sim-reserve"), &context())
}

fn conversion(transaction: &str, amount: &str, dry_run: bool) -> Statement<Operation> {
    operation(
        &context(),
        json!({
            "kind": "conversion",
            "transaction": transaction,
            "provider": "sim-exchange",
            "node": "polygon",
            "from": "USDC",
            "to": "USDT",
            "direction": "exact_in",
            "amount": amount,
            "dry_run": dry_run,
            "inputs": {},
        }),
    )
}

fn withdrawal(transaction: &str, amount: &str) -> Statement<Operation> {
    operation(
        &context(),
        json!({
            "kind": "withdrawal",
            "transaction": transaction,
            "provider": "sim-reserve",
            "session": context().session,
            "origin": { "network": "sim-reserve-bank" },
            "destination": "sim-reserve-ledger",
            "asset": "COP",
            "amount": amount,
            "dry_run": false,
            "inputs": { "payer.bank_account": "acct-1" },
        }),
    )
}

fn outcome(answer: &[u8]) -> Value {
    serde_json::from_slice::<Value>(answer).expect("a result")["outcome"].clone()
}

// verifies: REQ-MOD-4
#[test]
fn a_provider_with_no_script_answers_as_its_role_asks() {
    let mut sim = exchange();
    let answer = sim.answer(&conversion("tx-1", "2000000", false), &Behaviour::default());
    let done = outcome(&answer.body);
    assert_eq!(done["status"], "succeeded");
    assert_eq!(done["amount_out"], "2000000");
    assert!(
        answer.entry.is_some(),
        "a real conversion changes the books"
    );
    let quote = sim.answer(&conversion("tx-2", "2000000", true), &Behaviour::default());
    assert_eq!(outcome(&quote.body)["status"], "dry_run_accepted");
    assert!(quote.entry.is_none(), "a dry run changes nothing");
    assert!(quote.movements.is_empty());
}

// verifies: REQ-MOD-4
#[test]
fn a_scripted_refusal_changes_no_books() {
    let mut sim = exchange();
    let before = sim.state();
    for act in [
        Act::Refuse,
        Act::RefuseLifting,
        Act::RefuseWaitingOnThePerson,
    ] {
        let answer = sim.answer(&conversion("tx-1", "2000000", false), &Behaviour::act(act));
        assert_eq!(outcome(&answer.body)["status"], "refused", "{act:?}");
        assert!(answer.entry.is_none());
    }
    assert_eq!(sim.state(), before);
}

// verifies: REQ-MOD-4, REQ-ORD-1
#[test]
fn a_scripted_unknown_outcome_is_the_providers_own_result() {
    let mut sim = exchange();
    let answer = sim.answer(
        &conversion("tx-1", "2000000", false),
        &Behaviour::act(Act::Unknown),
    );
    assert_eq!(outcome(&answer.body)["status"], "unknown");
    // It did the operation, so its books changed.
    assert!(answer.entry.is_some());
    assert!(answer.movements.is_empty(), "nothing is said to have moved");
}

// verifies: REQ-MOD-4
#[test]
fn a_scripted_failure_is_a_failure_with_evidence() {
    let mut sim = exchange();
    let answer = sim.answer(
        &conversion("tx-1", "2000000", false),
        &Behaviour::act(Act::Fail),
    );
    let failed = outcome(&answer.body);
    assert_eq!(failed["status"], "failed");
    assert!(!failed["evidence"].as_array().expect("evidence").is_empty());
}

// verifies: REQ-MOD-4, REQ-ORD-21
#[test]
fn a_provider_scripted_to_deliver_short_states_what_it_took_in_and_gives_less() {
    let mut sim = exchange();
    let answer = sim.answer(
        &conversion("tx-1", "2000000", false),
        &Behaviour::delivering(Delivery::Short { bps: 250 }),
    );
    let done = outcome(&answer.body);
    assert_eq!(done["amount_in"], "2000000");
    assert_eq!(done["amount_out"], "1950000");
    // The movement on the network is what was delivered.
    assert_eq!(answer.movements.len(), 1);
    assert_eq!(answer.movements[0].amount, "1950000");
}

// verifies: REQ-MOD-4, REQ-ORD-30
#[test]
fn a_provider_scripted_to_deliver_in_pieces_gives_one_piece_of_evidence_and_one_movement_each() {
    let mut sim = exchange();
    let answer = sim.answer(
        &conversion("tx-1", "2000001", false),
        &Behaviour::delivering(Delivery::Pieces { count: 3 }),
    );
    let done = outcome(&answer.body);
    let references: Vec<&str> = done["evidence"]
        .as_array()
        .expect("evidence")
        .iter()
        .map(|p| p["reference"].as_str().expect("a reference"))
        .collect();
    assert_eq!(references.len(), 3);
    assert!(references[0].ends_with("tx-1.1") && references[2].ends_with("tx-1.3"));
    let ids: Vec<&str> = answer.movements.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["tx-1.1", "tx-1.2", "tx-1.3"]);
    let total: u128 = answer
        .movements
        .iter()
        .map(|m| m.amount.parse::<u128>().expect("an amount"))
        .sum();
    assert_eq!(total, 2_000_001, "the pieces add up to what was delivered");
}

// verifies: REQ-MOD-4
#[test]
fn a_provider_scripted_to_move_nothing_publicly_sends_no_movement() {
    let mut sim = exchange();
    let behaviour = Behaviour {
        unrecorded: true,
        ..Behaviour::default()
    };
    let answer = sim.answer(&conversion("tx-1", "2000000", false), &behaviour);
    assert_eq!(outcome(&answer.body)["status"], "succeeded");
    assert!(answer.movements.is_empty());
}

// verifies: REQ-MOD-4
#[test]
fn movements_are_made_only_by_a_success() {
    let mut sim = exchange();
    let answer = sim.answer(&conversion("tx-1", "2000000", false), &Behaviour::default());
    let result: Value = serde_json::from_slice(&answer.body).expect("a result");
    let Operation::Conversion(_) = conversion("tx-1", "2000000", false).statement else {
        unreachable!()
    };
    let made = movements_of(&conversion("tx-1", "2000000", false).statement, &result);
    assert_eq!(made, answer.movements);
    let mut failed = result;
    failed["outcome"]["status"] = Value::from("failed");
    assert!(movements_of(&conversion("tx-1", "2000000", false).statement, &failed).is_empty());
}

// verifies: REQ-MOD-4, ADR-0008
#[test]
fn a_reserve_confirms_a_reversal_only_when_its_script_says_the_credit_arrived() {
    let confirmation = operation(
        &context(),
        json!({
            "kind": "reversal_confirmation",
            "transaction": "tx-confirm",
            "provider": "sim-reserve",
            "segment": "seg-1",
            "asset": "COP",
            "amount": "1000",
        }),
    );
    let mut sim = reserve();
    let not = sim.answer(&confirmation, &Behaviour::default());
    assert_eq!(outcome(&not.body)["status"], "not_confirmed");
    let yes = sim.answer(&confirmation, &Behaviour::crediting_a_reversal());
    assert_eq!(outcome(&yes.body)["status"], "confirmed");
    assert!(yes.entry.is_some(), "the credit is on its books");
}

// verifies: REQ-MOD-4
#[test]
fn a_journal_replayed_into_a_fresh_provider_gives_its_books_back() {
    let mut sim = reserve();
    let mut journal: Vec<Entry> = Vec::new();
    for (n, behaviour) in [
        Behaviour::default(),
        Behaviour::delivering(Delivery::Short { bps: 100 }),
        Behaviour::act(Act::Unknown),
        Behaviour::crashing(Crash::AfterSettle),
    ]
    .into_iter()
    .enumerate()
    {
        let answer = sim.answer(&withdrawal(&format!("tx-{n}"), "1000"), &behaviour);
        journal.extend(answer.entry);
    }
    let mut fresh = reserve();
    fresh.restore(&journal);
    assert_eq!(fresh.state(), sim.state());
    assert_eq!(fresh.applied(), journal.len());
    // The journal is what a world stores: JSON, and back.
    let stored = serde_json::to_string(&journal).expect("a journal");
    let read: Vec<Entry> = serde_json::from_str(&stored).expect("a journal reads back");
    let mut again = reserve();
    again.restore(&read);
    assert_eq!(again.state(), sim.state());
}

// verifies: REQ-ORD-41, DEC-24
#[test]
fn a_private_nodes_inner_segments_are_recorded_from_its_journal() {
    let spec = Spec::of(Role::Reserve, "sim-reserve");
    let mut sim = Simulated::new(spec.clone(), &context());
    let mut journal = Vec::new();
    for n in 0..2 {
        let answer = sim.answer(
            &withdrawal(&format!("tx-{n}"), "1000"),
            &Behaviour::default(),
        );
        journal.extend(answer.entry);
    }
    let segments = recorded_segments(&spec, &context(), &journal);
    assert_eq!(segments.len(), 2);
    assert!(segments.iter().all(|s| s.asset_in == "COP"
        && s.amount_in == "1000"
        && s.amount_out == "1000"
        && s.fee == "0"));
}

// verifies: DEC-24
#[test]
fn two_reserves_have_two_publishers_two_nodes_and_two_sets_of_keys() {
    let a = Simulated::new(Spec::of(Role::Reserve, "sim-reserve-a"), &context());
    let b = Simulated::new(Spec::of(Role::Reserve, "sim-reserve-b"), &context());
    let (ad_a, ad_b) = (
        a.advertisement().into_statement().statement,
        b.advertisement().into_statement().statement,
    );
    assert_ne!(ad_a.publisher, ad_b.publisher);
    assert_ne!(ad_a.private_nodes, ad_b.private_nodes);
    assert_ne!(ad_a.provider, ad_b.provider);
    assert_ne!(a.spec().seed(), b.spec().seed());
    assert_ne!(ad_a.keys, ad_b.keys);
}

// verifies: REQ-MOD-4, INV-8
#[test]
fn a_repeat_of_a_real_movement_makes_no_second_entry_in_the_journal() {
    let mut sim = reserve();
    let first = sim.answer(&withdrawal("tx-1", "1000"), &Behaviour::default());
    assert!(first.entry.is_some());
    let repeat = sim.answer(&withdrawal("tx-1", "1000"), &Behaviour::default());
    assert!(repeat.entry.is_none(), "a repeat moves nothing");
    assert_eq!(repeat.body, first.body, "and is answered as before");
    // Asked again of a provider that has restored its books from the journal, it is the same.
    let mut restored = reserve();
    restored.restore(&[first.entry.expect("an entry")]);
    let again = restored.answer(&withdrawal("tx-1", "1000"), &Behaviour::default());
    assert!(again.entry.is_none());
    assert_eq!(again.body, first.body);
}

// verifies: REQ-MOD-4, REQ-MDL-11
#[test]
fn a_journal_of_sixty_withdrawals_fits_the_outside_message_a_restart_reads_it_in() {
    // The platform passes an outside message of 64 KiB at most (`REQ-MDL-11`), and the journal travels in one as a
    // JSON string inside the JSON of the response, so it is escaped once on the way.
    let mut sim = reserve();
    let journal: Vec<Entry> = (0..60)
        .filter_map(|n| {
            sim.answer(
                &withdrawal(&format!("tx-{n:06}"), "1000"),
                &Behaviour::default(),
            )
            .entry
        })
        .collect();
    assert_eq!(journal.len(), 60);
    let text = serde_json::to_string(&journal).expect("a journal");
    let carried = serde_json::to_string(&text).expect("a string");
    assert!(carried.len() < 64 * 1024 - 1024, "{} bytes", carried.len());
}
