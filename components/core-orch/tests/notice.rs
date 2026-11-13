//! Notices (`T-023-01`, `REQ-ORD-20`, `AC-41`): every change of an order's state, read from the log's entries,
//! notified to the order's requester and the operator, each notice sealed for its receiver, and checked by the
//! reference receiver ([`Inbox`]) with a real Ed25519 key. The entries are the log's shapes, built from the
//! state-machine contract's fixture of an order of three parts, each a withdrawal, an exchange and a deposit; every
//! input is replayed through `rutent-stm` here as well, so what the notices claim is checked against the machine,
//! and every log is put to core-internal's `admit_entry` ([`held_to_the_log`]). The policy here is a stand-in; the
//! engine's own answers are held in `conformance/scenarios/tests/notices.rs`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use ed25519_dalek::{Signer, SigningKey, Verifier as _, VerifyingKey};
use proptest::prelude::*;
use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, Operator, Reads};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Afterwards, Charge, Decision, DeploymentOperator, Entry, OrderRefusalClass,
    PermissionChange, Raise, ReadAnswer, ReadCheck, ReadOutcome, Reads as Read, Receipt,
    RefusalClass, Role, Through, What, Work, admit_entry, state_machine_takes,
};
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine::{self as stm, OrderState, RecordedInput, StateMachine};
use rutent_core_orch::notice::{
    About, AheadCharge, Attempt, BOUND, Channel, CurrentError, Inbox, Key, Ledger, NotTaken,
    NoticeError, NoticeKey, Notices, Path, Receiver, Retried, SILENCE, Taken, Transport, Verifier,
    Watch, signing_input,
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

/// Every notice sent, in order; it takes none while `down`, and takes but loses every one while `silent`. `tried`
/// counts every send handed to it.
#[derive(Default)]
struct Wire {
    sent: Vec<(Receiver, Vec<u8>)>,
    down: bool,
    silent: bool,
    tried: usize,
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
        self.tried += 1;
        if self.down {
            return false;
        }
        if self.silent {
            return true;
        }
        self.sent.push((to.clone(), notice.to_vec()));
        true
    }
}

/// A stand-in for policy's read rule for an order's state: its requester as a customer reads it, and so does an
/// operator login the register lists; anyone else is refused, with the class the engine would give: a missing
/// input for an order its record doesn't hold, not permitted for another's. Every check is kept, as the log
/// records it.
struct Policy {
    requesters: BTreeMap<String, String>,
    operator_logins: Vec<String>,
    checks: Vec<(ReadCheck, bool)>,
}

impl Policy {
    fn of(orders: &[(&str, &str)]) -> Self {
        Policy {
            requesters: orders
                .iter()
                .map(|(o, r)| ((*o).to_owned(), (*r).to_owned()))
                .collect(),
            operator_logins: vec![format!("{OPERATOR}-login"), "desk-login".to_owned()],
            checks: Vec::new(),
        }
    }
}

impl Reads for Policy {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        let refused = |class| ReadOutcome::Refused { class };
        let outcome = match (&check.reads, check.reader.role) {
            (Read::OrderState { order }, Role::Customer) => {
                match self.requesters.get(order.as_str()) {
                    None => refused(RefusalClass::MissingInput),
                    Some(r) if r == check.reader.party.as_str() => ReadOutcome::Allowed {},
                    Some(_) => refused(RefusalClass::NotPermitted),
                }
            }
            (Read::OrderState { .. }, Role::Operator)
                if self
                    .operator_logins
                    .iter()
                    .any(|l| l == check.reader.acts.as_str()) =>
            {
                ReadOutcome::Allowed {}
            }
            _ => refused(RefusalClass::NotPermitted),
        };
        let allowed = outcome == ReadOutcome::Allowed {};
        self.checks.push((check.clone(), allowed));
        Ok(ReadAnswer {
            request: check.request.clone(),
            outcome,
            receipt: receipt("log-policy", self.checks.len(), &Value::Null),
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

/// A receipt in `log` at `index` of an entry whose JSON is `of`: the digest a function of the entry, as the log's
/// is (the log hashes with SHA-256; a test needs only that two entries' receipts differ).
fn receipt(log: &str, index: usize, of: &Value) -> Receipt {
    let text = serde_json::to_string(of).expect("json");
    let mut digest = String::new();
    for round in 0u64..4 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ round;
        for b in text.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        digest.push_str(&format!("{h:016x}"));
    }
    serde_json::from_value(json!({
        "log": log,
        "index": index,
        "entry": {"algorithm": "sha-256", "value": digest}
    }))
    .expect("a receipt")
}

/// `entries` as the log `log` holds them, each with its receipt.
fn receipted_in(log: &str, entries: Vec<Entry>) -> Vec<(Receipt, Entry)> {
    entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| (receipt(log, i, &serde_json::to_value(&e).expect("json")), e))
        .collect()
}

fn receipted(entries: Vec<Entry>) -> Vec<(Receipt, Entry)> {
    receipted_in("log-main", entries)
}

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

/// The acceptance `acceptance` of `order`, requested by `requester`, with a yes for each of `parts`.
fn acceptance_of(acceptance: &str, order: &str, requester: &str, parts: &[&str]) -> Entry {
    let yeses: Vec<Value> = parts
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
            "acceptance": acceptance,
            "order": order,
            "request": format!("req-{acceptance}"),
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

/// The acceptance of `order`, requested by `requester`.
fn acceptance(order: &str, requester: &str) -> Entry {
    acceptance_of(
        &format!("acc-{order}"),
        order,
        requester,
        &["p1", "p2", "p3"],
    )
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

/// The log's entry of `input`, appended at `at` by Core's clock.
fn entry_at(input: &RecordedInput, at: &str) -> Entry {
    verified(json!({"call": "input", "input": input}), at)
}

/// The log's entry of `input`, appended by Core at its own clock's time, which is always UTC: the input's own time
/// where that is one, else a fixed one.
fn entry_of(input: &RecordedInput) -> Entry {
    let at = Timestamp::new(input.recorded_at.as_str())
        .map_or_else(|| time(59), |t| t.as_str().to_owned());
    entry_at(input, &at)
}

/// An endpoint at `node`, an inner node's name or an outer network's, in `asset`.
fn end(node: &str, outer: bool, asset: &str) -> Value {
    let decimals = if asset == "COP" { 2 } else { 6 };
    let node = if outer {
        json!({"network": node})
    } else {
        json!(node)
    };
    json!({"node": node, "asset": {"id": asset, "decimals": decimals}})
}

fn segment(id: &str, provider: &str, kind: &str, from: Value, to: Value) -> Value {
    json!({"segment_id": id, "provider": provider, "kind": kind, "from": from, "to": to})
}

/// An exchange of COP for USDC inside `ledger-in` by `exchange-b`, then the deposit to the order's destination:
/// a recalculated route from where a settled withdrawal left the value.
fn recalculated(part: &str) -> Vec<Value> {
    vec![
        segment(
            &format!("{part}-s4"),
            "exchange-b",
            "exchange",
            end("ledger-in", false, "COP"),
            end("ledger-in", false, "USDC"),
        ),
        segment(
            &format!("{part}-s5"),
            "reserve-wallet",
            "deposit",
            end("ledger-in", false, "USDC"),
            end("polygon", true, "USDC"),
        ),
    ]
}

/// The inputs of an order moving through several states, a failure among them: `p1` completes; `p2`'s withdrawal is
/// cancelled; `p3`'s route is recalculated after its withdrawal, its new exchange fails, the part is declared stuck
/// over time, and its resolution delivers it. Among them, two inputs the log takes and the machine doesn't apply: a
/// settlement recorded again (a repeat), and a failure reported of a segment the recalculation retired (late).
fn history(order: &str) -> Vec<RecordedInput> {
    let mut n = 0;
    let mut next = |part: Option<&str>, segment: Option<&str>, observed: Value| {
        n += 1;
        input(
            order,
            &format!("{order}-rec-{n:02}"),
            n,
            part,
            segment,
            observed,
        )
    };
    let bound = || json!({"kind": "provider_bound"});
    let settled = |ev: &str| json!({"kind": "segment_settled", "evidence": ev});
    let mut inputs = vec![route_chosen(order, &format!("{order}-rec-00"))];
    let first_bound = next(Some("p1"), Some("p1-s1"), bound());
    let first_settled = next(Some("p1"), Some("p1-s1"), settled("ev-1"));
    inputs.extend([
        first_bound,
        first_settled.clone(),
        // The same input again: the log takes it as a repeat, the machine applies nothing.
        first_settled,
        next(Some("p2"), Some("p2-s1"), bound()),
        next(Some("p2"), None, json!({"kind": "withdrawal_cancelled"})),
        next(Some("p1"), Some("p1-s2"), bound()),
        next(Some("p1"), Some("p1-s2"), settled("ev-2")),
        next(Some("p1"), Some("p1-s3"), bound()),
        next(Some("p1"), Some("p1-s3"), settled("ev-3")),
        next(Some("p3"), Some("p3-s1"), bound()),
        next(Some("p3"), Some("p3-s1"), settled("ev-4")),
        next(
            Some("p3"),
            None,
            json!({"kind": "route_recalculated", "decision_record": "dr-0002", "segments": recalculated("p3")}),
        ),
        // A failure reported of the segment the recalculation retired: late.
        next(
            Some("p3"),
            Some("p3-s2"),
            json!({"kind": "segment_failed"}),
        ),
        next(Some("p3"), Some("p3-s4"), bound()),
        next(
            Some("p3"),
            Some("p3-s4"),
            json!({"kind": "segment_failed"}),
        ),
        next(
            Some("p3"),
            None,
            json!({"kind": "stuck_declared", "reason": "over_time"}),
        ),
        next(
            Some("p3"),
            None,
            json!({"kind": "stuck_resolved", "to": "destination", "evidence": "ev-5"}),
        ),
    ]);
    inputs
}

/// The entries of `history`, the repeat appended at a later moment than its first, as a retried append is.
fn entries_of(history: &[RecordedInput]) -> Vec<Entry> {
    let mut seen = std::collections::BTreeSet::new();
    history
        .iter()
        .map(|i| {
            if seen.insert(i.record_id.as_str().to_owned()) {
                entry_of(i)
            } else {
                entry_at(i, &time(1800))
            }
        })
        .collect()
}

/// The order after each input the machine applies, by the machine itself: each input, and the order after it.
/// Only inputs of the order `inputs[0]` opens are read.
fn truth(inputs: &[RecordedInput]) -> Vec<(RecordedInput, stm::Order)> {
    let mut order = Stm::open(&inputs[0]).expect("the route opens the order");
    let mut applied = vec![(inputs[0].clone(), order.clone())];
    for input in &inputs[1..] {
        if input.subject.order != inputs[0].subject.order {
            continue;
        }
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
    let histories: Vec<_> = orders
        .iter()
        .map(|(o, _)| entries_of(&history(o)))
        .collect();
    let longest = histories.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for h in &histories {
            if let Some(entry) = h.get(i) {
                entries.push(entry.clone());
            }
        }
    }
    entries
}

/// The refusals core-internal's own admission gives for the entries of `log` that none of the notices read: each
/// a citation of a record the test doesn't build (the quote shown, the reserve's yes, the user's session, the
/// router's decision record, the evidence, the stuck resolution's result, the statement an `afterwards` rests on,
/// the authorisation that sets the customer's floor).
const UNBUILT_CITATIONS: [&str; 9] = [
    "acceptance-is-of-the-quote-shown",
    "acceptance-rests-on-the-reserves-yes",
    "acceptance-is-asked-in-the-users-session",
    "input-cites-its-decision-record",
    "settlement-rests-on-its-evidence",
    "reversal-rests-on-its-evidence",
    "stuck-resolved-follows-a-done-resolution",
    "afterwards-rests-on-a-statement",
    "waiting-at-floor-rests-on-the-customers-floor",
];

/// Holds `log` to the log: every input in it is one the log takes from where its order stands
/// (`state_machine_takes`, the row `input-is-one-the-state-machine-takes`, which every input is held to before any
/// citation of its), and `admit_entry` refuses nothing in it but a citation of a record the test doesn't build,
/// which `refusals` names by its row: those rows are the only ones `admit_entry` may answer.
fn held_to_the_log(log: &[(Receipt, Entry)], refusals: &[(&str, &str)]) {
    let operator = DeploymentOperator {
        maker: id("operator-maker"),
        party: id(OPERATOR),
    };
    for (i, (_, entry)) in log.iter().enumerate() {
        if let Entry::Verified(v) = entry
            && let What::Input { input } = &v.what
        {
            assert!(
                state_machine_takes(&log[..i], input),
                "the log takes entry {i}, {:?}",
                input.record_id
            );
        }
        if let Err(violation) = admit_entry(&log[..i], entry, Through::General, &operator) {
            assert!(
                refusals.iter().any(|(_, text)| *text == violation.0),
                "entry {i} refused for a reason the notices read: {violation:?}"
            );
        }
    }
}

/// The refusal text of each unbuilt citation, as `admit_entry` words it, found by asking it.
fn unbuilt_refusals() -> Vec<(&'static str, &'static str)> {
    use rutent_contracts::core_internal::{CLOSING_RELATIONS, RELATIONS};
    RELATIONS
        .iter()
        .chain(CLOSING_RELATIONS.iter())
        .filter(|row| UNBUILT_CITATIONS.contains(&row.name))
        .flat_map(|row| {
            std::iter::once((row.name, row.refusal))
                .chain(row.fields.iter().map(move |f| (row.name, f.1)))
        })
        .collect()
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

/// An order moving through several states, a failed segment, a cancelled withdrawal, a recalculated route and a
/// stuck part among them: each input the state machine applies is one notice to the order's requester and one to
/// the operator, at sequences 1, 2, 3, ..., each holding the order's state and every part's state after it as the
/// machine has them, and the input's kind; a repeat and a late report, which the log takes and the machine doesn't
/// apply, are notified to no one. The log takes every input in it. Each receiver's inbox takes every one of its own
/// and ends at the order's final state.
// verifies: AC-41, REQ-ORD-20
#[test]
fn each_state_change_is_notified_to_the_user_and_the_operator() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    held_to_the_log(&log, &unbuilt_refusals());
    let applied = truth(&history("ord-1"));
    assert_eq!(
        applied.len(),
        history("ord-1").len() - 2,
        "a repeat and a late report aren't applied"
    );
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    assert_eq!(n.notify(&log, &mut key, &mut wire), Ok(2 * applied.len()));

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

    // The failures, the recalculation and the stuck part are among the changes, the stuck one with its reason.
    let kinds: Vec<Value> = wire
        .to(&operator())
        .iter()
        .map(|b| said(b)["about"]["change"].clone())
        .collect();
    for kind in [
        "segment_failed",
        "withdrawal_cancelled",
        "route_recalculated",
        "stuck_declared",
    ] {
        assert!(
            kinds.iter().any(|c| c["input"] == json!(kind)),
            "{kind} notified"
        );
    }
    assert_eq!(
        kinds
            .iter()
            .filter(|c| c["input"] == json!("segment_failed"))
            .count(),
        1,
        "the late failure is notified to no one"
    );
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
    assert_eq!(n.notify(&log, &mut key, &mut wire), Ok(0));
}

/// Every kind of input the state-machine contract has is a change notified to the customer and the operator, each
/// with its kind and its part, in a log the log takes: an order ended on request for the part not past its first
/// segment, a route recalculated, a part waiting at its floor whose customer then changes its destination, a
/// segment of unknown outcome then reversed, a part with no route left, stuck, then handed on to a linked order. The
/// hand-on carries the linked order's id, and the linked order, accepted by the same customer, is notified to it
/// from its own first change.
// verifies: AC-41, REQ-ORD-20, REQ-ORD-43
#[test]
fn every_kind_of_input_is_notified() {
    let (order, linked, refused) = ("ord-k", "ord-k2", "ord-k3");
    let mut n = 0;
    let mut next = |part: Option<&str>, segment: Option<&str>, observed: Value| {
        n += 1;
        input(order, &format!("k-rec-{n:02}"), n, part, segment, observed)
    };
    let bound = || json!({"kind": "provider_bound"});
    let settled = |ev: &str| json!({"kind": "segment_settled", "evidence": ev});
    // After the recalculated route's exchange settled and its final deposit failed, the value sits as USDC at
    // `ledger-in`: the new destination starts from there (`INV-10`).
    let new_destination = vec![
        segment(
            "p1-s6",
            "exchange-b",
            "exchange",
            end("ledger-in", false, "USDC"),
            end("ledger-in", false, "COP"),
        ),
        segment(
            "p1-s7",
            "reserve-bank-co",
            "deposit",
            end("ledger-in", false, "COP"),
            end("bank-co", true, "COP"),
        ),
    ];
    let inputs = vec![
        route_chosen(order, "k-rec-00"),
        next(Some("p1"), Some("p1-s1"), bound()),
        next(Some("p1"), Some("p1-s1"), settled("ev-k1")),
        next(Some("p2"), Some("p2-s1"), bound()),
        next(Some("p2"), Some("p2-s1"), settled("ev-k2")),
        next(None, None, json!({"kind": "end_requested"})),
        next(
            Some("p1"),
            None,
            json!({"kind": "route_recalculated", "decision_record": "dr-k2", "segments": recalculated("p1")}),
        ),
        next(Some("p1"), Some("p1-s4"), bound()),
        next(Some("p1"), Some("p1-s4"), settled("ev-k5")),
        next(Some("p1"), Some("p1-s5"), bound()),
        next(Some("p1"), Some("p1-s5"), json!({"kind": "segment_failed"})),
        next(Some("p1"), None, json!({"kind": "waiting_at_floor"})),
        next(
            Some("p1"),
            None,
            json!({"kind": "destination_changed", "decision_record": "dr-k3", "acceptance": "acc-ord-k-dest", "segments": new_destination}),
        ),
        next(Some("p2"), Some("p2-s2"), bound()),
        next(
            Some("p2"),
            Some("p2-s2"),
            json!({"kind": "segment_outcome_unknown"}),
        ),
        next(
            Some("p2"),
            Some("p2-s2"),
            json!({"kind": "reversal_completed", "evidence": "ev-k4"}),
        ),
        next(
            Some("p2"),
            None,
            json!({"kind": "no_route", "decision_record": "dr-k4"}),
        ),
        next(
            Some("p2"),
            None,
            json!({"kind": "handed_on", "linked_order": linked, "acceptance": "acc-ord-k2"}),
        ),
        // The first part again, stuck and handed on to an order that is then refused before it opens: the hand-on is taken
        // back and the part is stuck again.
        next(
            Some("p1"),
            None,
            json!({"kind": "stuck_declared", "reason": "over_time"}),
        ),
        next(
            Some("p1"),
            None,
            json!({"kind": "handed_on", "linked_order": refused, "acceptance": "acc-ord-k3"}),
        ),
        next(
            Some("p1"),
            None,
            json!({"kind": "hand_on_taken_back", "linked_order": refused}),
        ),
    ];
    let linked_route: RecordedInput = serde_json::from_value(json!({
        "record_id": "k2-rec-00",
        "recorded_at": time(40),
        "time_source": "core-orch clock",
        "source": {"kind": "customer", "id": "alice-login"},
        "standing": "stated",
        "subject": {"order": linked},
        "observed": {
            "kind": "route_chosen",
            "decision_record": "dr-k5",
            "parts": [{"part_id": "p1", "segments": [segment(
                "p1-s1", "reserve-bank-co", "deposit", end("ledger-in", false, "COP"), end("bank-co", true, "COP"),
            )]}],
            "handed_on_from": order
        }
    }))
    .expect("the linked order's route");

    let mut entries = vec![
        acceptance(order, ALICE),
        acceptance_of("acc-ord-k-dest", order, ALICE, &["p1"]),
        acceptance_of("acc-ord-k2", linked, ALICE, &["p1"]),
    ];
    for input in &inputs {
        if matches!(&input.observed, stm::Observed::HandedOn { linked_order, .. } if linked_order.as_str() == refused)
        {
            entries.push(acceptance_of("acc-ord-k3", refused, ALICE, &["p1"]));
        }
        if matches!(input.observed, stm::Observed::HandOnTakenBack { .. }) {
            // The linked order's refusal, which a take-back rests on.
            entries.push(verified(
                json!({"call": "order_refusal", "request": "req-ord-k3", "order": refused, "class": "refused_by_policy"}),
                &time(39),
            ));
        }
        entries.push(entry_of(input));
    }
    entries.push(entry_of(&linked_route));
    let log = receipted(entries);
    held_to_the_log(&log, &unbuilt_refusals());
    let applied = truth(&inputs);
    assert_eq!(applied.len(), inputs.len(), "the machine applies every one");

    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&log, &mut key, &mut wire)
        .expect("notified");
    let to_alice: Vec<Value> = wire.to(&customer(ALICE)).iter().map(|b| said(b)).collect();
    assert_eq!(
        to_alice.len(),
        applied.len() + 1,
        "every change, and the linked order's"
    );
    assert_eq!(wire.to(&operator()).len(), applied.len() + 1);
    for kind in [
        "route_recalculated",
        "destination_changed",
        "handed_on",
        "hand_on_taken_back",
        "stuck_declared",
        "end_requested",
        "waiting_at_floor",
        "no_route",
        "reversal_completed",
        "segment_outcome_unknown",
    ] {
        let notice = to_alice
            .iter()
            .find(|n| n["about"]["change"]["input"] == json!(kind))
            .unwrap_or_else(|| panic!("{kind} notified"));
        let (_, after) = applied
            .iter()
            .find(|(i, _)| {
                serde_json::to_value(&i.observed).expect("observed")["kind"] == json!(kind)
            })
            .expect("applied");
        assert_eq!(
            notice["about"]["view"]["state"],
            state_name(after.state),
            "{kind}"
        );
    }
    let handed = to_alice
        .iter()
        .find(|n| n["about"]["change"]["input"] == json!("handed_on"))
        .expect("the hand-on");
    let taken_back = to_alice
        .iter()
        .find(|n| n["about"]["change"]["input"] == json!("hand_on_taken_back"))
        .expect("the take-back");
    assert_eq!(
        taken_back["about"]["change"],
        json!({"change": "input", "input": "hand_on_taken_back", "part": "p1", "linked_order": refused})
    );
    assert_eq!(taken_back["about"]["order"], json!(order));
    assert_eq!(
        to_alice
            .iter()
            .filter(|n| n["about"]["order"] == json!(refused))
            .count(),
        0,
        "the refused linked order never opened and is notified of nothing"
    );
    assert_eq!(
        handed["about"]["change"],
        json!({"change": "input", "input": "handed_on", "part": "p2", "linked_order": linked})
    );
    let ended = to_alice
        .iter()
        .find(|n| n["about"]["change"]["input"] == json!("end_requested"))
        .expect("the end");
    assert_eq!(
        ended["about"]["change"],
        json!({"change": "input", "input": "end_requested"})
    );
    let first_of_linked = to_alice
        .iter()
        .find(|n| n["about"]["order"] == json!(linked))
        .expect("the linked order is notified to the customer");
    assert_eq!(first_of_linked["about"]["sequence"], json!(1));
}

/// An input recorded at a time with an offset from UTC, a fraction of a second beyond nanoseconds, or both, as
/// the state-machine contract allows, is notified like any other, and the order is followed on past it: every later
/// change is notified too.
// verifies: AC-41, REQ-ORD-20
#[test]
fn input_recorded_with_an_offset_is_notified_and_the_order_followed_on() {
    let mut inputs = history("ord-1");
    inputs[1].recorded_at = stm::Timestamp::new("2026-11-12T11:00:01+01:00").expect("a time");
    inputs[4].recorded_at = stm::Timestamp::new("2026-11-12T10:00:04.1234567891Z").expect("a time");
    let mut entries = vec![acceptance("ord-1", ALICE)];
    entries.extend(entries_of(&inputs));
    let applied = truth(&inputs);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&receipted(entries), &mut key, &mut wire)
        .expect("notified");
    let got = wire.to(&customer(ALICE));
    assert_eq!(got.len(), applied.len());
    assert_eq!(
        said(&got[1])["about"]["at"],
        json!("2026-11-12T11:00:01+01:00")
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
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&log, &mut key, &mut wire)
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
            &log,
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
        let log = receipted(log_of(&[("ord-1", ALICE)]));
        let applied = truth(&history("ord-1"));
        let (mut key, mut wire) = (TestKey::new(), Wire::default());
        notices().notify(&log, &mut key, &mut wire).expect("notified");
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
/// is the order's latest change, as the log holds it to its end, sealed for the asker and marked as the current
/// state, which its inbox takes in place of those lost, closing the gap. Asking sends nothing to anyone: the
/// changes not yet notified go out on the next read of the log, to each of their receivers. The operator asks
/// through a login policy lists, and the answer is addressed to the deployment's operator, whatever party the login
/// states only when it is the operator's own party. A log that doesn't extend the one read gives no answer.
// verifies: REQ-ORD-20
#[test]
fn receiver_can_ask_for_the_current_state() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    let applied = truth(&history("ord-1"));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&log[..4], &mut key, &mut wire)
        .expect("the first changes");
    let mut alice = inbox(customer(ALICE), &key);
    alice
        .take(&Ed25519, &wire.to(&customer(ALICE))[0])
        .expect("the first");
    let sent = wire.sent.len();
    let mut policy = Policy::of(&[("ord-1", ALICE)]);
    let (clock, mut ids) = (At(NOW), Ids::default());

    let answer = n
        .current(
            &log,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
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
        !alice.missing(&id("ord-1")),
        "the current state closes the gap"
    );
    assert_eq!(policy.checks.len(), 1);
    assert_eq!(
        policy.checks[0].0.reads,
        Read::OrderState { order: id("ord-1") }
    );
    assert_eq!(wire.sent.len(), sent, "asking sent nothing");

    let desk = Actor {
        party: id(OPERATOR),
        acts: id("desk-login"),
        role: Role::Operator,
        kind: ActorKind::Person,
    };
    let answer = n
        .current(
            &log,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
            &desk,
            None,
            &id("ord-1"),
        )
        .expect("the operator reads the order's state");
    assert_eq!(
        said(&answer)["to"],
        serde_json::to_value(operator()).expect("a receiver")
    );
    assert_eq!(
        inbox(operator(), &key).take(&Ed25519, &answer),
        Ok(Taken::Later)
    );

    // The changes not notified yet go out on the next read, to both receivers, once each.
    let rest = n.notify(&log, &mut key, &mut wire).expect("notified");
    assert_eq!(rest, 2 * applied.len() - sent);

    let shorter = n.current(
        &log[..3],
        &mut policy,
        &clock,
        &mut ids,
        &mut key,
        &actor(ALICE, Role::Customer),
        None,
        &id("ord-1"),
    );
    assert_eq!(shorter, Err(CurrentError::NotThisLog));
}

/// A stranger asking for an order's state is refused alike whether the order exists or not: policy's refusal,
/// whose class differs between the two (a missing input for an order no record holds, not permitted for another's),
/// stays in the log, and the stranger is told one refusal, so it can't learn which orders exist.
// verifies: REQ-AUZ-7, REQ-ORD-20
#[test]
fn stranger_learns_nothing_of_whether_an_order_exists() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    let mut n = notices();
    let mut key = TestKey::new();
    n.notify(&log, &mut key, &mut Wire::default())
        .expect("notified");
    let mut policy = Policy::of(&[("ord-1", ALICE)]);
    let (clock, mut ids) = (At(NOW), Ids::default());
    let mut ask = |order: &str| {
        n.current(
            &log,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
            &actor(BOB, Role::Customer),
            None,
            &id(order),
        )
    };
    let (theirs, none) = (ask("ord-1"), ask("ord-none"));
    assert_eq!(theirs, Err(CurrentError::Refused));
    assert_eq!(theirs, none, "the two refusals are one and the same");
    let classes: Vec<_> = policy
        .checks
        .iter()
        .map(|(c, allowed)| (c.reads.clone(), *allowed))
        .collect();
    assert_eq!(classes.len(), 2, "both reads recorded");
}

/// Two orders of two requesters in one log: each requester receives its own order's notices and no other's, and
/// the operator receives both; a raise and a charge counted ahead reach the operator alone. Alice's notices name
/// no provider, segment or amount.
// verifies: REQ-AUZ-7, REQ-ORD-20
#[test]
fn each_customer_receives_only_its_own_orders_notices() {
    let log = receipted(log_of(&[("ord-1", ALICE), ("ord-2", BOB)]));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&log, &mut key, &mut wire).expect("notified");
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
    let log = receipted(entries_of(&history("ord-9")));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&log, &mut key, &mut wire).expect("notified");
    assert_eq!(wire.sent.len(), truth(&history("ord-9")).len());
    assert!(wire.sent.iter().all(|(to, _)| to == &operator()));
    assert_eq!(n.receivers(&id("ord-9")), vec![operator()]);
}

/// A destination's report that a complete order's payment came back, and an approval arriving after the order
/// ended, leave its state as it was and are notified each as the order's next change; so is a refusal naming the
/// order. An input the log recorded as not taken is notified to no one. The log takes each of them.
// verifies: REQ-ORD-31, REQ-ORD-25, REQ-ORD-20
#[test]
fn what_comes_after_an_order_ended_is_notified() {
    let mut entries = log_of(&[("ord-1", ALICE)]);
    let applied = truth(&history("ord-1")).len();
    let statement = serde_json::to_value(receipt("log-main", 9, &Value::Null)).expect("a receipt");
    entries.push(verified(
        json!({"call": "afterwards", "order": "ord-1", "event": "returned", "statement": statement}),
        &time(90),
    ));
    entries.push(verified(
        json!({"call": "afterwards", "order": "ord-1", "event": "late_approval", "statement": statement}),
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
        Some("p2"),
        Some("p2-s1"),
        json!({"kind": "provider_bound"}),
    );
    entries.push(verified(
        json!({"call": "input_not_taken", "input": not_taken, "reason": {"kind": "not_applicable", "reason": "ended"}}),
        &time(93),
    ));
    let log = receipted(entries);
    held_to_the_log(&log, &unbuilt_refusals());
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&log, &mut key, &mut wire)
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
    let log = receipted(log_of(&[("ord-1", ALICE), ("ord-2", BOB)]));
    let mut key = TestKey::new();
    let (mut first, mut again, mut pieces) = (Wire::default(), Wire::default(), Wire::default());
    notices()
        .notify(&log, &mut key, &mut first)
        .expect("notified");
    notices()
        .notify(&log, &mut key, &mut again)
        .expect("notified");
    let mut n = notices();
    for end in [3, 10, 11, log.len()] {
        n.notify(&log[..end], &mut key, &mut pieces)
            .expect("notified");
    }
    assert_eq!(first.sent.len(), 2 * 2 * truth(&history("ord-1")).len());
    assert_eq!(first.sent, again.sent);
    assert_eq!(first.sent, pieces.sent);
}

/// Entries that don't extend those read before are refused whole: a shorter log; another log, whose entry where the
/// read stopped is another; and the same entries as another log holds them. Only the last entry read is compared,
/// by its receipt, so the check costs the same however long the log. A key that can't sign sends nothing of the
/// entry, and the next read carries on from it; a notice the transport didn't take is kept for its retry.
// verifies: REQ-ORD-20
#[test]
fn another_log_or_a_locked_key_sends_nothing_amiss() {
    let entries = log_of(&[("ord-1", ALICE)]);
    let log = receipted(entries.clone());
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&log[..5], &mut key, &mut wire).expect("notified");
    let sent = wire.sent.len();
    assert_eq!(
        n.notify(&log[..3], &mut key, &mut wire),
        Err(NoticeError::NotThisLog)
    );
    // Bob's log, whose first entry, the acceptance, is another.
    let other = receipted(log_of(&[("ord-1", BOB)]));
    let mut first = notices();
    first
        .notify(&log[..1], &mut key, &mut Wire::default())
        .expect("notified");
    assert_eq!(
        first.notify(&other, &mut key, &mut wire),
        Err(NoticeError::NotThisLog)
    );
    let elsewhere = receipted_in("log-other", entries);
    assert_eq!(
        n.notify(&elsewhere, &mut key, &mut wire),
        Err(NoticeError::NotThisLog)
    );
    assert_eq!(
        n.notify(&log, &mut Locked, &mut wire),
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
    n.notify(&log[..7], &mut key, &mut down).expect("notified");
    assert_eq!(
        n.undelivered().len(),
        4,
        "two changes, to both receivers, kept"
    );
    n.notify(&log, &mut key, &mut wire).expect("notified");
    let mut fresh = Wire::default();
    notices()
        .notify(&log, &mut key, &mut fresh)
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
/// inbox holds each raise once however often it comes, and Core doesn't send a delivered raise again.
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
    let mut other = raise.clone();
    other.raise = id("raise-2");
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
        .raise(&other),
        Delivery::Undelivered
    );
    assert_eq!(n.undelivered().len(), 1);
    assert_eq!(n.undelivered()[0].to, operator());

    assert_eq!(wire.sent.len(), 1, "a raise delivered is not sent again");
    assert!(wire.sent.iter().all(|(to, _)| to == &operator()));
    let mut op = inbox(operator(), &key);
    assert_eq!(op.take(&Ed25519, &wire.sent[0].1), Ok(Taken::Event));
    assert_eq!(op.take(&Ed25519, &wire.sent[0].1), Ok(Taken::Repeat));
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
/// handed over again sends nothing more, and a new one is sent. What is kept of them is what policy still sets
/// apart: a charge no longer handed over is forgotten, and, set apart again later, sent again, which the operator's
/// inbox holds once.
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
    assert_eq!(n.ahead(&[], &clock, &mut key, &mut wire), Ok(0));
    assert_eq!(
        n.ahead(&first, &clock, &mut key, &mut wire),
        Ok(1),
        "forgotten, so sent again"
    );
    let again = &wire.sent[wire.sent.len() - 1].1;
    assert_eq!(op.take(&Ed25519, again), Ok(Taken::Repeat));
}

/// A clock that can't give a time now (past the year 9999) answers so: no charge is noticed and no current state
/// given on it, and nothing is taken for a key that can't sign.
// verifies: REQ-ORD-20
#[test]
fn clock_without_a_time_is_told_apart_from_a_locked_key() {
    let past_9999 = At(253_402_300_800);
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    assert_eq!(
        n.ahead(
            &[ahead_charge(BOB, "chg-1")],
            &past_9999,
            &mut key,
            &mut wire
        ),
        Err(NoticeError::NoTime)
    );
    assert!(wire.sent.is_empty());
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    n.notify(&log, &mut key, &mut wire).expect("notified");
    let mut policy = Policy::of(&[("ord-1", ALICE)]);
    let asked = n.current(
        &log,
        &mut policy,
        &past_9999,
        &mut Ids::default(),
        &mut key,
        &actor(ALICE, Role::Customer),
        None,
        &id("ord-1"),
    );
    assert_eq!(asked, Err(CurrentError::NoTime));
    let locked = n.current(
        &log,
        &mut policy,
        &At(NOW),
        &mut Ids::default(),
        &mut Locked,
        &actor(ALICE, Role::Customer),
        None,
        &id("ord-1"),
    );
    assert_eq!(locked, Err(CurrentError::Unsigned));
}

/// A notice's signing input starts with its own domain, which no canonical JSON object starts with, so the
/// deployment's signature over a notice never verifies as a provider-role statement's.
// verifies: REQ-ORD-20
#[test]
fn notice_signing_input_is_apart_from_every_statements() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    notices()
        .notify(&log, &mut key, &mut wire)
        .expect("notified");
    let sealed: Value = serde_json::from_slice(&wire.sent[0].1).expect("json");
    let notice = serde_json::from_value(sealed["notice"].clone()).expect("a notice");
    let input = signing_input(&notice).expect("an input");
    assert!(input.starts_with(rutent_core_orch::notice::DOMAIN));
    assert_ne!(input[0], b'{');
}

// --- Delivery and its watch (T-023-02) ---------------------------------------------------------------------------

/// A durable ledger a test keeps beside the notices: it outlives them, as a store outlives a restart.
#[derive(Debug, Clone, Default)]
struct Kept(Rc<RefCell<Vec<Attempt>>>);

impl Kept {
    fn history(&self) -> Vec<Attempt> {
        self.0.borrow().clone()
    }
}

impl Ledger for Kept {
    fn delivered(&self, key: &Key) -> bool {
        self.0.borrow().iter().any(|a| &a.key == key && a.taken)
    }

    fn attempts(&self, key: &Key) -> u32 {
        self.0
            .borrow()
            .iter()
            .filter(|a| &a.key == key)
            .map(|a| a.number)
            .max()
            .unwrap_or(0)
    }

    fn record(&mut self, attempt: &Attempt) -> bool {
        self.0.borrow_mut().push(attempt.clone());
        true
    }
}

fn resuming(ledger: &Kept) -> Notices {
    Notices::resuming(id(DEPLOYMENT), id(OPERATOR), Box::new(ledger.clone()))
}

/// How many attempts the ledger holds with this number, and this outcome.
fn attempts_numbered(ledger: &Kept, number: u32, taken: bool) -> usize {
    ledger
        .history()
        .iter()
        .filter(|a| a.number == number && a.taken == taken)
        .count()
}

/// A notice the transport doesn't take is kept in order and retried, each try recorded in the ledger; a pass of
/// retrying that the transport takes delivers them, in the order they were sent, and the receivers' inboxes end at
/// the order's state. The bound is the ledger's [`BOUND`] tries in all, the first send included: a notice that used
/// them is abandoned, no longer sent, whatever the transport does after, and the record says how many tries it had.
// verifies: REQ-ORD-20
#[test]
fn undelivered_notice_is_retried_and_recorded() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    held_to_the_log(&log, &unbuilt_refusals());
    let applied = truth(&history("ord-1"));
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let ledger = Kept::default();
    let mut n = resuming(&ledger);

    // The first send is refused: every notice is kept, in order, and its try recorded.
    wire.down = true;
    let first = n.notify(&log[..4], &mut key, &mut wire).expect("notified");
    assert_eq!(first, 6, "three changes, two receivers each");
    assert_eq!(n.undelivered().len(), 6);
    assert_eq!(wire.tried, 6);
    assert_eq!(attempts_numbered(&ledger, 1, false), 6);
    assert!(n.abandoned().is_empty());

    // A retry the transport still refuses is a second try, recorded; nothing is lost or reordered.
    let again = n.retry(&mut wire);
    assert_eq!(
        again,
        Retried {
            delivered: 0,
            failed: 6,
            abandoned: 0
        }
    );
    assert_eq!(wire.tried, 12);
    assert_eq!(attempts_numbered(&ledger, 2, false), 6);

    // Taken at last: delivered in the order they were sent, recorded as taken, and the inboxes agree.
    wire.down = false;
    let done = n.retry(&mut wire);
    assert_eq!(
        done,
        Retried {
            delivered: 6,
            failed: 0,
            abandoned: 0
        }
    );
    assert!(n.undelivered().is_empty());
    assert_eq!(attempts_numbered(&ledger, 3, true), 6);
    let mut alice = inbox(customer(ALICE), &key);
    for bytes in wire.to(&customer(ALICE)) {
        alice.take(&Ed25519, &bytes).expect("taken");
    }
    let sequences: Vec<_> = wire
        .to(&customer(ALICE))
        .iter()
        .map(|b| said(b)["about"]["sequence"].as_u64().expect("sequence"))
        .collect();
    assert_eq!(sequences, vec![1, 2, 3], "retried in the order sent");
    assert_eq!(alice.view(&id("ord-1")).map(|(s, _)| s), Some(3));
    assert_eq!(
        alice.view(&id("ord-1")).map(|(_, v)| v.state),
        Some(applied[2].1.state)
    );
    assert_eq!(
        n.retry(&mut wire),
        Retried::default(),
        "nothing is left to retry"
    );

    // The bound: the rest of the log, sent to a path that stays down, is tried BOUND times and then given up.
    wire.down = true;
    let rest = n.notify(&log, &mut key, &mut wire).expect("notified");
    assert!(rest > 0);
    for _ in 1..BOUND - 1 {
        let pass = n.retry(&mut wire);
        assert_eq!((pass.delivered, pass.failed, pass.abandoned), (0, rest, 0));
    }
    let last = n.retry(&mut wire);
    assert_eq!((last.delivered, last.failed, last.abandoned), (0, 0, rest));
    assert!(n.undelivered().is_empty());
    assert_eq!(n.abandoned().len(), rest);
    assert!(n.abandoned().iter().all(|u| u.attempts == BOUND));
    assert_eq!(attempts_numbered(&ledger, BOUND, false), rest);
    let tried = wire.tried;
    wire.down = false;
    assert_eq!(n.retry(&mut wire), Retried::default());
    assert_eq!(wire.tried, tried, "an abandoned notice is sent no more");
    assert!(
        ledger.history().iter().all(|a| a.number <= BOUND),
        "no try beyond the bound is recorded"
    );
}

/// After a restart the notices are built afresh from the log, with the ledger they left: what the ledger holds as
/// delivered is not sent again, what it doesn't is sent, its tries counted on from the record, and a notice that
/// used the bound before the restart is not given a fresh one.
// verifies: REQ-ORD-20
#[test]
fn rebuild_after_restart_sends_only_what_was_not_delivered() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    held_to_the_log(&log, &unbuilt_refusals());
    let applied = truth(&history("ord-1"));
    let mut key = TestKey::new();
    let ledger = Kept::default();

    // Before the restart: the first three changes go through, the rest meet a path that is down.
    let mut before = Wire::default();
    let mut n = resuming(&ledger);
    n.notify(&log[..4], &mut key, &mut before)
        .expect("notified");
    assert_eq!(before.sent.len(), 6);
    before.down = true;
    let rest = n.notify(&log, &mut key, &mut before).expect("notified");
    assert_eq!(rest, 2 * applied.len() - 6);
    drop(n);

    // The restart: the same log, the same ledger, a path that is up.
    let mut after = Wire::default();
    let mut n = resuming(&ledger);
    let sent = n.notify(&log, &mut key, &mut after).expect("caught up");
    assert_eq!(sent, rest, "only what was not delivered is sent");
    assert_eq!(after.sent.len(), rest);
    for (_, bytes) in &after.sent {
        let sequence = said(bytes)["about"]["sequence"].as_u64().expect("sequence");
        assert!(sequence > 3, "sequence {sequence} was delivered before");
    }
    assert_eq!(
        attempts_numbered(&ledger, 2, true),
        rest,
        "the tries go on from the record"
    );
    let mut alice = inbox(customer(ALICE), &key);
    for bytes in before
        .to(&customer(ALICE))
        .iter()
        .chain(&after.to(&customer(ALICE)))
    {
        alice.take(&Ed25519, bytes).expect("taken");
    }
    assert_eq!(
        alice.view(&id("ord-1")).map(|(s, _)| s),
        Some(applied.len() as u64)
    );
    assert!(!alice.missing(&id("ord-1")));

    // A restart again changes nothing: all is delivered.
    let mut third = Wire::default();
    let mut n = resuming(&ledger);
    assert_eq!(n.notify(&log, &mut key, &mut third), Ok(0));
    assert!(third.sent.is_empty());

    // A notice that used the bound before a restart is not tried again after it.
    let spent = Kept::default();
    let mut down = Wire {
        down: true,
        ..Wire::default()
    };
    let mut n = resuming(&spent);
    let all = n.notify(&log[..2], &mut key, &mut down).expect("notified");
    assert_eq!(all, 2);
    for _ in 1..BOUND {
        n.retry(&mut down);
    }
    assert_eq!(n.abandoned().len(), 2);
    let mut up = Wire::default();
    let mut n = resuming(&spent);
    assert_eq!(n.notify(&log[..2], &mut key, &mut up), Ok(0));
    assert!(up.sent.is_empty(), "no fresh bound after a restart");
    assert_eq!(
        n.abandoned().len(),
        2,
        "and it is still said to be given up"
    );
}

/// A login in the operator's role whose stated party is not the deployment's operator is answered like anyone
/// else: sealed for the party it states, in its role, which the operator's inbox does not take. Only the operator's
/// own party is answered as the operator.
// verifies: REQ-ORD-20
#[test]
fn operator_login_of_another_party_is_not_answered_as_the_operator() {
    let log = receipted(log_of(&[("ord-1", ALICE)]));
    held_to_the_log(&log, &unbuilt_refusals());
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let mut n = notices();
    n.notify(&log, &mut key, &mut wire).expect("notified");
    let mut policy = Policy::of(&[("ord-1", ALICE)]);
    let (clock, mut ids) = (At(NOW), Ids::default());
    let public = TestKey::new();
    let mut ask = |party: &str| {
        let reader = Actor {
            party: id(party),
            acts: id("desk-login"),
            role: Role::Operator,
            kind: ActorKind::Person,
        };
        n.current(
            &log,
            &mut policy,
            &clock,
            &mut ids,
            &mut key,
            &reader,
            None,
            &id("ord-1"),
        )
        .expect("policy lists the login")
    };

    let own = ask(OPERATOR);
    assert_eq!(inbox_take(&own, operator(), &public), Ok(Taken::Later));
    let other = ask("operator-desk");
    let desk = Receiver {
        party: id("operator-desk"),
        role: Role::Operator,
    };
    assert_eq!(
        said(&other)["to"],
        serde_json::to_value(&desk).expect("a receiver")
    );
    assert_eq!(
        inbox_take(&other, operator(), &public),
        Err(NotTaken::ForAnother),
        "the operator does not receive what was sealed for another party"
    );
    assert_eq!(inbox_take(&other, desk, &public), Ok(Taken::Later));
}

fn inbox_take(bytes: &[u8], me: Receiver, key: &TestKey) -> Result<Taken, NotTaken> {
    inbox(me, key).take(&Ed25519, bytes)
}

/// A clock a test moves.
struct Moving(Cell<u64>);

impl Clock for Moving {
    fn now(&self) -> Reading {
        Reading::at(self.0.get())
    }
}

/// The operator's warning path with its watch: the path watched (`warn`), a second path (`second`), the operator's
/// inbox on each, and the operator's side answering each heartbeat it takes.
struct Rig {
    clock: Moving,
    watch: Watch,
    key: TestKey,
    warn: Wire,
    second: Wire,
    on_warn: Inbox,
    on_second: Inbox,
    seen: usize,
}

impl Rig {
    fn new() -> Self {
        let key = TestKey::new();
        let clock = Moving(Cell::new(NOW));
        Rig {
            watch: Watch::new(id(DEPLOYMENT), id(OPERATOR), id("run-1"), &clock),
            on_warn: inbox(operator(), &key),
            on_second: inbox(operator(), &key),
            clock,
            key,
            warn: Wire::default(),
            second: Wire::default(),
            seen: 0,
        }
    }

    /// The watch ticks `seconds` after the start, then the operator's side answers what reached it.
    fn tick(&mut self, seconds: u64) -> rutent_core_orch::notice::Tick {
        self.clock.0.set(NOW + seconds);
        let tick = self
            .watch
            .tick(&self.clock, &mut self.key, &mut self.warn, &mut self.second)
            .expect("ticks");
        while self.seen < self.warn.sent.len() {
            let bytes = self.warn.sent[self.seen].1.clone();
            if self.on_warn.take(&Ed25519, &bytes) == Ok(Taken::Beat)
                && let Some((run, beat)) = self.on_warn.beat()
            {
                self.watch.answered(run, beat, &self.clock);
            }
            self.seen += 1;
        }
        tick
    }

    /// What the second path carried, as the operator's inbox there takes it.
    fn told(&mut self) -> usize {
        let sent: Vec<_> = self.second.sent.iter().map(|(_, b)| b.clone()).collect();
        sent.iter()
            .filter(|b| self.on_second.take(&Ed25519, b) == Ok(Taken::Event))
            .count()
    }
}

/// The path that warns the operator has a heartbeat of its own: sent every interval on that path, answered by the
/// operator's side. While it answers, nothing is said on the second path and a beat is not early. Cut it, whether
/// it refuses what it is given or takes it and loses it, and once it has been silent for [`SILENCE`] seconds the
/// cut is noticed on the second path, sealed for the operator alone, and once; a path that answers again is alive,
/// and a later cut is noticed again.
// verifies: REQ-ORD-7
#[test]
fn cut_warning_path_is_noticed() {
    for silent in [true, false] {
        let mut rig = Rig::new();

        // Alive: a beat each interval, none early, every one answered, and the second path says nothing.
        for seconds in (0..=300).step_by(30) {
            let tick = rig.tick(seconds);
            assert!(tick.beat.is_some(), "a beat is due at {seconds}");
            assert_eq!(tick.path, Path::Alive);
            assert!(!tick.told);
            assert_eq!(rig.tick(seconds + 10).beat, None, "not early");
        }
        assert_eq!(rig.on_warn.beat().map(|(_, b)| b), Some(11));
        assert!(rig.second.sent.is_empty());

        // Cut at 300 s, the last answer: no alarm before the silence has lasted.
        rig.warn.silent = silent;
        rig.warn.down = !silent;
        for seconds in (330..=419).step_by(30) {
            assert_eq!(rig.tick(seconds).path, Path::Alive, "{seconds} s");
        }
        assert_eq!(rig.tick(419).path, Path::Alive);
        assert!(rig.second.sent.is_empty(), "no false alarm");

        // The silence has lasted: noticed, on the second path, once.
        let found = rig.tick(300 + SILENCE);
        assert_eq!(found.path, Path::Cut { since: NOW + 300 });
        assert!(found.told);
        assert_eq!(rig.second.sent.len(), 1);
        assert_eq!(rig.second.sent[0].0, operator());
        assert_eq!(rig.told(), 1, "the operator takes it, checked");
        let cut = said(&rig.second.sent[0].1);
        assert_eq!(cut["about"]["about"], "path_cut");
        assert_eq!(cut["about"]["silent_for"], SILENCE);
        assert_eq!(
            inbox_take(&rig.second.sent[0].1, customer(ALICE), &rig.key),
            Err(NotTaken::ForAnother),
            "the operator's alone"
        );
        for seconds in [450, 480, 510] {
            let tick = rig.tick(seconds);
            assert_eq!(tick.path, Path::Cut { since: NOW + 300 });
            assert!(!tick.told);
        }
        assert_eq!(rig.second.sent.len(), 1, "told once");

        // Answering again ends the cut; a later cut is noticed again.
        rig.warn.silent = false;
        rig.warn.down = false;
        rig.tick(540);
        assert_eq!(rig.tick(570).path, Path::Alive);
        rig.warn.silent = silent;
        rig.warn.down = !silent;
        let mut told_at = None;
        for seconds in (600..=800).step_by(30) {
            if rig.tick(seconds).told {
                told_at = Some(seconds);
                break;
            }
        }
        assert_eq!(told_at, Some(570 + SILENCE));
        assert_eq!(rig.second.sent.len(), 2);
    }
}

/// The second path may be down when the cut is found: the operator is told once it takes the notice, not before
/// and not twice.
// verifies: REQ-ORD-7
#[test]
fn cut_is_told_again_until_the_second_path_takes_it() {
    let mut rig = Rig::new();
    rig.warn.silent = true;
    rig.second.down = true;
    let mut found = None;
    for seconds in (0..=400).step_by(30) {
        let tick = rig.tick(seconds);
        if matches!(tick.path, Path::Cut { .. }) && found.is_none() {
            found = Some(seconds);
            assert!(!tick.told, "the second path refused");
        }
    }
    assert!(found.is_some());
    assert!(rig.second.sent.is_empty());
    rig.second.down = false;
    assert!(rig.tick(420).told);
    assert!(!rig.tick(450).told);
    assert_eq!(rig.second.sent.len(), 1);
}

/// A raise the path refused is kept and retried like a notice: each try recorded in the ledger under the raise's
/// id, the operator's inbox taking it once it is delivered, and a raise that used the bound is abandoned.
// verifies: REQ-ORD-20
#[test]
fn undelivered_raise_is_retried_and_recorded() {
    let raise: Raise = serde_json::from_str(include_str!(
        "../../../contracts/core-internal/fixtures/valid/raise-of-a-change-in-a-providers-p99.json"
    ))
    .expect("a raise");
    let (mut key, mut wire) = (TestKey::new(), Wire::default());
    let ledger = Kept::default();
    let mut n = resuming(&ledger);
    wire.down = true;
    assert_eq!(
        Channel {
            notices: &mut n,
            key: &mut key,
            transport: &mut wire
        }
        .raise(&raise),
        Delivery::Undelivered
    );
    let named = Key::Raise {
        raise: raise.raise.as_str().to_owned(),
    };
    assert_eq!(ledger.attempts(&named), 1);
    assert!(!ledger.delivered(&named));
    assert_eq!(n.retry(&mut wire).failed, 1);
    assert_eq!(ledger.attempts(&named), 2);

    wire.down = false;
    assert_eq!(n.retry(&mut wire).delivered, 1);
    assert!(ledger.delivered(&named));
    assert_eq!(ledger.attempts(&named), 3);
    let mut op = inbox(operator(), &key);
    assert_eq!(op.take(&Ed25519, &wire.sent[0].1), Ok(Taken::Event));

    // Raised afresh and refused for good: the bound ends it.
    wire.down = true;
    let mut other = raise.clone();
    other.raise = id("raise-2");
    Channel {
        notices: &mut n,
        key: &mut key,
        transport: &mut wire,
    }
    .raise(&other);
    for _ in 1..BOUND {
        n.retry(&mut wire);
    }
    assert!(n.undelivered().is_empty());
    assert_eq!(n.abandoned().len(), 1);
    assert_eq!(
        ledger.attempts(&Key::Raise {
            raise: "raise-2".to_owned()
        }),
        BOUND
    );
}

/// The log's entry of a raise that did not reach the operator (outcome `failed`), citing the first two entries of
/// `log`: written once, never updated.
fn failed_raise_entry(raise: &Raise, log: &[(Receipt, Entry)]) -> Entry {
    let mut raise = raise.clone();
    raise.cites = vec![log[0].0.clone(), log[1].0.clone()];
    serde_json::from_value(json!({
        "kind": "verified",
        "who": core(),
        "what": {"call": "raise", "raise": raise},
        "when": {"at": time(40), "source": "core_clock"},
        "outcome": {"status": "failed"}
    }))
    .expect("a verified entry")
}

fn raise_notices(wire: &Wire) -> usize {
    wire.sent
        .iter()
        .filter(|(_, b)| said(b)["about"]["about"] == "raise")
        .count()
}

/// A raise refused by the path is recorded in the log once, as failed, and never updated. After a restart over the
/// same ledger, the rebuild resends it exactly once, its tries counted on from the ledger, and once delivered it is
/// not sent again; one that used its bound before the restart is not tried again and is said to be abandoned.
// verifies: REQ-ORD-20
#[test]
fn raise_lost_to_a_restart_is_resent_once() {
    let raise: Raise = serde_json::from_str(include_str!(
        "../../../contracts/core-internal/fixtures/valid/raise-of-a-change-in-a-providers-p99.json"
    ))
    .expect("a raise");
    let mut entries = log_of(&[("ord-1", ALICE)]);
    let base = receipted(entries.clone());
    entries.push(failed_raise_entry(&raise, &base));
    let log = receipted(entries);
    held_to_the_log(&log, &unbuilt_refusals());
    let named = Key::Raise {
        raise: raise.raise.as_str().to_owned(),
    };
    let mut key = TestKey::new();

    // Before the restart: the path refuses the raise, which the log records as failed.
    let ledger = Kept::default();
    let mut before = Wire {
        down: true,
        ..Wire::default()
    };
    let mut n = resuming(&ledger);
    Channel {
        notices: &mut n,
        key: &mut key,
        transport: &mut before,
    }
    .raise(&raise);
    assert_eq!(ledger.attempts(&named), 1);
    drop(n);

    // After it: the rebuild resends it once, the tries counted on.
    let mut after = Wire::default();
    let mut n = resuming(&ledger);
    n.notify(&log, &mut key, &mut after).expect("caught up");
    assert_eq!(raise_notices(&after), 1, "resent exactly once");
    assert_eq!(ledger.attempts(&named), 2);
    assert!(ledger.delivered(&named));
    let mut op = inbox(operator(), &key);
    let raised = after
        .sent
        .iter()
        .find(|(_, b)| said(b)["about"]["about"] == "raise")
        .expect("a raise");
    assert_eq!(op.take(&Ed25519, &raised.1), Ok(Taken::Event));

    // Another restart: delivered, so not sent again.
    let mut third = Wire::default();
    let mut n = resuming(&ledger);
    n.notify(&log, &mut key, &mut third).expect("caught up");
    assert_eq!(raise_notices(&third), 0);

    // In the same run the raise is already kept for the retry: reading its entry sends no second try.
    let same = Kept::default();
    let mut down = Wire {
        down: true,
        ..Wire::default()
    };
    let mut n = resuming(&same);
    Channel {
        notices: &mut n,
        key: &mut key,
        transport: &mut down,
    }
    .raise(&raise);
    let mut up = Wire::default();
    n.notify(&log, &mut key, &mut up).expect("read");
    assert_eq!(raise_notices(&up), 0);
    assert_eq!(same.attempts(&named), 1);

    // A raise that used its bound before a restart is not given a fresh one, and is said to be abandoned.
    let spent = Kept::default();
    let mut down = Wire {
        down: true,
        ..Wire::default()
    };
    let mut n = resuming(&spent);
    Channel {
        notices: &mut n,
        key: &mut key,
        transport: &mut down,
    }
    .raise(&raise);
    for _ in 1..BOUND {
        n.retry(&mut down);
    }
    assert_eq!(n.abandoned().len(), 1);
    drop(n);
    let mut up = Wire::default();
    let mut n = resuming(&spent);
    n.notify(&log, &mut key, &mut up).expect("caught up");
    assert_eq!(raise_notices(&up), 0, "no fresh bound");
    assert_eq!(n.abandoned().len(), 1);
    assert_eq!(n.abandoned()[0].key.as_ref(), Some(&named));
}

/// A silence no longer than the interval would call a healthy path cut between two beats: such a watch isn't built.
// verifies: REQ-ORD-7
#[test]
fn watch_refuses_a_silence_no_longer_than_its_interval() {
    let clock = Moving(Cell::new(NOW));
    let with = |interval, silence| {
        Watch::with(
            id(DEPLOYMENT),
            id(OPERATOR),
            id("run-1"),
            &clock,
            interval,
            silence,
        )
    };
    assert!(with(30, 30).is_none());
    assert!(with(30, 10).is_none());
    assert!(with(30, 31).is_some());
}

/// An answer to a beat older than the newest sent doesn't show the path alive now, so a late answer can't hide a
/// cut; and a beat of another run (one replayed from before a restart) is no answer at all.
// verifies: REQ-ORD-7
#[test]
fn late_or_replayed_answer_does_not_hide_a_cut() {
    let clock = Moving(Cell::new(NOW));
    let (mut key, mut warn, mut second) = (TestKey::new(), Wire::default(), Wire::default());
    let mut watch = Watch::new(id(DEPLOYMENT), id(OPERATOR), id("run-1"), &clock);
    clock.0.set(NOW);
    watch
        .tick(&clock, &mut key, &mut warn, &mut second)
        .expect("beat 1");
    clock.0.set(NOW + 30);
    watch
        .tick(&clock, &mut key, &mut warn, &mut second)
        .expect("beat 2");

    // A late answer to beat 1, after beat 2 went out: nothing.
    clock.0.set(NOW + 100);
    watch.answered(&id("run-1"), 1, &clock);
    clock.0.set(NOW + SILENCE);
    let tick = watch
        .tick(&clock, &mut key, &mut warn, &mut second)
        .expect("a tick");
    assert_eq!(tick.path, Path::Cut { since: NOW });

    // The same beat number from another run: nothing.
    let mut other = Watch::new(id(DEPLOYMENT), id(OPERATOR), id("run-0"), &clock);
    let mut sent = Wire::default();
    other
        .tick(&clock, &mut key, &mut sent, &mut Wire::default())
        .expect("a beat of another run");
    let mut op = inbox(operator(), &key);
    assert_eq!(op.take(&Ed25519, &sent.sent[0].1), Ok(Taken::Beat));
    let (run, beat) = op.beat().expect("a beat");
    let mut fresh = Watch::new(id(DEPLOYMENT), id(OPERATOR), id("run-1"), &clock);
    fresh
        .tick(&clock, &mut key, &mut Wire::default(), &mut Wire::default())
        .expect("beat 1");
    clock.0.set(NOW + SILENCE + 60);
    fresh.answered(run, beat, &clock);
    clock.0.set(NOW + SILENCE + 130);
    assert!(matches!(
        fresh
            .tick(&clock, &mut key, &mut Wire::default(), &mut Wire::default())
            .expect("a tick")
            .path,
        Path::Cut { .. }
    ));
}
