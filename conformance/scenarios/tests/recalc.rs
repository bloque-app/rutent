//! Recalculation on the running platform with simulated providers (`T-020-10`): a part's route recalculated by
//! Core's rounds from where its value is evidenced, recorded, and run, over `T-020-08`'s kit, with policy's engine
//! deciding each move and the log replayed as the log itself would admit it. The invariants' property tests are in
//! `components/core-orch/tests/recalc.rs`.

mod segments_support;

use std::rc::Rc;

use rutent_conformance::sim::{Act, Behaviour, Delivery};
use rutent_contracts::core_internal::calls::{AppendFailure, Charges, PersonQuestions};
use rutent_contracts::core_internal::{
    Action, Charge, ChargeAnswer, ChargeOutcome, PersonAnswer, PersonQuestion, Receipt, What,
};
use rutent_contracts::provider_role::{Operation as ProviderOperation, SafeInteger, Statement};
use rutent_contracts::router_api::{FeeFormula, Id, Objective, PositiveAmount};
use rutent_contracts::state_machine::StateMachine;
use rutent_contracts::state_machine::{self as stm, Observed, OrderState, SegmentState};
use rutent_core_orch::recalc::{
    self, Ask, NotRecalculable, RecalcError, Recalculated, Recalculation, Recalculations,
    passed_again,
};
use rutent_core_orch::rounds::{self, History, Presence, Providers};
use rutent_core_orch::segments::{Accounted, Live, NoProxy, Parts, Ran, SegmentOutcome};
use rutent_core_orch::snapshot::Releases;
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::Staging;
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::Handled;
use rutent_router::RouteFinder;
use rutent_scenarios::sim::SimWorld;
use rutent_stm::Stm;
use segments_support::*;

// The rounds' ports, over the platform --------------------------------------------------------------------------

/// The providers' modules as the rounds reach them: each question handed to the platform as signed.
struct Asking<'a> {
    platform: &'a mut Platform<SimWorld>,
    asked: usize,
}

impl Providers for Asking<'_> {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let operation: Statement<ProviderOperation> = serde_json::from_slice(question).ok()?;
        self.asked += 1;
        match self.platform.handle(provider.as_str(), &operation) {
            Handled::Answered(result) => serde_json::to_vec(&*result).ok(),
            Handled::Refused(_) => None,
        }
    }
}

/// The caller's bound: always within it. `T-021-05`'s charges aren't built; the rounds' own tests hold the bound.
struct Within;

impl Charges for Within {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        Ok(ChargeAnswer {
            request: charge.request.clone(),
            outcome: ChargeOutcome::Within {
                remaining: SafeInteger::try_from(1_000u64).unwrap(),
            },
            receipt: Receipt {
                log: id("policy"),
                index: SafeInteger::try_from(0u64).unwrap(),
                entry: digest(&"ef".repeat(32)),
            },
        })
    }
}

/// No question about a person: the simulated providers need no judgement.
struct NoOne;

impl PersonQuestions for NoOne {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        panic!("no question about a person is asked: {question:?}");
    }
}

// Recalculation around the kit's Core ------------------------------------------------------------------------------

/// Core's recalculations of the kit's order, its history resumed from the log, and what the rounds asked.
struct Recalc {
    recalculations: Recalculations,
    history: History,
    releases: Releases,
    floor: Option<PositiveAmount>,
    /// The operator's fee the recalculation runs under, where it isn't the kit's none.
    fee: Option<FeeFormula>,
    asked: usize,
}

impl Recalc {
    /// The kit's order followed for recalculation, from the choice round that chose its route; its history is the
    /// log's, so each recalculation's records chain to the order's last one (`REQ-AUD-4`).
    fn of(core: &Core, floor: Option<PositiveAmount>) -> Self {
        let mut recalculations = Recalculations::new();
        recalculations.follow(&id(ORDER), &core.record.request.request_id);
        // The order's last decision record in the kit's log: the one its route was chosen by.
        let previous = core.log.borrow().receipt_at(seeded_record(core));
        Recalc {
            recalculations,
            history: History::recorded(previous, Vec::new()),
            releases: Releases::new(),
            floor,
            fee: None,
            asked: 0,
        }
    }

    /// Recalculates the order's part, with the user present (`Presence::User`), over the world's catalogue and
    /// platform.
    fn recalculate(&mut self, core: &mut Core, world: &mut World) -> Recalculation {
        self.try_recalculate(core, world)
            .expect("the recalculation runs")
    }

    /// [`Recalc::recalculate`], with why it couldn't run.
    fn try_recalculate(
        &mut self,
        core: &mut Core,
        world: &mut World,
    ) -> Result<Recalculation, RecalcError> {
        self.try_recalculate_part(core, world, PART)
    }

    /// [`Recalc::try_recalculate`], of `part`.
    fn try_recalculate_part(
        &mut self,
        core: &mut Core,
        world: &mut World,
        part: &str,
    ) -> Result<Recalculation, RecalcError> {
        let context = world.context();
        let order = rounds::Order {
            order: id(ORDER),
            caller: segments_support::core(),
            origin_reserve: id(RESERVE),
            presence: Presence::User {
                session: id(&context.session),
            },
            inputs: core.inputs.clone(),
            needs: Vec::new(),
        };
        let authorisation = authorisation_of(&core.named, self.floor.clone());
        let mut bounds = bounds(&context);
        if let Some(fee) = &self.fee {
            bounds.operator_fee = fee.clone();
        }
        let prices = [
            price(tuple(RESERVE, bank(RESERVE), ledger(RESERVE))),
            price(tuple(RESERVE, ledger(RESERVE), bank(RESERVE))),
        ];
        let (core_actor, operator) = (segments_support::core(), core.operator.clone());
        let mut general = General(Rc::clone(&core.log));
        let mut providers = Asking {
            platform: &mut world.fleet.platform,
            asked: 0,
        };
        let mut parts = recalc::Parts {
            catalogue: &mut world.catalogue,
            clock: &*core.clock,
            exposures: &mut core.exposures,
            releases: &self.releases,
            log: &mut general,
            evidence: &mut core.store,
            charges: &mut Within,
            persons: &mut NoOne,
            providers: &mut providers,
            operator_key: &mut core.key,
            fresh: &mut core.ids,
            issued: &mut core.issued,
            segments: &mut core.segments,
        };
        let done = self.recalculations.recalculate::<RouteFinder>(
            &mut parts,
            Ask {
                order: &order,
                part: &id(part),
                authorisation: &authorisation,
                objective: &Objective::LowestTotalCost {},
                bounds: &bounds,
                prices: &prices,
                core: &core_actor,
                operator: &operator,
            },
            &mut self.history,
        );
        self.asked = providers.asked;
        done
    }

    /// Runs the part's recalculated route on the world's platform, its floor checked at each settled segment.
    fn run(&mut self, core: &mut Core, world: &mut World, recalculated: &Recalculated) -> Ran {
        self.run_route(
            core,
            world,
            PART,
            &recalculated.segments,
            &recalculated.record,
            &recalculated.route,
        )
        .expect("the recalculated part runs")
    }

    /// Runs `part` over `segments` of `record`'s chosen route, ending at `end`, on the world's platform.
    fn run_route(
        &mut self,
        core: &mut Core,
        world: &mut World,
        part: &str,
        segments: &[Id],
        record: &rutent_contracts::router_api::DecisionRecord,
        end: &rutent_contracts::core_internal::RouteEnd,
    ) -> Result<Ran, rutent_core_orch::segments::SegmentsError> {
        let context = world.context();
        let (order, part, auth, origin) = (
            id(ORDER),
            id(part),
            id(&core.named.authorisation()),
            id(RESERVE),
        );
        let (caller, core_actor, operator) = (
            segments_support::core(),
            segments_support::core(),
            core.operator.clone(),
        );
        let session = id(&context.session);
        let live = Live {
            order: &order,
            part: &part,
            segments,
            record,
            at: 0,
            authorisation: &auth,
            origin_reserve: &origin,
            route: end,
            caller: &caller,
            core: &core_actor,
            operator: &operator,
            session: Some(&session),
            inputs: &core.inputs,
            floor: self.floor.as_ref(),
        };
        let mut general = General(Rc::clone(&core.log));
        let mut parts = Parts {
            clock: &*core.clock,
            catalogue: &world.catalogue,
            log: &mut general,
            evidence: &mut core.store,
            decisions: &mut core.policy,
            modules: &mut world.fleet.platform,
            uses: &mut NoProxy,
            grading: &mut Recorded,
            operator_key: &mut core.key,
            fresh: &mut core.ids,
            issued: &core.issued,
            exposures: &mut core.exposures,
        };
        core.segments.run_part(&mut parts, &live)
    }
}

/// Where the kit's log holds the order's decision record, which its route was chosen by.
fn seeded_record(core: &Core) -> usize {
    core.log
        .borrow()
        .entries
        .iter()
        .position(|(e, _)| {
            matches!(e, rutent_contracts::core_internal::Entry::Verified(v)
                if matches!(v.what, What::DecisionRecord { .. }))
        })
        .expect("the kit seeds the order's decision record")
}

fn segments(ran: &Ran) -> &[Accounted] {
    match ran {
        Ran::Segments(done) => done,
        Ran::Refused { refused, .. } => panic!("refused: {refused:?}"),
    }
}

fn recalculated(done: Recalculation) -> Recalculated {
    match done {
        Recalculation::Recalculated(r) => *r,
        other => panic!("not recalculated: {other:?}"),
    }
}

fn part(core: &Core) -> stm::Part {
    core.segments.order(&id(ORDER)).unwrap().parts[0].clone()
}

fn handled(world: &World) -> usize {
    world.fleet.platform.operations_received(RESERVE)
}

fn moves(core: &Core) -> Vec<String> {
    core.log
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|v| match &v.what {
            What::Effect {
                action: Action::MoveValue { segment, .. },
                ..
            } => Some(segment.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

fn in_flight(core: &Core) -> String {
    let order = id(ORDER);
    core.exposures
        .held()
        .filter(|h| h.order == order)
        .count()
        .to_string()
}

/// Whether a segment failed: by its provider's word, or refused by it finally, which counts as failed.
fn failed(outcome: &SegmentOutcome) -> bool {
    matches!(outcome, SegmentOutcome::Failed | SegmentOutcome::Refused(_))
}

/// The order with its withdrawal settled and its deposit refused finally by the reserve, failed by its own word, the value left at the
/// reserve's ledger in full.
fn deposit_failed(floor: Option<PositiveAmount>) -> (World, Core, Recalc) {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::under(&mut world, floor.clone());
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let ran = core.run(&mut world);
    let done = segments(&ran);
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert!(failed(&done[1].outcome), "{done:?}");
    let recalc = Recalc::of(&core, floor);
    (world, core, recalc)
}

// The tests -----------------------------------------------------------------------------------------------------

/// The deposit fails at its reserve, by its own word: nothing of the value moved on, so it sits at the reserve's
/// ledger in full, where the withdrawal's settlement evidences it. Core recalculates from there, automatically and
/// with no yes asked, moving all of it: the failed provider's segment covered only the value's return to its origin,
/// which a failed deposit leaves it at, and the new route's charges are the user's, off what it delivers. Its hold
/// is released once the new route takes the value, so the value counts once in flight.
// verifies: REQ-ORD-22, INV-2
#[test]
fn failed_provider_pays_only_the_return_to_its_origin() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    assert_eq!(in_flight(&core), "1", "the failed deposit is held");
    // The operator's fee on what the new route delivers: the user's cost of the recalculation.
    recalc.fee = Some(flat_fee("10000"));
    let done = recalculated(recalc.recalculate(&mut core, &mut world));
    assert!(recalc.asked > 0, "the rounds asked the reserve");
    assert_eq!(done.from, endpoint(ledger(RESERVE)));
    assert_eq!(
        done.amount.amount().as_str(),
        AMOUNT,
        "all that is evidenced"
    );
    let operations = &done.record.chosen().unwrap().parts[0].operations;
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].amount_in.amount().as_str(), AMOUNT);
    assert_eq!(done.route.destination, route_end().destination);
    assert_eq!(done.route.to_asset, route_end().to_asset);
    assert_eq!(
        done.route.delivers.as_str(),
        "990000",
        "the user pays the new route: what it delivers less the operator's fee, and nothing of the failed one"
    );
    assert_eq!(done.recalculated, 1);
    assert_eq!(in_flight(&core), "0", "the old route's hold is released");
    // Its records chain to the order's last decision record, the one its route was chosen by (`REQ-AUD-4`).
    let previous: Vec<Option<Receipt>> = core
        .log
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|v| match &v.what {
            What::DecisionRecord { previous, .. } => Some(previous.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        previous.len(),
        3,
        "the kit's, the shortlist's and the choice's"
    );
    assert_eq!(
        previous[1],
        Some(core.log.borrow().receipt_at(seeded_record(&core)))
    );
    assert!(previous[2].is_some());
    let Observed::RouteRecalculated { segments: plan, .. } = &done.input.observed else {
        panic!("{:?}", done.input);
    };
    assert_eq!(
        plan[0].provider.as_str(),
        RESERVE,
        "its plan names its provider"
    );
    // The new route runs: its deposit moves all of it and settles.
    let ran = recalc.run(&mut core, &mut world, &done);
    let ran = segments(&ran);
    assert_eq!(ran.len(), 1);
    let SegmentOutcome::Settled { amount_out, .. } = &ran[0].outcome else {
        panic!("{ran:?}");
    };
    assert_eq!(amount_out.as_str(), AMOUNT);
    assert_eq!(part(&core).state, OrderState::Complete);
    assert_eq!(in_flight(&core), "0");
    assert_eq!(
        moves(&core),
        vec![
            WITHDRAWAL.to_owned(),
            DEPOSIT.to_owned(),
            done.segments[0].as_str().to_owned()
        ]
    );
    let admitted = core
        .admitted(&world.catalogue)
        .expect("the log admits every entry, the recalculation from its evidenced value included");
    // The new route's move is admitted on `move-segment-is-in-its-orders-plan`, its provider the one the
    // recalculated plan names; were the plan to name another, the same move would be refused on that row's
    // provider field.
    let history: Vec<(Receipt, rutent_contracts::core_internal::Entry)> = admitted
        .iter()
        .enumerate()
        .map(|(i, e)| (core.log.borrow().receipt_at(i), e.clone()))
        .collect();
    let at = history
        .iter()
        .position(|(_, e)| {
            matches!(e, rutent_contracts::core_internal::Entry::Verified(v)
                if matches!(&v.what, What::Effect { action: Action::MoveValue { segment, .. }, .. }
                    if *segment == done.segments[0]))
        })
        .expect("the new route's move");
    let mut elsewhere = history[..at].to_vec();
    for (_, entry) in &mut elsewhere {
        if let rutent_contracts::core_internal::Entry::Verified(v) = entry
            && let What::Input { input } = &mut v.what
            && let Observed::RouteRecalculated { segments, .. } = &mut input.observed
        {
            segments[0].provider = stm::Id::new("sim-reserve-z").unwrap();
        }
    }
    rutent_contracts::core_internal::admit_entry(
        &history[..at],
        &history[at].1,
        rutent_contracts::core_internal::Through::General,
        &operator().acts,
    )
    .expect("the move by the provider its recalculated plan names is admitted");
    let refused = rutent_contracts::core_internal::admit_entry(
        &elsewhere,
        &history[at].1,
        rutent_contracts::core_internal::Through::General,
        &operator().acts,
    )
    .expect_err("a move by a provider the plan doesn't name is refused");
    assert_eq!(
        refused.0,
        "a value-moving effect is made by the provider its plan names for the segment"
    );
}

/// Once the route is recalculated, the failed deposit is retired: running the old route again hands nothing over,
/// a late report about it moves nothing, and the value is moved once from the ledger, by the new route.
// verifies: INV-4
#[test]
fn late_report_on_a_retired_segment_moves_nothing_on_the_platform() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    let done = recalculated(recalc.recalculate(&mut core, &mut world));
    // The failed deposit's provider reports late that it delivered after all.
    let late = rutent_contracts::state_machine::RecordedInput {
        record_id: stm::RecordId::new("rec-late").unwrap(),
        recorded_at: done.input.recorded_at.clone(),
        time_source: stm::Id::new("core_clock").unwrap(),
        source: done.input.source.clone(),
        standing: stm::Standing::Stated,
        subject: stm::Subject {
            order: stm::Id::new(ORDER).unwrap(),
            part: Some(stm::Id::new(PART).unwrap()),
            segment: Some(stm::Id::new(DEPOSIT).unwrap()),
        },
        observed: Observed::SegmentSettled {
            evidence: stm::Id::new("ev-late").unwrap(),
        },
    };
    // Before the new route moves: the report is late, and the value is still at the ledger, for the new route.
    let followed = core.segments.order(&id(ORDER)).unwrap().clone();
    let after = Stm::transition(&followed, &late);
    assert_eq!(after.last_input.outcome, stm::Outcome::Late {});
    assert_eq!(after.parts, followed.parts, "late: it moves nothing");
    assert!(!core.segments.observe(&*core.clock, &late));
    // Nor any other input from outside the segments: only a recalculation moves Core's followed order there.
    let bound = rutent_contracts::state_machine::RecordedInput {
        record_id: stm::RecordId::new("rec-bound").unwrap(),
        subject: stm::Subject {
            segment: Some(stm::Id::new(done.segments[0].as_str()).unwrap()),
            ..late.subject.clone()
        },
        observed: Observed::ProviderBound {},
        ..late.clone()
    };
    assert!(!core.segments.observe(&*core.clock, &bound));
    let ran = recalc.run(&mut core, &mut world, &done);
    assert!(matches!(
        segments(&ran)[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    let before = (
        handled(&world),
        moves(&core),
        core.log.borrow().entries.len(),
    );
    // The old route, run again: every segment stands as it was accounted for, and nothing is handed over.
    let again = core.run(&mut world);
    assert!(
        segments(&again)
            .iter()
            .all(|a| !matches!(a.outcome, SegmentOutcome::NotHandedOver(_))),
        "{again:?}"
    );
    assert_eq!(
        (
            handled(&world),
            moves(&core),
            core.log.borrow().entries.len()
        ),
        before,
        "nothing was handed over, decided or recorded"
    );
    // After the order completes, the same report is after its end.
    let followed = core.segments.order(&id(ORDER)).unwrap().clone();
    let after = Stm::transition(&followed, &late);
    assert!(matches!(
        after.last_input.outcome,
        stm::Outcome::Late {} | stm::Outcome::NotApplicable { .. }
    ));
    assert_eq!(after.parts, followed.parts);
    assert_eq!(
        part(&core).retired_segments,
        vec![stm::Id::new(DEPOSIT).unwrap()]
    );
    assert_eq!(
        moves(&core),
        vec![
            WITHDRAWAL.to_owned(),
            DEPOSIT.to_owned(),
            done.segments[0].as_str().to_owned()
        ],
        "each segment moved once, and the value left the ledger once, by the new route"
    );
    // No second recalculation: the part is complete.
    assert_eq!(
        recalc.recalculate(&mut core, &mut world),
        Recalculation::NotNow(NotRecalculable::Ended(OrderState::Complete))
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A deposit whose provider is silent, says its outcome is unknown, or answers too late holds the value in flight:
/// no route is recalculated, nothing is asked and nothing recorded, and neither the provider's grant revoked (it
/// stops being offered) nor its version marked stale releases it.
// verifies: REQ-ORD-2, INV-4
#[test]
fn silent_unknown_unadvertised_or_stale_deposit_releases_no_route() {
    for behaviour in [
        Behaviour {
            silent: true,
            ..Behaviour::default()
        },
        Behaviour::act(Act::Unknown),
        Behaviour {
            late: true,
            ..Behaviour::default()
        },
    ] {
        let mut world = World::of(&[RESERVE]);
        admit_release(&mut world, "1.1.0", Staging::new(1, 0, 0));
        let mut core = Core::over(&mut world);
        world
            .fleet
            .world()
            .script_for(RESERVE, "deposit", behaviour.clone());
        let ran = core.run(&mut world);
        assert!(
            matches!(segments(&ran)[0].outcome, SegmentOutcome::Settled { .. }),
            "{behaviour:?}"
        );
        let mut recalc = Recalc::of(&core, None);
        let entries = core.log.borrow().entries.len();
        let held = |done: &Recalculation| {
            matches!(
                done,
                Recalculation::NotNow(NotRecalculable::InFlight { segment, state })
                    if segment.as_str() == DEPOSIT
                        && matches!(state, SegmentState::Started | SegmentState::OutcomeUnknown)
            )
        };
        let done = recalc.recalculate(&mut core, &mut world);
        assert!(held(&done), "{behaviour:?}: {done:?}");
        // The provider stops being offered: its grant is revoked.
        world
            .catalogue
            .revoke(&format!("grant-{RESERVE}"), Instant(START))
            .expect("revoked");
        let done = recalc.recalculate(&mut core, &mut world);
        assert!(held(&done), "{behaviour:?}: {done:?}");
        // Its version is marked stale.
        let at = Instant(START);
        world
            .fleet
            .platform
            .promote(RESERVE, "ops-lead", at)
            .expect("1.1.0 in force");
        world
            .fleet
            .platform
            .mark_stale(RESERVE, "1.0.0", "ops-lead", at)
            .expect("1.0.0 stale");
        let done = recalc.recalculate(&mut core, &mut world);
        assert!(held(&done), "{behaviour:?}: {done:?}");
        assert_eq!(recalc.asked, 0, "{behaviour:?}: no one was asked");
        assert_eq!(
            core.log.borrow().entries.len(),
            entries,
            "{behaviour:?}: nothing was recorded"
        );
        assert_eq!(recalc.recalculations.recalculated(&id(ORDER)), 0);
    }
}

/// The deposit fails again and again: each time Core recalculates from the ledger, until its recalculations reach
/// their bound; past it the order has no route left, and nothing more is asked or recorded.
// verifies: REQ-ORD-4
#[test]
fn recalculation_past_its_bound_leaves_no_route() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    for n in 1..=recalc::RECALCULATIONS {
        world
            .fleet
            .world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
        let done = recalculated(recalc.recalculate(&mut core, &mut world));
        assert_eq!(done.recalculated, n);
        let ran = recalc.run(&mut core, &mut world, &done);
        assert!(failed(&segments(&ran)[0].outcome), "{ran:?}");
    }
    let entries = core.log.borrow().entries.len();
    let done = recalc.recalculate(&mut core, &mut world);
    assert_eq!(
        done,
        Recalculation::NoRouteLeft {
            recalculated: recalc::RECALCULATIONS
        }
    );
    assert_eq!(recalc.asked, 0, "past the bound no one is asked");
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    assert_eq!(
        core.log
            .borrow()
            .inputs()
            .iter()
            .filter(|i| matches!(i.observed, Observed::RouteRecalculated { .. }))
            .count(),
        recalc::RECALCULATIONS
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The only route from the ledger is the deposit at the reserve whose deposit just failed there: the
/// recalculation passes that provider again, with the value no further on, and says so; the same is read from the
/// log's inputs alone. A second failure there names both earlier segments.
// verifies: REQ-ORD-4
#[test]
fn provider_passed_again_without_progress_is_visible() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let first = recalculated(recalc.recalculate(&mut core, &mut world));
    assert_eq!(first.passed_again.len(), 1, "{:?}", first.passed_again);
    assert_eq!(first.passed_again[0].provider.as_str(), RESERVE);
    assert_eq!(
        first.passed_again[0].before,
        vec![stm::Id::new(DEPOSIT).unwrap()]
    );
    let Observed::RouteRecalculated { segments: plan, .. } = &first.input.observed else {
        panic!();
    };
    let at = first.from.clone();
    let inputs: Vec<_> = core.log.borrow().inputs().into_iter().cloned().collect();
    let before_it: Vec<_> = inputs
        .iter()
        .take_while(|i| **i != first.input)
        .cloned()
        .collect();
    assert_eq!(
        passed_again(&before_it, &stm::Id::new(PART).unwrap(), &at, plan),
        first.passed_again,
        "the log's inputs show it"
    );
    let ran = recalc.run(&mut core, &mut world, &first);
    assert!(failed(&segments(&ran)[0].outcome));
    let second = recalculated(recalc.recalculate(&mut core, &mut world));
    assert_eq!(
        second.passed_again[0].before,
        vec![
            stm::Id::new(DEPOSIT).unwrap(),
            stm::Id::new(first.segments[0].as_str()).unwrap()
        ]
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The customer will take no less than 995 000 at the destination. The withdrawal settles 1 % short: 990 000 at the
/// ledger, and the deposit still ahead, quoted at no charge, would leave 990 000. The floor is checked there, at the
/// settled segment, before the deposit: the order waits for the customer, nothing more is handed over, and no route
/// is recalculated past it. Above the floor, the same run goes on.
// verifies: REQ-ORD-27
#[test]
fn settled_segment_that_crosses_the_floor_makes_the_order_wait() {
    let run_under = |floor: &str| {
        let floor = PositiveAmount::new(floor).unwrap();
        let mut world = World::of(&[RESERVE]);
        let mut core = Core::under(&mut world, Some(floor.clone()));
        world.fleet.world().script_for(
            RESERVE,
            "withdrawal",
            Behaviour::delivering(Delivery::Short { bps: 100 }),
        );
        let recalc = Recalc::of(&core, Some(floor));
        let (order, part, auth, origin, end) = (
            id(ORDER),
            id(PART),
            id(&core.named.authorisation()),
            id(RESERVE),
            route_end(),
        );
        let ids = [id(WITHDRAWAL), id(DEPOSIT)];
        let (caller, core_actor, operator) = (
            segments_support::core(),
            segments_support::core(),
            core.operator.clone(),
        );
        let session = id(&world.context().session);
        let record = core.record.clone();
        let inputs = core.inputs.clone();
        let live = Live {
            order: &order,
            part: &part,
            segments: &ids,
            record: &record,
            at: 0,
            authorisation: &auth,
            origin_reserve: &origin,
            route: &end,
            caller: &caller,
            core: &core_actor,
            operator: &operator,
            session: Some(&session),
            inputs: &inputs,
            floor: recalc.floor.as_ref(),
        };
        let mut general = General(Rc::clone(&core.log));
        let mut parts = Parts {
            clock: &*core.clock,
            catalogue: &world.catalogue,
            log: &mut general,
            evidence: &mut core.store,
            decisions: &mut core.policy,
            modules: &mut world.fleet.platform,
            uses: &mut NoProxy,
            grading: &mut Recorded,
            operator_key: &mut core.key,
            fresh: &mut core.ids,
            issued: &core.issued,
            exposures: &mut core.exposures,
        };
        let ran = core
            .segments
            .run_part(&mut parts, &live)
            .expect("the part runs");
        let waits = core.segments.waiting(&order, &part).cloned();
        (world, core, recalc, (ran, waits))
    };

    let (mut world, mut core, mut recalc, run) = run_under("995000");
    let done = segments(&run.0);
    assert_eq!(done.len(), 1, "the deposit is not handed over: {done:?}");
    let waits = run.1.expect("the order waits for the customer");
    assert_eq!(waits.after.as_str(), WITHDRAWAL);
    assert_eq!(waits.evidenced.amount().as_str(), "990000");
    assert_eq!(waits.left.as_ref().map(|l| l.as_str()), Some("990000"));
    assert_eq!(core.segments.waiting(&id(ORDER), &id(PART)), Some(&waits));
    assert_eq!(moves(&core), vec![WITHDRAWAL.to_owned()]);
    let entries = core.log.borrow().entries.len();
    assert_eq!(
        recalc.recalculate(&mut core, &mut world),
        Recalculation::NotNow(NotRecalculable::Held(OrderState::WaitingForCustomer))
    );
    assert_eq!(recalc.asked, 0);
    assert_eq!(core.log.borrow().entries.len(), entries);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");

    let (_, core, _, run) = run_under("985000");
    assert!(run.1.is_none());
    assert_eq!(segments(&run.0).len(), 2);
    assert_eq!(part(&core).state, OrderState::Complete);
}

fn flat_fee(flat: &str) -> FeeFormula {
    serde_json::from_value(serde_json::json!({
        "flat": flat, "proportional": { "numerator": "0", "denominator": "1" }
    }))
    .unwrap()
}

fn endpoint(value: serde_json::Value) -> rutent_contracts::router_api::Endpoint {
    serde_json::from_value(value).unwrap()
}

/// The log can't take the recalculation's input now: nothing moves on it, and the next try appends the input as it
/// was built, asking no one again, so the part is recalculated once.
// verifies: INV-8, INV-4
#[test]
fn recalculation_whose_append_failed_is_recorded_once() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Recalculated, Fault::Unavailable));
    }
    let first = recalc.try_recalculate(&mut core, &mut world);
    assert_eq!(first, Err(RecalcError::Append(AppendFailure::Unavailable)));
    assert!(recalc.asked > 0);
    assert_eq!(
        part(&core).retired_segments,
        Vec::<stm::Id>::new(),
        "nothing moved on it"
    );
    assert_eq!(in_flight(&core), "1", "the failed deposit is still held");
    let done = recalculated(recalc.recalculate(&mut core, &mut world));
    assert_eq!(recalc.asked, 0, "no one is asked again");
    assert_eq!(done.recalculated, 1);
    assert_eq!(
        core.log
            .borrow()
            .inputs()
            .iter()
            .filter(|i| matches!(i.observed, Observed::RouteRecalculated { .. }))
            .count(),
        1
    );
    assert_eq!(in_flight(&core), "0");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The deposit fails at the reserve, and the only route from the ledger, under the operator's fee of 10 000, would
/// leave the customer 990 000, below the floor of 995 000: the router drops it as below the floor, and the order
/// waits for the customer rather than take it. The routing's record shows why; no route is recorded.
// verifies: REQ-ORD-4, REQ-ORD-27
#[test]
fn recalculation_that_would_pass_the_floor_waits_for_the_customer() {
    let floor = PositiveAmount::new("995000").unwrap();
    let (mut world, mut core, mut recalc) = deposit_failed(Some(floor));
    recalc.fee = Some(flat_fee("10000"));
    let done = recalc.recalculate(&mut core, &mut world);
    let Recalculation::WaitsForCustomer { record } = &done else {
        panic!("{done:?}");
    };
    assert!(
        record
            .record
            .candidates()
            .iter()
            .all(|c| c.dropped == Some(rutent_contracts::router_api::DropReason::BelowFloor {})),
        "{:?}",
        record.record.candidates()
    );
    assert!(
        recalc
            .recalculations
            .waiting(&id(ORDER), &id(PART))
            .is_some()
    );
    assert!(
        core.log
            .borrow()
            .inputs()
            .iter()
            .all(|i| !matches!(i.observed, Observed::RouteRecalculated { .. })),
        "no route recorded"
    );
    assert_eq!(
        recalc.recalculate(&mut core, &mut world),
        Recalculation::NotNow(NotRecalculable::Held(OrderState::WaitingForCustomer))
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The reserve refuses the dry run of the only route from the ledger: the routing offers no route, the order has no
/// route left, and asking again routes nothing, so providers that refuse in turn can't make Core ask without end.
// verifies: REQ-ORD-4
#[test]
fn routing_that_offered_no_route_is_not_asked_again() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    world
        .fleet
        .world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let done = recalc.recalculate(&mut core, &mut world);
    assert!(matches!(done, Recalculation::NoRoute { .. }), "{done:?}");
    assert!(recalc.asked > 0);
    let entries = core.log.borrow().entries.len();
    let again = recalc.recalculate(&mut core, &mut world);
    assert_eq!(again, Recalculation::NoRouteLeft { recalculated: 0 });
    assert_eq!(recalc.asked, 0, "no one is asked again");
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The deposit's effect is recorded but the log's answers are lost, so the deposit is still `pending` in the order
/// Core follows, its value about to move. No recalculation goes around it: it counts as in flight. Resumed, the
/// deposit moves once, and the value is moved from one place only.
// verifies: INV-4, REQ-ORD-2
#[test]
fn deposit_whose_effect_may_be_recorded_is_not_recalculated_around() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::EffectOf(DEPOSIT), Fault::LostAnswer));
    }
    let session = world.context().session;
    let stopped = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut Recorded,
        &session,
    );
    assert_eq!(
        stopped,
        Err(rutent_core_orch::segments::SegmentsError::Append(
            AppendFailure::Unavailable
        ))
    );
    let deposit = part(&core)
        .segments
        .into_iter()
        .find(|s| s.segment_id.as_str() == DEPOSIT)
        .unwrap();
    assert_eq!(
        deposit.state,
        SegmentState::Pending,
        "its provider not yet bound"
    );
    let mut recalc = Recalc::of(&core, None);
    let entries = core.log.borrow().entries.len();
    let done = recalc.recalculate(&mut core, &mut world);
    assert!(
        matches!(&done, Recalculation::NotNow(NotRecalculable::HandedOver { segment }) if segment.as_str() == DEPOSIT),
        "{done:?}"
    );
    assert_eq!(recalc.asked, 0, "no one is asked");
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    // Resumed, the deposit moves once.
    let before = handled(&world);
    let ran = core.run(&mut world);
    assert!(matches!(
        segments(&ran)[1].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert_eq!(handled(&world) - before, 1, "one deposit");
    assert_eq!(part(&core).state, OrderState::Complete);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A segment the order Core follows has retired, its route recalculated, is never handed over or taken on again,
/// whatever it kept: the value moves only on the route that replaced it.
// verifies: INV-4
#[test]
fn retired_segment_is_never_handed_over() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::EffectOf(DEPOSIT), Fault::LostAnswer));
    }
    let session = world.context().session;
    let _ = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut Recorded,
        &session,
    );
    // A route recalculated from the ledger, recorded elsewhere, retires the deposit in the order Core follows.
    let plan = &route_chosen(&core.record);
    let Observed::RouteChosen { parts, .. } = &plan.observed else {
        unreachable!();
    };
    let mut again = parts[0].segments[1].clone();
    again.segment_id = stm::Id::new("p1-s2-again").unwrap();
    let recalculated = rutent_contracts::state_machine::RecordedInput {
        record_id: stm::RecordId::new("rec-again").unwrap(),
        subject: stm::Subject {
            order: stm::Id::new(ORDER).unwrap(),
            part: Some(stm::Id::new(PART).unwrap()),
            segment: None,
        },
        observed: Observed::RouteRecalculated {
            decision_record: stm::Id::new(core.record.record_id().as_str()).unwrap(),
            segments: vec![again],
        },
        ..plan.clone()
    };
    assert!(core.segments.observe(&*core.clock, &recalculated));
    assert_eq!(
        part(&core).retired_segments,
        vec![stm::Id::new(DEPOSIT).unwrap()]
    );
    let before = handled(&world);
    let resumed = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut NoProxy,
        &mut Recorded,
        &session,
    );
    assert_eq!(
        resumed,
        Err(rutent_core_orch::segments::SegmentsError::Retired(id(
            DEPOSIT
        )))
    );
    assert_eq!(handled(&world), before, "the retired deposit moved nothing");
}

/// A split order, both parts' deposits refused at the ledger. The order's recalculations are bounded together, a
/// recalculation waiting on the log counted among them: with two recorded and one waiting, the other part is told
/// there is no route left, and the log never holds more than the bound.
// verifies: REQ-ORD-4
#[test]
fn recalculations_of_a_split_order_waiting_on_the_log_count_against_its_bound() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::split(&mut world);
    let first = route_end();
    let record = core.record.clone();
    let mut recalc = Recalc::of(&core, None);
    for (part, ids) in [
        (PART, [WITHDRAWAL, DEPOSIT]),
        (SECOND_PART, [SECOND_WITHDRAWAL, SECOND_DEPOSIT]),
    ] {
        world
            .fleet
            .world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
        let ran = recalc
            .run_route(&mut core, &mut world, part, &ids.map(id), &record, &first)
            .expect("the part runs");
        assert!(failed(&segments(&ran)[1].outcome), "{ran:?}");
    }
    // Two recalculations recorded, each run and refused again.
    for _ in 0..2 {
        world
            .fleet
            .world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
        let done = recalculated(recalc.recalculate(&mut core, &mut world));
        let ran = recalc.run(&mut core, &mut world, &done);
        assert!(failed(&segments(&ran)[0].outcome));
    }
    assert_eq!(recalc::RECALCULATIONS, 3);
    // The third waits on the log.
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Recalculated, Fault::Unavailable));
    }
    assert!(recalc.try_recalculate(&mut core, &mut world).is_err());
    // The other part is past the bound.
    let other = recalc
        .try_recalculate_part(&mut core, &mut world, SECOND_PART)
        .unwrap();
    assert_eq!(other, Recalculation::NoRouteLeft { recalculated: 2 });
    let waiting = recalculated(recalc.recalculate(&mut core, &mut world));
    assert_eq!(waiting.recalculated, 3);
    let recorded = core
        .log
        .borrow()
        .inputs()
        .iter()
        .filter(|i| matches!(i.observed, Observed::RouteRecalculated { .. }))
        .count();
    assert_eq!(recorded, recalc::RECALCULATIONS);
    // Rebuilt from the log, the count is the same.
    let inputs: Vec<_> = core.log.borrow().inputs().into_iter().cloned().collect();
    let mut rebuilt = Recalculations::new();
    rebuilt.follow_recorded(&id(ORDER), &core.record.request.request_id, &inputs);
    assert_eq!(rebuilt.recalculated(&id(ORDER)), recalc::RECALCULATIONS);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A recalculation waits on the log while the part is recalculated elsewhere and completes. When the log can take
/// it again, Core checks the part first: its route is no longer the part's to replace, so nothing is recorded, no
/// hold is released and nothing is counted.
// verifies: INV-4, INV-8
#[test]
fn recalculation_retried_after_its_part_moved_on_is_not_recorded() {
    let (mut world, mut core, mut recalc) = deposit_failed(None);
    for _ in 0..3 {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Recalculated, Fault::Unavailable));
    }
    assert!(recalc.try_recalculate(&mut core, &mut world).is_err());
    // Recalculated elsewhere, and run to the end.
    let mut elsewhere = Recalc::of(&core, None);
    let done = recalculated(elsewhere.recalculate(&mut core, &mut world));
    let ran = elsewhere.run(&mut core, &mut world, &done);
    assert!(matches!(
        segments(&ran)[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    assert_eq!(part(&core).state, OrderState::Complete);
    let entries = core.log.borrow().entries.len();
    let retried = recalc.recalculate(&mut core, &mut world);
    assert_eq!(
        retried,
        Recalculation::NotNow(NotRecalculable::Ended(OrderState::Complete))
    );
    assert_eq!(core.log.borrow().entries.len(), entries, "nothing recorded");
    assert_eq!(recalc.recalculations.recalculated(&id(ORDER)), 0);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The floor can't be skipped: a part run through the segments themselves, as any caller runs one, waits at its
/// floor all the same, and running it again hands nothing more over.
// verifies: REQ-ORD-27
#[test]
fn part_run_through_the_segments_alone_waits_at_its_floor() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::under(&mut world, Some(PositiveAmount::new("995000").unwrap()));
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour::delivering(Delivery::Short { bps: 100 }),
    );
    let ran = core.run(&mut world);
    assert_eq!(segments(&ran).len(), 1, "the deposit is not handed over");
    let waits = core
        .segments
        .waiting(&id(ORDER), &id(PART))
        .cloned()
        .expect("the part waits");
    assert_eq!(waits.after.as_str(), WITHDRAWAL);
    assert_eq!(waits.evidenced.amount().as_str(), "990000");
    let before = handled(&world);
    let again = core.run(&mut world);
    assert_eq!(segments(&again).len(), 1);
    assert_eq!(handled(&world), before, "nothing more is handed over");
    assert_eq!(moves(&core), vec![WITHDRAWAL.to_owned()]);
    let mut recalc = Recalc::of(&core, Some(PositiveAmount::new("995000").unwrap()));
    assert_eq!(
        recalc.recalculate(&mut core, &mut world),
        Recalculation::NotNow(NotRecalculable::Held(OrderState::WaitingForCustomer))
    );
}
