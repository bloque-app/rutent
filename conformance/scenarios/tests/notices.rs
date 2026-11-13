//! Notices over the assembly (`T-023-01`): an order run end to end against the simulated providers, its log as
//! `CMP-CORE-AUDIT` holds it, every entry admitted, and policy's reads answered by `CMP-CORE-AUTHZ`'s engine itself.
//! The notices read that log; the current state is asked through the engine's own read check.

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Entry, Outcome, Reads as Read, RefusalClass, Role, What,
};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{self as stm, StateMachine};
use rutent_core_orch::notice::{CurrentError, NoticeKey, Notices, Receiver, Transport};
use rutent_core_orch::rounds::Fresh;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::assembly::Staged;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::{Given, Scenario, Step};
use rutent_scenarios::kit::{self, AMOUNT, id};
use rutent_stm::Stm;

struct Key(SigningKey);

impl NoticeKey for Key {
    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

#[derive(Default)]
struct Wire(Vec<(Receiver, Vec<u8>)>);

impl Transport for Wire {
    fn send(&mut self, to: &Receiver, notice: &[u8]) -> bool {
        self.0.push((to.clone(), notice.to_vec()));
        true
    }
}

#[derive(Default)]
struct Ids(u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("notice-read-{}", self.0))
    }
}

fn one_order_run() -> (Core, Id) {
    let (core, orders) = Scenario::new(
        "one order, run",
        "AC-41",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Run)
    .core(1, &mut Reference);
    (core, orders[0].clone())
}

/// How many inputs of `order` in `entries` the state machine applies.
fn applied(entries: &[(rutent_contracts::core_internal::Receipt, Entry)], order: &Id) -> usize {
    let mut followed: Option<stm::Order> = None;
    let mut count = 0;
    for (_, entry) in entries {
        let Entry::Verified(v) = entry else { continue };
        let What::Input { input } = &v.what else {
            continue;
        };
        if input.subject.order.as_str() != order.as_str() {
            continue;
        }
        followed = match followed {
            None => Stm::open(input).inspect(|_| count += 1),
            Some(o) => {
                let next = Stm::transition(&o, input);
                if next.last_input.outcome == (stm::Outcome::Applied {}) {
                    count += 1;
                    Some(next)
                } else {
                    Some(o)
                }
            }
        };
    }
    count
}

/// An order run through the assembly is notified, every change of it, to its customer and the operator, from the
/// log the assembly's own log holds. Then a stranger, listed and acting for its own party, asks the engine for the
/// state of the order and of one that doesn't exist: the engine refuses both, each for its own reason, and records
/// both, but the stranger is told one and the same refusal, so it can't tell which order exists. The order's own
/// customer is answered.
// verifies: REQ-AUZ-7, REQ-ORD-20, AC-41
#[test]
fn stranger_is_told_one_refusal_whether_the_order_exists_or_not() {
    let (core, order) = one_order_run();
    let log = core.log().entries().to_vec();
    let mut notices = Notices::new(id("deployment-1"), kit::operator().party);
    let (mut key, mut wire) = (Key(SigningKey::from_bytes(&[9u8; 32])), Wire::default());
    let changes = applied(&log, &order);
    assert!(changes >= 3, "the run moved the order through its states");
    assert_eq!(
        notices.notify(&log, &mut key, &mut wire),
        Ok(2 * changes),
        "each change, to the customer and the operator"
    );
    let customer = Receiver {
        party: kit::customer().party,
        role: Role::Customer,
    };
    assert_eq!(
        wire.0.iter().filter(|(to, _)| to == &customer).count(),
        changes
    );

    let stranger = Actor {
        party: id("another-customer"),
        acts: id("user-9"),
        role: Role::Customer,
        kind: ActorKind::Person,
    };
    core.registers().list(&stranger);
    let clock = core.clock.clone();
    let mut ids = Ids::default();
    let before = core.log().entries().len();
    let mut ask = |core: &Core, reader: &Actor, about: &Id| {
        let log = core.log().entries().to_vec();
        notices.current(
            &log,
            &mut core.policy(),
            &clock,
            &mut ids,
            &mut key,
            reader,
            None,
            about,
        )
    };
    let theirs = ask(&core, &stranger, &order);
    let none = ask(&core, &stranger, &id("ord-that-never-was"));
    assert_eq!(theirs, Err(CurrentError::Refused));
    assert_eq!(theirs, none, "one refusal, whatever the order");

    // The engine's own answers stay in the log, and since the order's status tells a refused reader nothing of
    // whether the order exists (`T-020-08`) they are alike too.
    let refused: Vec<RefusalClass> = core.log().entries()[before..]
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => match (&v.what, &v.outcome) {
                (What::CheckRead { check }, Outcome::Refused { class })
                    if matches!(check.reads, Read::OrderState { .. }) =>
                {
                    Some(*class)
                }
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect();
    assert_eq!(refused.len(), 2, "both reads recorded, refused");
    assert_eq!(
        refused[0], refused[1],
        "the engine refuses each alike: a missing order is not told from another's"
    );

    let own = ask(&core, &kit::customer(), &order);
    assert!(own.is_ok(), "the order's customer is answered: {own:?}");
}
