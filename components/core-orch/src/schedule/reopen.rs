//! Core opened again on its log (`NFR-6`, `INV-8`; `T-020-61`, the review of #355's round 2, M3): Core's own entry
//! point, [`reopen`], which rebuilds every part of Core from the log, in order, and hands the deployment back the
//! parts to hold. Core keeps no durable store beside the log, so this is what a reopened Core is; the deployment hands
//! in only what the log doesn't hold ([`Deployment`]) and decides nothing of the order.
//!
//! In this order, each from the log and what the parts before it rebuilt:
//!
//! 1. **The catalogue**: the host's holding accounts listed first, as the deployment keeps them; then every grant and
//!    advertisement the log holds, by the key register's reading and the operator's settings held when each was
//!    received ([`crate::settings::listing::relist`]).
//! 2. **The settings**: rebuilt from the log, a setting whose result the log never took completed from what the
//!    rebuilt catalogue answers now ([`Settings::reopen`]); then put back in the catalogue, over the floors and public
//!    nodes the listing's joins set (a setting completes against the catalogue, so it comes after it).
//! 3. **Closing** and **the holds** reserves gave of Core's quotes, the transactions the rounds asked under, the prices
//!    reported, **intake**'s orders and **arrivals**' filings, each from the log and the evidence store.
//! 4. **The segments**, the value in flight beside them, with each segment carried on before Core stopped known as such
//!    (item (8) carries a segment on once, across restarts too); the routes handed on whose start the log proved; the
//!    hand-ons the log holds half of; the landings owed, asked of the holding accounts; **the stuck orders**, a
//!    resolution done whose `stuck_resolved` the log lacks finished.
//! 5. **The reversals**: the late answers kept, and each reversal's asks, their carrying-on, its confirmations and its
//!    carrier (`asks`, H1).
//! 6. **The schedule**: when each segment was last asked its status, and the operator's cadences put in it; the
//!    segments' node-record retry and the reversals' retry (`reversal_retries`) from the same settings; arrivals' and
//!    the platform's settings.
//! 7. **What only the log completes**: a restrictive lifecycle effect's missing result, from what the platform holds,
//!    and the erasures decided and done but not recorded.
//!
//! The deployment then holds what this answers and lets Core's run resume ([`super::resume`]): the run builds what each
//! started order runs on from the parts, which are all rebuilt by then.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::KeptStatements;
use rutent_contracts::core_internal::{Actor, Authorisation, Entry, Receipt, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{DECISION_RECORD_SHAPE, DecisionRecord, Id};
use rutent_prov::catalogue::Catalogue;

use super::Schedule;
use crate::arrival::Arrivals;
use crate::closing::Closing;
use crate::erasure::{Statements, Unsettled};
use crate::hand_on::HandOn;
use crate::intake::Intake;
use crate::quote::{Holds, Prices};
use crate::reversal::Reversals;
use crate::rounds::{Fresh, Issued};
use crate::segments::lifecycle::{Journal, Releases};
use crate::segments::{Landings, Reopening, Segments};
use crate::settings::listing::{RelistRefused, Relisting, relist};
use crate::settings::{Cutovers, Settings, Targets};
use crate::snapshot::{Clock, Exposures, OrchestrationLog};
use crate::stuck::{Parts as StuckParts, Stuck};

/// Core's general handle on the log, as reopening writes through it: a setting's or a lifecycle effect's missing
/// result, a stuck resolution finished, an erasure recorded.
pub trait ReopenLog: Journal + OrchestrationLog {}

impl<T: Journal + OrchestrationLog> ReopenLog for T {}

/// The evidence store, as a reopening Core reads it.
pub trait Kept {
    /// The bytes kept behind `commitment`, borne out by it: `None` where the store keeps none.
    fn bytes(&mut self, commitment: &Digest) -> Option<Vec<u8>>;

    /// The store's read, as the log reads it.
    fn statements(&mut self) -> &mut dyn KeptStatements;
}

/// The module platform, as a reopening Core reaches it: the cutovers it admits by, what it holds of each release, and
/// the holding accounts its wallets run.
pub trait Platform {
    /// Where the cutovers go.
    fn cutovers(&mut self) -> &mut dyn Cutovers;

    /// Hands `with` what the platform holds of each release.
    fn releases(&mut self, with: &mut dyn FnMut(&dyn Releases));

    /// Hands `with` the holding accounts, which a landing owed is asked of.
    fn landings(&mut self, with: &mut dyn FnMut(&mut dyn Landings));
}

/// What the deployment hands Core's reopening: only what the log doesn't hold.
pub struct Deployment<'a> {
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Core, who writes its own entries.
    pub core: &'a Actor,
    /// Core's general handle on the log.
    pub log: &'a mut dyn ReopenLog,
    /// The evidence store.
    pub kept: &'a mut dyn Kept,
    /// The terms policy's register holds in force for an order's authorisation.
    pub in_force: &'a mut dyn FnMut(&Id, &Id) -> Option<Authorisation>,
    /// Core's catalogue, empty, which the reopening lists into.
    pub catalogue: &'a mut Catalogue,
    /// The host's holding accounts and the publishers' accounts, listed in the catalogue as the deployment keeps them.
    pub listed: &'a mut dyn FnMut(&mut Catalogue),
    /// What the catalogue's rebuild needs that the log doesn't hold, its read of the store but for the store's own.
    pub relisting: Relist<'a>,
    /// The module platform.
    pub platform: &'a mut dyn Platform,
    /// Fresh identifiers, for a stuck resolution finished.
    pub fresh: &'a mut dyn Fresh,
    /// The evidence store, as an erasure settles in it.
    pub erasing: &'a mut dyn Statements,
}

/// What the catalogue's rebuild needs that the log doesn't hold ([`Relisting`], but for its read of the store).
pub struct Relist<'a> {
    /// The commitment of the advertisement the log's key register accepted last for a provider, if any.
    pub accepted: &'a dyn Fn(&str) -> Option<Digest>,
    /// The advertisements the deployment installed, by provider, as received.
    pub installed: &'a [(String, Vec<u8>)],
    /// The deployment's terms of a grant to a provider, granted by the decider at a time.
    pub terms: &'a dyn Fn(
        &str,
        &str,
        rutent_prov::catalogue::Instant,
    ) -> Option<rutent_prov::catalogue::GrantTerms>,
    /// The id the log names a provider's tuple by.
    pub tuple_id: &'a dyn Fn(&str, &rutent_prov::catalogue::TupleKey) -> Id,
    /// What the deployment sets on its catalogue once an advertisement, its bytes, is joined.
    pub joined: &'a mut dyn FnMut(&mut Catalogue, &[u8]),
}

/// Core's parts, as a reopened Core rebuilt them from its log.
pub struct Reopened {
    /// The decision records Core signed, as the evidence store keeps them, held to the log's digests, by id.
    pub records: BTreeMap<String, Vec<u8>>,
    /// The operator's settings.
    pub settings: Settings,
    /// The grading's record of what closed.
    pub closing: Closing,
    /// The holds reserves gave of Core's quotes.
    pub holds: Holds,
    /// The transactions the routing rounds asked under.
    pub issued: Issued,
    /// The executed prices reported.
    pub prices: Prices,
    /// Intake's orders and acceptances.
    pub intake: Intake,
    /// The arrivals, the deposits held and the waits for a customer.
    pub arrivals: Arrivals,
    /// The segments.
    pub segments: Segments,
    /// The value in flight.
    pub exposures: Exposures,
    /// The hand-ons.
    pub hand_on: HandOn,
    /// The stuck orders.
    pub stuck: Stuck,
    /// The reversals.
    pub reversals: Reversals,
    /// The schedule.
    pub schedule: Schedule,
    /// How many landings owed were found placed in their holding accounts.
    pub landings_found: usize,
    /// How many erasures decided and done were recorded.
    pub erasures_settled: usize,
}

/// Why Core didn't reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReopenRefused {
    /// The catalogue's rebuild was refused.
    Catalogue(RelistRefused),
    /// A setting's missing result couldn't be completed.
    Setting(String),
    /// A lifecycle effect's missing result couldn't be completed.
    Lifecycle(String),
}

/// The reading the parts rebuild through: the decision records held to the log's digests, the evidence store, and
/// policy's register of authorisations.
struct Reading<'a, 'b> {
    records: &'a BTreeMap<String, DecisionRecord>,
    kept: &'a mut dyn Kept,
    in_force: &'a mut (dyn FnMut(&Id, &Id) -> Option<Authorisation> + 'b),
}

impl Reopening for Reading<'_, '_> {
    fn record(&self, id: &Id) -> Option<DecisionRecord> {
        self.records.get(id.as_str()).cloned()
    }

    fn kept(&mut self, commitment: &Digest) -> Option<Vec<u8>> {
        self.kept.bytes(commitment)
    }

    fn in_force(&mut self, order: &Id, authorisation: &Id) -> Option<Authorisation> {
        (self.in_force)(order, authorisation)
    }
}

/// The decision records Core signed, read back from the evidence store: each the operator's stated entry of router-api's
/// decision-record shape whose kept bytes, canonical, hash to the digest the log's `decision_record` entry names the
/// record by. A record the store no longer keeps, or whose bytes aren't the ones the log names, is not read, and what
/// rests on it fails closed.
fn records_in_the_store(
    history: &[(Receipt, Entry)],
    kept: &mut dyn Kept,
) -> (BTreeMap<String, Vec<u8>>, BTreeMap<String, DecisionRecord>) {
    let named: BTreeMap<String, String> = history
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::DecisionRecord {
                    decision_record,
                    record,
                    ..
                } => Some((
                    decision_record.as_str().to_owned(),
                    record.value.as_str().to_owned(),
                )),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect();
    let mut bytes_of = BTreeMap::new();
    let mut records = BTreeMap::new();
    for (_, entry) in history {
        let Entry::Stated(stated) = entry else {
            continue;
        };
        if stated.what.statement.as_str() != DECISION_RECORD_SHAPE {
            continue;
        }
        let Some(bytes) = kept.bytes(&stated.commitment) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let Ok(record) = serde_json::from_value::<DecisionRecord>(value["statement"].clone())
        else {
            continue;
        };
        let Ok(canonical) = rutent_contracts::provider_role::to_canonical(&value) else {
            continue;
        };
        let id = record.record_id().as_str().to_owned();
        if named.get(&id).map(String::as_str)
            != Some(rutent_prov::module::wire::sha256(&canonical).as_str())
        {
            continue;
        }
        bytes_of.insert(id.clone(), bytes);
        records.insert(id, record);
    }
    (bytes_of, records)
}

/// Reopens Core on `history`, its log's entries in order, with what the deployment hands in (see the module's docs):
/// every part rebuilt from the log, in order, and answered for the deployment to hold.
///
/// # Errors
///
/// As [`ReopenRefused`] names: the catalogue's rebuild refused, or a missing result the log wouldn't take.
pub fn reopen(
    history: &[(Receipt, Entry)],
    deployment: &mut Deployment<'_>,
) -> Result<Reopened, ReopenRefused> {
    let (records, read) = records_in_the_store(history, deployment.kept);
    // 1. The catalogue, the deployment's accounts listed first.
    (deployment.listed)(deployment.catalogue);
    {
        let kept = &mut *deployment.kept;
        let mut bytes = |commitment: &Digest| kept.bytes(commitment);
        relist(
            history,
            deployment.catalogue,
            Some(deployment.platform.cutovers()),
            &mut Relisting {
                kept: &mut bytes,
                accepted: deployment.relisting.accepted,
                installed: deployment.relisting.installed,
                terms: deployment.relisting.terms,
                tuple_id: deployment.relisting.tuple_id,
                joined: deployment.relisting.joined,
            },
        )
        .map_err(ReopenRefused::Catalogue)?;
    }
    // 2. The settings, a missing result completed against the rebuilt catalogue, then put back over the joins' defaults.
    let (settings, completed) = Settings::reopen(
        deployment.log,
        deployment.clock,
        deployment.core,
        &mut Targets {
            catalogue: Some(deployment.catalogue),
            ..Targets::default()
        },
    );
    completed.map_err(|e| ReopenRefused::Setting(format!("{e:?}")))?;
    settings.apply(&mut Targets {
        catalogue: Some(deployment.catalogue),
        ..Targets::default()
    });
    // 3. Closing, the holds, the rounds' transactions, the prices reported, intake and arrivals.
    let closing = Closing::recorded(history);
    let mut reading = Reading {
        records: &read,
        kept: deployment.kept,
        in_force: deployment.in_force,
    };
    let holds = Holds::reopened(history, &mut reading);
    let issued = Issued::recorded(history);
    let prices = Prices::recorded(history);
    let intake = Intake::reopened(history, &mut reading);
    let arrivals = Arrivals::reopened(history, reading.kept.statements());
    // 4. The segments and the value in flight, the starts proven, the hand-ons, the landings and the stuck orders.
    let mut exposures = Exposures::new();
    let mut segments = Segments::reopened(
        deployment.clock,
        deployment.catalogue,
        history,
        &mut reading,
        &mut exposures,
    );
    let entries: Vec<Entry> = history.iter().map(|(_, e)| e.clone()).collect();
    HandOn::recover_starts(&mut segments, &entries);
    let hand_on = HandOn::reopened(history, &segments, &mut reading);
    let mut landings_found = 0;
    deployment
        .platform
        .landings(&mut |landings| landings_found = segments.reconcile_landings(landings));
    let mut stuck = Stuck::reopened(&segments, deployment.clock, history, &mut exposures);
    // 5. The reversals.
    let mut reversals =
        Reversals::reopened(history, &mut |commitment| reading.kept.bytes(commitment));
    {
        let mut parts = StuckParts {
            clock: deployment.clock,
            log: deployment.log,
            fresh: deployment.fresh,
            issued: &issued,
            segments: &mut segments,
            exposures: &mut exposures,
        };
        let _ = stuck.finish_reopened(&mut parts, deployment.core);
    }
    // 6. The schedule, and the operator's settings put where they are read.
    let mut schedule = Schedule::recorded(history);
    let mut arrivals = arrivals;
    settings.apply(&mut Targets {
        catalogue: Some(deployment.catalogue),
        cadences: Some(schedule.cadences_mut()),
        arrivals: Some(&mut arrivals),
        platform: Some(deployment.platform.cutovers()),
        segments: None,
        reversals: Some(&mut reversals),
    });
    segments.set_record_retry_seconds(schedule.cadences().of(super::Job::PublishersRecords));
    // 7. A lifecycle effect's missing result, and the erasures decided and done.
    let mut lifecycle = Ok(0);
    {
        let log = &mut *deployment.log;
        let (clock, core) = (deployment.clock, deployment.core);
        deployment.platform.releases(&mut |releases| {
            lifecycle = Segments::reopen_lifecycle(log, releases, clock, core);
        });
    }
    lifecycle.map_err(|e| ReopenRefused::Lifecycle(format!("{e:?}")))?;
    let now: Vec<(Receipt, Entry)> = deployment.log.history();
    let mut erasures_settled = 0;
    for unsettled in Unsettled::reopened(&now, deployment.core) {
        if unsettled
            .settle(deployment.log, deployment.erasing, deployment.clock)
            .is_ok()
        {
            erasures_settled += 1;
        }
    }
    Ok(Reopened {
        records,
        settings,
        closing,
        holds,
        issued,
        prices,
        intake,
        arrivals,
        segments,
        exposures,
        hand_on,
        stuck,
        reversals,
        schedule,
        landings_found,
        erasures_settled,
    })
}
