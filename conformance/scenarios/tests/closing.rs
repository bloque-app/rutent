//! Evidence and closing on the running platform with simulated providers (`T-020-09`): each segment's evidence graded
//! through `CMP-EVIDENCE` over the simulated public networks the providers' movements are recorded on, a part closed
//! at its final deposit on the public record and the reserve's signed statement, the catalogue held to as it stands
//! before each segment, and a fee record per settled segment, over `T-020-08`'s kit. The routes the simulated providers
//! can't run (inside a private node, across two, one movement shared among orders) are graded with fakes in
//! `components/core-orch/tests/closing.rs`.

mod segments_support;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::{
    Behaviour, Entry as BookEntry, Role as SimRole, Spec, recorded_segments, transaction_of,
};
use rutent_contracts::core_internal::{Effect, FeeTo, ReplayFound, What};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{EvidenceGrade, Operation as ProviderOperation, Statement};
use rutent_contracts::router_api::{Amount, DecisionRecord, Id};
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_orch::closing::{Books, Closing, Evidenced, Moved, PublicRecords};
use rutent_core_orch::segments::{Answered, Modules, NoProxy, Ran, SegmentOutcome, Short};
use rutent_core_orch::snapshot::RouteRefusal;
use rutent_evidence::replay::RecordedSegment;
use rutent_evidence::{MovementId, NetworkRules, PublicNetwork, SimulatedNetwork, Status};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::Events;
use rutent_prov::proxy::RouteNotNamed;
use rutent_scenarios::sim::SimWorld;
use rutent_scenarios::sim::world::WORLD_AT;
use segments_support::*;

// What Core reads of the simulated world ----------------------------------------------------------------------------

/// The simulated public networks and the reserves' books, as they stood after the last operation carried out.
#[derive(Default)]
struct Seen {
    networks: BTreeMap<String, SimulatedNetwork>,
    books: BTreeMap<String, Vec<BookEntry>>,
}

/// The public networks, as Core reads them: each simulated network's movements, a movement tied to a segment by its
/// transaction, which the simulated providers give it as its identifier (`rutent_conformance::sim::movements_of`).
#[derive(Clone, Default)]
struct World_(Rc<RefCell<Seen>>);

impl PublicRecords for World_ {
    fn rules(&self, network: &str) -> Option<NetworkRules> {
        self.0
            .borrow()
            .networks
            .get(network)
            .map(PublicNetwork::rules)
    }

    fn status(&self, network: &str, id: &MovementId) -> Status {
        self.0
            .borrow()
            .networks
            .get(network)
            .map_or(Status::Unknown, |net| net.status(id))
    }

    fn movement(&self, network: &str, transaction: &Id) -> Option<Moved> {
        let id = MovementId::new(transaction.as_str())?;
        match self.status(network, &id) {
            Status::Pending(m) | Status::Final(m) => Some(Moved {
                id,
                destination: m.destination,
            }),
            Status::Unknown | Status::ReorgedOut => None,
        }
    }
}

/// Each reserve's ledger, as its publisher records the segments inside it: its book's entries for the transactions
/// asked, replayed as `rutent_conformance::sim::recorded_segments` reads a book.
impl Books for World_ {
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Vec<RecordedSegment>> {
        let reserve = node.strip_suffix("-ledger")?;
        let seen = self.0.borrow();
        let book: Vec<BookEntry> = seen
            .books
            .get(reserve)?
            .iter()
            .filter(|e| {
                transaction_of(&e.operation.statement)
                    .is_some_and(|tx| transactions.iter().any(|t| t.as_str() == tx))
            })
            .cloned()
            .collect();
        let spec = Spec::of(SimRole::Reserve, reserve);
        let segments: Vec<RecordedSegment> =
            recorded_segments(&spec, &SuiteContext::default(), &book)
                .into_iter()
                .map(|s| RecordedSegment {
                    asset_in: s.asset_in,
                    amount_in: s.amount_in,
                    asset_out: s.asset_out,
                    amount_out: s.amount_out,
                    fee: s.fee,
                })
                .collect();
        (!segments.is_empty()).then_some(segments)
    }
}

/// The platform, keeping what Core reads of the world current after each operation it carries out.
struct Recording<'a> {
    platform: &'a mut Platform<SimWorld>,
    seen: World_,
}

impl Recording<'_> {
    fn refresh(&mut self) {
        let world = &self.platform.world;
        let mut seen = self.seen.0.borrow_mut();
        // The kit's one reserve: its bank network, and its book.
        let bank = format!("{RESERVE}-bank");
        if let Some(net) = world.network(&bank) {
            seen.networks.insert(bank, net.clone());
        }
        seen.books
            .insert(RESERVE.to_owned(), world.journal(RESERVE).to_vec());
    }
}

impl Modules for Recording<'_> {
    fn begin_segment(
        &mut self,
        provider: &Id,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        Modules::begin_segment(&mut *self.platform, provider, transaction)
    }

    fn name_calls(
        &mut self,
        transaction: &Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        Modules::name_calls(&mut *self.platform, transaction, record, at, segment)
    }

    fn carry_out(
        &mut self,
        effect: &Effect,
        provider: &Id,
        operation: &Statement<ProviderOperation>,
    ) -> Answered {
        let answered = Modules::carry_out(&mut *self.platform, effect, provider, operation);
        self.refresh();
        answered
    }

    fn take_events(&mut self) -> Events {
        Modules::take_events(&mut *self.platform)
    }

    fn end_segment(&mut self, transaction: &Identifier) -> Option<SegmentRecord> {
        Modules::end_segment(&mut *self.platform, transaction)
    }

    fn end_calls(&mut self, transaction: &Identifier) {
        Modules::end_calls(&mut *self.platform, transaction)
    }

    fn staged(&self, provider: &Id) -> Option<String> {
        Modules::staged(&*self.platform, provider)
    }

    fn promote(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        Modules::promote(&mut *self.platform, effect, provider, by, at)
    }
}

/// Runs `core`'s order over `world`'s platform, graded through `CMP-EVIDENCE` with `closing`, against `catalogue`.
fn run(core: &mut Core, world: &mut World, catalogue: &Catalogue, closing: &mut Closing) -> Ran {
    let seen = World_::default();
    let session = world.context().session;
    let mut modules = Recording {
        platform: &mut world.fleet.platform,
        seen: seen.clone(),
    };
    modules.refresh();
    let mut grading = Evidenced {
        catalogue,
        records: &seen,
        books: &seen,
        closing,
    };
    core.run_with(
        catalogue,
        &mut modules,
        &mut NoProxy,
        &mut grading,
        &session,
    )
}

fn segments(ran: &Ran) -> &[rutent_core_orch::segments::Accounted] {
    match ran {
        Ran::Segments(done) => done,
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

/// The `evidence` entries of `segment`: (grade, whose).
fn evidence_of(core: &Core, segment: &str) -> Vec<(EvidenceGrade, String)> {
    core.log
        .borrow()
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::Evidence {
                order,
                segment: s,
                grade,
                whose,
                ..
            } if order.as_str() == core.named.order && s.as_str() == segment => {
                Some((*grade, whose.as_str().to_owned()))
            }
            _ => None,
        })
        .collect()
}

fn settled_inputs(core: &Core) -> Vec<String> {
    core.log
        .borrow()
        .inputs()
        .into_iter()
        .filter(|i| matches!(i.observed, Observed::SegmentSettled { .. }))
        .filter_map(|i| i.subject.segment.as_ref().map(|s| s.as_str().to_owned()))
        .collect()
}

fn state(core: &Core) -> OrderState {
    core.segments
        .order(&id(&core.named.order))
        .expect("followed")
        .state
}

fn bank() -> String {
    format!("{RESERVE}-bank")
}

// The tests ----------------------------------------------------------------------------------------------------

/// A reserve that says it deposited, its signed statement and nothing on its bank's public record, closes nothing:
/// the deposit doesn't settle, its value stays held, and the order isn't complete. Its evidence is still graded and
/// recorded, the withdrawal's as the reserve's signed statement. The same deposit, made on the public record, closes
/// the order on it and the reserve's statement, the record saying whose each is, and the private ledger it left is
/// replayed against the movement out.
// verifies: REQ-ORD-33, REQ-ORD-39
#[test]
fn statement_alone_does_not_close_an_order() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    world.fleet.platform.world.script_for(
        RESERVE,
        "deposit",
        Behaviour {
            unrecorded: true,
            ..Behaviour::default()
        },
    );
    let catalogue = world.catalogue.clone();
    let ran = run(&mut core, &mut world, &catalogue, &mut closing);
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert!(
        matches!(
            done[1].outcome,
            SegmentOutcome::AwaitsEvidence(Short::NoPublicRecord(_))
        ),
        "{done:?}"
    );
    assert_eq!(
        settled_inputs(&core),
        [WITHDRAWAL],
        "the deposit isn't settled"
    );
    assert_ne!(state(&core), OrderState::Complete);
    assert!(
        core.exposures.held().any(|h| h.segment.as_str() == DEPOSIT),
        "the deposit's value held"
    );
    assert_eq!(
        evidence_of(&core, WITHDRAWAL),
        [(EvidenceGrade::SignedByProvider, RESERVE.to_owned())],
        "graded and recorded: the reserve's signed statement, not proof"
    );
    assert_eq!(
        evidence_of(&core, DEPOSIT),
        [],
        "nothing settles the deposit"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");

    // The same route, the deposit on the public record: the order closes on it and the reserve's statement.
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    let catalogue = world.catalogue.clone();
    let ran = run(&mut core, &mut world, &catalogue, &mut closing);
    let done = segments(&ran);
    assert!(
        matches!(done[1].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert_eq!(settled_inputs(&core), [WITHDRAWAL, DEPOSIT]);
    assert_eq!(state(&core), OrderState::Complete);
    assert_eq!(
        evidence_of(&core, DEPOSIT),
        [
            (EvidenceGrade::PublicRecord, bank()),
            (EvidenceGrade::SignedByProvider, RESERVE.to_owned()),
        ],
        "the network's public record and the reserve's statement, each whose it is"
    );
    let replays: Vec<(Vec<String>, ReplayFound)> = core
        .log
        .borrow()
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::Replay { nodes, found, .. } => Some((
                nodes.iter().map(|n| n.as_str().to_owned()).collect(),
                found.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        replays,
        [(
            vec![format!("{RESERVE}-ledger")],
            ReplayFound::ConsistentWithTheExit {}
        )],
        "the ledger replayed on the way out: consistency, not proof"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A deposit waiting for its public record is taken on from where it stopped once the record is there: graded again,
/// it settles and the order closes, each entry it rests on, its result and its fee records appended once, and its hold
/// released. Where the log can't take its evidence, then its fee record, now, the next run appends what is missing and
/// nothing twice (`INV-8`).
// verifies: REQ-ORD-33, INV-8
#[test]
fn deposit_waiting_for_its_public_record_closes_once_it_is_recorded() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    world.fleet.platform.world.script_for(
        RESERVE,
        "deposit",
        Behaviour {
            unrecorded: true,
            ..Behaviour::default()
        },
    );
    let catalogue = world.catalogue.clone();
    let ran = run(&mut core, &mut world, &catalogue, &mut closing);
    let waiting = segments(&ran)[1].clone();
    assert!(
        matches!(
            waiting.outcome,
            SegmentOutcome::AwaitsEvidence(Short::NoPublicRecord(_))
        ),
        "{waiting:?}"
    );
    // Run again with nothing changed: it still waits, and nothing more is recorded.
    let before = core.log.borrow().entries.len();
    let again = run(&mut core, &mut world, &catalogue, &mut closing);
    assert!(matches!(
        segments(&again)[1].outcome,
        SegmentOutcome::AwaitsEvidence(_)
    ));
    assert_eq!(
        core.log.borrow().entries.len(),
        before,
        "nothing appended while it waits"
    );
    // The movement reaches the bank's public record.
    let transaction = waiting.transaction.clone().expect("a transaction");
    let book = world.fleet.platform.world.journal(RESERVE).to_vec();
    let deposit = book
        .iter()
        .find(|e| transaction_of(&e.operation.statement).as_deref() == Some(transaction.as_str()))
        .expect("the deposit in the reserve's book");
    let amount = serde_json::to_value(&deposit.operation.statement).unwrap()["amount"]
        .as_str()
        .unwrap()
        .to_owned();
    world
        .fleet
        .platform
        .world
        .network_mut(&bank())
        .expect("the bank's network")
        .record(
            MovementId::new(transaction.as_str()).unwrap(),
            rutent_evidence::Movement {
                destination: "acct-2".into(),
                asset: "COP".into(),
                amount,
            },
        )
        .expect("recorded");
    // The log can't take its evidence, then its fee record, now: each run takes it on from where it stopped.
    for on in [On::Evidence, On::Fee] {
        for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
            core.log
                .borrow_mut()
                .faults
                .push_back((on, Fault::Unavailable));
        }
        let session = world.context().session;
        let seen = World_::default();
        let mut modules = Recording {
            platform: &mut world.fleet.platform,
            seen: seen.clone(),
        };
        modules.refresh();
        let mut grading = Evidenced {
            catalogue: &catalogue,
            records: &seen,
            books: &seen,
            closing: &mut closing,
        };
        let stopped = core.try_run_with(
            &catalogue,
            &mut modules,
            &mut NoProxy,
            &mut grading,
            &session,
        );
        assert!(stopped.is_err(), "{on:?}: {stopped:?}");
        core.log.borrow_mut().faults.clear();
    }
    let ran = run(&mut core, &mut world, &catalogue, &mut closing);
    assert!(
        matches!(segments(&ran)[1].outcome, SegmentOutcome::Settled { .. }),
        "{ran:?}"
    );
    assert_eq!(state(&core), OrderState::Complete);
    assert_eq!(
        settled_inputs(&core),
        [WITHDRAWAL, DEPOSIT],
        "one settled input each"
    );
    let log = core.log.borrow();
    assert_eq!(log.count(is_result), 2, "one result each");
    let evidence: Vec<String> = log
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::Evidence { evidence, .. } => Some(evidence.as_str().to_owned()),
            _ => None,
        })
        .collect();
    let mut distinct = evidence.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        evidence.len(),
        "each evidence entry once: {evidence:?}"
    );
    assert_eq!(
        evidence_of(&core, DEPOSIT).len(),
        2,
        "the record and the reserve's statement"
    );
    assert_eq!(
        log.count(|w| matches!(w, What::Fee { segment, .. } if segment.as_str() == DEPOSIT)),
        2,
        "the reserve's fee and the operator's, once each"
    );
    drop(log);
    assert!(
        !core.exposures.held().any(|h| h.segment.as_str() == DEPOSIT),
        "its hold released"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The route was chosen when the reserve's tuples took its evidence; the operator then raises the floor. The tuple a
/// segment runs through is held to the floor as it stands before the segment is handed over: the withdrawal's floor
/// raised, nothing is handed over; the deposit's raised once the withdrawal settled, the deposit isn't.
// verifies: REQ-ORD-39
#[test]
fn provider_below_the_tuples_floor_is_not_routed_through() {
    for (raised, place) in [(Category::Reserve, 0), (Category::Reserve, 1)] {
        let mut world = World::of(&[RESERVE]);
        let mut core = Core::over(&mut world);
        let mut closing = Closing::new();
        let mut catalogue = world.catalogue.clone();
        let ad = world.installed[0]
            .advertisement
            .statement()
            .statement
            .clone();
        let key = tuples_of(&ad)
            .into_iter()
            .find(|k| {
                k.category == raised
                    && matches!(
                        (&k.origin, place),
                        (rutent_prov::catalogue::Endpoint::Outer(_), 0)
                            | (rutent_prov::catalogue::Endpoint::Inner(_), 1)
                    )
            })
            .expect("the tuple");
        // The reserve declares grade 3; the floor becomes grade 1.
        catalogue.set_evidence_floor(key, EvidenceGrade::PublicRecord);
        let before = world.fleet.platform.world.journal(RESERVE).len();
        let ran = run(&mut core, &mut world, &catalogue, &mut closing);
        let Ran::Refused { refused, .. } = &ran else {
            panic!("{ran:?}");
        };
        assert_eq!(refused.reason, RouteRefusal::BelowEvidenceFloor(place));
        let log = core.log.borrow();
        assert_eq!(
            log.count(is_effect),
            place,
            "no effect for the segment below the floor"
        );
        assert_eq!(
            world.fleet.platform.world.journal(RESERVE).len() - before,
            place,
            "the reserve moved nothing for it"
        );
        assert_eq!(
            log.count(|w| matches!(w, What::OrderRefusal { .. })),
            1,
            "the refusal recorded"
        );
        drop(log);
        core.admitted(&world.catalogue)
            .expect("the log admits every entry");
    }
}

/// The floor on the deposit's tuple is raised once the withdrawal settled: the deposit isn't routed through, and the
/// refusal is recorded once, however often the part runs. The value is evidenced at the ledger, where the withdrawal
/// left it and nothing more is in flight, so the order is one a recalculation takes up (`T-020-10`).
// verifies: REQ-ORD-39, INV-8
#[test]
fn refusal_mid_route_is_recorded_once_and_leaves_the_order_to_be_recalculated() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    let mut catalogue = world.catalogue.clone();
    let ad = world.installed[0]
        .advertisement
        .statement()
        .statement
        .clone();
    let deposit = tuples_of(&ad)
        .into_iter()
        .find(|k| matches!(k.origin, rutent_prov::catalogue::Endpoint::Inner(_)))
        .expect("the deposit's tuple");
    catalogue.set_evidence_floor(deposit, EvidenceGrade::PublicRecord);
    for _ in 0..3 {
        // Time passes between the runs, so a refusal built again would be another entry.
        core.clock.advance(60);
        let ran = run(&mut core, &mut world, &catalogue, &mut closing);
        assert!(
            matches!(&ran, Ran::Refused { refused, .. } if refused.reason == RouteRefusal::BelowEvidenceFloor(1)),
            "{ran:?}"
        );
    }
    assert_eq!(
        core.log
            .borrow()
            .count(|w| matches!(w, What::OrderRefusal { .. })),
        1,
        "recorded once"
    );
    let order = core
        .segments
        .order(&id(&core.named.order))
        .expect("followed");
    let (at, settled) =
        rutent_core_orch::recalc::evidenced(&order.parts[0]).expect("evidenced at the ledger");
    assert_eq!(settled.segment_id.as_str(), WITHDRAWAL);
    assert_eq!(
        at.node,
        rutent_contracts::provider_role::NodeRef::Inner(format!("{RESERVE}-ledger"))
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A route ending in a deposit to a destination Core reads no public record of, which no publisher declares a private
/// rail, could never close: it isn't routed, and nothing is handed over.
// verifies: REQ-ORD-33
#[test]
fn reserve_that_cant_give_a_public_record_is_not_routed_to() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    let catalogue = world.catalogue.clone();
    let session = world.context().session;
    // Core reads no network at all.
    let unread = World_::default();
    let mut modules = Recording {
        platform: &mut world.fleet.platform,
        seen: World_::default(),
    };
    let mut grading = Evidenced {
        catalogue: &catalogue,
        records: &unread,
        books: &unread,
        closing: &mut closing,
    };
    let ran = core.run_with(
        &catalogue,
        &mut modules,
        &mut NoProxy,
        &mut grading,
        &session,
    );
    let Ran::Refused { refused, .. } = &ran else {
        panic!("{ran:?}");
    };
    assert_eq!(refused.reason, RouteRefusal::NoPublicRecord(1));
    assert_eq!(core.log.borrow().count(is_effect), 0, "nothing handed over");
    assert!(
        world.fleet.platform.world.journal(RESERVE).is_empty(),
        "nothing moved"
    );
}

/// The operator's catalogue lists the reserve's ledger, but no holding account at it: a withdrawal into it would
/// leave the value between segments outside a holding account, so it isn't handed over. With the ledger's holding
/// account registered, the same route runs.
// verifies: INV-5
#[test]
fn value_between_segments_rests_only_in_a_holding_account() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    let unheld = listed_without_holdings(&world);
    assert!(
        world
            .catalogue
            .nodes()
            .holding(&format!("{RESERVE}-ledger"))
            .is_some()
    );
    assert!(
        unheld
            .nodes()
            .holding(&format!("{RESERVE}-ledger"))
            .is_none()
    );
    let ran = run(&mut core, &mut world, &unheld, &mut closing);
    let Ran::Refused { refused, .. } = &ran else {
        panic!("{ran:?}");
    };
    assert_eq!(refused.reason, RouteRefusal::NoHoldingAccount(0));
    assert_eq!(core.log.borrow().count(is_effect), 0, "nothing handed over");
    assert!(
        world.fleet.platform.world.journal(RESERVE).is_empty(),
        "nothing moved"
    );
    assert_eq!(core.exposures.held().count(), 0, "nothing held");

    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let catalogue = world.catalogue.clone();
    let ran = run(&mut core, &mut world, &catalogue, &mut closing);
    assert!(
        segments(&ran)
            .iter()
            .all(|a| matches!(a.outcome, SegmentOutcome::Settled { .. }))
    );
}

/// Each settled segment has a fee record: the fee its result states, attributed to its provider, and on the final
/// deposit the operator's fee, which the deposit's reserve keeps back, here zero. Each is its own segment's: a second
/// order over the same route records its own, none summing the first's, and nothing in the log or in Core holds a
/// running total.
// verifies: REQ-AUD-11
#[test]
fn every_settled_segment_has_its_fee_record_and_no_balance_is_kept() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let mut closing = Closing::new();
    let catalogue = world.catalogue.clone();
    let mut fees_by_order = Vec::new();
    for named in [Named::first(), Named::another(2)] {
        if named != Named::first() {
            core.take_up(named.clone());
        }
        let ran = run(&mut core, &mut world, &catalogue, &mut closing);
        assert!(
            segments(&ran)
                .iter()
                .all(|a| matches!(a.outcome, SegmentOutcome::Settled { .. }))
        );
        let log = core.log.borrow();
        let fees: Vec<(String, FeeTo, String, Amount)> = log
            .records()
            .into_iter()
            .filter_map(|(_, w)| match w {
                What::Fee {
                    order,
                    segment,
                    to,
                    party,
                    amount,
                    billed_separately,
                    ..
                } if order.as_str() == named.order => {
                    assert_eq!(
                        *billed_separately, None,
                        "the reserve says nothing of billing, so neither does its record"
                    );
                    Some((
                        segment.as_str().to_owned(),
                        *to,
                        party.as_str().to_owned(),
                        amount.clone(),
                    ))
                }
                _ => None,
            })
            .collect();
        // One per settled segment, its provider's, and the operator's on the deposit.
        let mut settled: Vec<String> = log
            .inputs()
            .into_iter()
            .filter(|i| i.subject.order.as_str() == named.order)
            .filter(|i| matches!(i.observed, Observed::SegmentSettled { .. }))
            .filter_map(|i| i.subject.segment.as_ref().map(|s| s.as_str().to_owned()))
            .collect();
        settled.sort();
        let mut by_provider: Vec<String> = fees
            .iter()
            .filter(|(_, to, party, _)| *to == FeeTo::Provider && party == RESERVE)
            .map(|(s, ..)| s.clone())
            .collect();
        by_provider.sort();
        assert_eq!(by_provider, settled);
        let operators: Vec<&(String, FeeTo, String, Amount)> = fees
            .iter()
            .filter(|(_, to, ..)| *to == FeeTo::Operator)
            .collect();
        assert_eq!(operators.len(), 1);
        assert_eq!(operators[0].0, named.deposit);
        assert_eq!(
            operators[0].3.as_str(),
            "0",
            "the operator's formula charges zero here"
        );
        fees_by_order.push(
            fees.into_iter()
                .map(|(_, to, _, amount)| (to, amount))
                .collect::<Vec<_>>(),
        );
        drop(log);
        core.admitted(&world.catalogue)
            .expect("the log admits every entry");
    }
    assert_eq!(
        fees_by_order[0], fees_by_order[1],
        "the second order's records are its own segments' fees, not a total carried on"
    );
}

/// [`World::catalogue`] as the kit lists it, but with no holding account registered at any node.
fn listed_without_holdings(world: &World) -> Catalogue {
    let context = world.context();
    let mut catalogue = Catalogue::new();
    catalogue.set_context(context.deployment.clone(), &context.protocol_version);
    for installed in &world.installed {
        let statement = installed.advertisement.statement();
        let ad = &statement.statement;
        let grant_id = format!("grant-{}", ad.provider);
        let capacity = ReversalCapacity::check(
            Bond {
                asset: "USDC".into(),
                amount: 1_000,
                held_by: "escrow-co".into(),
            },
            500,
            "ops-lead",
            Instant(1_000),
        )
        .expect("a capacity");
        let grant = Grant::new(GrantTerms {
            id: grant_id.clone(),
            provider: ad.provider.clone(),
            party: format!("{} Ltd", ad.publisher),
            granted_by: "operator-admin".into(),
            granted_at: Instant(1_000),
            expires_at: Instant(5_000_000_000),
            categories: vec![Category::Reserve],
            tuples: tuples_of(ad),
            reversal_capacity: capacity,
        })
        .expect("a grant");
        catalogue.grant(grant).expect("granted");
        catalogue
            .trust_advertising_key(&grant_id, statement.signature.key.clone())
            .expect("trusted");
        catalogue
            .join(installed.advertisement.bytes(), WORLD_AT)
            .expect("joined");
        catalogue.set_module_version(
            &ad.provider,
            rutent_contracts::router_api::Version::new("1.0.0").unwrap(),
        );
        for key in tuples_of(ad) {
            catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
        }
    }
    catalogue
}
