//! Recalculating the kit's order on the world's platform (`T-020-10`'s rounds and `Recalculations`), for the rows
//! that act on what a recalculation answers: `T-020-11` records a part a recalculation left no route.

use std::rc::Rc;

use rutent_contracts::core_internal::calls::{AppendFailure, Charges, PersonQuestions};
use rutent_contracts::core_internal::{
    Charge, ChargeAnswer, ChargeOutcome, PersonAnswer, PersonQuestion, Receipt,
};
use rutent_contracts::provider_role::{Operation as ProviderOperation, SafeInteger, Statement};
use rutent_contracts::router_api::{Id, Objective};
use rutent_core_orch::recalc::{self, Ask, RecalcError, Recalculation, Recalculations};
use rutent_core_orch::rounds::{self, History, Presence, Providers};
use rutent_core_orch::snapshot::Releases;
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::Handled;
use rutent_router::RouteFinder;
use rutent_scenarios::sim::SimWorld;

use super::{
    Core, General, PART, RESERVE, World, authorisation_of, bank, bounds, digest, id, ledger, price,
    tuple,
};

/// The providers' modules as the rounds reach them: each question handed to the platform as signed.
pub struct Asking<'a> {
    pub platform: &'a mut Platform<SimWorld>,
    pub asked: usize,
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

/// The caller's bound: always within it.
pub struct Within;

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
pub struct NoOne;

impl PersonQuestions for NoOne {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        panic!("no question about a person is asked: {question:?}");
    }
}

/// Core's recalculations of the kit's order, its history resumed from the log, and how many providers the last one
/// asked.
pub struct Rerouting {
    pub recalculations: Recalculations,
    pub history: History,
    pub releases: Releases,
    pub asked: usize,
}

impl Rerouting {
    /// The kit's current order followed for recalculation from the choice round that chose its route, each
    /// recalculation's records chained to that order's own decision record in the kit's log.
    pub fn of(core: &Core) -> Self {
        let mut recalculations = Recalculations::new();
        recalculations.follow(&id(&core.named.order), &core.record.request.request_id);
        let log = core.log.borrow();
        let seeded = log
            .entries
            .iter()
            .position(|(e, _)| {
                matches!(e, rutent_contracts::core_internal::Entry::Verified(v)
                    if matches!(&v.what, rutent_contracts::core_internal::What::DecisionRecord { order, .. }
                        if order.as_str() == core.named.order))
            })
            .expect("the kit seeds the order's decision record");
        let previous = log.receipt_at(seeded);
        Rerouting {
            recalculations,
            history: History::recorded(previous, Vec::new()),
            releases: Releases::new(),
            asked: 0,
        }
    }

    /// Recalculates the order's part, with the user present, over the world's catalogue and platform.
    pub fn recalculate(
        &mut self,
        core: &mut Core,
        world: &mut World,
    ) -> Result<Recalculation, RecalcError> {
        let context = world.context();
        let order = rounds::Order {
            order: id(&core.named.order),
            caller: super::core(),
            origin_reserve: id(RESERVE),
            presence: Presence::User {
                session: id(&context.session),
            },
            inputs: core.inputs.clone(),
            needs: Vec::new(),
        };
        let authorisation = authorisation_of(&core.named, core.floor.clone());
        let bounds = bounds(&context);
        let prices = [
            price(tuple(RESERVE, bank(RESERVE), ledger(RESERVE))),
            price(tuple(RESERVE, ledger(RESERVE), bank(RESERVE))),
        ];
        let (core_actor, operator) = (super::core(), core.operator.clone());
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
                part: &id(PART),
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
}

impl Rerouting {
    /// Runs the part's recalculated route over `modules`, as the segments run any part.
    pub fn run(
        core: &mut Core,
        catalogue: &rutent_prov::catalogue::Catalogue,
        modules: &mut dyn rutent_core_orch::segments::Modules,
        recalculated: &rutent_core_orch::recalc::Recalculated,
    ) -> Result<rutent_core_orch::segments::Ran, rutent_core_orch::segments::SegmentsError> {
        let session = rutent_conformance::role::SuiteContext::default().session;
        let (order, part, auth, origin) = (
            id(&core.named.order),
            id(PART),
            id(&core.named.authorisation()),
            id(RESERVE),
        );
        let (caller, core_actor, operator) = (super::core(), super::core(), core.operator.clone());
        let session = id(&session);
        let live = rutent_core_orch::segments::Live {
            order: &order,
            part: &part,
            segments: &recalculated.segments,
            record: &recalculated.record,
            at: 0,
            authorisation: &auth,
            origin_reserve: &origin,
            route: &recalculated.route,
            caller: &caller,
            core: &core_actor,
            operator: &operator,
            session: Some(&session),
            inputs: &core.inputs,
            floor: core.floor.as_ref(),
        };
        let mut general = General(Rc::clone(&core.log));
        let mut parts = rutent_core_orch::segments::Parts {
            clock: &*core.clock,
            catalogue,
            log: &mut general,
            evidence: &mut core.store,
            decisions: &mut core.policy,
            modules,
            uses: &mut rutent_core_orch::segments::NoProxy,
            grading: &mut super::Recorded,
            operator_key: &mut core.key,
            fresh: &mut core.ids,
            issued: &core.issued,
            exposures: &mut core.exposures,
        };
        core.segments.run_part(&mut parts, &live)
    }
}
