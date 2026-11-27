//! The operator's settings, each set through policy and rebuilt from the log (`T-020-49`, finding 4's slippage and
//! public-node settings and `M2`'s item 6 of `M3`'s milestone review of 2026-11-24).
//!
//! **One command.** [`Settings::set`] is the operator's command for every setting core-internal names
//! ([`OperatorSetting`]): the fee formula, a tuple's slippage limit and evidence floor, a public node's settings, a
//! provider's and a publisher's ceiling on value in flight, a scheduled job's cadence (a provider's own status cadence
//! among them), a protocol version's cutover date and the deposit wait. It asks policy for core-internal's `set_setting`
//! change (`T-020-45`), which `core-authz` holds to a permission of its own for each kind, by the deployment operator's
//! login (`T-021-15`); on a permit it appends the effect before it applies the setting and its result after. Nothing is
//! applied on a refusal; a permitted deposit wait above [`DEPOSIT_WAIT_CEILING_SECONDS`] takes no effect.
//!
//! **The log is the record.** A reopened Core rebuilds every setting from the `set_setting` effects the log holds whose
//! result isn't `failed`, in the log's order, the later of two for one thing standing ([`Settings::reopened`]); nothing
//! else holds a setting across a restart. An effect with no result was applied when it was set, so it stands after the
//! restart as before, and [`Settings::reopen`] appends its missing `done` result.
//!
//! **Who reads them.** The router's bounds are the deployment's own (its hop and part limits, its policy version, its
//! exclusions) overlaid with the operator's fee, slippage limits and ceilings ([`Settings::bounds`]); the catalogue takes
//! each evidence floor and each public node the operator lists, or takes off its public list ([`Settings::apply`]); the schedule takes each cadence
//! ([`Settings::apply`]); arrivals take the deposit wait, and [`crate::arrival::Arrivals::reopened`] rebuilds it from the
//! log too. The values stay the deployment's to choose; the per-network evidence rules (finality, reorgs) are `F-031`'s
//! under `DEC-13`, and a public node's `confirmations` reach Core here ([`Settings::public_node`]) for the network's
//! reader to apply (see `F-020`'s notes for `T-020-49`'s gap on it).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, Log};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, CatalogueChange, Count, DecisionRequest, Effect, Entry, OperatorSetting,
    Outcome, Receipt, STATUS_ASKS_JOB, VerifiedEntry, What,
};
use rutent_contracts::provider_role::{EvidenceGrade, NodeRef};
use rutent_contracts::router_api::{
    FeeFormula, PolicyBounds, ProviderCeiling, PublisherCeiling, SlippageLimit, Timestamp, TupleKey,
};
use rutent_prov::catalogue::{Catalogue, Category, Endpoint, TupleKey as CatalogueTuple};

use crate::arrival::Arrivals;
use crate::rounds::Fresh;
use crate::schedule::{Cadences, Job};
use crate::segments::journal;
use crate::segments::lifecycle::Journal;
use crate::snapshot::Clock;

/// The longest a deposit's wait for its customer may be set to: thirty days (`T-020-51`'s deposit wait, bounded by
/// `T-020-49`). A longer wait would hold an automatic deposit out of every other order for longer than a reserve's
/// customer could be expected to answer; the operator sets anything up to it.
pub const DEPOSIT_WAIT_CEILING_SECONDS: u64 = 30 * 86_400;

/// A public node's settings, as the operator set them last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicNode {
    /// Whether the operator lists it public.
    pub listed: bool,
    /// The confirmations a movement at it waits for before Core takes it as final, where the operator set them.
    pub confirmations: Option<u64>,
}

/// The operator's settings, as the log's decided changes set them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    fee: Option<FeeFormula>,
    slippage: Vec<SlippageLimit>,
    floors: Vec<(TupleKey, EvidenceGrade)>,
    public_nodes: BTreeMap<String, PublicNode>,
    provider_ceilings: Vec<ProviderCeiling>,
    publisher_ceilings: Vec<PublisherCeiling>,
    cadences: BTreeMap<String, u64>,
    status_cadences: BTreeMap<String, u64>,
    cutovers: BTreeMap<String, Timestamp>,
    deposit_wait: Option<Count>,
}

/// A setting to set, asked by `caller`.
#[derive(Debug, Clone, Copy)]
pub struct SettingAsked<'a> {
    /// The setting and its value.
    pub setting: &'a OperatorSetting,
    /// Who asks: an operator's login, which policy decides.
    pub caller: &'a Actor,
    /// Core, which writes the effect and its result.
    pub core: &'a Actor,
}

/// Why a setting wasn't set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingRefused {
    /// A deposit wait above [`DEPOSIT_WAIT_CEILING_SECONDS`]: policy is asked first (its decision records the attempt),
    /// and, permitted, no effect is recorded.
    AboveCeiling {
        /// The ceiling, in whole seconds.
        ceiling: u64,
    },
    /// Policy didn't permit it: no effect was recorded, and nothing changed.
    NotPermitted(Answer),
    /// Permitted and recorded, but its target refused it (the catalogue won't list a node a provider declared private):
    /// its result is `failed` and the setting isn't the operator's.
    NotTaken(String),
    /// Permitted, but the node it unlists has value of an order Core follows resting at it, or a segment in flight from
    /// or to it, or there was no record of the segments to tell: an unlist would change how they are graded, and let a
    /// publisher claim the node as its own private one (`T-020-65`). No effect was recorded.
    Rests {
        /// The node.
        node: String,
    },
    /// The log refused or failed an append: before the effect's receipt nothing changed; after it, the setting is in
    /// force now, and a reopened Core holds it too and completes its result ([`Settings::reopen`]).
    Append(AppendFailure),
}

impl From<AppendFailure> for SettingRefused {
    fn from(failure: AppendFailure) -> Self {
        SettingRefused::Append(failure)
    }
}

/// What a setting is applied to beyond the settings' own record: the catalogue (evidence floors, public nodes), Core's
/// schedule (cadences) and arrivals (the deposit wait). Each is optional, for a caller that holds only some of them. The
/// segments, read only, are what an unlist is held to: with none, no node is unlisted (it fails closed).
#[derive(Default)]
pub struct Targets<'a> {
    /// Core's segments, read: whether value rests at a node, or a segment in flight moves value from or to it.
    pub segments: Option<&'a crate::segments::Segments>,
    /// Core's catalogue.
    pub catalogue: Option<&'a mut Catalogue>,
    /// Core's schedule's cadences.
    pub cadences: Option<&'a mut Cadences>,
    /// Arrivals and their waits.
    pub arrivals: Option<&'a mut Arrivals>,
    /// The module platform, which admits a release assertion by the protocol versions in force (`REQ-PRT-4`,
    /// `T-020-54`).
    pub platform: Option<&'a mut dyn Cutovers>,
}

/// What takes an older protocol version's cutover date: the module platform's admission of a release assertion.
pub trait Cutovers {
    /// Puts the older protocol version `version` in force until `cutover`.
    fn set_cutover(&mut self, version: &str, cutover: &str);
}

impl<W: rutent_prov::module::platform::World> Cutovers
    for rutent_prov::module::platform::Platform<W>
{
    fn set_cutover(&mut self, version: &str, cutover: &str) {
        rutent_prov::module::platform::Platform::set_cutover(self, version, cutover);
    }
}

/// The catalogue's key for the router's tuple: an exchange where both ends are one node in two assets, a reserve where
/// an end is an outer node, and a ramp where both ends are inner nodes of one asset.
pub fn catalogue_tuple(tuple: &TupleKey) -> CatalogueTuple {
    let (from, to) = (&tuple.from, &tuple.to);
    let category = if from.node == to.node && from.asset.id != to.asset.id {
        Category::Exchange
    } else if matches!(from.node, NodeRef::Outer(_)) || matches!(to.node, NodeRef::Outer(_)) {
        Category::Reserve
    } else {
        Category::Ramp
    };
    CatalogueTuple {
        category,
        origin: Endpoint::from(&from.node),
        destination: Endpoint::from(&to.node),
        asset: from.asset.id.clone(),
        to_asset: (category == Category::Exchange).then(|| to.asset.id.clone()),
    }
}

impl Settings {
    /// None set: the deployment's own bounds, the catalogue's and the schedule's defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// The settings rebuilt from `history`, the log's own entries, as a Core that reopens reads them: each `set_setting`
    /// effect whose result isn't `failed`, in the log's order. An effect with no result was applied when it was set (the
    /// result is appended after the setting takes effect), so it stands after a restart as it stood before
    /// ([`Settings::reopen`] appends its missing result; M2 of #337's review). A deposit wait above its ceiling, which no
    /// command sets, is never taken.
    pub fn reopened(history: &[(Receipt, Entry)]) -> Self {
        let failed: std::collections::BTreeSet<String> = history
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what: What::Result { effect },
                    outcome: Outcome::Failed {},
                    ..
                }) => Some(receipt_key(effect)),
                _ => None,
            })
            .collect();
        let mut settings = Settings::new();
        for (receipt, entry) in history {
            if let Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change: CatalogueChange::SetSetting,
                                setting: Some(setting),
                                ..
                            },
                        ..
                    },
                ..
            }) = entry
                && !failed.contains(&receipt_key(receipt))
                && !above_ceiling(setting)
            {
                settings.take(setting);
            }
        }
        settings
    }

    /// The settings rebuilt from Core's log, as [`Settings::reopened`] reads them, with the `done` result appended, by
    /// `core`, for each `set_setting` effect the log holds no result of: the setting was in force when Core stopped, so
    /// its result completes the log, and every later reopening reads it the same (M2 of #337's review).
    ///
    /// # Errors
    ///
    /// The log's, appending a missing result; the settings are rebuilt the same whatever it answers.
    pub fn reopen(
        log: &mut dyn Journal,
        clock: &dyn Clock,
        core: &Actor,
    ) -> (Self, Result<usize, AppendFailure>) {
        let history = log.history();
        let settings = Settings::reopened(&history);
        let resulted: std::collections::BTreeSet<String> = history
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what: What::Result { effect },
                    ..
                }) => Some(receipt_key(effect)),
                _ => None,
            })
            .collect();
        let unfinished: Vec<Receipt> = history
            .iter()
            .filter_map(|(receipt, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what:
                        What::Effect {
                            action:
                                Action::ChangeCatalogue {
                                    change: CatalogueChange::SetSetting,
                                    ..
                                },
                            ..
                        },
                    ..
                }) if !resulted.contains(&receipt_key(receipt)) => Some(receipt.clone()),
                _ => None,
            })
            .collect();
        let completed = (|| {
            for effect in &unfinished {
                journal::append(
                    log,
                    journal::verified(
                        core,
                        What::Result {
                            effect: effect.clone(),
                        },
                        &now(clock)?,
                        Outcome::Done {},
                    ),
                )?;
            }
            Ok(unfinished.len())
        })();
        (settings, completed)
    }

    /// Sets `asked.setting`, as policy decides, by the decision's caller: Core asks the decision itself, records the
    /// effect before it applies the setting, to these settings and to `targets`, and records its result after. The
    /// effect's receipt.
    ///
    /// # Errors
    ///
    /// A deposit wait above its ceiling, refused before policy is asked; a refusal of policy's; the log's.
    pub fn set(
        &mut self,
        log: &mut dyn Log,
        decisions: &mut dyn Decisions,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        targets: &mut Targets<'_>,
        asked: &SettingAsked<'_>,
    ) -> Result<Receipt, SettingRefused> {
        // A setting about a provider names it; one about the whole deployment names the operator's party, whose login
        // asks.
        let provider = asked
            .setting
            .provider()
            .cloned()
            .unwrap_or_else(|| asked.caller.party.clone());
        let action = Action::ChangeCatalogue {
            change: CatalogueChange::SetSetting,
            provider,
            version: None,
            tuple: None,
            class: None,
            advertising_keys: Vec::new(),
            statement_key: None,
            setting: Some(Box::new(asked.setting.clone())),
            versions: Vec::new(),
        };
        let at = now(clock)?;
        let request = DecisionRequest {
            request: fresh.id(),
            caller: asked.caller.clone(),
            action: action.clone(),
            at: at.clone(),
        };
        let decision = decisions.decide(&request)?;
        let Some(permit) = decision.permit() else {
            return Err(SettingRefused::NotPermitted(decision.outcome));
        };
        // Policy first, so a refused caller's attempt is recorded whatever it asked (L4 of #337's review); a permitted
        // deposit wait above its ceiling takes no effect.
        if above_ceiling(asked.setting) {
            return Err(SettingRefused::AboveCeiling {
                ceiling: DEPOSIT_WAIT_CEILING_SECONDS,
            });
        }
        // A node is unlisted only while nothing of Core's rests at it or moves through it (`T-020-65`, the rule in
        // `F-020`'s notes): an unlist would regrade a segment in flight there as non-public and let a publisher claim
        // the node as private, so it is refused while either could matter, and where the segments can't be read.
        if let OperatorSetting::PublicNode {
            node,
            listed: false,
            ..
        } = asked.setting
            && targets
                .segments
                .is_none_or(|segments| segments.touches_node(&node.network))
        {
            return Err(SettingRefused::Rests {
                node: node.network.clone(),
            });
        }
        let receipt = journal::append(
            log,
            journal::verified(
                asked.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &now(clock)?,
                Outcome::ToTakeEffect {},
            ),
        )?;
        let effect = Effect {
            action,
            decision: permit,
            receipt,
        };
        let applied = self.apply_effect(&effect, targets);
        journal::append(
            log,
            journal::verified(
                asked.core,
                What::Result {
                    effect: effect.receipt.clone(),
                },
                &now(clock)?,
                match applied {
                    Ok(()) => Outcome::Done {},
                    Err(_) => Outcome::Failed {},
                },
            ),
        )?;
        applied.map_err(SettingRefused::NotTaken)?;
        Ok(effect.receipt)
    }

    /// Applies the setting `effect` permits, to these settings and to `targets`; anything but a `set_setting` sets
    /// nothing.
    fn apply_effect(&mut self, effect: &Effect, targets: &mut Targets<'_>) -> Result<(), String> {
        let Action::ChangeCatalogue {
            change: CatalogueChange::SetSetting,
            setting: Some(setting),
            ..
        } = &effect.action
        else {
            return Ok(());
        };
        // Applied first: a setting its target refuses is no setting of the operator's, and its result is `failed`.
        apply_one(setting, targets)?;
        self.take(setting);
        Ok(())
    }

    /// Takes `setting` into these settings, in place of any earlier one for the same thing.
    fn take(&mut self, setting: &OperatorSetting) {
        match setting {
            OperatorSetting::OperatorFee { formula } => self.fee = Some(formula.clone()),
            OperatorSetting::SlippageLimit { limit } => {
                self.slippage.retain(|l| l.tuple != limit.tuple);
                self.slippage.push(limit.clone());
            }
            OperatorSetting::EvidenceFloor { tuple, grade } => {
                self.floors.retain(|(t, _)| t != tuple);
                self.floors.push((tuple.clone(), *grade));
            }
            OperatorSetting::PublicNode {
                node,
                listed,
                confirmations,
            } => {
                self.public_nodes.insert(
                    node.network.clone(),
                    PublicNode {
                        listed: *listed,
                        confirmations: confirmations.map(Count::get),
                    },
                );
            }
            OperatorSetting::ProviderCeiling { ceiling } => {
                self.provider_ceilings
                    .retain(|c| !(c.provider == ceiling.provider && c.asset == ceiling.asset));
                self.provider_ceilings.push(ceiling.clone());
            }
            OperatorSetting::PublisherCeiling { ceiling } => {
                self.publisher_ceilings
                    .retain(|c| !(c.publisher == ceiling.publisher && c.asset == ceiling.asset));
                self.publisher_ceilings.push(ceiling.clone());
            }
            OperatorSetting::Cadence {
                job,
                seconds,
                provider,
            } => match provider {
                Some(provider) => {
                    self.status_cadences
                        .insert(provider.as_str().to_owned(), seconds.get());
                }
                None => {
                    self.cadences.insert(job.as_str().to_owned(), seconds.get());
                }
            },
            OperatorSetting::Cutover { version, cutover } => {
                self.cutovers
                    .insert(version.as_str().to_owned(), cutover.clone());
            }
            OperatorSetting::DepositWait { seconds } => self.deposit_wait = Some(*seconds),
        }
    }

    /// Applies every setting held to `targets`, as a reopened Core does once its parts are put together again.
    pub fn apply(&self, targets: &mut Targets<'_>) {
        for (tuple, grade) in &self.floors {
            let _ = apply_one(
                &OperatorSetting::EvidenceFloor {
                    tuple: tuple.clone(),
                    grade: *grade,
                },
                targets,
            );
        }
        for (node, set) in &self.public_nodes {
            if let Some(catalogue) = targets.catalogue.as_deref_mut() {
                let _ = list(catalogue, node, set.listed);
            }
        }
        if let Some(cadences) = targets.cadences.as_deref_mut() {
            for (job, seconds) in &self.cadences {
                if let Some(job) = Job::named(job) {
                    cadences.set(job, *seconds);
                }
            }
            for (provider, seconds) in &self.status_cadences {
                cadences.set_status_of(provider, *seconds);
            }
        }
        if let Some(catalogue) = targets.catalogue.as_deref_mut() {
            for (version, cutover) in &self.cutovers {
                catalogue.set_cutover(version, cutover.as_str());
            }
        }
        if let Some(platform) = targets.platform.as_deref_mut() {
            for (version, cutover) in &self.cutovers {
                platform.set_cutover(version, cutover.as_str());
            }
        }
        if let (Some(arrivals), Some(seconds)) =
            (targets.arrivals.as_deref_mut(), self.deposit_wait)
        {
            arrivals.set_deposit_wait(seconds);
        }
    }

    /// The router's bounds: the deployment's own, `base`, with the operator's fee, slippage limits and ceilings in
    /// place of its, each setting standing for its tuple, provider and asset, or publisher and asset.
    pub fn bounds(&self, base: &PolicyBounds) -> PolicyBounds {
        let mut bounds = base.clone();
        // The older protocol versions in force, each with the operator's cutover date: the router judges the snapshot's
        // statements by them (`REQ-PRT-4`, `T-020-54`). A cutover set for the current version is no older version.
        for (version, cutover) in &self.cutovers {
            if version == bounds.protocol_version.as_str() {
                continue;
            }
            if let Some(known) = bounds
                .older_protocol_versions
                .iter_mut()
                .find(|o| o.version.as_str() == version)
            {
                known.cutover = cutover.clone();
            } else if let Some(older) = rutent_contracts::router_api::ProtocolVersion::new(version)
            {
                bounds.older_protocol_versions.push(
                    rutent_contracts::router_api::OlderProtocolVersion {
                        version: older,
                        cutover: cutover.clone(),
                    },
                );
            }
        }
        if let Some(fee) = &self.fee {
            bounds.operator_fee = fee.clone();
        }
        for limit in &self.slippage {
            bounds.slippage_limits.retain(|l| l.tuple != limit.tuple);
            bounds.slippage_limits.push(limit.clone());
        }
        for ceiling in &self.provider_ceilings {
            bounds
                .provider_ceilings
                .retain(|c| !(c.provider == ceiling.provider && c.asset == ceiling.asset));
            bounds.provider_ceilings.push(ceiling.clone());
        }
        for ceiling in &self.publisher_ceilings {
            bounds
                .publisher_ceilings
                .retain(|c| !(c.publisher == ceiling.publisher && c.asset == ceiling.asset));
            bounds.publisher_ceilings.push(ceiling.clone());
        }
        bounds
    }

    /// The evidence floor the operator set on `tuple`, if any.
    pub fn evidence_floor(&self, tuple: &TupleKey) -> Option<EvidenceGrade> {
        self.floors
            .iter()
            .find(|(t, _)| t == tuple)
            .map(|(_, grade)| *grade)
    }

    /// The operator's settings of the public node `node`, if it set any.
    pub fn public_node(&self, node: &str) -> Option<PublicNode> {
        self.public_nodes.get(node).copied()
    }

    /// The cadence the operator set for the job named `job`, in whole seconds, if any.
    pub fn cadence(&self, job: &str) -> Option<u64> {
        self.cadences.get(job).copied()
    }

    /// The status cadence the operator set for `provider`, in whole seconds, if any.
    pub fn status_cadence(&self, provider: &str) -> Option<u64> {
        self.status_cadences.get(provider).copied()
    }

    /// The cutover date the operator set for the older protocol version `version`, if any (the catalogue holds it, `T-020-54`).
    pub fn cutover(&self, version: &str) -> Option<&Timestamp> {
        self.cutovers.get(version)
    }

    /// The deposit wait the operator set, if it did.
    pub fn deposit_wait(&self) -> Option<Count> {
        self.deposit_wait
    }
}

/// Applies one setting to `targets`: an evidence floor, a listed public node and a protocol version's cutover to the
/// catalogue, a cadence to the
/// schedule, the deposit wait to arrivals. The bounds and a node's confirmations are read from the settings.
fn apply_one(setting: &OperatorSetting, targets: &mut Targets<'_>) -> Result<(), String> {
    match setting {
        OperatorSetting::EvidenceFloor { tuple, grade } => {
            if let Some(catalogue) = targets.catalogue.as_deref_mut() {
                catalogue.set_evidence_floor(catalogue_tuple(tuple), *grade);
            }
        }
        OperatorSetting::PublicNode { node, listed, .. } => {
            if let Some(catalogue) = targets.catalogue.as_deref_mut() {
                list(catalogue, &node.network, *listed)?;
            }
        }
        OperatorSetting::Cadence {
            job,
            seconds,
            provider,
        } => {
            if let Some(cadences) = targets.cadences.as_deref_mut() {
                match provider {
                    Some(provider) if job.as_str() == STATUS_ASKS_JOB => {
                        cadences.set_status_of(provider.as_str(), seconds.get());
                    }
                    Some(_) => {}
                    None => {
                        if let Some(job) = Job::named(job.as_str()) {
                            cadences.set(job, seconds.get());
                        }
                    }
                }
            }
        }
        OperatorSetting::DepositWait { seconds } => {
            if let Some(arrivals) = targets.arrivals.as_deref_mut() {
                arrivals.set_deposit_wait(*seconds);
            }
        }
        // The older version is in force in the catalogue until the date, and a statement under it is void from then
        // on (`REQ-PRT-4`, `THR-64`).
        OperatorSetting::Cutover { version, cutover } => {
            if let Some(catalogue) = targets.catalogue.as_deref_mut() {
                catalogue.set_cutover(version.as_str(), cutover.as_str());
            }
            if let Some(platform) = targets.platform.as_deref_mut() {
                platform.set_cutover(version.as_str(), cutover.as_str());
            }
        }
        OperatorSetting::OperatorFee { .. }
        | OperatorSetting::SlippageLimit { .. }
        | OperatorSetting::ProviderCeiling { .. }
        | OperatorSetting::PublisherCeiling { .. } => {}
    }
    Ok(())
}

/// Puts `node` on the catalogue's public list, or takes it off, as the operator's setting says. Until `T-020-59`'s
/// listing change replaces it, unlisting is this setting's.
fn list(catalogue: &mut Catalogue, node: &str, listed: bool) -> Result<(), String> {
    if listed {
        catalogue
            .add_public_node(node)
            .map_err(|e| format!("the catalogue refuses to list {node} public: {e:?}"))
    } else {
        catalogue.remove_public_node(node);
        Ok(())
    }
}

/// Whether `setting` is a deposit wait above its ceiling, which no command sets and no reopening takes.
fn above_ceiling(setting: &OperatorSetting) -> bool {
    matches!(setting, OperatorSetting::DepositWait { seconds } if seconds.get() > DEPOSIT_WAIT_CEILING_SECONDS)
}

fn receipt_key(receipt: &Receipt) -> String {
    serde_json::to_string(receipt).unwrap_or_default()
}

/// Core's clock, as a timestamp.
fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(journal::NO_TIME))
}
