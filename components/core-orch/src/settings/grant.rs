//! A provider admitted to the catalogue through policy (`REQ-CAT-1`, `REQ-AUZ-6`, `T-020-61`): the operator's grant of
//! each tuple a provider's advertisement offers, and of each class a verifier offers in place of tuples (`T-020-30`),
//! as core-internal's `change_catalogue` names a grant.
//!
//! [`grant`] is Core's command. For each tuple or class, in turn, Core asks policy for the grant by the caller, records
//! the effect, grants it in its own catalogue at the effect's time, by the caller, under the advertising key, and
//! records its result: `done` where the catalogue took it, `failed` where it refused. Nothing is granted before its
//! effect's receipt, and nothing on a refusal. The terms core-internal's grant doesn't carry (the party it is scoped
//! to, its expiry, its categories and the reversal capacity behind it) are the deployment's, handed in with the ask;
//! Core sets the rest from the log: its id (the effect's place in the log, so a tuple granted again is granted anew),
//! who granted it, when, and the one tuple.

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, Log};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, CatalogueChange, DecisionRequest, Effect, Outcome, Receipt, What,
};
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::{Catalogue, Grant, GrantTerms, TupleKey};

use crate::rounds::Fresh;
use crate::segments::journal;
use crate::snapshot::Clock;

/// One thing a grant admits: a tuple, named as the log names it, or a class a verifier judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Granted {
    /// A tuple of the provider's advertisement, and the id the log names it by.
    Tuple {
        /// The id of the tuple in the log's grant.
        id: Id,
        /// The tuple.
        tuple: TupleKey,
    },
    /// A class a verifier offers (`T-020-30`): its grant names the class in place of a tuple.
    Class(Id),
}

/// A provider's grant, as the operator asks it.
#[derive(Debug, Clone, Copy)]
pub struct GrantAsked<'a> {
    /// The provider admitted.
    pub provider: &'a Id,
    /// The key its advertisements are signed with, which the grant trusts.
    pub advertising_key: &'a PublicKey,
    /// What it admits, each granted in turn.
    pub granting: &'a [Granted],
    /// The deployment's terms of the grant: its party, expiry, categories and reversal capacity. Core sets its id,
    /// who granted it, when and its tuple.
    pub terms: &'a GrantTerms,
    /// Who asks: an operator's login, which policy decides.
    pub caller: &'a Actor,
    /// Core, acting on its own account: who writes Core's entries.
    pub core: &'a Actor,
}

/// Why a grant wasn't made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRefused {
    /// Policy didn't permit it: no effect was recorded, and nothing was granted.
    NotPermitted(Answer),
    /// Permitted and recorded, but the catalogue refused it: its result is `failed`.
    NotTaken(String),
    /// The log refused or failed an append.
    Append(AppendFailure),
}

impl From<AppendFailure> for GrantRefused {
    fn from(failure: AppendFailure) -> Self {
        GrantRefused::Append(failure)
    }
}

/// Grants each of `asked.granting` in `catalogue`, as policy decides, by the decision's caller: the decision first, the
/// effect recorded before the catalogue takes the grant, and its result after. The receipts of the effects, in turn.
///
/// # Errors
///
/// The first refusal: policy's, the catalogue's (after its `failed` result), or the log's. What was granted before it
/// stands.
pub fn grant(
    log: &mut dyn Log,
    decisions: &mut dyn Decisions,
    catalogue: &mut Catalogue,
    clock: &dyn Clock,
    fresh: &mut dyn Fresh,
    asked: &GrantAsked<'_>,
) -> Result<Vec<Receipt>, GrantRefused> {
    let mut receipts = Vec::new();
    for one in asked.granting {
        let (tuple, class) = match one {
            Granted::Tuple { id, .. } => (Some(id.clone()), None),
            Granted::Class(class) => (None, Some(class.clone())),
        };
        let action = Action::ChangeCatalogue {
            change: CatalogueChange::Grant,
            provider: asked.provider.clone(),
            version: None,
            tuple,
            class,
            advertising_keys: vec![asked.advertising_key.clone()],
            statement_key: None,
            setting: None,
            versions: Vec::new(),
            public_network: None,
        };
        let request = DecisionRequest {
            request: fresh.id(),
            caller: asked.caller.clone(),
            action: action.clone(),
            at: now(clock)?,
        };
        let decision = decisions.decide(&request)?;
        let Some(permit) = decision.permit() else {
            return Err(GrantRefused::NotPermitted(decision.outcome));
        };
        // Granted at the effect's own time, as the log holds it, in the catalogue's whole seconds.
        let when = now(clock)?;
        let at = rutent_prov::catalogue::Instant(
            u64::try_from(when.unix_nanos().div_euclid(1_000_000_000)).unwrap_or(0),
        );
        let receipt = journal::append(
            log,
            journal::verified(
                asked.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &when,
                Outcome::ToTakeEffect {},
            ),
        )?;
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        let taken = carry_out_grant(catalogue, &effect, asked, one, &request.caller.acts, at);
        journal::append(
            log,
            journal::verified(
                asked.core,
                What::Result {
                    effect: receipt.clone(),
                },
                &now(clock)?,
                if taken.is_ok() {
                    Outcome::Done {}
                } else {
                    Outcome::Failed {}
                },
            ),
        )?;
        taken.map_err(GrantRefused::NotTaken)?;
        receipts.push(receipt);
    }
    Ok(receipts)
}

/// Grants `one` in the catalogue on the terms the log's `effect` holds, under an id of its own, the effect's place in
/// the log, and trusts the advertising key under it.
fn carry_out_grant(
    catalogue: &mut Catalogue,
    effect: &Effect,
    asked: &GrantAsked<'_>,
    one: &Granted,
    by: &Id,
    at: rutent_prov::catalogue::Instant,
) -> Result<(), String> {
    let mut terms = asked.terms.clone();
    terms.id = format!(
        "grant-{}-{}",
        asked.provider.as_str(),
        u64::from(effect.receipt.index)
    );
    terms.provider = asked.provider.as_str().to_owned();
    terms.granted_by = by.as_str().to_owned();
    terms.granted_at = at;
    terms.tuples = match one {
        Granted::Tuple { tuple, .. } => vec![tuple.clone()],
        Granted::Class(_) => Vec::new(),
    };
    let id = terms.id.clone();
    let made = Grant::new(terms).map_err(|e| format!("{e:?}"))?;
    catalogue.grant(made).map_err(|e| format!("{e:?}"))?;
    catalogue
        .trust_advertising_key(&id, asked.advertising_key.clone())
        .map_err(|e| format!("{e:?}"))
}

/// Core's clock, as a timestamp.
fn now(clock: &dyn Clock) -> Result<rutent_contracts::router_api::Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(journal::NO_TIME))
}
