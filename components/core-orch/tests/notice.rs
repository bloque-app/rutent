//! Notices (`T-023-01`, `REQ-ORD-20`, `AC-41`): every change of an order's state, read from the log's entries,
//! notified to the order's requester and the operator, each notice sealed for its receiver, and checked by the
//! reference receiver ([`Inbox`]) with a real Ed25519 key. The entries are the log's shapes, built from the
//! state-machine contract's fixture of an order of three parts, each a withdrawal, an exchange and a deposit; every
//! input is replayed through `rutent-stm` here as well, so what the notices claim is checked against the machine.

use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey, Verifier as _, VerifyingKey};
use proptest::prelude::*;
use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, Operator, Reads};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Charge, Decision, Entry, OrderRefusalClass, PermissionChange, Raise,
    ReadAnswer, ReadCheck, ReadOutcome, RefusalClass, Role, Work,
};
use rutent_contracts::core_internal::{Afterwards, Reads as Read};
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine::{self as stm, OrderState, RecordedInput, StateMachine};
use rutent_core_orch::notice::{
    About, AheadCharge, Channel, CurrentError, Inbox, NotTaken, NoticeError, NoticeKey, Notices,
    Receiver, Taken, Transport, Verifier, signing_input,
};
use rutent_core_orch::rounds::Fresh;
use rutent_core_orch::snapshot::{Clock, Reading};
use rutent_stm::Stm;
use serde_json::{Value, json};

const DEPLOYMENT: &str = "deployment-1";
const OPERATOR: &str = "operator";
const ALICE: &str = "alice";
const BOB: &str = "bob";
const NOW: u64 = 1_794_200_000;

fn id(text: &str) -> Id {
    Id::new(text).expect("an id")
}

fn customer(party: &str) -> Receiver {
    Receiver {
        party: id(party),
        role: Role::Customer,
    }
}

fn operator() -> Receiver {
    Receiver {
        party: id(OPERATOR),
        role: Role::Operator,
    }
}

fn actor(party: &str, role: Role) -> Actor {
    Actor {
        party: id(party),
        acts: id(&format!("{party}-login")),
        role,
        kind: ActorKind::Person,
    }
}

// --- The key, the receiver's check, the transport, policy and the clock ------------------------------------------

struct TestKey(SigningKey);

impl TestKey {
    fn new() -> Self {
        TestKey(SigningKey::from_bytes(&[42u8; 32]))
    }

    fn public(&self) -> PublicKey {
        PublicKey::new(base64url(self.0.verifying_key().as_bytes())).expect("a canonical key")
    }
}

impl NoticeKey for TestKey {
    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

/// A key that can't sign now.
struct Locked;

impl NoticeKey for Locked {
    fn sign(&mut self, _: &[u8]) -> Option<[u8; 64]> {
        None
    }
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

struct Ed25519;

impl Verifier for Ed25519 {
    fn verifies(&self, key: &PublicKey, input: &[u8], signature: &[u8; 64]) -> bool {
        VerifyingKey::from_bytes(&key.bytes()).is_ok_and(|k| {
            k.verify(input, &ed25519_dalek::Signature::from_bytes(signature))
                .is_ok()
        })
    }
}

/// Every notice sent, in order; it takes none while `down`.
#[derive(Default)]
struct Wire {
    sent: Vec<(Receiver, Vec<u8>)>,
    down: bool,
}

impl Wire {
    fn to(&self, receiver: &Receiver) -> Vec<Vec<u8>> {
        self.sent
            .iter()
            .filter(|(to, _)| to == receiver)
            .map(|(_, n)| n.clone())
            .collect()
    }
}

impl Transport for Wire {
    fn send(&mut self, to: &Receiver, notice: &[u8]) -> bool {
        if self.down {
            return false;
        }
        self.sent.push((to.clone(), notice.to_vec()));
        true
    }
}

/// Policy's read rule for an order's state: its requester as a customer, and the operator, read it; anyone else
/// is refused. Every check is kept, as the log records it.
struct Policy {
    requesters: BTreeMap<String, String>,
    checks: Vec<(ReadCheck, bool)>,
}

impl Policy {
    fn of(orders: &[(&str, &str)]) -> Self {
        Policy {
            requesters: orders
                .iter()
                .map(|(o, r)| ((*o).to_owned(), (*r).to_owned()))
                .collect(),
            checks: Vec::new(),
        }
    }
}

fn receipt(index: usize) -> rutent_contracts::core_internal::Receipt {
    serde_json::from_value(json!({
        "log": "log-main",
        "index": index,
        "entry": {"algorithm": "sha-256", "value": "ab".repeat(32)}
    }))
    .expect("a receipt")
}

impl Reads for Policy {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        let allowed = match (&check.reads, check.reader.role) {
            (Read::OrderState { order }, Role::Customer) => self
                .requesters
                .get(order.as_str())
                .is_some_and(|r| r == check.reader.party.as_str()),
            (Read::OrderState { .. }, Role::Operator) => check.reader.party.as_str() == OPERATOR,
            _ => false,
        };
        self.checks.push((check.clone(), allowed));
        Ok(ReadAnswer {
            request: check.request.clone(),
            outcome: if allowed {
                ReadOutcome::Allowed {}
            } else {
                ReadOutcome::Refused {
                    class: RefusalClass::NotPermitted,
                }
            },
            receipt: receipt(self.checks.len()),
        })
    }

    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

struct At(u64);

impl Clock for At {
    fn now(&self) -> Reading {
        Reading::at(self.0)
    }
}

#[derive(Default)]
struct Ids(u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("read-{}", self.0))
    }
}

// --- The log's entries ---------------------------------------------------------------------------------------------

fn core() -> Value {
    json!({"party": "rutent", "acts": "core-orch", "role": "core", "kind": "component"})
}

fn verified(what: Value, at: &str) -> Entry {
    serde_json::from_value(json!({
        "kind": "verified",
        "who": core(),
        "what": what,
        "when": {"at": at, "source": "core_clock"},
        "outcome": {"status": "observed"}
    }))
    .expect("a verified entry")
}

fn time(n: usize) -> String {
    format!("2026-11-12T10:{:02}:{:02}Z", n / 60, n % 60)
}

/// The acceptance of `order`, requested by `requester`.
fn acceptance(order: &str, requester: &str) -> Entry {
    let yeses: Vec<Value> = ["p1", "p2", "p3"]
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "part": p,
                "origin_reserve": "reserve-bank-co",
                "reserve_yes": {"log": "log-main", "index": 80 + i, "entry": {"algorithm": "sha-256", "value": "86".repeat(32)}}
            })
        })
        .collect();
    serde_json::from_value(json!({
        "kind": "verified",
        "who": {"party": requester, "acts": format!("{requester}-login"), "role": "customer", "kind": "person"},
        "what": {
            "call": "acceptance",
            "acceptance": format!("acc-{order}"),
            "order": order,
            "request": format!("req-{order}"),
            "quote": {"algorithm": "sha-256", "value": "91".repeat(32)},
            "authorisation": format!("auth-{order}"),
            "basis": "given",
            "parties": {"requester": requester, "beneficiary": "payee-ref-3", "payer": requester},
            "origin_reserve": "reserve-bank-co",
            "yeses": yeses,
            "session": "sess-300"
        },
        "when": {"at": time(0), "source": "core_clock"},
        "outcome": {"status": "observed"}
    }))
    .expect("an acceptance")
}

/// The fixture's `route_chosen`, for `order`.
fn route_chosen(order: &str, record: &str) -> RecordedInput {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../contracts/state-machine/fixtures/valid/recorded-input-route-chosen-three-parts.json"
    ))
    .expect("the fixture");
    input["record_id"] = json!(record);
    input["subject"]["order"] = json!(order);
    serde_json::from_value(input).expect("an input")
}

/// An input about `order`, `part` and `segment`, observing `observed`.
fn input(
    order: &str,
    record: &str,
    n: usize,
    part: Option<&str>,
    segment: Option<&str>,
    observed: Value,
) -> RecordedInput {
    let mut subject = json!({"order": order});
    if let Some(p) = part {
        subject["part"] = json!(p);
    }
    if let Some(s) = segment {
        subject["segment"] = json!(s);
    }
    serde_json::from_value(json!({
        "record_id": record,
        "recorded_at": time(n),
        "time_source": "core-orch clock",
        "source": {"kind": "orchestrator", "id": "core-orch"},
        "standing": "stated",
        "subject": subject,
        "observed": observed
    }))
    .expect("an input")
}

/// The log's entry of `input`, appended by Core at its own clock's time, which is always UTC.
fn entry_of(input: &RecordedInput) -> Entry {
    verified(json!({"call": "input", "input": input}), &time(59))
}

/// The inputs of an order moving through several states, a failure among them: `p1` completes; `p2`'s withdrawal
/// is cancelled; `p3`'s exchange fails, the part is declared stuck over time, and its resolution delivers it. One
/// input the machine doesn't apply (a provider bound on the ended `p2`) sits among them.
fn history(order: &str) -> Vec<RecordedInput> {
    let mut n = 0;
    let mut next = |part: &str, segment: Option<&str>, observed: Value| {
        n += 1;
        input(
            order,
            &format!("{order}-rec-{n:02}"),
            n,
            Some(part),
            segment,
            observed,
        )
    };
    let bound = || json!({"kind": "provider_bound"});
    let settled = |ev: &str| json!({"kind": "segment_settled", "evidence": ev});
    let mut inputs = vec![route_chosen(order, &format!("{order}-rec-00"))];
    inputs.extend([
        next("p1", Some("p1-s1"), bound()),
        next("p1", Some("p1-s1"), settled("ev-1")),
        next("p2", Some("p2-s1"), bound()),
        next("p2", None, json!({"kind": "withdrawal_cancelled"})),
        next("p2", Some("p2-s1"), bound()),
        next("p1", Some("p1-s2"), bound()),
        next("p1", Some("p1-s2"), settled("ev-2")),
        next("p1", Some("p1-s3"), bound()),
        next("p1", Some("p1-s3"), settled("ev-3")),
        next("p3", Some("p3-s1"), bound()),
        next("p3", Some("p3-s1"), settled("ev-4")),
        next("p3", Some("p3-s2"), bound()),
        next("p3", Some("p3-s2"), json!({"kind": "segment_failed"})),
        next(
            "p3",
            None,
            json!({"kind": "stuck_declared", "reason": "over_time"}),
        ),
        next(
            "p3",
            None,
            json!({"kind": "stuck_resolved", "to": "destination", "evidence": "ev-5"}),
        ),
    ]);
    inputs
}

/// The order after each input the machine applies, by the machine itself: each input, and the order after it.
fn truth(inputs: &[RecordedInput]) -> Vec<(RecordedInput, stm::Order)> {
    let mut order = Stm::open(&inputs[0]).expect("the route opens the order");
    let mut applied = vec![(inputs[0].clone(), order.clone())];
    for input in &inputs[1..] {
        let next = Stm::transition(&order, input);
        if next.last_input.outcome == (stm::Outcome::Applied {}) {
            order = next;
            applied.push((input.clone(), order.clone()));
        }
    }
    applied
}

/// The log of `orders`, each with its requester: each acceptance, then the orders' inputs, interleaved.
fn log_of(orders: &[(&str, &str)]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = orders.iter().map(|(o, r)| acceptance(o, r)).collect();
    let histories: Vec<_> = orders.iter().map(|(o, _)| history(o)).collect();
    let longest = histories.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for h in &histories {
            if let Some(input) = h.get(i) {
                entries.push(entry_of(input));
            }
        }
    }
    entries
}

fn notices() -> Notices {
    Notices::new(id(DEPLOYMENT), id(OPERATOR))
}

fn inbox(me: Receiver, key: &TestKey) -> Inbox {
    Inbox::new(me, id(DEPLOYMENT), key.public())
}

/// What a sealed notice says.
fn said(bytes: &[u8]) -> Value {
    serde_json::from_slice::<Value>(bytes).expect("json")["notice"].clone()
}

/// A notice re-addressed to `to`, its signature left as it was.
fn readdressed(bytes: &[u8], to: &Receiver) -> Vec<u8> {
    let mut sealed: Value = serde_json::from_slice(bytes).expect("json");
    sealed["notice"]["to"] = serde_json::to_value(to).expect("a receiver");
    serde_json::to_vec(&sealed).expect("json")
}

fn state_name(state: OrderState) -> Value {
    serde_json::to_value(state).expect("a state")
}

// --- The tests -----------------------------------------------------------------------------------------------------

/// An order moving through several states, a failed segment, a cancelled withdrawal and a stuck part among them:
/// each input the state machine applies is one notice to the order's requester and one to the operator, at
/// sequences 1, 2, 3, ..., each holding the order's state and every part's state after it as the machine has them,
/// and the input's kind; the input the machine doesn't apply is notified to no one. Each receiver's inbox takes
/// every one of its own and ends at the order's final state.
// verifies: AC-41, REQ-ORD-20
#[test]
fn each_state_change_is_notified_to_the_user_and_the_operator() {
    let entries = log_of(&[("ord-1", ALICE)]);
    let applied = truth(&history("ord-1"));
    assert_eq!(
        applied.len(),
        history("ord-1").len() - 1,
        "one input isn't applied"
    );
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    assert_eq!(
        n.notify(&entries, &mut key, &mut wire),
        Ok(2 * applied.len())
    );

    for receiver in [customer(ALICE), operator()] {
        let got = wire.to(&receiver);
        assert_eq!(got.len(), applied.len(), "every change, to {receiver:?}");
        let mut inbox = inbox(receiver.clone(), &key);
        for (i, (bytes, (input, order))) in got.iter().zip(&applied).enumerate() {
            let notice = said(bytes);
            assert_eq!(notice["about"]["sequence"], json!(i + 1));
            assert_eq!(notice["about"]["order"], json!("ord-1"));
            assert_eq!(
                notice["about"]["change"]["input"],
                serde_json::to_value(&input.observed).expect("observed")["kind"]
            );
            assert_eq!(notice["about"]["view"]["state"], state_name(order.state));
            let parts: Vec<Value> = order
                .parts
                .iter()
                .map(|p| json!({"part": p.part_id.as_str(), "state": state_name(p.state)}))
                .collect();
            assert_eq!(notice["about"]["view"]["parts"], json!(parts));
            assert_eq!(inbox.take(&Ed25519, bytes), Ok(Taken::Later));
        }
        let (latest, view) = inbox.view(&id("ord-1")).expect("the order");
        assert_eq!(latest, applied.len() as u64);
        assert_eq!(view.state, OrderState::EndedWithoutCompleting);
        let parts: Vec<_> = view.parts.iter().map(|p| p.state).collect();
        assert_eq!(
            parts,
            [
                OrderState::Complete,
                OrderState::EndedWithoutCompleting,
                OrderState::Complete
            ]
        );
        assert!(!inbox.missing(&id("ord-1")));
    }

    // The failures and the stuck part are among the changes, the stuck one with its reason.
    let kinds: Vec<Value> = wire
        .to(&operator())
        .iter()
        .map(|b| said(b)["about"]["change"].clone())
        .collect();
    for kind in ["segment_failed", "withdrawal_cancelled", "stuck_declared"] {
        assert!(
            kinds.iter().any(|c| c["input"] == json!(kind)),
            "{kind} notified"
        );
    }
    assert!(kinds.contains(
        &json!({"change": "input", "input": "stuck_declared", "part": "p3", "reason": "over_time"})
    ));
    let stuck = wire
        .to(&operator())
        .iter()
        .map(|b| said(b))
        .find(|n| n["about"]["change"]["input"] == json!("stuck_declared"))
        .expect("the stuck notice");
    assert_eq!(stuck["about"]["view"]["state"], json!("stuck"));

    // Read again, the log notifies nothing more.
    assert_eq!(n.notify(&entries, &mut key, &mut wire), Ok(0));
}

/// An input recorded at a time with an offset from UTC, a fraction of a second beyond nanoseconds, or both, as
/// the state-machine contract allows, is notified like any other, and the order is followed on past it: every later
/// change is notified too.
// verifies: AC-41, REQ-ORD-20
#[test]
fn input_recorded_with_an_offset_is_notified_and_the_order_followed_on() {
    let mut inputs = history("ord-1");
    inputs[2].recorded_at = stm::Timestamp::new("2026-11-12T11:00:02+01:00").expect("a time");
    inputs[3].recorded_at = stm::Timestamp::new("2026-11-12T10:00:03.1234567891Z").expect("a time");
    let mut entries = vec![acceptance("ord-1", ALICE)];
    entries.extend(inputs.iter().map(entry_of));
    let applied = truth(&inputs);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&entries, &mut key, &mut wire)
        .expect("notified");
    let got = wire.to(&customer(ALICE));
    assert_eq!(got.len(), applied.len());
    assert_eq!(
        said(&got[2])["about"]["at"],
        json!("2026-11-12T11:00:02+01:00")
    );
    let last = said(&got[got.len() - 1]);
    assert_eq!(last["about"]["sequence"], json!(applied.len()));
    assert_eq!(
        last["about"]["view"]["state"],
        json!("ended_without_completing")
    );
}

/// A notice is taken only by the receiver it is sealed for. The operator's notice taken by the order's customer,
/// the customer's taken by another customer, and either at another deployment, fail their check, and the inbox
/// holds nothing of them; a notice re-addressed to another receiver fails its signature; a notice altered fails it
/// too; and bytes that aren't a sealed notice are malformed.
// verifies: REQ-ORD-20
#[test]
fn notice_for_another_receiver_fails_its_check() {
    let entries = log_of(&[("ord-1", ALICE)]);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&entries, &mut key, &mut wire)
        .expect("notified");
    let alices = wire.to(&customer(ALICE))[0].clone();
    let operators = wire.to(&operator())[0].clone();

    let mut bob = inbox(customer(BOB), &key);
    assert_eq!(bob.take(&Ed25519, &alices), Err(NotTaken::ForAnother));
    assert_eq!(bob.take(&Ed25519, &operators), Err(NotTaken::ForAnother));
    let mut alice = inbox(customer(ALICE), &key);
    assert_eq!(alice.take(&Ed25519, &operators), Err(NotTaken::ForAnother));
    // The operator's party, receiving as a customer, is another receiver.
    let mut posing = inbox(
        Receiver {
            party: id(OPERATOR),
            role: Role::Customer,
        },
        &key,
    );
    assert_eq!(posing.take(&Ed25519, &operators), Err(NotTaken::ForAnother));
    let mut elsewhere = Inbox::new(customer(ALICE), id("deployment-2"), key.public());
    assert_eq!(
        elsewhere.take(&Ed25519, &alices),
        Err(NotTaken::OtherDeployment)
    );

    assert_eq!(
        bob.take(&Ed25519, &readdressed(&alices, &customer(BOB))),
        Err(NotTaken::Forged)
    );
    let mut altered: Value = serde_json::from_slice(&alices).expect("json");
    altered["notice"]["about"]["view"]["state"] = json!("complete");
    assert_eq!(
        alice.take(&Ed25519, &serde_json::to_vec(&altered).expect("json")),
        Err(NotTaken::Forged)
    );
    let mut by_other = Wire::default();
    notices()
        .notify(
            &entries,
            &mut TestKey(SigningKey::from_bytes(&[7u8; 32])),
            &mut by_other,
        )
        .expect("notified");
    assert_eq!(
        alice.take(&Ed25519, &by_other.to(&customer(ALICE))[0]),
        Err(NotTaken::Forged)
    );
    assert_eq!(alice.take(&Ed25519, b"{}"), Err(NotTaken::Malformed));

    for inbox in [&bob, &posing, &elsewhere] {
        assert_eq!(inbox.orders().count(), 0);
    }
    assert_eq!(alice.orders().count(), 0, "nothing forged was taken");
    assert_eq!(alice.take(&Ed25519, &alices), Ok(Taken::Later));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any delivery of an order's notices, each any number of times and in any order, some never: the receiver's
    /// view never goes back, is always the state of the latest sequence it took, and, once the last notice
    /// arrived, is the order's current state; a notice missing before the latest shows as a gap.
    // verifies: REQ-ORD-20
    #[test]
    fn repeated_or_reordered_notices_leave_the_receivers_view_right(
        delivery in prop::collection::vec(0usize..16, 0..48),
    ) {
        let entries = log_of(&[("ord-1", ALICE)]);
        let applied = truth(&history("ord-1"));
        let (mut key, mut wire) = (TestKey::new(), Wire::default());
        notices().notify(&entries, &mut key, &mut wire).expect("notified");
        let sent = wire.to(&customer(ALICE));
        prop_assert_eq!(sent.len(), applied.len());

        let mut inbox = inbox(customer(ALICE), &key);
        let mut latest = 0u64;
        let mut taken = std::collections::BTreeSet::new();
        for i in delivery.iter().map(|i| i % sent.len()) {
            let sequence = i as u64 + 1;
            let answer = inbox.take(&Ed25519, &sent[i]).expect("its own notice");
            prop_assert_eq!(answer, if sequence > latest { Taken::Later } else { Taken::Earlier });
            latest = latest.max(sequence);
            taken.insert(sequence);
            let (held, view) = inbox.view(&id("ord-1")).expect("held");
            prop_assert_eq!(held, latest);
            prop_assert_eq!(view.state, applied[latest as usize - 1].1.state);
            prop_assert_eq!(inbox.missing(&id("ord-1")), (taken.len() as u64) < latest);
        }
        // The last notice delivered, the view is the order's current state, whatever came before it.
        inbox.take(&Ed25519, &sent[sent.len() - 1]).expect("its own notice");
        let (_, view) = inbox.view(&id("ord-1")).expect("held");
        let current = &applied[applied.len() - 1].1;
        prop_assert_eq!(view.state, current.state);
        let parts: Vec<_> = current.parts.iter().map(|p| p.state).collect();
        prop_assert_eq!(view.parts.iter().map(|p| p.state).collect::<Vec<_>>(), parts);
    }
}

/// A receiver that lost notices asks for the current state: policy checks the read and records it, and the answer
/// is the order's latest notice, sealed for the asker, which its inbox takes in place of those lost. The operator
/// asks too. A stranger is refused, and its refusal is recorded; it learns nothing, not even whether the order
/// exists, since the check comes first.
// verifies: REQ-ORD-20
#[test]
fn receiver_can_ask_for_the_current_state() {
    let entries = log_of(&[("ord-1", ALICE)]);
    let applied = truth(&history("ord-1"));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&entries[..4], &mut key, &mut wire)
        .expect("the first changes");
    let mut alice = inbox(customer(ALICE), &key);
    alice
        .take(&Ed25519, &wire.to(&customer(ALICE))[0])
        .expect("the first");
    // The rest are lost on the way.
    let mut policy = Policy::of(&[("ord-1", ALICE)]);
    let (clock, mut ids) = (At(NOW), Ids::default());
    let mut lost = Wire {
        down: true,
        ..Wire::default()
    };

    let answer = n
        .current(
            &entries,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
            &mut lost,
            &actor(ALICE, Role::Customer),
            None,
            &id("ord-1"),
        )
        .expect("alice reads her order");
    assert_eq!(alice.take(&Ed25519, &answer), Ok(Taken::Later));
    let (latest, view) = alice.view(&id("ord-1")).expect("held");
    assert_eq!(latest, applied.len() as u64);
    assert_eq!(view.state, applied[applied.len() - 1].1.state);
    assert!(
        alice.missing(&id("ord-1")),
        "the lost ones show as a gap, the state is current"
    );
    assert_eq!(policy.checks.len(), 1);
    assert_eq!(
        policy.checks[0].0.reads,
        Read::OrderState { order: id("ord-1") }
    );

    let mut op = inbox(operator(), &key);
    let answer = n
        .current(
            &entries,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
            &mut lost,
            &actor(OPERATOR, Role::Operator),
            None,
            &id("ord-1"),
        )
        .expect("the operator reads the order's state");
    assert_eq!(op.take(&Ed25519, &answer), Ok(Taken::Later));

    for order in ["ord-1", "ord-none"] {
        let refused = n.current(
            &entries,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
            &mut lost,
            &actor(BOB, Role::Customer),
            None,
            &id(order),
        );
        assert!(
            matches!(refused, Err(CurrentError::Refused(ref a)) if a.outcome != ReadOutcome::Allowed {}),
            "bob is refused {order}"
        );
    }
    assert_eq!(
        policy.checks.len(),
        4,
        "every read recorded, the refused ones too"
    );
    assert!(policy.checks[2..].iter().all(|(_, allowed)| !allowed));
}

/// Two orders of two requesters in one log: each requester receives its own order's notices and no other's, and
/// the operator receives both; a raise and a charge counted ahead reach the operator alone. Alice's notices name
/// no provider, segment or amount.
// verifies: REQ-AUZ-7, REQ-ORD-20
#[test]
fn each_customer_receives_only_its_own_orders_notices() {
    let entries = log_of(&[("ord-1", ALICE), ("ord-2", BOB)]);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&entries, &mut key, &mut wire).expect("notified");
    let raise: Raise = serde_json::from_str(include_str!(
        "../../../contracts/core-internal/fixtures/valid/raise-of-a-change-in-a-providers-p99.json"
    ))
    .expect("a raise");
    Channel {
        notices: &mut n,
        key: &mut key,
        transport: &mut wire,
    }
    .raise(&raise);
    n.ahead(&[ahead_charge(BOB, "chg-1")], &At(NOW), &mut key, &mut wire)
        .expect("noticed");

    let orders_of = |receiver: &Receiver| -> Vec<Value> {
        wire.to(receiver)
            .iter()
            .map(|b| said(b)["about"]["order"].clone())
            .collect()
    };
    let per_order = truth(&history("ord-1")).len();
    let alices = orders_of(&customer(ALICE));
    assert_eq!(alices.len(), per_order);
    assert!(alices.iter().all(|o| o == &json!("ord-1")));
    let bobs = orders_of(&customer(BOB));
    assert_eq!(bobs.len(), per_order);
    assert!(bobs.iter().all(|o| o == &json!("ord-2")));
    let operators = wire.to(&operator());
    assert_eq!(operators.len(), 2 * per_order + 2);
    assert!(
        wire.sent
            .iter()
            .filter(|(to, _)| to.role != Role::Operator)
            .all(|(_, b)| said(b)["about"]["about"] == json!("order")),
        "no raise or charge reaches a customer"
    );
    for bytes in wire.to(&customer(ALICE)) {
        let text = String::from_utf8(bytes).expect("text");
        for word in [
            "reserve-bank-co",
            "exchange-a",
            "p1-s1",
            "ev-",
            "amount",
            BOB,
        ] {
            assert!(!text.contains(word), "{word} in alice's notice");
        }
    }
}

/// An order whose acceptance the log doesn't hold is notified to the operator alone: no customer is guessed.
// verifies: REQ-AUZ-7, REQ-ORD-20
#[test]
fn order_with_no_recorded_acceptance_is_notified_to_the_operator_alone() {
    let entries: Vec<Entry> = history("ord-9").iter().map(entry_of).collect();
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&entries, &mut key, &mut wire).expect("notified");
    assert_eq!(wire.sent.len(), truth(&history("ord-9")).len());
    assert!(wire.sent.iter().all(|(to, _)| to == &operator()));
    assert_eq!(n.receivers(&id("ord-9")), vec![operator()]);
}

/// A destination's report that a complete order's payment came back, and an approval arriving after the order
/// ended, leave its state as it was and are notified each as the order's next change; so is a refusal naming the
/// order. An input the log recorded as not taken is notified to no one.
// verifies: REQ-ORD-31, REQ-ORD-25, REQ-ORD-20
#[test]
fn what_comes_after_an_order_ended_is_notified() {
    let mut entries = log_of(&[("ord-1", ALICE)]);
    let applied = truth(&history("ord-1")).len();
    let receipt = serde_json::to_value(receipt(9)).expect("a receipt");
    entries.push(verified(
        json!({"call": "afterwards", "order": "ord-1", "event": "returned", "statement": receipt}),
        &time(90),
    ));
    entries.push(verified(
        json!({"call": "afterwards", "order": "ord-1", "event": "late_approval", "statement": receipt}),
        &time(91),
    ));
    entries.push(verified(
        json!({"call": "order_refusal", "request": "req-ord-1", "order": "ord-1", "class": "no_route_right_now"}),
        &time(92),
    ));
    let not_taken = input(
        "ord-1",
        "ord-1-rec-99",
        93,
        Some("p1"),
        Some("p1-s1"),
        json!({"kind": "provider_bound"}),
    );
    entries.push(verified(
        json!({"call": "input_not_taken", "input": not_taken, "reason": {"kind": "not_applicable", "reason": "ended"}}),
        &time(93),
    ));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&entries, &mut key, &mut wire)
        .expect("notified");
    let got: Vec<Value> = wire.to(&customer(ALICE)).iter().map(|b| said(b)).collect();
    assert_eq!(got.len(), applied + 3);
    let tail: Vec<_> = got[applied..]
        .iter()
        .map(|n| {
            (
                n["about"]["sequence"].clone(),
                n["about"]["change"].clone(),
                n["about"]["view"]["state"].clone(),
            )
        })
        .collect();
    let ended = json!("ended_without_completing");
    assert_eq!(
        tail,
        [
            (
                json!(applied + 1),
                json!({"change": "afterwards", "event": serde_json::to_value(Afterwards::Returned).expect("event")}),
                ended.clone()
            ),
            (
                json!(applied + 2),
                json!({"change": "afterwards", "event": "late_approval"}),
                ended.clone()
            ),
            (
                json!(applied + 3),
                json!({"change": "refused", "class": serde_json::to_value(OrderRefusalClass::NoRouteRightNow).expect("class")}),
                ended
            ),
        ]
    );
}

/// The notices are a function of the log: the same log read by notices built afresh, as after a restart, gives the
/// same notices byte for byte, so a receiver's sequence still holds; read in pieces, it gives them too.
// verifies: REQ-ORD-20
#[test]
fn same_log_gives_the_same_notices_after_a_restart() {
    let entries = log_of(&[("ord-1", ALICE), ("ord-2", BOB)]);
    let mut key = TestKey::new();
    let (mut first, mut again, mut pieces) = (Wire::default(), Wire::default(), Wire::default());
    notices()
        .notify(&entries, &mut key, &mut first)
        .expect("notified");
    notices()
        .notify(&entries, &mut key, &mut again)
        .expect("notified");
    let mut n = notices();
    for end in [3, 10, 11, entries.len()] {
        n.notify(&entries[..end], &mut key, &mut pieces)
            .expect("notified");
    }
    assert_eq!(first.sent.len(), 2 * 2 * truth(&history("ord-1")).len());
    assert_eq!(first.sent, again.sent);
    assert_eq!(first.sent, pieces.sent);
}

/// Entries that don't extend those read before (a shorter log, or another) are refused whole; a key that can't
/// sign sends nothing of the entry, and the next read carries on from it; a notice the transport didn't take is
/// kept for its retry.
// verifies: REQ-ORD-20
#[test]
fn another_log_or_a_locked_key_sends_nothing_amiss() {
    let entries = log_of(&[("ord-1", ALICE)]);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&entries[..5], &mut key, &mut wire)
        .expect("notified");
    let sent = wire.sent.len();
    assert_eq!(
        n.notify(&entries[..3], &mut key, &mut wire),
        Err(NoticeError::NotThisLog)
    );
    let other = log_of(&[("ord-1", BOB)]);
    assert_eq!(
        n.notify(&other, &mut key, &mut wire),
        Err(NoticeError::NotThisLog)
    );
    assert_eq!(
        n.notify(&entries, &mut Locked, &mut wire),
        Err(NoticeError::Unsigned)
    );
    assert_eq!(
        wire.sent.len(),
        sent,
        "nothing sent on another log or unsigned"
    );
    let mut down = Wire {
        down: true,
        ..Wire::default()
    };
    n.notify(&entries[..6], &mut key, &mut down)
        .expect("notified");
    assert_eq!(
        n.undelivered().len(),
        2,
        "one change, to both receivers, kept"
    );
    n.notify(&entries, &mut key, &mut wire).expect("notified");
    let mut fresh = Wire::default();
    notices()
        .notify(&entries, &mut key, &mut fresh)
        .expect("notified");
    let mut delivered = wire.sent.clone();
    delivered.splice(
        sent..sent,
        n.undelivered()
            .iter()
            .map(|u| (u.to.clone(), u.notice.clone())),
    );
    assert_eq!(delivered, fresh.sent, "nothing lost, nothing twice");
}

/// A raise, a change in a provider's p99 among them, is sealed for the operator alone and answers delivered when
/// the transport took it, undelivered otherwise, kept; a raise that fails its check isn't sent. The operator's
/// inbox holds each raise once however often it comes.
// verifies: REQ-ORD-6, REQ-ORD-9
#[test]
fn raise_reaches_the_operator_alone_and_answers_its_delivery() {
    let raise: Raise = serde_json::from_str(include_str!(
        "../../../contracts/core-internal/fixtures/valid/raise-of-a-change-in-a-providers-p99.json"
    ))
    .expect("a raise");
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    let mut channel = Channel {
        notices: &mut n,
        key: &mut key,
        transport: &mut wire,
    };
    assert_eq!(channel.raise(&raise), Delivery::Delivered);
    assert_eq!(channel.raise(&raise), Delivery::Delivered);
    let mut broken = raise.clone();
    broken.cites.clear();
    assert_eq!(channel.raise(&broken), Delivery::Undelivered);
    let mut down = Wire {
        down: true,
        ..Wire::default()
    };
    assert_eq!(
        Channel {
            notices: &mut n,
            key: &mut key,
            transport: &mut down
        }
        .raise(&raise),
        Delivery::Undelivered
    );
    assert_eq!(n.undelivered().len(), 1);
    assert_eq!(n.undelivered()[0].to, operator());

    assert_eq!(wire.sent.len(), 2);
    assert!(wire.sent.iter().all(|(to, _)| to == &operator()));
    let mut op = inbox(operator(), &key);
    assert_eq!(op.take(&Ed25519, &wire.sent[0].1), Ok(Taken::Event));
    assert_eq!(op.take(&Ed25519, &wire.sent[1].1), Ok(Taken::Repeat));
    assert!(matches!(op.events().next(), Some(About::Raise { raise: r }) if r == &raise));
}

fn ahead_charge(party: &str, request: &str) -> AheadCharge {
    let charge: Charge = serde_json::from_value(json!({
        "request": request,
        "party": party,
        "caller": {"party": party, "acts": format!("{party}-agent"), "role": "customer", "kind": "agent"},
        "work": "fan_out",
        "count": 3,
        "order": "ord-2",
        "at": "2030-01-01T00:00:00Z"
    }))
    .expect("a charge");
    AheadCharge {
        charge,
        counted_at: Timestamp::new("2030-01-01T00:00:00Z").expect("a time"),
    }
}

/// Each charge policy set apart as counted far ahead of Core's clock reaches the operator once, by its party and
/// request, with its party, request id, kind of work, count and counted time beside Core's now; the same set
/// handed over again sends nothing more, and a new one is sent.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn charge_counted_far_ahead_is_notified_to_the_operator_once() {
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    let clock = At(NOW);
    let first = [ahead_charge(BOB, "chg-1")];
    assert_eq!(n.ahead(&first, &clock, &mut key, &mut wire), Ok(1));
    assert_eq!(n.ahead(&first, &clock, &mut key, &mut wire), Ok(0));
    let both = [ahead_charge(BOB, "chg-1"), ahead_charge(BOB, "chg-2")];
    assert_eq!(n.ahead(&both, &clock, &mut key, &mut wire), Ok(1));
    assert!(wire.sent.iter().all(|(to, _)| to == &operator()));
    let charge = said(&wire.sent[0].1)["about"]["charge"].clone();
    assert_eq!(
        charge,
        json!({
            "party": BOB,
            "request": "chg-1",
            "work": serde_json::to_value(Work::FanOut).expect("work"),
            "count": 3,
            "counted_at": "2030-01-01T00:00:00Z",
            "now": Reading::at(NOW).timestamp().expect("now").as_str(),
        })
    );
    let mut op = inbox(operator(), &key);
    for (_, bytes) in &wire.sent {
        assert_eq!(op.take(&Ed25519, bytes), Ok(Taken::Event));
    }
}

/// A notice's signing input starts with its own domain, which no canonical JSON object starts with, so the
/// deployment's signature over a notice never verifies as a provider-role statement's.
// verifies: REQ-ORD-20
#[test]
fn notice_signing_input_is_apart_from_every_statements() {
    let entries = log_of(&[("ord-1", ALICE)]);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&entries, &mut key, &mut wire)
        .expect("notified");
    let sealed: Value = serde_json::from_slice(&wire.sent[0].1).expect("json");
    let notice = serde_json::from_value(sealed["notice"].clone()).expect("a notice");
    let input = signing_input(&notice).expect("an input");
    assert!(input.starts_with(rutent_core_orch::notice::DOMAIN));
    assert_ne!(input[0], b'{');
}
