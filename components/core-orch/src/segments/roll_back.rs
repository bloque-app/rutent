//! Rolling a release's staging back (`T-021-12`, ADR-0014): the platform's `roll_back` ends a provider's staging and
//! keeps the version in force, and Core carries it out as it carries out `promote`, on a permitted decision's effect
//! and on nothing else (`REQ-AUZ-6`, `REQ-AUZ-1`).
//!
//! `CMP-CORE-AUTHZ`'s scan holds a call of `roll_back` to an [`Effect`] in hand, by name. It can't read which action
//! the effect is, so [`carry_out_roll_back`] checks the permitted action itself: an effect whose action isn't a
//! `change_catalogue` of `roll_back` carries nothing out, however it was permitted, and the version it names must be the
//! one staged right before the platform is asked, so a permit to roll one release back ends no other staging.
//!
//! The platform is reached through [`RollsBack`], apart from [`super::Modules`], so no implementation of that port
//! changes with this seam.

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, Log};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, CatalogueChange, DecisionRequest, Effect, Outcome, What,
};
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::LifecycleRefusal;
use rutent_prov::module::platform::{Platform, World};

use super::{Segments, journal, key, now};
use crate::rounds::Fresh;
use crate::snapshot::Clock;

/// The platform's staging, as the roll back reaches it.
pub trait RollsBack {
    /// The provider's staged version, if one is.
    fn staged(&self, provider: &Id) -> Option<String>;

    /// Ends the provider's staging, the version in force kept, by `by` at `at`, as `effect` permits
    /// (`Platform::roll_back`). The version whose staging ended.
    fn roll_back(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal>;
}

impl<W: World> RollsBack for Platform<W> {
    fn staged(&self, provider: &Id) -> Option<String> {
        self.lifecycle()
            .staged(provider.as_str())
            .map(str::to_owned)
    }

    fn roll_back(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        let _permitted = effect;
        Platform::roll_back(self, provider.as_str(), by.as_str(), at)
    }
}

/// A roll back to carry out: the staging of `provider` that `version` names, asked by `caller`.
#[derive(Debug, Clone, Copy)]
pub struct RollBackAsked<'a> {
    /// The provider.
    pub provider: &'a Id,
    /// The staged version whose staging ends.
    pub version: &'a Id,
    /// Who asks: the operator's login, which policy decides and the platform records as `by`.
    pub caller: &'a Actor,
    /// Core, which writes the effect and its result.
    pub core: &'a Actor,
}

/// Why a roll back wasn't carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollBackRefused {
    /// The provider has nothing staged.
    NothingStaged,
    /// The version asked for isn't the one staged now: a permit to roll one release back ends no other staging.
    /// Nothing was asked of policy, or nothing was carried out.
    NotTheStagedVersion {
        /// The version staged now, empty where none is.
        staged: String,
    },
    /// The effect's action isn't a roll back of the staging it was carried out for: nothing was asked of the platform.
    NotARollBack,
    /// Policy didn't permit it: no effect was recorded.
    NotPermitted(Answer),
    /// The log refused or failed an append: before the effect's receipt nothing changed.
    Append(AppendFailure),
    /// The platform refused: the effect's result is `failed`, and nothing changed.
    Lifecycle(LifecycleRefusal),
}

impl From<AppendFailure> for RollBackRefused {
    fn from(failure: AppendFailure) -> Self {
        RollBackRefused::Append(failure)
    }
}

/// Ends the staging `effect` permits, by `by`: only where its action is a `change_catalogue` of `roll_back`, at the
/// provider and version it names, and that version is the one staged now. Anything else asks nothing of the platform.
pub fn carry_out_roll_back(
    modules: &mut dyn RollsBack,
    effect: &Effect,
    by: &Id,
    clock: &dyn Clock,
) -> Result<String, RollBackRefused> {
    let Action::ChangeCatalogue {
        change: CatalogueChange::RollBack,
        provider,
        version: Some(version),
        ..
    } = &effect.action
    else {
        return Err(RollBackRefused::NotARollBack);
    };
    let staged = modules
        .staged(provider)
        .ok_or(RollBackRefused::NothingStaged)?;
    if staged != version.as_str() {
        return Err(RollBackRefused::NotTheStagedVersion { staged });
    }
    modules
        .roll_back(effect, provider, by, clock.now().floor())
        .map_err(RollBackRefused::Lifecycle)
}

impl Segments {
    /// Ends `asked.provider`'s staging of `asked.version`, as policy decides, by the decision's caller: only the
    /// version staged now, so a permit to roll one release back ends no other staging (`REQ-AUZ-6`, ADR-0014). Core
    /// asks the decision itself, records the effect before it calls the platform, and records its result after; an
    /// effect is marked acted on only once the platform ended the staging.
    pub fn roll_back(
        &mut self,
        log: &mut dyn Log,
        decisions: &mut dyn Decisions,
        modules: &mut dyn RollsBack,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        asked: &RollBackAsked<'_>,
    ) -> Result<String, RollBackRefused> {
        let staged = modules
            .staged(asked.provider)
            .ok_or(RollBackRefused::NothingStaged)?;
        if staged != asked.version.as_str() {
            return Err(RollBackRefused::NotTheStagedVersion { staged });
        }
        let action = Action::ChangeCatalogue {
            change: CatalogueChange::RollBack,
            provider: asked.provider.clone(),
            version: Some(asked.version.clone()),
            tuple: None,
            class: None,
            advertising_keys: Vec::new(),
            statement_key: None,
        };
        let request = DecisionRequest {
            request: fresh.id(),
            caller: asked.caller.clone(),
            action: action.clone(),
            at: now(clock)?,
        };
        let decision = decisions.decide(&request)?;
        let Some(permit) = decision.permit() else {
            return Err(RollBackRefused::NotPermitted(decision.outcome));
        };
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
        let rolled_back = carry_out_roll_back(modules, &effect, &request.caller.acts, clock);
        let outcome = match rolled_back {
            Ok(_) => Outcome::Done {},
            Err(_) => Outcome::Failed {},
        };
        journal::append(
            log,
            journal::verified(
                asked.core,
                What::Result {
                    effect: effect.receipt.clone(),
                },
                &now(clock)?,
                outcome,
            ),
        )?;
        if rolled_back.is_ok() {
            self.acted.insert(key(&effect.receipt));
        }
        rolled_back
    }
}
