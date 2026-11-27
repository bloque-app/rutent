//! A holding account's balance is read as any action policy decides is (`T-020-63`, `REQ-AUD-15`, `REQ-AUZ-1`,
//! `INV-7`): Core draws a fresh transaction, policy decides `read_holding_balance` as Core, the effect is recorded
//! before the module is asked, the question is the operator's signed one, and how the read came out is recorded; a
//! refusal asks nothing. The read the schedule's holding check makes is `Segments::read_holding_balance`.

mod snapshot_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Decisions, Evidence, Fetched, GeneralEntry, Kept, Log,
};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, Decision, DecisionRequest, Effect, Entry, Outcome, ReadCheck,
    Receipt, RefusalClass, Role, What,
};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{Operation, PublicKey, Statement};
use rutent_contracts::router_api::{DecisionRecord, Id, PolicyBounds};
use rutent_core_orch::rounds::{Fresh, Issued, OperatorKey};
use rutent_core_orch::segments::{
    Answered, Balance, BalanceRead, Modules, NoProxy, Parts, ProvidersWord, Segments,
};
use rutent_core_orch::snapshot::{Clock, Exposures, Reading};
use rutent_prov::catalogue::{Catalogue, Instant};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::types::Events;
use rutent_prov::proxy::RouteNotNamed;
use serde_json::{Value, json};

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

struct TestClock(Cell<Reading>);

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

struct TestKey(SigningKey);

impl OperatorKey for TestKey {
    fn key(&self) -> PublicKey {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in self.0.verifying_key().as_bytes().chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            }
        }
        PublicKey::new(out).unwrap()
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

#[derive(Default)]
struct Ids(u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("id-{:04}", self.0))
    }
}

fn receipt(index: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-1",
        "index": index,
        "entry": { "algorithm": "sha-256", "value": format!("{index:064x}") },
    }))
    .unwrap()
}

/// The log: the entries it took, shared with the platform so it can say how many were in when it was asked, and how many
/// it takes before it can take no more.
#[derive(Clone)]
struct KeptLog {
    entries: Rc<RefCell<Vec<Entry>>>,
    takes: Rc<Cell<usize>>,
}

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        let mut entries = self.entries.borrow_mut();
        if entries.len() >= self.takes.get() {
            return Err(AppendFailure::Unavailable);
        }
        entries.push(entry.entry().clone());
        Ok(receipt(entries.len() - 1))
    }
}

struct Store;

impl Evidence for Store {
    fn keep(&mut self, _: &[u8]) -> Result<Kept, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn fetch(&mut self, _: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

/// Policy: permits every request, or refuses it; it keeps the requests it was asked.
struct Policy {
    permits: bool,
    asked: Vec<DecisionRequest>,
}

impl Decisions for Policy {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.asked.push(request.clone());
        Ok(Decision {
            request: request.request.clone(),
            decision: id(&format!("dec-{}", self.asked.len())),
            outcome: if self.permits {
                Answer::Permit {}
            } else {
                Answer::Refuse {
                    class: RefusalClass::NotPermitted,
                }
            },
            receipt: receipt(10_000 + self.asked.len()),
        })
    }
}

/// The platform: what it was asked, how many entries the log held when it was, and what it answers.
struct Platform {
    log: Rc<RefCell<Vec<Entry>>>,
    asked: Vec<(Effect, Id, Statement<Operation>, usize)>,
    answer: Option<Vec<u8>>,
}

impl Modules for Platform {
    fn begin_segment(&mut self, _: &Id, _: &Identifier) -> Result<String, PlatformRefusal> {
        Err(PlatformRefusal::NotHandled)
    }

    fn name_calls(
        &mut self,
        _: &Identifier,
        _: &DecisionRecord,
        _: (usize, usize),
        _: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        Ok(0)
    }

    fn carry_out(&mut self, _: &Effect, _: &Id, _: &Statement<Operation>) -> Answered {
        Answered::Refused(PlatformRefusal::NotHandled)
    }

    fn ask_for_balance(
        &mut self,
        effect: &Effect,
        provider: &Id,
        question: &Statement<Operation>,
    ) -> Answered {
        let held = self.log.borrow().len();
        self.asked
            .push((effect.clone(), provider.clone(), question.clone(), held));
        match &self.answer {
            Some(bytes) => Answered::Bytes(bytes.clone()),
            None => Answered::Refused(PlatformRefusal::Unavailable),
        }
    }

    fn take_events(&mut self) -> Events {
        Events::default()
    }

    fn end_segment(&mut self, _: &Identifier) -> Option<SegmentRecord> {
        None
    }

    fn end_calls(&mut self, _: &Identifier) {}

    fn staged(&self, _: &Id) -> Option<String> {
        None
    }

    fn promote(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Id,
        _: Instant,
    ) -> Result<String, LifecycleRefusal> {
        Err(LifecycleRefusal::NotStaged)
    }
}

fn core() -> Actor {
    Actor {
        party: id("rutent"),
        acts: id("rutent-core"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}

fn operator() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("rutent-operator"),
        role: Role::Operator,
        kind: ActorKind::Component,
    }
}

fn bounds() -> PolicyBounds {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/router-api/fixtures/valid/policy-bounds-without-splitting.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// What one read came to, and everything it touched.
struct Done {
    read: Balance,
    log: KeptLog,
    policy: Policy,
    platform: Platform,
}

/// The read of `fx-desk`'s balance at `fx-desk-ledger`, asked of `platform` under `policy`, with the log taking `takes`
/// entries and no more; the platform answers with `answer`, a result's body, where it does.
fn read(permits: bool, takes: usize, answer: Option<Value>) -> Done {
    let log = KeptLog {
        entries: Rc::default(),
        takes: Rc::new(Cell::new(takes)),
    };
    let mut platform = Platform {
        log: Rc::clone(&log.entries),
        asked: Vec::new(),
        answer: answer.map(|body| serde_json::to_vec(&body).unwrap()),
    };
    let mut policy = Policy {
        permits,
        asked: Vec::new(),
    };
    let clock = TestClock(Cell::new(Reading::at(1_794_200_000)));
    let catalogue = Catalogue::new();
    let mut append = log.clone();
    let mut store = Store;
    let mut key = TestKey(SigningKey::from_bytes(&[7; 32]));
    let mut ids = Ids::default();
    let issued = Issued::new();
    let mut exposures = Exposures::new();
    let mut segments = Segments::new();
    let (core, operator, bounds) = (core(), operator(), bounds());
    let mut parts = Parts {
        clock: &clock,
        catalogue: &catalogue,
        log: &mut append,
        evidence: &mut store,
        decisions: &mut policy,
        modules: &mut platform,
        uses: &mut NoProxy,
        grading: &mut ProvidersWord,
        operator_key: &mut key,
        fresh: &mut ids,
        issued: &issued,
        exposures: &mut exposures,
        channel: &mut rutent_core_orch::notice::Unbuilt,
    };
    let read = segments.read_holding_balance(
        &mut parts,
        &BalanceRead {
            node: "fx-desk-ledger",
            provider: "fx-desk",
            asset: "USDC",
            bounds: &bounds,
            operator: &operator,
            core: &core,
        },
    );
    Done {
        read,
        log,
        policy,
        platform,
    }
}

/// The module's answer to the question it was asked, as it would sign none here: a result body naming the transaction.
fn answering(transaction: &str, balance: &str) -> Value {
    json!({
        "deployment": "rutent-test", "protocol_version": "1.0.0", "shape": "urn:rutent:provider-role:result",
        "maker": "fx-desk", "made_at": "2026-11-10T09:00:00Z",
        "signature": { "scheme": "ed25519", "key": "A", "value": "A" },
        "statement": { "kind": "holding_balance", "transaction": transaction,
            "outcome": { "status": "answered", "balance": balance } },
    })
}

fn whats(done: &Done) -> Vec<(What, Outcome)> {
    done.log
        .entries
        .borrow()
        .iter()
        .filter_map(|e| match e {
            Entry::Verified(v) => Some((v.what.clone(), v.outcome)),
            Entry::Stated(_) => None,
        })
        .collect()
}

/// A read is decided as Core, whose carrying the operator's permission onward policy checks, then recorded as an effect
/// before the platform is asked, the question the operator's own, signed, under the transaction the action names; the
/// result is recorded once the platform answered, and the answer is handed back with that transaction (`REQ-AUD-15`,
/// `REQ-AUZ-1`, `INV-7`, `T-020-63`).
// verifies: REQ-AUZ-1, INV-7
#[test]
fn holding_balance_is_read_on_a_decision_and_recorded_before_the_module_is_asked() {
    let done = read(true, usize::MAX, Some(answering("id-0001", "1500")));
    let Balance::Answered(transaction, _) = &done.read else {
        panic!("answered: {:?}", done.read);
    };
    // Policy was asked once, as Core, for this read, naming a fresh transaction.
    let [request] = done.policy.asked.as_slice() else {
        panic!("one decision: {:?}", done.policy.asked);
    };
    assert_eq!(request.caller, core());
    let Action::ReadHoldingBalance {
        node,
        provider,
        asset,
        transaction: named,
    } = &request.action
    else {
        panic!("the action decided: {:?}", request.action);
    };
    assert_eq!(
        (node.as_str(), provider.as_str(), asset.as_str()),
        ("fx-desk-ledger", "fx-desk", "USDC")
    );
    assert_eq!(
        named, transaction,
        "the action names the transaction asked under"
    );
    // The effect is in the log before the platform is asked: the one entry, which the permit rests on.
    let [(effect, asked, question, held)] = done.platform.asked.as_slice() else {
        panic!("asked once: {:?}", done.platform.asked.len());
    };
    assert_eq!(*held, 1, "the effect, and nothing else, was in the log");
    assert_eq!(effect.action, request.action);
    assert_eq!(asked.as_str(), "fx-desk");
    // The question is the operator's, under that transaction.
    assert_eq!(question.maker, "rutent-operator");
    let Operation::HoldingBalance(q) = &question.statement else {
        panic!("a balance question: {:?}", question.statement);
    };
    assert_eq!(q.transaction, transaction.as_str());
    assert_eq!(
        (q.node.as_str(), q.asset.as_str()),
        ("fx-desk-ledger", "USDC")
    );
    // The log: the effect to take effect, then its result, done.
    let recorded = whats(&done);
    assert_eq!(recorded.len(), 2, "{recorded:?}");
    assert!(matches!(
        &recorded[0],
        (What::Effect { action, .. }, Outcome::ToTakeEffect {}) if action == &request.action
    ));
    assert!(matches!(
        &recorded[1],
        (What::Result { .. }, Outcome::Done {})
    ));
    // And the answer is the module's result of that transaction, as the platform passed it on.
    let Balance::Answered(_, bytes) = &done.read else {
        unreachable!()
    };
    let body: Value = serde_json::from_slice(bytes).expect("a result");
    assert_eq!(body["statement"]["transaction"], transaction.as_str());
}

/// Where policy refuses the read, the platform is asked nothing and no effect is recorded: the account is read as not
/// read, which fails closed (`REQ-AUZ-1`, `T-020-63`).
// verifies: REQ-AUZ-1
#[test]
fn holding_balance_policy_refuses_is_asked_of_no_module() {
    let done = read(false, usize::MAX, Some(answering("id-0001", "1500")));
    assert_eq!(done.read, Balance::Denied);
    assert!(done.platform.asked.is_empty(), "nothing was asked");
    assert!(whats(&done).is_empty(), "and no effect recorded");
    assert_eq!(done.policy.asked.len(), 1);
}

/// Where the log can't take the effect, the platform is asked nothing: the read is tried again, never carried out
/// before it is recorded (`INV-7`, `T-020-63`).
// verifies: INV-7
#[test]
fn holding_balance_whose_effect_the_log_cannot_take_is_asked_of_no_module() {
    let done = read(true, 0, Some(answering("id-0001", "1500")));
    assert_eq!(done.read, Balance::Failed);
    assert!(done.platform.asked.is_empty(), "nothing was asked");
    assert!(whats(&done).is_empty());
}

/// Where the platform passes no answer, the read's result is recorded unanswered and no balance comes back; and where
/// the log can't take that result, the answer is not used (`INV-7`, `REQ-AUD-15`, `T-020-63`).
// verifies: REQ-AUD-15
#[test]
fn holding_balance_with_no_answer_or_no_record_of_its_result_hands_back_none() {
    let done = read(true, usize::MAX, None);
    assert_eq!(done.read, Balance::Failed);
    let recorded = whats(&done);
    assert!(matches!(
        recorded.as_slice(),
        [
            (What::Effect { .. }, _),
            (What::Result { .. }, Outcome::Unanswered {})
        ]
    ));
    // The effect is taken, then the log can take no more: the module was asked, and the answer is not used.
    let done = read(true, 1, Some(answering("id-0001", "1500")));
    assert_eq!(done.read, Balance::Failed);
    assert_eq!(done.platform.asked.len(), 1);
    assert_eq!(whats(&done).len(), 1, "the effect alone");
}
