//! A public network listed or delisted by the operator, carried out by Core (`T-020-60` (2), `DEC-28`, `REQ-CAT-22`;
//! the review of round 1, M1): [`change_public_network`] is Core's command, as `settings::grant::grant` is a grant's.
//!
//! In turn: policy decides the change, asked by the operator's login, before anything is checked, so a caller policy
//! refuses learns nothing of whether a network is listed or in use (`T-020-68`, the check of `T-020-60`); a delisting is
//! then held to [`may_delist`] (a network no listing names has nothing to delist, and one anything of Core's is at keeps
//! its standing while it is: [`Segments::touches_node`], value resting there, a segment not yet final, a reversal owed
//! there, an open order whose route names it, as a node does under `T-020-66`'s unlist rule, and a quoted order whose
//! route does); Core records the effect; then carries it out, in its adapter and
//! the catalogue's public list of networks, a listing only after its reader's start-up check; then records the
//! result, `done` where the catalogue took it and `failed` where it refused. A listed network whose reader didn't start
//! is held off every route and raised to the operator, once in its spell, citing the listing just recorded. Nothing is
//! carried out before the effect's receipt, and nothing on a refusal.

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, Operator};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, CatalogueChange, DecisionRequest, Outcome, PublicNetwork, Receipt, What,
};
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_prov::catalogue::Catalogue;

use super::{DelistRefused, NotStarted, Readers, Standing, listings_of, may_delist};
use crate::intake::Intake;
use crate::rounds::Fresh;
use crate::segments::journal;
use crate::segments::lifecycle::Journal;
use crate::segments::{RaiseDesk, Segments};
use crate::snapshot::{Clock, Exposures};

/// A listing or delisting of a public network, as the operator asks it.
#[derive(Debug, Clone, Copy)]
pub struct NetworkAsked<'a> {
    /// `list_public_network` or `delist_public_network`.
    pub change: CatalogueChange,
    /// The network, with its listing where it is listed and none where it is delisted.
    pub network: &'a PublicNetwork,
    /// The request's id, the operator's own: a network's own id (`list-{network}`) keeps every identifier Core draws
    /// as it was.
    pub request: &'a Id,
    /// Who asks: an operator's login, which policy decides.
    pub caller: &'a Actor,
}

/// What the command reaches: Core's log, policy, its catalogue, clock, identifiers and operator channel, and the orders
/// a delisting is held to.
pub struct NetworkDesk<'a> {
    /// Core's general handle on the log, with the entries it holds.
    pub log: &'a mut dyn Journal,
    /// Policy's one decision call.
    pub decisions: &'a mut dyn Decisions,
    /// Core's catalogue, whose public list of networks the change is carried out in.
    pub catalogue: &'a mut Catalogue,
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Fresh identifiers.
    pub fresh: &'a mut dyn Fresh,
    /// The operator's channel, which a network that didn't start is raised through.
    pub operator: &'a mut dyn Operator,
    /// Core, who writes its own entries.
    pub core: &'a Actor,
    /// Core's intake: the orders quoted and started.
    pub intake: &'a Intake,
    /// The value in flight.
    pub exposures: &'a Exposures,
    /// Core's segments: what a delisting is held to, as an unlist of a node is ([`Segments::touches_node`]).
    pub segments: &'a Segments,
}

/// How a listing or delisting came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The receipt of the change's effect.
    pub effect: Receipt,
    /// Where the network stands now with Core's adapter.
    pub standing: Standing,
}

/// Why a listing or delisting wasn't carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRefused {
    /// The change is no listing or delisting of a public network, or its network doesn't name its listing exactly
    /// when it is listed.
    NotAPublicNetworkChange,
    /// A delisting refused before policy is asked (#343's review, L7; #346's review, L3).
    Delist(DelistRefused),
    /// Policy didn't permit it: no effect was recorded, and nothing was carried out.
    NotPermitted(Answer),
    /// Permitted and recorded, but the catalogue refused it: its result is `failed`.
    NotTaken(String),
    /// The log refused or failed an append.
    Append(AppendFailure),
}

impl From<AppendFailure> for ChangeRefused {
    fn from(failure: AppendFailure) -> Self {
        ChangeRefused::Append(failure)
    }
}

/// Lists or delists `asked.network` as policy decides, carried out by Core in `readers` and the catalogue's public list
/// (see the module's docs).
///
/// # Errors
///
/// As [`ChangeRefused`] names. A raise the log couldn't take is no refusal: the network stays out of its spell, and the
/// next check raises it.
pub fn change_public_network(
    readers: &mut Readers,
    desk: &mut NetworkDesk<'_>,
    asked: &NetworkAsked<'_>,
) -> Result<Changed, ChangeRefused> {
    let listing = match asked.change {
        CatalogueChange::ListPublicNetwork if asked.network.listing.is_some() => true,
        CatalogueChange::DelistPublicNetwork if asked.network.listing.is_none() => false,
        _ => return Err(ChangeRefused::NotAPublicNetworkChange),
    };
    let name = asked.network.network.as_str().to_owned();
    let action = Action::ChangeCatalogue {
        change: asked.change,
        provider: asked.caller.party.clone(),
        version: None,
        tuple: None,
        class: None,
        advertising_keys: Vec::new(),
        statement_key: None,
        setting: None,
        versions: Vec::new(),
        public_network: Some(Box::new(asked.network.clone())),
    };
    let request = DecisionRequest {
        request: asked.request.clone(),
        caller: asked.caller.clone(),
        action: action.clone(),
        at: now(desk.clock)?,
    };
    let decision = desk.decisions.decide(&request)?;
    let Some(permit) = decision.permit() else {
        return Err(ChangeRefused::NotPermitted(decision.outcome));
    };
    // Only a caller policy permits learns why a delisting can't be carried out now: no effect is recorded for it.
    if !listing {
        let history = desk.log.history();
        may_delist(
            &listings_of(&history),
            &name,
            in_use(&name, desk.segments, desk.intake, desk.exposures),
        )
        .map_err(ChangeRefused::Delist)?;
    }
    let effect = journal::append(
        desk.log,
        journal::verified(
            desk.core,
            What::Effect {
                action,
                decision: permit,
            },
            &now(desk.clock)?,
            Outcome::ToTakeEffect {},
        ),
    )?;
    // Carried out: the adapter takes the listing in, or off, and the catalogue's public list follows the check.
    let taken = if listing {
        readers.take(asked.network);
        let started = matches!(readers.check(&name), Standing::Started);
        desk.catalogue
            .list_public_network(&name, true, started)
            .map_err(|e| format!("{e:?}"))
    } else {
        readers.take(asked.network);
        desk.catalogue.drop_public_network(&name);
        Ok(())
    };
    journal::append(
        desk.log,
        journal::verified(
            desk.core,
            What::Result {
                effect: effect.clone(),
            },
            &now(desk.clock)?,
            if taken.is_ok() {
                Outcome::Done {}
            } else {
                Outcome::Failed {}
            },
        ),
    )?;
    taken.map_err(ChangeRefused::NotTaken)?;
    // A listed network whose reader didn't start is raised, citing the listing the log now holds done.
    let history = desk.log.history();
    let names: Vec<String> = readers.listed().keys().cloned().collect();
    let not_started: std::collections::BTreeMap<String, NotStarted> = names
        .iter()
        .filter_map(|n| match readers.standing(n) {
            Standing::NotStarted(why) => Some((n.clone(), why)),
            _ => None,
        })
        .collect();
    let _ = readers.raised.raise(
        &names,
        &not_started,
        &history,
        RaiseDesk {
            log: desk.log,
            clock: desk.clock,
            fresh: desk.fresh,
            operator: desk.operator,
            core: desk.core,
        },
    );
    Ok(Changed {
        effect,
        standing: readers.standing(&name),
    })
}

/// Whether anything of Core's is at `network` now, so its standing may not change under it (`T-020-68`, the check of
/// `T-020-60`'s Low-Medium 1): what [`Segments::touches_node`] holds an unlist of a node to (value resting there, a
/// segment moving from or to it whose outcome isn't final, a failed one whose reversal is owed, an order not ended whose
/// route names it), or an open or quoted order that uses it ([`uses_network`]).
pub fn in_use(network: &str, segments: &Segments, intake: &Intake, exposures: &Exposures) -> bool {
    segments.touches_node(network) || uses_network(network, intake, exposures)
}

/// Whether an open or quoted order uses `network` (`T-020-66`'s unlist rule, which a network's standing keeps too; the
/// review of round 1, L6): one with value still in flight whose route names the network, by an outer node's network
/// or an inner node, or one quoted, neither started nor ended, whose quoted route does.
pub fn uses_network(network: &str, intake: &Intake, exposures: &Exposures) -> bool {
    let names = |record: &DecisionRecord| {
        serde_json::to_value(record).is_ok_and(|v| value_names(&v, network))
    };
    exposures.held().any(|held| {
        intake
            .started(&held.order)
            .is_some_and(|started| names(&started.record))
    }) || intake.quoted_over(&mut |node| match node {
        NodeRef::Inner(id) => id.as_str() == network,
        NodeRef::Outer(outer) => outer.network.as_str() == network,
    })
}

/// Whether `value` names `network` as a node or a network anywhere in it.
fn value_names(value: &serde_json::Value, network: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, v)| {
            ((key == "network" || key == "node") && v.as_str() == Some(network))
                || value_names(v, network)
        }),
        serde_json::Value::Array(items) => items.iter().any(|v| value_names(v, network)),
        _ => false,
    }
}

/// Core's clock, as a timestamp.
fn now(clock: &dyn Clock) -> Result<rutent_contracts::router_api::Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(journal::NO_TIME))
}
