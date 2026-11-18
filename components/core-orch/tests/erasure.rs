//! Core carries out an erasure (`T-020-37`): policy's permit, the effect, its result, the store's erasure and the
//! `erasure` entry, in that order, over a log that holds each entry to the real `admit_entry`. A refused or failed
//! erasure changes nothing. The assembled Core's own scenario is `F-024`'s file
//! `conformance/scenarios/tests/audit_scenarios.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Decision, DecisionRequest, DeploymentOperator, Entry, Outcome,
    Receipt, Role, Through, Token, Violation, What, admit_entry,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Id;
use rutent_core_orch::erasure::{Erasure, NotErased, Statements, erase_statement};
use rutent_core_orch::rounds::Fresh;
use rutent_core_orch::snapshot::{Clock, Reading};
use serde_json::json;

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn digest(n: u64) -> Digest {
    serde_json::from_value(json!({ "algorithm": "sha-256", "value": format!("{n:064x}") })).unwrap()
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
        acts: id("ops-lead"),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}

struct Now;

impl Clock for Now {
    fn now(&self) -> Reading {
        Reading::at(1_794_200_000)
    }
}

struct Ids(u32);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("req-{}", self.0))
    }
}

/// The log, each entry admitted by `admit_entry` over what came before it, as the real log does.
#[derive(Default)]
struct Entries(RefCell<Vec<(Receipt, Entry)>>);

impl Entries {
    fn admit(&self, entry: Entry, through: Through) -> Result<Receipt, Violation> {
        let history = self.0.borrow().clone();
        admit_entry(
            &history,
            &entry,
            through,
            &DeploymentOperator {
                maker: id("operator"),
                party: id("operator"),
            },
        )?;
        Ok(self.push(entry))
    }

    fn push(&self, entry: Entry) -> Receipt {
        let mut all = self.0.borrow_mut();
        let receipt: Receipt = serde_json::from_value(json!({
            "log": "log-1",
            "index": all.len(),
            "entry": { "algorithm": "sha-256", "value": format!("{:064x}", all.len() + 1) },
        }))
        .unwrap();
        all.push((receipt.clone(), entry));
        receipt
    }

    fn what(&self) -> Vec<(String, String)> {
        self.0
            .borrow()
            .iter()
            .map(|(_, e)| match e {
                Entry::Verified(v) => (
                    serde_json::to_value(v.what.clone()).unwrap()["call"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    serde_json::to_value(v.outcome).unwrap()["status"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                ),
                Entry::Stated(_) => ("stated".into(), "stated".into()),
            })
            .collect()
    }

    fn calls(&self) -> Vec<String> {
        self.what().into_iter().map(|(c, _)| c).collect()
    }
}

/// Orchestration's handle: every append admitted over the history.
struct General(Rc<Entries>);

impl Log for General {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.0
            .admit(entry.entry().clone(), Through::General)
            .map_err(AppendFailure::Refused)
    }
}

/// Policy: permits or refuses, its decision entry in the log on its own handle.
struct Policy {
    log: Rc<Entries>,
    permitting: bool,
    asked: Rc<RefCell<Vec<DecisionRequest>>>,
}

impl Decisions for Policy {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.asked.borrow_mut().push(request.clone());
        let decision = id(&format!("dec-{}", self.asked.borrow().len()));
        let entry: Entry = serde_json::from_value(json!({
            "kind": "verified",
            "who": request.caller,
            "what": { "call": "decide", "request": request, "decision": decision },
            "when": { "at": request.at, "source": "core_clock" },
            "outcome": if self.permitting {
                json!({ "status": "permitted" })
            } else {
                json!({ "status": "refused", "class": "not_permitted" })
            },
        }))
        .unwrap();
        let receipt = self
            .log
            .admit(entry, Through::Policy)
            .map_err(AppendFailure::Refused)?;
        Ok(Decision {
            request: request.request.clone(),
            decision,
            outcome: if self.permitting {
                Answer::Permit {}
            } else {
                serde_json::from_value(json!({ "answer": "refuse", "class": "not_permitted" }))
                    .unwrap()
            },
            receipt,
        })
    }
}

/// The evidence store: holds one statement and erases it only on the result of a done erase effect in the log, as
/// the bound store does.
struct Store {
    log: Rc<Entries>,
    commitment: Digest,
    kept: Cell<bool>,
    /// How many times `erasable` answers yes before it refuses.
    erasable_for: Cell<usize>,
    /// `erase` answers `Unavailable` this many times first.
    unavailable: Cell<usize>,
    erased_on: RefCell<Vec<Receipt>>,
}

impl Store {
    fn new(log: &Rc<Entries>) -> Self {
        Store {
            log: Rc::clone(log),
            commitment: digest(77),
            kept: Cell::new(true),
            erasable_for: Cell::new(usize::MAX),
            unavailable: Cell::new(0),
            erased_on: RefCell::new(Vec::new()),
        }
    }
}

impl Statements for Store {
    fn erasable(&mut self, commitment: &Digest) -> Result<(), AppendFailure> {
        if *commitment != self.commitment || !self.kept.get() {
            return Err(AppendFailure::Refused(Violation("not kept")));
        }
        match self.erasable_for.get() {
            0 => Err(AppendFailure::Refused(Violation("evidence rested on"))),
            n => {
                self.erasable_for.set(n.saturating_sub(1));
                Ok(())
            }
        }
    }

    fn erase(&mut self, result: &Receipt) -> Result<(), AppendFailure> {
        // The store reads the decision back from the log: a done result of an erase effect.
        let history = self.log.0.borrow();
        let done = history.iter().any(|(r, e)| {
            r == result
                && matches!(e, Entry::Verified(v)
                    if matches!(v.what, What::Result { .. }) && v.outcome == (Outcome::Done {}))
        });
        if !done {
            return Err(AppendFailure::Refused(Violation("no decided erasure")));
        }
        if self.unavailable.get() > 0 {
            self.unavailable.set(self.unavailable.get() - 1);
            return Err(AppendFailure::Unavailable);
        }
        self.erased_on.borrow_mut().push(result.clone());
        self.kept.set(false);
        Ok(())
    }
}

/// A log holding the stated entry that names the statement, which the `erasure` entry's own row needs.
fn world() -> (Rc<Entries>, Store) {
    let log = Rc::new(Entries::default());
    let stated: Entry = serde_json::from_value(json!({
        "kind": "stated",
        "attribution": { "signature": "verified", "who": { "party": "bank-co", "acts": "reserve-bank-co", "role": "provider", "kind": "component" } },
        "what": { "statement": "urn:rutent:provider-role:result" },
        "when": { "at": "2026-11-06T15:04:05Z", "source": "party_stated" },
        "received_at": "2026-11-06T15:04:06Z",
        "outcome": { "status": "stated" },
        "signed_with": { "scheme": "ed25519", "key": "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w" },
        "commitment": digest(77),
        "about": { "order": "order-1", "transaction": "tx-1", "segment": "seg-1" },
    }))
    .unwrap();
    log.push(stated);
    let store = Store::new(&log);
    (log, store)
}

fn policy(log: &Rc<Entries>, permitting: bool) -> (Policy, Rc<RefCell<Vec<DecisionRequest>>>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    (
        Policy {
            log: Rc::clone(log),
            permitting,
            asked: Rc::clone(&asked),
        },
        asked,
    )
}

fn request<'a>(
    commitment: &'a Digest,
    reason: &'a Token,
    caller: &'a Actor,
    core: &'a Actor,
) -> Erasure<'a> {
    Erasure {
        commitment,
        reason,
        caller,
        core,
    }
}

/// Policy permits: the decision, the effect, its done result, the store's erasure on that result and the `erasure`
/// entry follow in that order, each admitted by the log, and the statement is gone.
// verifies: REQ-AUD-13, DEC-18
#[test]
fn a_permitted_erasure_is_decided_recorded_carried_out_and_recorded_again() {
    let (log, mut store) = world();
    let (mut decisions, asked) = policy(&log, true);
    let (operator, core, reason) = (operator(), core(), Token::new("retention").unwrap());
    let commitment = store.commitment.clone();
    let erased = erase_statement(
        &mut General(Rc::clone(&log)),
        &mut decisions,
        &mut store,
        &Now,
        &mut Ids(0),
        &request(&commitment, &reason, &operator, &core),
    )
    .expect("a permitted erasure is carried out");
    assert_eq!(
        log.what(),
        [
            ("stated", "stated"),
            ("decide", "permitted"),
            ("effect", "to_take_effect"),
            ("result", "done"),
            ("erasure", "done"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    assert!(!store.kept.get());
    assert_eq!(
        *store.erased_on.borrow(),
        std::slice::from_ref(&erased.result)
    );
    assert_eq!(asked.borrow().len(), 1);
    assert_eq!(asked.borrow()[0].caller, operator);
    assert!(matches!(
        &asked.borrow()[0].action,
        rutent_contracts::core_internal::Action::EraseStatement { commitment: c, .. } if *c == commitment
    ));
}

/// Policy refuses: no effect, no result, no erasure entry, and the store is never asked to erase.
// verifies: DEC-18
#[test]
fn erasure_without_a_permitting_decision_erases_nothing() {
    let (log, mut store) = world();
    let (mut decisions, _) = policy(&log, false);
    let (operator, core, reason) = (operator(), core(), Token::new("retention").unwrap());
    let commitment = store.commitment.clone();
    let refused = erase_statement(
        &mut General(Rc::clone(&log)),
        &mut decisions,
        &mut store,
        &Now,
        &mut Ids(0),
        &request(&commitment, &reason, &operator, &core),
    );
    assert!(
        matches!(
            refused.as_ref().map_err(|e| &**e),
            Err(NotErased::NotPermitted(Answer::Refuse { .. }))
        ),
        "{refused:?}"
    );
    assert_eq!(log.calls(), ["stated", "decide"]);
    assert!(store.kept.get());
    assert!(store.erased_on.borrow().is_empty());
}

/// A statement the store wouldn't erase is never decided: nothing is asked of policy, nothing is recorded.
// verifies: DEC-18
#[test]
fn erasure_of_what_the_store_would_refuse_is_not_even_decided() {
    let (log, mut store) = world();
    store.erasable_for.set(0);
    let (mut decisions, asked) = policy(&log, true);
    let (operator, core, reason) = (operator(), core(), Token::new("retention").unwrap());
    let commitment = store.commitment.clone();
    let refused = erase_statement(
        &mut General(Rc::clone(&log)),
        &mut decisions,
        &mut store,
        &Now,
        &mut Ids(0),
        &request(&commitment, &reason, &operator, &core),
    );
    assert!(
        matches!(
            refused.as_ref().map_err(|e| &**e),
            Err(NotErased::NotErasable(_))
        ),
        "{refused:?}"
    );
    assert!(asked.borrow().is_empty());
    assert_eq!(log.calls(), ["stated"]);
    assert!(store.kept.get());
}

/// The store refuses once the effect is recorded (something now rests on the statement): the effect's result is
/// `failed`, the statement stays, and no `erasure` entry is written.
// verifies: REQ-AUD-14
#[test]
fn erase_that_fails_leaves_the_statement_and_records_the_failure() {
    let (log, mut store) = world();
    // Yes before the decision, no once the effect is recorded.
    store.erasable_for.set(1);
    let (mut decisions, _) = policy(&log, true);
    let (operator, core, reason) = (operator(), core(), Token::new("retention").unwrap());
    let commitment = store.commitment.clone();
    let refused = erase_statement(
        &mut General(Rc::clone(&log)),
        &mut decisions,
        &mut store,
        &Now,
        &mut Ids(0),
        &request(&commitment, &reason, &operator, &core),
    );
    assert!(
        matches!(
            refused.as_ref().map_err(|e| &**e),
            Err(NotErased::Failed { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(
        log.what(),
        [
            ("stated", "stated"),
            ("decide", "permitted"),
            ("effect", "to_take_effect"),
            ("result", "failed"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    assert!(store.kept.get());
    assert!(store.erased_on.borrow().is_empty());
}

/// A store that can't be reached after the done result leaves an erasure to finish, which settles once it can: the
/// statement is erased once and the `erasure` entry appended once.
// verifies: REQ-AUD-13
#[test]
fn an_erasure_the_store_cannot_finish_now_settles_later() {
    let (log, mut store) = world();
    store.unavailable.set(3);
    let (mut decisions, _) = policy(&log, true);
    let (operator, core, reason) = (operator(), core(), Token::new("retention").unwrap());
    let commitment = store.commitment.clone();
    let mut general = General(Rc::clone(&log));
    let stopped = erase_statement(
        &mut general,
        &mut decisions,
        &mut store,
        &Now,
        &mut Ids(0),
        &request(&commitment, &reason, &operator, &core),
    );
    let Err(NotErased::Unsettled(unsettled)) = stopped.map_err(|e| *e) else {
        panic!("not unsettled")
    };
    assert_eq!(log.calls(), ["stated", "decide", "effect", "result"]);
    assert!(store.kept.get());
    let erased = unsettled
        .settle(&mut general, &mut store, &Now)
        .expect("the store answers now");
    assert_eq!(
        log.calls(),
        ["stated", "decide", "effect", "result", "erasure"]
    );
    assert_eq!(*store.erased_on.borrow(), [erased.result]);
    assert!(!store.kept.get());
}
