//! Intake's status reads held to policy's read check (`T-020-52`; `REQ-AUZ-8`, `REQ-AUZ-7`; finding 6 of `M3`'s
//! milestone review): every status intake tells an order's requester, a person or an agent, is asked of policy first,
//! as a filed order's until the acceptance is in the log and as the order's state after it, naming the requester
//! intake's own record of the filed request holds, never the caller's word. A refused reader is told no more than
//! `ReadRefused`; each check is recorded, allowed or refused. Every log a test writes is replayed as the log itself
//! would admit it. The deployment's rules for what a read allows are the assembly's, over the real policy engine, in
//! `conformance/scenarios/tests/assembly_intake.rs`; here the deployment is a double that answers as set.

mod intake_support;
mod rounds_support;
mod snapshot_support;

use intake_support::*;
use rounds_support::customer;
use rutent_contracts::core_internal::{Entry, ReadCheck, Reads as Read, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_core_orch::intake::{Intake, IntakeError, Resolution, Status, Told, Wait};
use rutent_core_orch::segments::Reopening;
use snapshot_support::id;

/// The log replays as the log itself would admit it.
fn assert_admitted(env: &Env) {
    if let Err((place, why)) = env.admitted() {
        panic!("the log refuses entry {place}: {why}");
    }
}

/// The reads policy recorded, in order, each with whether it was allowed.
fn recorded_reads(env: &Env) -> Vec<(ReadCheck, bool)> {
    env.book
        .borrow()
        .entries
        .iter()
        .filter_map(|(_, entry)| match entry {
            Entry::Verified(v) => match &v.what {
                What::CheckRead { check } => Some((
                    check.clone(),
                    v.outcome == rutent_contracts::core_internal::Outcome::Permitted {},
                )),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// Whether `read` is a filed order's read of `order`, by `requester`.
fn filed(read: &Read, order: &rutent_contracts::router_api::Id, requester: &str) -> bool {
    matches!(read, Read::FiledOrder { order: o, requester: r }
        if o == order && r.as_str() == requester)
}

/// Intake asks policy's read check for every status it tells, naming the requester its own record of the filed request
/// holds and the reader as authentication established it: a filed order's status until the acceptance is in the log,
/// and the order's state from then on, the acceptance told or not (a wait on policy after the acceptance is logged is
/// an order's state). Each check is recorded, allowed.
// verifies: REQ-AUZ-8, REQ-AUD-12
#[test]
fn intake_reads_a_filed_orders_status_until_the_acceptance_is_logged_and_the_orders_state_after() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let request = order_request("client-req-1", agent());
    let opened = env.open(&mut sim, &request).expect("opens");
    let order = order_of(&opened);
    let requester = customer().party;
    let digest = match env.quote_by(&mut sim, &agent(), &order).expect("quotes") {
        Told::Exists {
            status: Status::Quoted { digest, .. },
            ..
        } => digest,
        other => panic!("{other:?}"),
    };
    // An acceptance that waits on its destination isn't in the log: still a filed order.
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::NotYet);
    let waiting = env
        .accept(&mut sim, &order, &agent(), &digest)
        .expect("waits");
    assert!(matches!(
        waiting,
        Told::Exists {
            status: Status::Waiting {
                wait: Wait::DestinationNotYetResolved
            },
            ..
        }
    ));
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Owner(id(OWNER)));
    // Policy can't decide the authorisation once the acceptance is in the log: the order waits, and its state is read.
    env.policy.answer = rutent_contracts::core_internal::Answer::NotKnown {
        cause: rutent_contracts::core_internal::NotKnownCause::PolicyUnreachable,
    };
    let not_known = env
        .accept(&mut sim, &order, &agent(), &digest)
        .expect("waits on policy");
    assert!(matches!(
        not_known,
        Told::Exists {
            status: Status::Waiting {
                wait: Wait::PolicyNotKnown
            },
            ..
        }
    ));
    env.policy.answer = rutent_contracts::core_internal::Answer::Permit {};
    let accepted = env
        .accept(&mut sim, &order, &agent(), &digest)
        .expect("accepted");
    assert!(matches!(
        accepted,
        Told::Exists {
            status: Status::Accepted { .. },
            ..
        }
    ));
    // Told again by request: the order's state.
    assert!(env.told(&agent(), "client-req-1").is_some());

    let reads = recorded_reads(&env);
    let kinds: Vec<&str> = reads
        .iter()
        .map(|(c, _)| match &c.reads {
            Read::FiledOrder { .. } => "filed_order",
            Read::OrderState { .. } => "order_state",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "filed_order", // open
            "filed_order", // quote
            "filed_order", // accept, waiting on the destination: no acceptance in the log
            "order_state", // accept, waiting on policy: the acceptance is in the log
            "order_state", // accept, accepted
            "order_state", // told
        ]
    );
    for (check, allowed) in &reads {
        assert!(allowed, "{check:?}");
        assert_eq!(check.reader, agent(), "the reader is the caller, as stated");
        assert_eq!(check.permission, None);
        match &check.reads {
            Read::FiledOrder { .. } => {
                assert!(filed(&check.reads, &order, requester.as_str()), "{check:?}");
            }
            Read::OrderState { order: o } => assert_eq!(o, &order),
            other => panic!("{other:?}"),
        }
    }
    // The requester named is the one intake's own record holds.
    assert_eq!(
        env.intake
            .record(&order)
            .expect("intake's record")
            .parties
            .requester,
        requester
    );
    assert_admitted(&env);
}

/// An agent policy refuses the read is told nothing of the order's status by any call: not on opening, quoting,
/// accepting or asking, and the refusal names no class and no state (`T-023-01`'s `CurrentError::Refused`, alone).
/// What the call did stands: the order opened, the quote shown, the acceptance recorded. Each check is recorded,
/// refused.
// verifies: REQ-AUZ-8, REQ-AUD-12
#[test]
fn a_reader_policy_refuses_is_told_no_status_by_any_call_and_the_check_is_recorded() {
    let mut env = Env::new();
    let mut sim = env.sim();
    // The agent is listed but its party set it no scope for reading.
    env.reads.scoped.clear();
    let request = order_request("client-req-1", agent());
    assert_eq!(
        env.open(&mut sim, &request),
        Err(IntakeError::ReadRefused),
        "opening tells nothing"
    );
    // The order opened all the same, and the customer's own person, whom the deployment lets read, finds it.
    let opened = env.open(&mut sim, &order_request("client-req-1", customer()));
    let order = order_of(&opened.expect("the person is told"));
    assert!(
        env.book
            .borrow()
            .verified()
            .iter()
            .any(|w| matches!(w, What::OrderOpened { order: o, .. } if o == &order)),
        "the order opened"
    );
    assert_eq!(env.told(&agent(), "client-req-1"), None);
    assert_eq!(
        env.quote_by(&mut sim, &agent(), &order),
        Err(IntakeError::ReadRefused)
    );
    // The quote was shown all the same: the person is told it.
    let digest = match env.told_customer("client-req-1") {
        Some(Told::Exists {
            status: Status::Quoted { digest, .. },
            ..
        }) => digest,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        env.accept(&mut sim, &order, &agent(), &digest),
        Err(IntakeError::ReadRefused)
    );
    assert_eq!(env.acceptances().len(), 1, "the acceptance stands");
    let reads = recorded_reads(&env);
    let by_agent: Vec<_> = reads.iter().filter(|(c, _)| c.reader == agent()).collect();
    assert_eq!(by_agent.len(), 4, "open, told, quote, accept: {by_agent:?}");
    assert!(by_agent.iter().all(|(_, allowed)| !allowed));
    // A customer's person is read as ever.
    assert!(
        reads
            .iter()
            .any(|(c, allowed)| c.reader == customer() && *allowed)
    );
    assert_admitted(&env);
}

/// A policy that refuses every reader, the operator's among them, leaves the requester's own login told nothing too:
/// the status is told only on `Allowed`.
// verifies: REQ-AUZ-8
#[test]
fn status_is_told_only_when_policy_allows_it() {
    let mut env = Env::new();
    let mut sim = env.sim();
    env.reads.refuse_all = true;
    assert_eq!(
        env.open(&mut sim, &order_request("client-req-1", customer())),
        Err(IntakeError::ReadRefused)
    );
    assert_eq!(env.told_customer("client-req-1"), None);
    env.reads.refuse_all = false;
    assert!(env.told_customer("client-req-1").is_some());
    assert_admitted(&env);
}

/// A caller that can't be asked about, because policy's check couldn't be recorded, is told nothing either
/// (failing closed): the status isn't told where the check is lost.
// verifies: REQ-AUZ-8
#[test]
fn status_is_not_told_where_policy_cannot_be_asked() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let order = order_of(
        &env.open(&mut sim, &order_request("client-req-1", customer()))
            .expect("opens"),
    );
    env.reads.fail_next = true;
    assert!(matches!(
        env.quote_by(&mut sim, &customer(), &order),
        Err(IntakeError::Log(_))
    ));
    assert_admitted(&env);
}

/// What Core reads back of the evidence store when it reopens: every statement kept, or none of them (a store that
/// lost what an order's inputs were kept as).
struct Shelf {
    statements: Vec<Vec<u8>>,
    lost: bool,
}

impl Reopening for Shelf {
    fn record(&self, _: &Id) -> Option<DecisionRecord> {
        None
    }

    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
        if self.lost {
            return None;
        }
        self.statements
            .iter()
            .enumerate()
            .find(|(place, bytes)| &intake_support::commitment(*place, bytes) == commitment)
            .map(|(_, bytes)| bytes.clone())
    }
}

/// An order accepted and started before a restart, whose inputs statement can't be read back after it, is held: the
/// reopened intake answers no start for it (what the segments follow) and no record of it (what policy's sources
/// answer from), so nothing runs it; the same order whose statement reads back keeps both (`T-020-46`, `NFR-6`).
// verifies: NFR-6, REQ-AUD-6
#[test]
fn held_order_accepted_before_a_restart_never_starts_after_it() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, told) = env.accepted(
        &mut sim,
        &order_request("client-req-1", customer()),
        &customer(),
    );
    assert!(matches!(
        told,
        Told::Exists {
            status: Status::Accepted { .. },
            ..
        }
    ));
    let statements = env.store.borrow().kept.clone();

    // The statement reads back: the order is rebuilt started, and answers policy's sources.
    let read = Intake::reopened(
        &env.history(),
        &mut Shelf {
            statements: statements.clone(),
            lost: false,
        },
    );
    assert!(!read.held(&order));
    assert!(
        read.start(&order).is_some(),
        "an order read back is started"
    );
    assert!(read.record(&order).is_some());

    // It doesn't: the order is held, never started again, and policy's sources hold no record of it.
    let lost = Intake::reopened(
        &env.history(),
        &mut Shelf {
            statements,
            lost: true,
        },
    );
    assert!(lost.held(&order));
    assert_eq!(lost.start(&order), None, "a held order never starts");
    assert_eq!(lost.record(&order), None, "a held order has no record");
    assert_eq!(lost.request_of(&order), None);
    assert_admitted(&env);
}

/// A held order whose quote shown before the restart is named is not accepted, quoted or told otherwise than the log
/// holds it: the acceptance is refused as not now, and nothing is recorded (`T-020-46`'s second guard in
/// `Intake::accept`, which `T-020-51`'s rebuilt waiting quotes reach: the reopened order is held, its quote's digest
/// the log last shows).
// verifies: NFR-6
#[test]
fn held_order_whose_waiting_quote_is_rebuilt_is_not_accepted() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
    let statements = env.store.borrow().kept.clone();
    env.intake = Intake::reopened(
        &env.history(),
        &mut Shelf {
            statements,
            lost: true,
        },
    );
    assert!(env.intake.held(&order));
    let before = env.len();
    assert_eq!(
        env.accept(&mut sim, &order, &customer(), &digest),
        Err(IntakeError::NotNow)
    );
    assert_eq!(
        env.quote_by(&mut sim, &customer(), &order),
        Err(IntakeError::NotNow)
    );
    assert_eq!(env.len(), before, "nothing is recorded");
    assert!(env.acceptances().is_empty());
    assert!(env.started().is_empty());
    assert_admitted(&env);
}

/// An order that ends after its acceptance is in the log (policy refused the authorisation) is read as the order's
/// state, as the log's parties are the ones to say; one that ends before it (the log refused the acceptance) is read
/// as a filed order's still.
// verifies: REQ-AUZ-8
#[test]
fn order_ended_is_read_as_the_state_of_the_log_only_once_its_acceptance_is_logged() {
    let kinds = |env: &Env| -> Vec<&'static str> {
        recorded_reads(env)
            .iter()
            .map(|(c, _)| match &c.reads {
                Read::FiledOrder { .. } => "filed_order",
                Read::OrderState { .. } => "order_state",
                _ => "other",
            })
            .collect()
    };
    // Policy refuses the authorisation once the acceptance is appended: the order ends after it.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
    env.policy.answer = rutent_contracts::core_internal::Answer::Refuse {
        class: rutent_contracts::core_internal::RefusalClass::NotPermitted,
    };
    let told = env
        .accept(&mut sim, &order, &customer(), &digest)
        .expect("told it ended");
    assert!(matches!(
        told,
        Told::Exists {
            status: Status::CantProceed { .. },
            ..
        }
    ));
    assert_eq!(env.acceptances().len(), 1, "the acceptance is in the log");
    assert_eq!(kinds(&env).last(), Some(&"order_state"));
    assert_admitted(&env);

    // The log refuses the acceptance: it ends before one is in the log.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-2", customer()));
    env.book.borrow_mut().refuse_next = Some("acceptance");
    let told = env
        .accept(&mut sim, &order, &customer(), &digest)
        .expect("told it ended");
    assert!(matches!(
        told,
        Told::Exists {
            status: Status::CantProceed { .. },
            ..
        }
    ));
    assert!(env.acceptances().is_empty());
    assert_eq!(kinds(&env).last(), Some(&"filed_order"));
    assert_admitted(&env);
}

/// A request in a session of policy's register that is unread is refused for its missing input, nothing created, and
/// told that class; one whose session the register reads and doesn't hold is refused of policy's class, as before
/// (`T-021-17`, `T-020-52`).
// verifies: REQ-AUZ-5
#[test]
fn request_in_a_session_of_an_unread_register_is_refused_for_its_missing_input() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let mut other = order_request("client-req-2", customer());
    other.session = id("session-nobody-opened");
    assert_eq!(
        env.open(&mut sim, &other),
        Ok(Told::Refused {
            class: rutent_contracts::core_internal::OrderRefusalClass::RefusedByPolicy
        })
    );
    env.sessions.unread = true;
    assert_eq!(
        env.open(&mut sim, &order_request("client-req-1", customer())),
        Ok(Told::Refused {
            class: rutent_contracts::core_internal::OrderRefusalClass::MissingInput
        })
    );
    assert!(
        env.refusals().iter().any(|w| matches!(
            w,
            What::OrderRefusal {
                order: None,
                class: rutent_contracts::core_internal::OrderRefusalClass::MissingInput,
                ..
            }
        )),
        "recorded as an order refusal of its class, naming no order"
    );
    assert_admitted(&env);
}
