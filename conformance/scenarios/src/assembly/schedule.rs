//! Core's schedule, as the assembly wires it (`T-020-48`, finding 4 of `M3`'s milestone review of 2026-11-24, and the
//! review of its first build).
//!
//! `core-orch`'s [`Schedule`](rutent_core_orch::schedule::Schedule) decides what is due and its
//! [`Jobs`](rutent_core_orch::schedule::Jobs) what each job does; this module answers only their
//! [`Ports`](rutent_core_orch::schedule::Ports), over what the assembly holds: the platform, the deployment's register
//! of holding accounts, the log, the evidence store, the notices channel, the clock and policy, and Core's own parts
//! as the assembly keeps them, lent with the ports they are reached with. No method here decides anything: each reads,
//! asks or lends. [`Core::tick`] is the one entry point, the one call the deployment keeps (on a timer), and the one
//! call a scenario makes on Core's clock.
//!
//! **A private node's account.** At a public node the account's module is the operator's plain wallet, registered in
//! [`Holdings`](rutent_prov::holding::wallet::Holdings). At a private node it is its publisher's module, which the
//! deployment's register of holding accounts names with the publisher and the asset
//! ([`Registers::publisher_accounts`](super::policy::Registers)), and which the register keeps across a reopen as it
//! keeps the rest. No simulated provider stands in for a publisher's holding account, so a scenario installs the plain
//! wallet as one ([`Core::install_publisher_account`]).

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{Comparisons, Measures, Operator, StuckOrders};
use rutent_contracts::core_internal::{Actor, Entry, Receipt};
use rutent_contracts::provider_role::Operation;
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_core_audit::compare::Compare;
use rutent_core_audit::log::General;
use rutent_core_orch::arrival::Arrivals;
use rutent_core_orch::closing::PriceDesk;
use rutent_core_orch::intake::Desk;
use rutent_core_orch::notice::{Channel, NoticeKey, Notices, Transport};
use rutent_core_orch::reversal::{Moving, Parts as ReversalParts, Reversals};
use rutent_core_orch::rounds::Fresh as _;
use rutent_core_orch::schedule::{
    AccountRead, Actors, Job, Jobs, Ports, Published, PublisherAccount, Round, Started, StuckLent,
};
use rutent_core_orch::segments::{Landings, NoProxy, Parts as SegmentParts, Segments};
use rutent_core_orch::snapshot::Clock;
use rutent_core_orch::stuck::{OperatorSigns, Parts as StuckParts, Stuck};
use rutent_prov::catalogue::Catalogue;
use rutent_prov::module::types::Handled;

use super::{AssemblyError, Core, CoreLog, Running};
use crate::kit::{self, RealKey, SharedClock};
use crate::sim::InstalledWallet;

/// The decimals of every asset the simulated world moves (`kit`): an account's reading is stated in them, as the
/// deployment's register names its asset.
const DECIMALS: u8 = 2;

impl Core {
    /// Core's own clock, in whole seconds: what the schedule's cadences count in. Reading it moves nothing.
    pub(super) fn clock_seconds(&self) -> u64 {
        u64::try_from(self.clock.peek() / 1_000_000_000).unwrap_or(u64::MAX)
    }

    /// One tick of Core's schedule (`T-020-48`): runs each job that is due on Core's clock, Core's own body of it over
    /// the ports the assembly answers, and answers what ran and what it came to. The deployment calls it on a timer; a
    /// scenario calls it where the deployment's timer would have. A job not yet due is left for a later tick.
    pub fn tick(&mut self) -> Round {
        let now = self.clock_seconds();
        let mut schedule = std::mem::take(&mut self.schedule);
        let round = schedule.tick(now, &mut Jobs::new(&mut Due(self)));
        self.schedule = schedule;
        // What the round answered, kept for the readers that ask after it.
        for tried in &round.statuses {
            self.life.status_asks.insert(
                (
                    tried.unknown.order.as_str().to_owned(),
                    tried.unknown.segment.as_str().to_owned(),
                ),
                match (&tried.status, &tried.asked) {
                    (Some(asked), _) => Ok(asked.clone()),
                    (None, Err(why)) => Err(why.clone()),
                    (None, Ok(_)) => Err("no question was put".to_owned()),
                },
            );
        }
        self.closed.took(&round.priced);
        let _ = self.feed();
        round
    }

    /// Sets how often `job` runs (the operator's setting of the job's cadence; `T-020-49` decides it).
    pub fn set_cadence(&mut self, job: Job, seconds: u64) {
        self.schedule.cadences_mut().set(job, seconds);
    }

    /// Sets how often a segment unknown at `provider` is asked its status again.
    pub fn set_status_cadence(&mut self, provider: &str, seconds: u64) {
        self.schedule
            .cadences_mut()
            .set_status_of(provider, seconds);
    }

    /// Installs the plain wallet as the account `publisher` keeps for the private node `node`, holding `asset`: the
    /// platform runs it, the catalogue records it as the node's holding account, and the deployment's register of
    /// holding accounts names it, its publisher and its asset, so the schedule reads it as the publisher's word, and
    /// reads it again after Core reopens. The node must be one a provider joined to the catalogue declares private.
    ///
    /// # Errors
    ///
    /// If the catalogue refuses the node or the way to its account.
    pub fn install_publisher_account(
        &mut self,
        node: &str,
        publisher: &str,
        asset: &str,
    ) -> Result<InstalledWallet, AssemblyError> {
        let wallet = self.fleet.install_wallet(&format!("{publisher}-account"));
        self.catalogue
            .register_private_holding(node, publisher, &wallet.provider)
            .map_err(|e| AssemblyError::Listing(format!("the publisher's account: {e:?}")))?;
        self.deployed
            .registers
            .borrow_mut()
            .publisher_accounts
            .insert(
                node.to_owned(),
                PublisherAccount {
                    node: node.to_owned(),
                    publisher: publisher.to_owned(),
                    module: wallet.provider.clone(),
                    asset: asset.to_owned(),
                    decimals: DECIMALS,
                },
            );
        Ok(wallet)
    }

    /// The private nodes' accounts the deployment's register names, listed again in the reopened catalogue, as
    /// [`Core::install_publisher_account`] listed them (the review's S1).
    pub(super) fn relist_publisher_accounts(&mut self) {
        let accounts: Vec<PublisherAccount> = self
            .deployed
            .registers
            .borrow()
            .publisher_accounts
            .values()
            .cloned()
            .collect();
        for account in accounts {
            let _ = self.catalogue.register_private_holding(
                &account.node,
                &account.publisher,
                &account.module,
            );
        }
    }

    /// Confirms the failed outcome a mover reported for the unknown `segment` of `order`, as Core does
    /// ([`rutent_core_orch::schedule::confirm_reported_failure`], `AC-6`): `confirmer` is another actor than the
    /// reporter, `report` the report's version and the statement it rests on (what the schedule's round told), and
    /// `other` another maker's statement about the move's transaction, which bears the report out.
    ///
    /// # Errors
    ///
    /// If the order hasn't started, the statement isn't taken as evidence, or the confirmation is refused.
    pub fn confirm_reported_failure(
        &mut self,
        order: &Id,
        segment: &Id,
        confirmer: &Actor,
        report: (u64, &Receipt),
        other: &[u8],
    ) -> Result<rutent_core_orch::stuck::Moved, super::StepError> {
        let confirmed = rutent_core_orch::schedule::confirm_reported_failure(
            &mut Due(self),
            order,
            segment,
            confirmer,
            report,
            other,
        );
        self.feed()?;
        confirmed.map_err(|why| match why {
            rutent_core_orch::schedule::NotConfirmed::Refused(why) => {
                super::StepError::Resolve(why)
            }
            rutent_core_orch::schedule::NotConfirmed::NotStarted => {
                super::StepError::NotNow("the order started")
            }
            _ => super::StepError::NotNow("the statement is taken as evidence"),
        })
    }
}

/// The deployment's ports over a [`Core`]: each reads, asks or lends, and decides nothing.
struct Due<'a>(&'a mut Core);

impl Due<'_> {
    /// The operator's key, signing as the operator at Core's clock, for `with`; the world's clock set to Core's.
    fn as_operator<T>(
        &mut self,
        with: impl FnOnce(&mut Core, &OperatorSigns<'_>) -> T,
    ) -> Option<T> {
        let core = &mut *self.0;
        core.fleet.platform.world.set_clock(core.clock.instant());
        let made_at = core.clock.now().timestamp()?;
        let mut key = RealKey::assembly();
        let bounds = core.bounds.clone();
        let operator = OperatorSigns::new(&mut key, &bounds, kit::operator().acts, made_at);
        Some(with(core, &operator))
    }
}

impl Ports for Due<'_> {
    fn clock(&self) -> &dyn Clock {
        &self.0.clock
    }

    fn actors(&self) -> Actors {
        Actors {
            core: kit::core(),
            operator: kit::operator(),
        }
    }

    fn started(&self) -> &BTreeMap<String, Started> {
        &self.0.started
    }

    fn recalculated(&self, order: &Id) -> Option<(DecisionRecord, Vec<Id>)> {
        self.0.life.routes.get(order.as_str()).cloned()
    }

    fn history(&self) -> Vec<(Receipt, Entry)> {
        self.0.log.borrow().entries().to_vec()
    }

    fn decision_record(&self, order: &Id, id: &Id) -> Option<DecisionRecord> {
        self.0.record_of(order, id).ok()
    }

    fn segments(&self) -> &Segments {
        &self.0.segments
    }

    fn stuck(&self) -> &Stuck {
        &self.0.stuck
    }

    fn reversals(&self) -> &Reversals {
        &self.0.life.reversals
    }

    fn catalogue(&self) -> &Catalogue {
        &self.0.catalogue
    }

    fn fresh(&mut self) -> Id {
        self.0.fresh.id()
    }

    fn late_answers(&mut self, transactions: &BTreeSet<String>) -> Vec<(String, Vec<u8>)> {
        let core = &mut *self.0;
        core.fleet.platform.world.set_clock(core.clock.instant());
        core.fleet
            .late_answers(transactions)
            .into_iter()
            .map(|(_, transaction, bytes)| (transaction, bytes))
            .collect()
    }

    fn holding_readings(&mut self) -> Vec<AccountRead> {
        self.as_operator(|core, operator| {
            let mut holdings = core.holdings.borrow_mut();
            let readings = holdings.readings(&mut core.fleet.platform, operator);
            readings
                .into_iter()
                .map(|(node, read)| AccountRead {
                    provider: holdings
                        .account(&node)
                        .map(|a| a.provider().to_owned())
                        .unwrap_or_default(),
                    node,
                    decimals: DECIMALS,
                    read,
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn publisher_accounts(&self) -> Vec<PublisherAccount> {
        self.0
            .deployed
            .registers
            .borrow()
            .publisher_accounts
            .values()
            .cloned()
            .collect()
    }

    fn ask_as_operator(&mut self, module: &str, operation: Operation) -> Handled {
        self.as_operator(|core, operator| {
            core.fleet.platform.handle(
                module,
                &rutent_prov::holding::wallet::Operator::statement(operator, operation),
            )
        })
        .unwrap_or(Handled::Refused(
            rutent_contracts::module_interface::PlatformRefusal::Unavailable,
        ))
    }

    fn publishers_records(&mut self) -> Vec<Published> {
        // `T-020-58`: no operation asks a publisher for its record yet, so it is read as the scenario world holds it.
        let core = &mut *self.0;
        core.readings.refresh(&core.fleet.platform.world);
        let mut nodes: Vec<String> = core
            .installed
            .iter()
            .flat_map(|one| {
                one.advertisement
                    .statement()
                    .statement
                    .private_nodes
                    .clone()
            })
            .collect();
        nodes.sort();
        nodes.dedup();
        nodes
            .into_iter()
            .flat_map(|node| {
                core.readings
                    .published_in(&node)
                    .into_iter()
                    .map(move |record| Published {
                        node: node.clone(),
                        record,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn run_checkpoints(&mut self) -> Result<usize, String> {
        self.0.run_checkpoints()
    }

    fn fed(&mut self) {
        let _ = self.0.feed();
    }

    fn with_segments(&mut self, with: &mut dyn FnMut(&mut Segments, &mut SegmentParts<'_>)) {
        let core = &mut *self.0;
        let readings = core.readings.clone();
        readings.refresh(&core.fleet.platform.world);
        let mut running = Running {
            platform: &mut core.fleet.platform,
            clock: &core.clock,
            faults: &mut core.faults,
            feed: &mut core.feed,
            log: &core.log,
            swap_to: core.swap_to.as_deref_mut(),
            readings: &readings,
        };
        let mut grading = rutent_core_orch::closing::Evidenced {
            catalogue: &core.catalogue,
            records: &readings,
            books: &readings,
            closing: &mut core.closing,
        };
        let mut decisions = core.policy.clone();
        let mut channel = Channel {
            notices: &mut core.notices,
            key: &mut core.notice_signer,
            transport: &mut core.wire,
        };
        let mut parts = SegmentParts {
            clock: &core.clock,
            catalogue: &core.catalogue,
            log: &mut core.general,
            evidence: &mut core.keeper,
            decisions: &mut decisions,
            modules: &mut running,
            uses: &mut NoProxy,
            grading: &mut grading,
            operator_key: &mut core.signer,
            fresh: &mut core.fresh,
            issued: &core.issued,
            exposures: &mut core.exposures,
            channel: &mut channel,
        };
        with(&mut core.segments, &mut parts);
    }

    fn with_reversals(
        &mut self,
        with: &mut dyn FnMut(&mut Reversals, &mut ReversalParts<'_>, &mut Moving<'_>),
    ) {
        let core = &mut *self.0;
        let (core_actor, operator) = (kit::core(), kit::operator());
        let readings = core.readings.clone();
        readings.refresh(&core.fleet.platform.world);
        let mut running = Running {
            platform: &mut core.fleet.platform,
            clock: &core.clock,
            faults: &mut core.faults,
            feed: &mut core.feed,
            log: &core.log,
            swap_to: core.swap_to.as_deref_mut(),
            readings: &readings,
        };
        let mut decisions = core.policy.clone();
        let mut moving = Moving {
            decisions: &mut decisions,
            modules: &mut running,
        };
        let mut parts = ReversalParts {
            clock: &core.clock,
            catalogue: &mut core.catalogue,
            log: &mut core.general,
            evidence: &mut core.keeper,
            fresh: &mut core.fresh,
            issued: &core.issued,
            segments: &mut core.segments,
            exposures: &mut core.exposures,
            core: &core_actor,
            operator: &operator,
        };
        with(&mut core.life.reversals, &mut parts, &mut moving);
    }

    fn with_stuck(&mut self, with: &mut StuckLent<'_>) {
        let core = &mut *self.0;
        let mut policy = core.policy.clone();
        let mut channel = Channel {
            notices: &mut core.notices,
            key: &mut core.notice_signer,
            transport: &mut core.wire,
        };
        let mut parts = StuckParts {
            clock: &core.clock,
            log: &mut core.general,
            fresh: &mut core.fresh,
            issued: &core.issued,
            segments: &mut core.segments,
            exposures: &mut core.exposures,
        };
        with(
            &mut core.stuck,
            &mut parts,
            &mut policy as &mut dyn StuckOrders,
            &mut channel as &mut dyn Operator,
        );
    }

    fn with_landings(&mut self, with: &mut dyn FnMut(&mut Segments, &mut dyn Landings)) {
        let _ = self.as_operator(|core, operator| {
            let holdings = std::rc::Rc::clone(&core.holdings);
            let mut held = holdings.borrow_mut();
            let mut landing = super::holding::Landing {
                holdings: &mut held,
                platform: &mut core.fleet.platform,
                operator,
            };
            with(&mut core.segments, &mut landing);
        });
    }

    fn with_audit(
        &mut self,
        with: &mut dyn FnMut(&mut Segments, &mut dyn Comparisons, &mut dyn Measures),
    ) {
        let core = &mut *self.0;
        let channel = Channel {
            notices: &mut core.notices,
            key: &mut core.notice_signer,
            transport: &mut core.wire,
        };
        let mut compare = Compare::new(
            Compare::<CoreLog, Channel<'_>, SharedClock>::core_audit(kit::core().party),
            General::new(&core.log),
            channel,
            core.clock.clone(),
        );
        let mut measures = rutent_core_audit::measures::Record::new(
            rutent_core_audit::measures::Record::<CoreLog, SharedClock>::core_audit(kit::id(
                kit::CORE_PARTY,
            )),
            General::new(&core.log),
            core.clock.clone(),
        );
        with(&mut core.segments, &mut compare, &mut measures);
    }

    fn with_prices(&mut self, with: &mut dyn FnMut(PriceDesk<'_>, &Segments)) {
        let core = &mut *self.0;
        let core_actor = kit::core();
        let desk = PriceDesk {
            prices: &mut core.reported,
            log: &mut core.general,
            clock: &core.clock,
            core: &core_actor,
        };
        with(desk, &core.segments);
    }

    fn with_arrivals(&mut self, with: &mut dyn FnMut(&mut Arrivals, &mut Desk<'_, '_>)) {
        self.0.with_arrivals(|arrivals, desk| with(arrivals, desk));
    }

    fn with_notices(
        &mut self,
        with: &mut dyn FnMut(&mut Notices, &mut dyn NoticeKey, &mut dyn Transport),
    ) {
        let core = &mut *self.0;
        with(&mut core.notices, &mut core.notice_signer, &mut core.wire);
    }
}
