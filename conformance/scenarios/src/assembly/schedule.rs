//! Core's schedule, as the assembly wires it (`T-020-48`, finding 4 of `M3`'s milestone review of 2026-11-24, and the
//! review of its first build).
//!
//! `core-orch`'s [`Schedule`](rutent_core_orch::schedule::Schedule) decides what is due and its
//! [`Jobs`] what each job does; this module answers only their [`Ports`], over what the assembly holds: the platform, the deployment's register
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

use std::collections::BTreeSet;

use rutent_contracts::core_internal::calls::{Comparisons, Measures, Operator, StuckOrders};
use rutent_contracts::core_internal::{Actor, Authorisation, Entry, Receipt};
use rutent_contracts::router_api::{Id, PolicyBounds};
use rutent_core_audit::compare::Compare;
use rutent_core_audit::log::General;
use rutent_core_orch::arrival::Arrivals;
use rutent_core_orch::closing::PriceDesk;
use rutent_core_orch::intake::Desk;
use rutent_core_orch::intake::Intake;
use rutent_core_orch::notice::{Channel, NoticeKey, Notices, Transport};
use rutent_core_orch::reversal::{Moving, Parts as ReversalParts, Reversals};
use rutent_core_orch::rounds::Fresh as _;
use rutent_core_orch::schedule::{
    Actors, Job, Jobs, MoverLent, Ports, PublicAccount, PublisherAccount, RecalculationsLent,
    Round, StuckLent,
};
use rutent_core_orch::segments::{Landings, Parts as SegmentParts, Segments};
use rutent_core_orch::snapshot::Clock;
use rutent_core_orch::stuck::{Mover, OperatorSigns, Parts as StuckParts, Stuck};
use rutent_prov::catalogue::Catalogue;

use super::{AssemblyError, Core, CoreLog, NoProxy, Running};
use crate::kit::{self, RealKey, SharedClock};
use crate::sim::InstalledWallet;

/// The decimals of every asset the simulated world moves (`kit`): what the deployment's register of holding accounts is
/// given for each account's asset where an account is installed (`T-020-63`: the schedule reads them from the register,
/// never from a constant of its own).
pub(super) const DECIMALS: u8 = 2;

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
        // What the round answered, kept for the readers that ask after it: the run's own questions after each part, and
        // the schedule's again.
        let run_asked = round.runs.parts.iter().flat_map(|p| p.asked.iter());
        for tried in run_asked.chain(round.statuses.iter()) {
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
        for part in &round.runs.parts {
            // A reversal completed: what was told of the part's recalculation before it is superseded.
            if part.reversed.iter().any(|r| r.completed()) {
                self.recalculations_told.remove(part.order.as_str());
            }
            if let Some(recalculated) = &part.recalculated {
                self.recalculations_told
                    .insert(part.order.as_str().to_owned(), recalculated.clone());
            }
        }
        self.told_reversals(&round);
        let _ = self.feed();
        round
    }

    /// The recalculation Core's run tried for `order` on a tick since this was last taken, as the round told it: what the
    /// operator's view reads of it. Taken once.
    pub fn take_recalculation(
        &mut self,
        order: &Id,
    ) -> Option<Result<rutent_core_orch::recalc::Recalculation, String>> {
        self.recalculations_told.remove(order.as_str())
    }

    /// Sets how often `job` runs, through the operator's command (`Core::set_setting`, `T-020-49`): decided by
    /// policy, recorded in the log and rebuilt from it when Core reopens. A period past 2^53 - 1 seconds, which no setting
    /// holds, is set at that (never, in practice).
    ///
    /// # Panics
    ///
    /// If policy doesn't permit the setting.
    pub fn set_cadence(&mut self, job: Job, seconds: u64) {
        self.set_setting(&cadence(job.name(), seconds, None))
            .expect("the job's cadence is set");
    }

    /// Sets how often a segment unknown at `provider` is asked its status again, through the operator's command, as
    /// [`Core::set_cadence`] does: the status question's cadence for that provider.
    ///
    /// # Panics
    ///
    /// If policy doesn't permit the setting.
    pub fn set_status_cadence(&mut self, provider: &str, seconds: u64) {
        self.set_setting(&cadence(Job::StatusAsks.name(), seconds, Some(provider)))
            .expect("the provider's status cadence is set");
    }

    /// From now on the record `order`'s route was chosen on states no price or quote for any of its operations, so no
    /// operation of it has a declared settlement time: the router's "never settles" (`declared_times`), a fault the
    /// scenarios plant to show Core's tick holds such a tuple to the floor window (`T-020-63`). Whether the order is one
    /// Core started.
    pub fn plant_unpriced_route(&mut self, order: &Id) -> bool {
        let Some(started) = self.intake.started_mut(order) else {
            return false;
        };
        started.record.snapshot.figures.retain(|figure| {
            !matches!(
                figure.value,
                rutent_contracts::router_api::FigureValue::Price { .. }
                    | rutent_contracts::router_api::FigureValue::FreshQuote { .. }
            )
        });
        true
    }

    /// Installs the plain wallet as the account `publisher` keeps for the private node `node`, holding `asset`: the
    /// platform runs it once Core's decided accept takes its release (`Core::accept_release`), the catalogue records
    /// it as the node's holding account, and the deployment's register of holding accounts names it, its publisher and its asset, so the schedule reads it as the publisher's word, and
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
        // Its release runs once Core's decided accept takes it (`T-020-61`).
        self.accept_release(&wallet)?;
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

impl Core {
    /// Reads the records of the private node `node` about the transactions `about`, of each provider of its publisher that
    /// declares it, by Core's own read ([`rutent_core_orch::segments::Segments::read_node_record`]): decided by policy,
    /// recorded as an effect, put through the platform, and each answer taken as a record only as
    /// `closing::publishers::taken` allows. For an order and a segment of it that pass the node (the effect names them), and
    /// for a scenario that wants to see what a node holds that closing and the tick don't read: a route that ends inside it,
    /// or an order that has settled.
    pub fn read_records_about(
        &mut self,
        order: &Id,
        segment: &Id,
        node: &str,
        about: &[Id],
    ) -> Vec<
        rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::InnerSegments>,
    > {
        use rutent_core_orch::closing::publishers;
        let Some(bounds) = self
            .intake
            .started(order)
            .map(|started| started.record.bounds.clone())
        else {
            return Vec::new();
        };
        let (core, operator) = (kit::core(), kit::operator());
        let providers = publishers::serving(&self.catalogue, node);
        let mut answers = Vec::new();
        Due(self).with_segments(&mut |segments, parts| {
            for provider in &providers {
                let read = rutent_core_orch::segments::RecordRead {
                    order,
                    segment,
                    node,
                    provider,
                    about,
                    bounds: &bounds,
                    operator: &operator,
                    core: &core,
                };
                if let rutent_core_orch::segments::Read::Answered(question, bytes, _) =
                    segments.read_node_record(parts, &read)
                {
                    answers.push((question, bytes));
                }
            }
        });
        let at = self.clock.now().floor();
        answers
            .into_iter()
            .filter_map(|(question, bytes)| {
                publishers::taken(&self.catalogue, at, &question.statement, &bytes).ok()
            })
            .collect()
    }

    /// Puts the question of `provider`'s record of `node` to the production platform, as the permitted `effect` lets,
    /// through [`rutent_core_orch::segments::Modules::ask_for_record`]: the path closing takes for a platform of its own
    /// (`T-020-58`). For a test that shows the production platform answers it; Core's own asking is closing's.
    pub fn ask_record_of_the_platform(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &str,
        node: &str,
        transaction: &str,
        about: &[String],
    ) -> rutent_core_orch::segments::Answered {
        use rutent_core_orch::segments::Modules;
        let refused = || {
            rutent_core_orch::segments::Answered::Refused(
                rutent_contracts::module_interface::PlatformRefusal::Unavailable,
            )
        };
        let (Some(to), Some(made_at)) = (Id::new(provider), self.clock.now().timestamp()) else {
            return refused();
        };
        self.fleet.platform.world.set_clock(self.clock.instant());
        let mut key = RealKey::assembly();
        let bounds = self.bounds.clone();
        let signs = OperatorSigns::new(&mut key, &bounds, kit::operator().acts, made_at);
        let question = rutent_prov::holding::wallet::Operator::statement(
            &signs,
            rutent_core_orch::closing::publishers::question(provider, node, transaction, about),
        );
        Modules::ask_for_record(&mut self.fleet.platform, effect, &to, &question)
    }
}

/// The deployment's ports over a [`Core`]: each reads, asks or lends, and decides nothing. A scenario hands an operator's
/// command to Core's entry point over them ([`Core::ports`]), as the deployment's own transport would.
pub struct Due<'a>(pub(super) &'a mut Core);

impl Core {
    /// The deployment's ports over this Core, for a call to one of Core's entry points (`T-020-61`): the operator's
    /// decided commands on a stuck order (`rutent_core_orch::stuck::decide_stuck`, `move_stuck`) and the reported
    /// failure's confirmation. The deployment's transport hands those to Core so; nothing here decides anything.
    pub fn ports(&mut self) -> Due<'_> {
        Due(self)
    }
}

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

    fn history(&self) -> Vec<(Receipt, Entry)> {
        self.0.log.borrow().entries().to_vec()
    }

    fn kept(&mut self, commitment: &rutent_contracts::module_interface::Digest) -> Option<Vec<u8>> {
        let core = &mut *self.0;
        let kept = rutent_contracts::core_internal::calls::KeptStatements::kept(
            &mut super::Reading {
                kept: &mut core.kept,
                through: core.kept_through.as_deref_mut(),
            },
            commitment,
        )
        .ok()??;
        rutent_core_audit::receipts::store::bearing_out(&kept)
    }

    fn in_force(&mut self, order: &Id, authorisation: &Id) -> Option<Authorisation> {
        let logged: Vec<Entry> = self
            .0
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        super::authorised(&logged, order, authorisation)
    }

    fn intake(&self) -> &Intake {
        &self.0.intake
    }

    fn arrivals(&self) -> &Arrivals {
        &self.0.arrivals
    }

    fn mover(&self) -> &Mover {
        &self.0.mover
    }

    fn with_intake(&mut self, with: &mut dyn FnMut(&mut Intake)) {
        with(&mut self.0.intake);
    }

    fn with_authorisations(
        &mut self,
        with: &mut dyn FnMut(&mut dyn rutent_contracts::core_internal::calls::Authorisations),
    ) {
        let mut policy = self.0.policy.clone();
        with(&mut policy);
    }

    fn with_recalculations(&mut self, with: &mut RecalculationsLent<'_>) {
        let core = &mut *self.0;
        let mut charges = core.policy.clone();
        let mut persons = core.policy.clone();
        let mut asking = super::Asking {
            fleet: &mut core.fleet,
            clock: &core.clock,
        };
        let log = std::rc::Rc::clone(&core.log);
        let recorded = super::LogNow(&log);
        let mut kept = super::Reading {
            kept: &mut core.kept,
            through: core.kept_through.as_deref_mut(),
        };
        let mut parts = rutent_core_orch::recalc::Parts {
            catalogue: &mut core.catalogue,
            clock: &core.clock,
            exposures: &mut core.exposures,
            releases: &core.releases,
            log: &mut core.general,
            evidence: &mut core.keeper,
            charges: &mut charges,
            persons: &mut persons,
            providers: &mut asking,
            operator_key: &mut core.signer,
            fresh: &mut core.fresh,
            issued: &mut core.issued,
            segments: &mut core.segments,
            kept: &mut kept,
            recorded: &recorded,
        };
        with(&mut core.life.recalculations, &mut parts, &core.prices);
    }

    fn with_hand_on(
        &mut self,
        with: &mut dyn FnMut(
            &mut rutent_core_orch::hand_on::HandOn,
            &mut rutent_core_orch::hand_on::Parts<'_>,
        ),
    ) {
        let core = &mut *self.0;
        let mut parts = rutent_core_orch::hand_on::Parts {
            clock: &core.clock,
            log: &mut core.general,
            fresh: &mut core.fresh,
            segments: &mut core.segments,
            exposures: &mut core.exposures,
            stuck: &mut core.stuck,
        };
        with(&mut core.hand_on, &mut parts);
    }

    fn with_mover(&mut self, with: &mut MoverLent<'_>) {
        let core = &mut *self.0;
        let operator = kit::operator();
        let mut policy = core.policy.clone();
        let resting = core.segments.resting_totals();
        let booked = core.booked.clone();
        let running = std::cell::RefCell::new(Running {
            platform: &mut core.fleet.platform,
            clock: &core.clock,
            faults: &mut core.faults,
            feed: &mut core.feed,
            log: &core.log,
            swap_to: core.swap_to.as_deref_mut(),
            booked: &booked,
        });
        let mut parts = StuckParts {
            clock: &core.clock,
            log: &mut core.general,
            fresh: &mut core.fresh,
            issued: &core.issued,
            segments: &mut core.segments,
            exposures: &mut core.exposures,
        };
        let mut no_proxy = NoProxy;
        // The hand-on's reach (`T-020-26`): the operator's holding accounts, run on the platform the move's other calls go
        // to; the linked orders' accepted routes; and the log's own handle, read as it stands each time.
        let mut modules = super::holding::Shared(&running);
        let mut wallets = super::holding::HeldWallets {
            holdings: &core.holdings,
            running: &running,
            bounds: &core.bounds,
            resting,
        };
        let accepted = super::holding::Accepted(&core.linked);
        let log = super::holding::LogHandle(std::rc::Rc::clone(&core.log));
        let mut delivering = core
            .mover
            .on(rutent_core_orch::stuck::Ports {
                modules: &mut modules,
                uses: &mut no_proxy,
                evidence: &mut core.keeper,
                catalogue: &core.catalogue,
                operator_key: &mut core.signer,
                bounds: &core.bounds,
                operator: &operator,
            })
            .handing_on(rutent_core_orch::stuck::HandingOn {
                withdrawals: &mut wallets,
                routes: &accepted,
                log: &log,
            });
        with(
            &mut core.stuck,
            &mut parts,
            &mut policy as &mut dyn StuckOrders,
            &mut delivering,
        );
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

    fn bounds(&self) -> PolicyBounds {
        self.0.bounds.clone()
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

    fn late_kept(&mut self, transaction: &str) {
        self.0.fleet.late_kept(transaction);
    }

    fn public_accounts(&self) -> Vec<PublicAccount> {
        // The register names each account and the decimals of its asset; the host's holdings give the asset the wallet
        // holds and its own record of each order. A node the register gives no decimals is left out, and so is read as not
        // named: the register is the definition of what is checked (`T-020-63`).
        let registers = self.0.deployed.registers.borrow();
        let holdings = self.0.holdings.borrow();
        let mut accounts: Vec<PublicAccount> = registers
            .holding_accounts
            .iter()
            .filter_map(|(node, provider)| {
                let decimals = *registers.holding_decimals.get(node)?;
                let account = holdings.account(node);
                Some(PublicAccount {
                    node: node.clone(),
                    provider: provider.clone(),
                    asset: account.map(|a| a.asset().to_owned()).unwrap_or_default(),
                    decimals,
                    attribution: account.map(|a| a.attribution()).unwrap_or_default(),
                })
            })
            .collect();
        // The kit's public nodes no wallet was installed at, whose catalogued holder is the kit's operator wallet, which
        // the platform doesn't run: the test world's stand-in holder answers in its place, and its own record of each
        // order is what Core's orders rest there (`kit::LEDGER_HOLDER`).
        let resting = self.0.segments.resting_totals_by_asset();
        for (node, asset) in &registers.stand_in_public {
            if registers.holding_accounts.contains_key(node)
                || self
                    .0
                    .catalogue
                    .nodes()
                    .holding(node)
                    .is_none_or(|h| h.reference != kit::OPERATOR_WALLET)
            {
                continue;
            }
            let attribution = resting
                .iter()
                .filter(|((at, _, held), _)| at == node && held == asset)
                .filter_map(|((_, order, _), amount)| {
                    Some(rutent_prov::holding::HeldForOrder {
                        order: order.clone(),
                        amount: amount.as_str().parse().ok()?,
                    })
                })
                .collect();
            accounts.push(PublicAccount {
                node: node.clone(),
                provider: kit::LEDGER_HOLDER.to_owned(),
                asset: asset.clone(),
                decimals: DECIMALS,
                attribution,
            });
        }
        accounts
    }

    fn publisher_accounts(&self) -> Vec<PublisherAccount> {
        // The stand-in holder's answer is what Core's orders rest at each node now (`kit::LEDGER_HOLDER`).
        let mut ledger: std::collections::BTreeMap<(String, String), u128> =
            std::collections::BTreeMap::new();
        for ((node, _, asset), amount) in self.0.segments.resting_totals_by_asset() {
            if let Ok(units) = amount.as_str().parse::<u128>() {
                *ledger.entry((node, asset)).or_insert(0) += units;
            }
        }
        self.0.fleet.platform.world.set_ledger(ledger);
        self.0
            .deployed
            .registers
            .borrow()
            .publisher_accounts
            .values()
            .cloned()
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
        let booked = core.booked.clone();
        let mut running = Running {
            platform: &mut core.fleet.platform,
            clock: &core.clock,
            faults: &mut core.faults,
            feed: &mut core.feed,
            log: &core.log,
            swap_to: core.swap_to.as_deref_mut(),
            booked: &booked,
        };
        // One pass over Core's adapter, at Core's clock: each question its readers are asked once in it, its answers
        // dropped with it (`T-020-60` (3)).
        let pass = core.readers.pass(&core.clock);
        let mut grading = rutent_core_orch::closing::Evidenced {
            catalogue: &core.catalogue,
            records: &pass,
            books: &booked,
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
        let booked = core.booked.clone();
        let mut running = Running {
            platform: &mut core.fleet.platform,
            clock: &core.clock,
            faults: &mut core.faults,
            feed: &mut core.feed,
            log: &core.log,
            swap_to: core.swap_to.as_deref_mut(),
            booked: &booked,
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

    fn with_reversal_desk(&mut self, with: &mut rutent_core_orch::schedule::ReversalsLent<'_>) {
        let core = &mut *self.0;
        let (core_actor, operator) = (kit::core(), kit::operator());
        let booked = core.booked.clone();
        let mut running = Running {
            platform: &mut core.fleet.platform,
            clock: &core.clock,
            faults: &mut core.faults,
            feed: &mut core.feed,
            log: &core.log,
            swap_to: core.swap_to.as_deref_mut(),
            booked: &booked,
        };
        let mut decisions = core.policy.clone();
        let mut actors = super::Acting(std::rc::Rc::clone(&core.deployed.registers));
        // One pass over Core's adapter for the reversals' reads too (`T-020-60` (3), (6)).
        let pass = core.readers.pass(&core.clock);
        let mut desk = rutent_core_orch::schedule::ReversalDesk {
            key: &mut core.signer,
            decisions: &mut decisions,
            platform: &mut running,
            stuck: &mut core.stuck,
            actors: &mut actors,
            records: &pass,
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
        with(&mut core.life.reversals, &mut parts, &mut desk);
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

/// The operator's cadence setting of the job named `job`, at `seconds` (at most 2^53 - 1), for `provider` where given.
fn cadence(
    job: &str,
    seconds: u64,
    provider: Option<&str>,
) -> rutent_contracts::core_internal::OperatorSetting {
    let seconds = seconds.min(rutent_contracts::provider_role::SafeInteger::MAX);
    let mut setting = serde_json::json!({"setting": "cadence", "job": job, "seconds": seconds});
    if let Some(provider) = provider {
        setting["provider"] = provider.into();
    }
    serde_json::from_value(setting).expect("a cadence setting")
}
