//! Core's catalogue rebuilt from its log on reopening (`NFR-6`, `T-020-61`, the coordinator's ruling on the round's
//! third pass): Core keeps no durable store beside the log, so a reopened Core lists again what it listed before, from
//! the log's own entries and the statements its evidence store kept, as `Closing::recorded` and `Holds::recorded`
//! rebuild theirs.
//!
//! [`relist`] reads the log in its order:
//!
//! - **Each `done` grant**, of a tuple or of a verifier's class, is granted again at its effect's time, by its decider,
//!   under the advertising key the effect names and the id Core's grant gave it (the effect's place in the log), on the
//!   deployment's terms ([`Relisting::terms`]: the party, expiry, categories and reversal capacity core-internal's grant
//!   doesn't carry). A tuple's grant is the tuple of the provider's advertisement the log's key register accepted that
//!   the effect names.
//! - **Each advertisement** is joined at the time the log received it, but only the stated entry whose commitment is the
//!   one the log's key register accepted last for its maker ([`Relisting::accepted`]), read back from the evidence store:
//!   a re-advertisement the register accepted is listed as such, one it refused is not listed at all (`T-024-21`). It is
//!   judged by the listings and cutovers the log held when it was received, never by those it holds now (`REQ-PRT-4`,
//!   `T-020-54`), and the deployment then sets what it sets on a joined advertisement ([`Relisting::joined`]).
//! - **Any other `done` catalogue change** but the ones the platform or the settings hold (a setting, a release's
//!   acceptance or confirmation, a public network's listing) refuses the reopening: a catalogue rebuilt without it would
//!   grant what the log revoked.
//!
//! An advertisement the deployment installed that the log committed to in none of its stated entries is no
//! advertisement this Core ever recorded, tampered with or never received: the reopening is refused (`REQ-AUD-2`).
//! Every cutover the log holds stands at the end, in the catalogue and the platform.
//!
//! **A live Core lists by the same rule** ([`advertise`], the review of #355's round 2, M2): an advertisement a provider
//! sends is kept, stated in the log as its maker's claim, and joined into the catalogue only where the log's key register
//! accepted that very statement, judged by the listings and cutovers the log holds when it was received, then the
//! deployment sets what it sets on a joined advertisement. An advertisement is its maker's signed statement, not an
//! action anyone decides: what policy decided of it is the operator's grant before it (`super::grant`) and the
//! operator's settings the log holds, and the key register is the log's. So a live Core and a reopened one list the
//! same advertisements, by one reading (`join_received`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Action, CatalogueChange, Effect, Entry, Outcome, Permit, Receipt, StatedEntry, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, Advertisement, Statement};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::catalogue::{Catalogue, Grant, GrantTerms, Instant, TupleKey, tuples_of};

use super::{Cutovers, Settings, Targets};

/// What the deployment hands Core to rebuild its catalogue that the log doesn't hold.
pub struct Relisting<'a> {
    /// The statement the evidence store kept under a commitment, its bytes as received.
    pub kept: &'a mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    /// The commitment of the advertisement the log's key register accepted last for a provider, if any.
    pub accepted: &'a dyn Fn(&str) -> Option<Digest>,
    /// The advertisements the deployment installed, by provider, as received: each must be one the log committed to.
    pub installed: &'a [(String, Vec<u8>)],
    /// The deployment's terms of a grant to a provider, granted by the decider at a time; `None` where the deployment
    /// installs no such provider.
    pub terms: &'a dyn Fn(&str, &str, Instant) -> Option<GrantTerms>,
    /// The id the log names a provider's tuple by.
    pub tuple_id: &'a dyn Fn(&str, &TupleKey) -> Id,
    /// What the deployment sets on its catalogue once an advertisement, its bytes, is joined: the module's version and
    /// paths, its tuples' evidence floors, its private nodes' holdings.
    pub joined: &'a mut dyn FnMut(&mut Catalogue, &[u8]),
}

/// Why the catalogue wasn't rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelistRefused {
    /// A `done` catalogue change that isn't rebuilt here.
    NotReplayed {
        /// The change.
        change: CatalogueChange,
        /// Its provider.
        provider: Id,
    },
    /// An installed advertisement no stated entry of the log commits to.
    Uncommitted(String),
    /// The register's accepted advertisement for a provider can't be read back from the evidence store.
    Unreadable(String),
    /// The catalogue refused a grant or an advertisement the log holds.
    Refused(String),
}

/// Rebuilds `catalogue` from `history`, the log's entries in order, with what the deployment hands in `relisting`;
/// every cutover the log holds put in force in the catalogue and `platform` at the end.
///
/// # Errors
///
/// As [`RelistRefused`] names; the catalogue is left as far as it was rebuilt.
pub fn relist<'c>(
    history: &[(Receipt, Entry)],
    catalogue: &'c mut Catalogue,
    platform: Option<&'c mut (dyn Cutovers + 'c)>,
    relisting: &mut Relisting<'_>,
) -> Result<(), RelistRefused> {
    let done: Vec<&Receipt> = history
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) if v.outcome == (Outcome::Done {}) => match &v.what {
                What::Result { effect } => Some(effect),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let installed: BTreeMap<&str, &[u8]> = relisting
        .installed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    // Each installed provider the log holds an advertisement of, and whether one of its stated entries commits to the
    // installed bytes.
    let mut committed: BTreeMap<String, bool> = BTreeMap::new();
    for (place, (receipt, entry)) in history.iter().enumerate() {
        match entry {
            Entry::Stated(s) if s.what.statement.as_str() == ADVERTISEMENT_SHAPE => {
                let maker = s.attribution.maker().acts.as_str().to_owned();
                let Some(mine) = installed.get(maker.as_str()) else {
                    continue;
                };
                let bytes = (relisting.kept)(&s.commitment);
                *committed.entry(maker.clone()).or_default() |= bytes.as_deref() == Some(*mine);
                if (relisting.accepted)(&maker).as_ref() != Some(&s.commitment) {
                    continue;
                }
                let bytes = bytes.ok_or_else(|| RelistRefused::Unreadable(maker.clone()))?;
                join_received(&history[..place], s, &bytes, catalogue, relisting.joined)?;
            }
            Entry::Verified(v) => {
                let What::Effect {
                    action:
                        Action::ChangeCatalogue {
                            change,
                            provider,
                            tuple,
                            class,
                            ..
                        },
                    decision,
                } = &v.what
                else {
                    continue;
                };
                if !done.contains(&receipt) || skipped(*change) {
                    continue;
                }
                if *change != CatalogueChange::Grant || (tuple.is_none() && class.is_none()) {
                    return Err(RelistRefused::NotReplayed {
                        change: *change,
                        provider: provider.clone(),
                    });
                }
                let at = instant(&v.when.at);
                let decider = decider_of(history, decision.decision());
                let Some(mut terms) = (relisting.terms)(
                    provider.as_str(),
                    decider.as_ref().map_or("", Id::as_str),
                    at,
                ) else {
                    continue;
                };
                terms.id = format!("grant-{}-{}", provider.as_str(), u64::from(receipt.index));
                terms.tuples = match tuple {
                    Some(id) => {
                        let Some(found) = accepted_tuples(relisting, provider.as_str())
                            .into_iter()
                            .find(|t| (relisting.tuple_id)(provider.as_str(), t) == *id)
                        else {
                            continue;
                        };
                        vec![found]
                    }
                    None => Vec::new(),
                };
                // Granted again on the effect the log holds, done: the grant rests on that effect, as it did when made.
                let effect = logged(&v.what, decision, receipt)
                    .ok_or_else(|| RelistRefused::Refused("a grant's effect".to_owned()))?;
                regrant(catalogue, &effect, terms)?;
            }
            Entry::Stated(_) => {}
        }
    }
    if let Some((maker, _)) = committed.iter().find(|(_, c)| !**c) {
        return Err(RelistRefused::Uncommitted(maker.clone()));
    }
    Settings::reopened(history).apply_cutovers(&mut Targets {
        catalogue: Some(catalogue),
        platform,
        ..Targets::default()
    });
    Ok(())
}

/// Joins `bytes`, the advertisement the log received as `stated`, into `catalogue` as Core lists one, live or reopened:
/// judged by the listings and cutovers `before`, the log's entries before it, held, at the time the log received it;
/// then the deployment sets on it what `joined` sets. The caller has held it to the key register's accepted commitment.
fn join_received(
    before: &[(Receipt, Entry)],
    stated: &StatedEntry,
    bytes: &[u8],
    catalogue: &mut Catalogue,
    joined: &mut dyn FnMut(&mut Catalogue, &[u8]),
) -> Result<(), RelistRefused> {
    let held = Settings::reopened(before);
    held.apply_listings(&mut Targets {
        catalogue: Some(catalogue),
        ..Targets::default()
    });
    held.apply_cutovers(&mut Targets {
        catalogue: Some(catalogue),
        ..Targets::default()
    });
    catalogue
        .join(bytes, instant(&stated.received_at))
        .map_err(|e| {
            RelistRefused::Refused(format!(
                "{}'s advertisement: {e:?}",
                stated.attribution.maker().acts.as_str()
            ))
        })?;
    joined(catalogue, bytes);
    Ok(())
}

/// What the deployment hands Core with an advertisement a provider sent: only what the log doesn't hold.
pub struct Advertising<'a> {
    /// Core's general handle on the log.
    pub log: &'a mut dyn Log,
    /// The log's entries as it holds them now, read after the advertisement's entry is appended.
    pub history: &'a dyn Fn() -> Vec<(Receipt, Entry)>,
    /// The evidence store, which keeps the advertisement's bytes before the log names them.
    pub evidence: &'a mut dyn Evidence,
    /// When the log receives it: Core's clock, as a timestamp.
    pub received_at: Timestamp,
    /// The commitment of the advertisement the log's key register accepted last for a provider, if any.
    pub accepted: &'a dyn Fn(&str) -> Option<Digest>,
    /// What the deployment sets on its catalogue once an advertisement, its bytes, is joined.
    pub joined: &'a mut dyn FnMut(&mut Catalogue, &[u8]),
}

/// How an advertisement Core received came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advertised {
    /// Kept, stated at this receipt, the register accepted it, and it is listed in the catalogue.
    Listed(Receipt),
    /// Kept and stated at this receipt, but the log's key register didn't accept it (an older one replayed, a key it
    /// doesn't know): nothing is listed, and the catalogue keeps what it held.
    NotAccepted(Receipt),
}

/// Why an advertisement wasn't received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvertiseRefused {
    /// The bytes aren't a provider's advertisement Core can read.
    Unreadable,
    /// The evidence store or the log refused, or couldn't take it now.
    Append(AppendFailure),
    /// The catalogue refused the advertisement the register accepted.
    Refused(String),
}

/// Core's entry point for an advertisement a provider sends (see the module's docs, the review of #355's round 2, M2):
/// its bytes kept in the evidence store, its stated entry appended as its maker's claim for its publisher, made when it
/// says and received now, and, where the log's key register accepted that very statement, joined into `catalogue` by
/// [`relist`]'s own reading. The deployment hands in only what the log doesn't hold ([`Advertising`]).
///
/// # Errors
///
/// As [`AdvertiseRefused`] names.
pub fn advertise(
    bytes: &[u8],
    catalogue: &mut Catalogue,
    advertising: &mut Advertising<'_>,
) -> Result<Advertised, AdvertiseRefused> {
    let statement: Statement<Advertisement> =
        serde_json::from_slice(bytes).map_err(|_| AdvertiseRefused::Unreadable)?;
    let kept = advertising
        .evidence
        .keep(bytes)
        .map_err(AdvertiseRefused::Append)?;
    let entry: GeneralEntry = serde_json::from_value(serde_json::json!({
        "kind": "stated",
        "attribution": {"signature": "unverified", "claimed_by": {
            "party": statement.statement.publisher,
            "acts": statement.maker,
            "role": "provider",
            "kind": "component",
        }},
        "what": {"statement": ADVERTISEMENT_SHAPE},
        "when": {"at": statement.made_at, "source": "party_stated"},
        "received_at": advertising.received_at,
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": statement.signature.key},
        "commitment": kept.commitment,
    }))
    .map_err(|_| AdvertiseRefused::Unreadable)?;
    let receipt = advertising
        .log
        .append(&entry)
        .map_err(AdvertiseRefused::Append)?;
    let history = (advertising.history)();
    let Some(place) = history.iter().position(|(r, _)| *r == receipt) else {
        return Err(AdvertiseRefused::Append(AppendFailure::Unavailable));
    };
    let Entry::Stated(stated) = &history[place].1 else {
        return Err(AdvertiseRefused::Unreadable);
    };
    // Only the statement the register accepted is listed, as on a reopening.
    if (advertising.accepted)(stated.attribution.maker().acts.as_str()).as_ref()
        != Some(&stated.commitment)
    {
        return Ok(Advertised::NotAccepted(receipt));
    }
    join_received(
        &history[..place],
        stated,
        bytes,
        catalogue,
        advertising.joined,
    )
    .map_err(|e| AdvertiseRefused::Refused(format!("{e:?}")))?;
    Ok(Advertised::Listed(receipt))
}

/// The effect the log holds at `receipt`, `what` its entry's, on the permit `decision` its entry names: what a grant
/// granted again on reopening rests on, never one Core makes anew.
fn logged(what: &What, decision: &Permit, receipt: &Receipt) -> Option<Effect> {
    match what {
        What::Effect { action, .. } => Some(Effect {
            action: action.clone(),
            decision: decision.clone(),
            receipt: receipt.clone(),
        }),
        _ => None,
    }
}

/// Grants `terms` in `catalogue` again on the `done` grant `effect` the log holds, trusting each advertising key it names.
fn regrant(
    catalogue: &mut Catalogue,
    effect: &Effect,
    terms: GrantTerms,
) -> Result<(), RelistRefused> {
    let Action::ChangeCatalogue {
        advertising_keys, ..
    } = &effect.action
    else {
        return Err(RelistRefused::Refused("not a grant".to_owned()));
    };
    let id = terms.id.clone();
    let made = Grant::new(terms).map_err(|e| RelistRefused::Refused(format!("{e:?}")))?;
    catalogue
        .grant(made)
        .map_err(|e| RelistRefused::Refused(format!("{e:?}")))?;
    for key in advertising_keys {
        catalogue
            .trust_advertising_key(&id, key.clone())
            .map_err(|e| RelistRefused::Refused(format!("{e:?}")))?;
    }
    Ok(())
}

/// The catalogue changes the log holds that Core's catalogue lists nothing for: a setting (rebuilt by
/// [`Settings::reopened`]), a release accepted or confirmed (the platform's record holds it), and a public network's
/// listing (`T-020-60`'s, until Core reads through the network's reader).
fn skipped(change: CatalogueChange) -> bool {
    matches!(
        change,
        CatalogueChange::SetSetting
            | CatalogueChange::ConfirmReleases
            | CatalogueChange::AcceptModule
    ) || change.is_about_a_public_network()
}

/// The tuples of the advertisement the log's key register accepted last for `provider`, read back from the evidence
/// store: what a grant the log names by tuple id is of.
fn accepted_tuples(relisting: &mut Relisting<'_>, provider: &str) -> Vec<TupleKey> {
    let Some(commitment) = (relisting.accepted)(provider) else {
        return Vec::new();
    };
    (relisting.kept)(&commitment)
        .and_then(|bytes| serde_json::from_slice::<Statement<Advertisement>>(&bytes).ok())
        .map(|ad| tuples_of(&ad.statement))
        .unwrap_or_default()
}

/// Who asked for the decision `decision`, as the log's decision entry names its caller.
fn decider_of(history: &[(Receipt, Entry)], decision: &Id) -> Option<Id> {
    history.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::Decide {
                request,
                decision: d,
            } if d == decision => Some(request.caller.acts.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })
}

/// `at` on the catalogue's clock: whole seconds since the epoch.
fn instant(at: &Timestamp) -> Instant {
    Instant(u64::try_from(at.unix_nanos().div_euclid(1_000_000_000)).unwrap_or(0))
}
